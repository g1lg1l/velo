//! The app's own load, for the status bar's right end: the CPU time and memory the system says
//! it uses, and on Windows how busy it keeps the GPU and the video memory it holds, read a
//! second apart, with the last half minute of each for its sparkline. No crate: one call or one
//! file per platform.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How often the numbers change: the bar shouldn't flicker.
const EVERY: Duration = Duration::from_secs(1);
/// How many readings a trail keeps: half a minute.
pub const TRAIL: usize = 30;

pub struct Usage {
    last: Option<(Instant, f64)>,
    /// The engine's frames a second.
    pub fps: Option<f32>,
    /// Of the whole machine, 0..100.
    pub cpu: Option<f32>,
    /// Resident memory, bytes, and all the machine has.
    pub memory: Option<u64>,
    pub memory_total: Option<u64>,
    /// The GPU's 3D engine, as Task Manager counts it for this process, 0..100 (Windows).
    pub gpu: Option<f32>,
    /// Video memory held, bytes, and what the system lets this process hold (Windows).
    pub vram: Option<u64>,
    pub vram_budget: Option<u64>,
    /// Each reading's last half minute, oldest first: fps as they were, the rest as shares of
    /// their whole (0..1).
    pub trails: Trails,
    #[cfg(windows)]
    counters: Option<gpu::Counters>,
}

#[derive(Default)]
pub struct Trails {
    pub fps: VecDeque<f32>,
    pub cpu: VecDeque<f32>,
    pub memory: VecDeque<f32>,
    pub gpu: VecDeque<f32>,
    pub vram: VecDeque<f32>,
}

fn push(trail: &mut VecDeque<f32>, value: Option<f32>) {
    let Some(value) = value else { return };
    if trail.len() == TRAIL {
        trail.pop_front();
    }
    trail.push_back(value);
}

/// `part` of `whole`, 0..1.
fn share(part: Option<u64>, whole: Option<u64>) -> Option<f32> {
    Some((part? as f64 / whole.filter(|w| *w > 0)? as f64).clamp(0.0, 1.0) as f32)
}

impl Usage {
    pub fn new() -> Self {
        Self {
            last: None,
            fps: None,
            cpu: None,
            memory: None,
            memory_total: total_memory(),
            gpu: None,
            vram: None,
            vram_budget: None,
            trails: Trails::default(),
            #[cfg(windows)]
            counters: gpu::Counters::open(),
        }
    }

    /// Reads the system again if a second has gone by; `fps` is the engine's rate now.
    pub fn sample(&mut self, fps: Option<f32>) {
        let now = Instant::now();
        if self.last.is_some_and(|(at, _)| now - at < EVERY) {
            return;
        }
        let Some((cpu_s, memory)) = read() else { return };
        if let Some((at, before)) = self.last {
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
            self.cpu = Some(((cpu_s - before) / (now - at).as_secs_f64() / cores * 100.0).clamp(0.0, 100.0) as f32);
        }
        self.fps = fps;
        self.memory = memory;
        self.last = Some((now, cpu_s));
        #[cfg(windows)]
        {
            self.gpu = self.counters.as_ref().and_then(gpu::Counters::busy);
            (self.vram, self.vram_budget) = gpu::vram().unzip();
        }
        let t = &mut self.trails;
        push(&mut t.fps, self.fps);
        push(&mut t.cpu, self.cpu.map(|c| c / 100.0));
        push(&mut t.memory, share(self.memory, self.memory_total));
        push(&mut t.gpu, self.gpu.map(|g| g / 100.0));
        push(&mut t.vram, share(self.vram, self.vram_budget));
    }
}

#[cfg(windows)]
mod gpu {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3, IDXGIFactory1};
    use windows::Win32::System::Performance::{PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW};
    use windows::core::{HSTRING, Interface};

    /// The GPU engines' load counters (Task Manager's), one query kept open: each read is the
    /// load since the one before.
    pub struct Counters {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
    }

    impl Counters {
        pub fn open() -> Option<Self> {
            let (mut query, mut counter) = (PDH_HQUERY(std::ptr::null_mut()), PDH_HCOUNTER(std::ptr::null_mut()));
            // SAFETY: out-parameters PDH fills; the query is closed if the counter can't be had
            unsafe {
                if PdhOpenQueryW(None, 0, &mut query) != 0 {
                    return None;
                }
                if PdhAddEnglishCounterW(query, &HSTRING::from("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut counter) != 0 {
                    PdhCloseQuery(query);
                    return None;
                }
                PdhCollectQueryData(query);
            }
            Some(Self { query, counter })
        }

        /// How busy this process keeps the 3D engines, 0..100.
        pub fn busy(&self) -> Option<f32> {
            let pid = format!("pid_{}_", std::process::id());
            // SAFETY: the buffer is as large as PDH asks, 8-aligned, and read within the count it gives
            unsafe {
                PdhCollectQueryData(self.query);
                let (mut size, mut count) = (0u32, 0u32);
                if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None) != PDH_MORE_DATA {
                    return None;
                }
                let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
                let items = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
                if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                    return None;
                }
                let busy: f64 = std::slice::from_raw_parts(items, count as usize)
                    .iter()
                    .filter(|i| i.FmtValue.CStatus == 0 && i.szName.to_string().is_ok_and(|n| n.starts_with(&pid) && n.ends_with("engtype_3D")))
                    .map(|i| i.FmtValue.Anonymous.doubleValue)
                    .sum();
                Some(busy.clamp(0.0, 100.0) as f32)
            }
        }
    }

    impl Drop for Counters {
        fn drop(&mut self) {
            // SAFETY: the query this opened
            unsafe {
                PdhCloseQuery(self.query);
            }
        }
    }

    /// The video memory this process holds on every adapter, and the budget the system gives
    /// it on the ones it uses.
    pub fn vram() -> Option<(u64, u64)> {
        // SAFETY: COM calls on interfaces DXGI hands out, an out-parameter of the size asked
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let (mut used, mut budget) = (0, 0);
            for i in 0.. {
                let Ok(adapter) = factory.EnumAdapters1(i) else { break };
                let Ok(adapter) = adapter.cast::<IDXGIAdapter3>() else { continue };
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                if adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info).is_ok() && info.CurrentUsage > 0 {
                    used += info.CurrentUsage;
                    budget += info.Budget;
                }
            }
            Some((used, budget))
        }
    }
}

/// Seconds of CPU the process has used, and its resident memory.
#[cfg(windows)]
fn read() -> Option<(f64, Option<u64>)> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let seconds = |f: FILETIME| ((u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)) as f64 / 1e7;
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    let mut memory = PROCESS_MEMORY_COUNTERS::default();
    // SAFETY: the current process' own handle, and out-parameters of the sizes asked for
    unsafe {
        let process = GetCurrentProcess();
        GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user).ok()?;
        let ok = GetProcessMemoryInfo(process, &mut memory, size_of::<PROCESS_MEMORY_COUNTERS>() as u32).is_ok();
        Some((seconds(kernel) + seconds(user), ok.then_some(memory.WorkingSetSize as u64)))
    }
}

/// The machine's physical memory, bytes.
#[cfg(windows)]
fn total_memory() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    // SAFETY: an out-parameter whose length field says its size
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok().map(|_| status.ullTotalPhys)
}

/// ponytail: ticks taken as 100 a second and pages as 4 KiB, as on nearly every Linux; sysconf
/// through libc if one ever differs
#[cfg(target_os = "linux")]
fn read() -> Option<(f64, Option<u64>)> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // after the command's closing parenthesis: state is the first field, utime the 12th, stime the 13th
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let ticks: f64 = fields.get(11)?.parse::<f64>().ok()? + fields.get(12)?.parse::<f64>().ok()?;
    let pages = std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok());
    Some((ticks / 100.0, pages.map(|p| p * 4096)))
}

#[cfg(target_os = "linux")]
fn total_memory() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = info.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kb * 1024)
}

/// ponytail: CPU only; the resident size needs mach's task_info, worth it when someone's on a Mac
#[cfg(target_os = "macos")]
fn read() -> Option<(f64, Option<u64>)> {
    // SAFETY: getrusage fills the struct it's given
    let usage = unsafe {
        let mut usage = std::mem::zeroed::<libc::rusage>();
        (libc::getrusage(libc::RUSAGE_SELF, &mut usage) == 0).then_some(usage)?
    };
    let seconds = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    Some((seconds(usage.ru_utime) + seconds(usage.ru_stime), None))
}

#[cfg(target_os = "macos")]
fn total_memory() -> Option<u64> {
    let (mut bytes, mut size) = (0u64, size_of::<u64>());
    // SAFETY: hw.memsize is a u64, written into one of the size given
    let ok = unsafe { libc::sysctlbyname(c"hw.memsize".as_ptr(), (&raw mut bytes).cast(), &mut size, std::ptr::null_mut(), 0) } == 0;
    ok.then_some(bytes)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn read() -> Option<(f64, Option<u64>)> {
    None
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn total_memory() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_its_own_load_a_second_apart() {
        let mut usage = Usage::new();
        usage.sample(Some(60.0));
        assert_eq!(usage.cpu, None, "one reading says nothing about a rate");
        // a little work, and a second gone by
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(50) {
            std::hint::black_box(start.elapsed());
        }
        usage.last = usage.last.map(|(at, s)| (at - EVERY, s));
        usage.sample(Some(59.0));
        assert!(usage.cpu.is_some_and(|c| (0.0..=100.0).contains(&c)), "{:?}", usage.cpu);
        assert_eq!(usage.trails.fps, [60.0, 59.0]);
        assert_eq!(usage.trails.cpu.len(), 1, "a share once there's a rate");
        if cfg!(any(windows, target_os = "linux")) {
            assert!(usage.memory.is_some_and(|m| m > 1 << 20), "{:?}", usage.memory);
            assert!(usage.memory_total > usage.memory, "{:?} of {:?}", usage.memory, usage.memory_total);
            assert!(usage.trails.memory.iter().all(|s| (0.0..=1.0).contains(s)));
        }
    }

    #[test]
    fn a_trail_keeps_half_a_minute() {
        let mut trail = VecDeque::new();
        for i in 0..TRAIL + 5 {
            push(&mut trail, Some(i as f32));
        }
        push(&mut trail, None);
        assert_eq!(trail.len(), TRAIL);
        assert_eq!(trail.front(), Some(&5.0), "the oldest go");
        assert_eq!(share(Some(1), Some(4)), Some(0.25));
        assert_eq!(share(Some(1), Some(0)), None, "no whole, no share");
    }
}
