//! One app's audio (docs/audio.md §1, "only Spotify"): on macOS 14.2+, a Core Audio process tap
//! on the app's processes (its helpers too: a browser plays from one) in a private aggregate
//! device, which opens like any input; on Windows 11 (10 from build 20348), a WASAPI process
//! loopback on the app's outermost process and its children, streamed by a thread of its own.
//! The app keeps playing out loud. Linux not yet.

/// An app that uses audio: what it's saved as, what it's shown as, whether it's playing.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    /// Its bundle on disk ("/Applications/Spotify.app"), on Windows its executable: every
    /// process of it is the app's (a browser's helpers too).
    pub id: String,
    pub name: String,
    pub playing: bool,
}

/// The app an executable belongs to: its outermost `.app` bundle, if that's an application's
/// (in an Applications folder; not a daemon, not a system service). On Windows the executable
/// itself, unless it's the system's (`C:\Windows\…`).
pub fn app_of(executable: &str) -> Option<&str> {
    if executable.contains('\\') {
        return (!executable.get(1..11).is_some_and(|p| p.eq_ignore_ascii_case(r":\Windows\"))).then_some(executable);
    }
    let root = &executable[..executable.find(".app/").map(|i| i + 4).unwrap_or(0)];
    root.contains("/Applications/").then_some(root)
}

/// Apps by their processes: (executable, playing) each, folded into their app; playing ones
/// first, then by name.
pub fn group(processes: impl IntoIterator<Item = (String, bool)>) -> Vec<App> {
    let mut apps: Vec<App> = Vec::new();
    for (executable, playing) in processes {
        let Some(id) = app_of(&executable) else { continue };
        match apps.iter_mut().find(|a| a.id == id) {
            Some(app) => app.playing |= playing,
            None => {
                // its file without the extension, with either platform's separators
                let file = id.rsplit(['/', '\\']).next().unwrap_or(id);
                let name = file.rsplit_once('.').map_or(file, |(stem, _)| stem).to_string();
                apps.push(App { id: id.to_string(), name, playing });
            }
        }
    }
    apps.sort_by(|a, b| b.playing.cmp(&a.playing).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    apps
}

#[cfg(target_os = "macos")]
pub use mac::{Tap, apps, processes};

#[cfg(windows)]
pub use win::{Tap, apps, processes};

#[cfg(not(any(target_os = "macos", windows)))]
pub use elsewhere::{Tap, apps, processes};

#[cfg(target_os = "macos")]
mod mac {
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicU32, Ordering};

    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_core_audio::{
        AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
        AudioObjectPropertyAddress, CATapDescription, CATapMuteBehavior, kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceNameKey, kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey,
        kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioProcessPropertyIsRunningOutput, kAudioProcessPropertyPID, kAudioSubTapDriftCompensationKey,
        kAudioSubTapUIDKey,
    };
    use objc2_core_foundation::CFDictionary;
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

    use super::{App, app_of, group};

    fn address(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress { mSelector: selector, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain }
    }

    /// A property of a fixed size.
    fn get<T: Copy + Default>(object: AudioObjectID, selector: u32) -> Option<T> {
        let (mut value, mut size) = (T::default(), size_of::<T>() as u32);
        let status = unsafe { AudioObjectGetPropertyData(object, NonNull::from(&address(selector)), 0, std::ptr::null(), NonNull::from(&mut size), NonNull::from(&mut value).cast()) };
        (status == 0).then_some(value)
    }

    /// The process objects Core Audio knows (each process that opened audio).
    fn objects() -> Vec<AudioObjectID> {
        let system = kAudioObjectSystemObject as AudioObjectID;
        let mut size = 0u32;
        let a = address(kAudioHardwarePropertyProcessObjectList);
        if unsafe { AudioObjectGetPropertyDataSize(system, NonNull::from(&a), 0, std::ptr::null(), NonNull::from(&mut size)) } != 0 {
            return Vec::new();
        }
        let mut list = vec![0 as AudioObjectID; size as usize / size_of::<AudioObjectID>()];
        if list.is_empty() || unsafe { AudioObjectGetPropertyData(system, NonNull::from(&a), 0, std::ptr::null(), NonNull::from(&mut size), NonNull::new(list.as_mut_ptr()).unwrap().cast()) } != 0 {
            return Vec::new();
        }
        list.truncate(size as usize / size_of::<AudioObjectID>());
        list
    }

    /// A process' executable ("/Applications/Spotify.app/Contents/MacOS/Spotify").
    fn executable(pid: i32) -> String {
        let mut buffer = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let n = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        String::from_utf8_lossy(&buffer[..n.max(0) as usize]).into_owned()
    }

    /// Every process object with its executable and whether it plays now; not ours.
    fn described() -> Vec<(AudioObjectID, String, bool)> {
        let ours = std::process::id() as i32;
        objects()
            .into_iter()
            .filter_map(|o| {
                let pid: i32 = get(o, kAudioProcessPropertyPID).filter(|&p| p != ours)?;
                let playing = get::<u32>(o, kAudioProcessPropertyIsRunningOutput).is_some_and(|v| v != 0);
                Some((o, executable(pid), playing))
            })
            .collect()
    }

    /// The apps that use audio now (macOS 14.2+; earlier, none).
    pub fn apps() -> Vec<App> {
        group(described().into_iter().map(|(_, executable, playing)| (executable, playing)))
    }

    /// The process objects of app `id`, its helpers included.
    pub fn processes(id: &str) -> Vec<AudioObjectID> {
        described().into_iter().filter(|p| app_of(&p.1) == Some(id)).map(|p| p.0).collect()
    }

    static TAPS: AtomicU32 = AtomicU32::new(0);

    /// A tap on an app's processes, in a private aggregate device named [`Self::device`]: open
    /// that device as an input. Dropping it takes both down.
    pub struct Tap {
        pub device: String,
        /// The process objects tapped (an app started again has new ones).
        pub processes: Vec<AudioObjectID>,
        tap: AudioObjectID,
        aggregate: AudioObjectID,
    }

    impl Tap {
        pub fn new(id: &str, name: &str) -> Result<Self, String> {
            let processes = processes(id);
            if processes.is_empty() {
                return Err(format!("{name} isn't open, or hasn't played anything yet"));
            }
            let n = TAPS.fetch_add(1, Ordering::Relaxed);
            let device = format!("Velo · {name} {}.{n}", std::process::id());
            let numbers: Vec<Retained<NSNumber>> = processes.iter().map(|&p| NSNumber::new_u32(p)).collect();
            let list = NSArray::from_retained_slice(&numbers);
            let description = unsafe { CATapDescription::initStereoMixdownOfProcesses(CATapDescription::alloc(), &list) };
            unsafe {
                // the app still plays out loud; nobody else sees the tap
                description.setMuteBehavior(CATapMuteBehavior::Unmuted);
                description.setPrivate(true);
                description.setName(&NSString::from_str(&device));
            }
            let mut tap = 0;
            let status = unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap) };
            if status != 0 {
                return Err(format!("can't listen to {name} (Core Audio error {status}; macOS 14.2 or later, and Velo allowed to record audio)"));
            }
            // the aggregate device: the tap as its input, started with it, only ours
            let key = |k: &std::ffi::CStr| NSString::from_str(k.to_str().unwrap_or_default());
            let uid = unsafe { description.UUID().UUIDString() };
            let yes = NSNumber::new_bool(true);
            let sub = NSDictionary::<NSString, AnyObject>::from_slices(&[&*key(kAudioSubTapUIDKey), &*key(kAudioSubTapDriftCompensationKey)], &[uid.as_ref(), yes.as_ref()]);
            let taps = NSArray::from_retained_slice(&[sub]);
            let (dev_name, dev_uid) = (NSString::from_str(&device), NSString::from_str(&format!("app.velo.tap.{}.{n}", std::process::id())));
            let keys = [kAudioAggregateDeviceNameKey, kAudioAggregateDeviceUIDKey, kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceIsPrivateKey].map(key);
            let values: [&AnyObject; 5] = [dev_name.as_ref(), dev_uid.as_ref(), taps.as_ref(), yes.as_ref(), yes.as_ref()];
            let properties = NSDictionary::<NSString, AnyObject>::from_slices(&keys.each_ref().map(|k| &**k), &values);
            // NSDictionary and CFDictionary are the same object (toll-free bridged)
            let properties = unsafe { &*(Retained::as_ptr(&properties) as *const CFDictionary) };
            let mut aggregate = 0;
            let status = unsafe { AudioHardwareCreateAggregateDevice(properties, NonNull::from(&mut aggregate)) };
            if status != 0 {
                unsafe { AudioHardwareDestroyProcessTap(tap) };
                return Err(format!("can't listen to {name} (Core Audio error {status} making its device)"));
            }
            Ok(Self { device, processes, tap, aggregate })
        }
    }

    impl Drop for Tap {
        fn drop(&mut self) {
            unsafe {
                AudioHardwareDestroyAggregateDevice(self.aggregate);
                AudioHardwareDestroyProcessTap(self.tap);
            }
        }
    }
}

#[cfg(windows)]
mod win {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::Foundation::{CloseHandle, ERROR_TIMEOUT, HANDLE, S_OK, WAIT_OBJECT_0};
    use windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS,
        AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync, AudioSessionStateActive, AudioSessionStateExpired, DEVICE_STATE_ACTIVE,
        IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        MMDeviceEnumerator, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX, eRender,
    };
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    use windows::Win32::System::Threading::{CreateEventW, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, WaitForSingleObject};
    use windows::Win32::System::Variant::VT_BLOB;
    use windows::core::{Error, HRESULT, IUnknown, Interface, PWSTR, Ref, implement};

    use super::{App, app_of, group};
    use crate::capture::{Capture, pick};

    /// What a tap hears: 48 kHz, stereo floats (the client has no format of its own to ask;
    /// Windows converts the app's to this).
    const RATE: u32 = 48_000;
    const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

    /// Process loopback came with Windows 10 build 20348; Windows 11 has it.
    fn supported() -> bool {
        let mut version = OSVERSIONINFOW { dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
        let _ = unsafe { RtlGetVersion(&mut version) };
        version.dwBuildNumber >= 20348
    }

    /// COM on the thread asking, once (a thread with COM up already, cpal's, keeps it).
    fn com() {
        thread_local! {
            static COM: () = { let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }; };
        }
        COM.with(|_| {});
    }

    /// Every audio session's process, on every output, and whether it plays now; not the
    /// system's sounds.
    fn sessions() -> Vec<(u32, bool)> {
        com();
        let mut found = Vec::new();
        unsafe {
            let Ok(devices) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return found };
            let Ok(outputs) = devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return found };
            for i in 0..outputs.GetCount().unwrap_or(0) {
                let Ok(list) = outputs.Item(i).and_then(|d| d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)).and_then(|m| m.GetSessionEnumerator()) else { continue };
                for k in 0..list.GetCount().unwrap_or(0) {
                    let Ok(session) = list.GetSession(k).and_then(|s| s.cast::<IAudioSessionControl2>()) else { continue };
                    let (Ok(state), Ok(pid)) = (session.GetState(), session.GetProcessId()) else { continue };
                    if state != AudioSessionStateExpired && pid != 0 && session.IsSystemSoundsSession() != S_OK {
                        found.push((pid, state == AudioSessionStateActive));
                    }
                }
            }
        }
        found
    }

    /// A process' executable ("C:\Program Files\Spotify\Spotify.exe"), empty if it isn't ours to see.
    fn executable(pid: u32) -> String {
        let mut path = [0u16; 1024];
        let mut size = path.len() as u32;
        unsafe {
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
            let named = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut size);
            let _ = CloseHandle(process);
            if named.is_err() {
                return String::new();
            }
        }
        String::from_utf16_lossy(&path[..size as usize])
    }

    /// Every process with an audio session: its id, its executable, whether it plays now; not ours.
    fn described() -> Vec<(u32, String, bool)> {
        let ours = std::process::id();
        sessions().into_iter().filter(|p| p.0 != ours).map(|(pid, playing)| (pid, executable(pid), playing)).collect()
    }

    /// The apps that use audio now (Windows 10 build 20348 and later; earlier, none).
    pub fn apps() -> Vec<App> {
        if !supported() {
            return Vec::new();
        }
        group(described().into_iter().map(|(_, executable, playing)| (executable, playing)))
    }

    /// The processes app `id` plays from, each as the outermost one of its executable (a
    /// browser's, whose helpers play): the trees a tap takes.
    pub fn processes(id: &str) -> Vec<u32> {
        let family = family();
        let mut roots: Vec<u32> = described().into_iter().filter(|p| app_of(&p.1) == Some(id)).map(|p| root(p.0, &family)).collect();
        roots.sort_unstable();
        roots.dedup();
        roots
    }

    /// Each process' parent and its executable's file name, from a snapshot of them all.
    fn family() -> HashMap<u32, (u32, String)> {
        let mut all = HashMap::new();
        unsafe {
            let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return all };
            let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
            while more {
                let file = &entry.szExeFile[..entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len())];
                all.insert(entry.th32ProcessID, (entry.th32ParentProcessID, String::from_utf16_lossy(file)));
                more = Process32NextW(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
        }
        all
    }

    /// `pid`'s outermost ancestor running the same executable.
    fn root(mut pid: u32, family: &HashMap<u32, (u32, String)>) -> u32 {
        // a few steps at most: a dead parent's id can be a younger process' now
        for _ in 0..8 {
            let Some((parent, file)) = family.get(&pid) else { break };
            if *parent == pid || !family.get(parent).is_some_and(|(_, up)| up.eq_ignore_ascii_case(file)) {
                break;
            }
            pid = *parent;
        }
        pid
    }

    /// A tap on an app's processes and their children: [`Self::capture`] streams it. Dropping
    /// it stops the stream.
    pub struct Tap {
        /// None: the tap streams by itself, there's no device to open.
        pub device: String,
        /// The outermost processes found (an app started again has new ones).
        pub processes: Vec<u32>,
        stop: Arc<AtomicBool>,
    }

    impl Tap {
        pub fn new(id: &str, name: &str) -> Result<Self, String> {
            if !supported() {
                return Err(format!("listening to {name} alone needs Windows 11 (or Windows 10 build 20348)"));
            }
            let processes = processes(id);
            if processes.is_empty() {
                return Err(format!("{name} isn't open, or hasn't played anything yet"));
            }
            Ok(Self { device: String::new(), processes, stop: Arc::default() })
        }

        /// The tap as an open input named `name`: a thread of its own pushes what the app plays
        /// into its rings, as a device's callback would.
        pub fn capture(self, name: &str) -> Result<Capture, String> {
            // shortcut: an app run as two separate trees of processes is heard from the first;
            // both, mixed, when someone has one
            let pid = self.processes[0];
            // a second of audio: if analysis stalls, the newest audio is dropped, nothing waits
            let (tx, samples) = rtrb::RingBuffer::new(RATE as usize);
            let (both, stereo) = rtrb::RingBuffer::new(RATE as usize);
            let (broken, (ready, started)) = (Arc::new(AtomicBool::new(false)), mpsc::channel());
            let (stop, gone) = (self.stop.clone(), broken.clone());
            std::thread::Builder::new().name("velo app tap".into()).spawn(move || stream(pid, (tx, both), &ready, &stop, &gone)).map_err(|e| e.to_string())?;
            match started.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(())) => Ok(Capture { stream: None, samples, stereo, rate: RATE as f32, channels: 2, name: name.to_string(), tap: Some(self), broken }),
                Ok(Err(e)) => Err(format!("can't listen to {name} ({e})")),
                Err(_) => Err(format!("can't listen to {name} (Windows didn't answer)")),
            }
        }
    }

    impl Drop for Tap {
        fn drop(&mut self) {
            self.stop.store(true, Relaxed);
        }
    }

    /// Process `pid`'s and its children's sound into `tx` (mono) and `both` (stereo) until
    /// `stop`; `ready` says whether it started, `broken` when it can't go on.
    fn stream(pid: u32, (mut tx, mut both): (rtrb::Producer<f32>, rtrb::Producer<[f32; 2]>), ready: &mpsc::Sender<Result<(), String>>, stop: &AtomicBool, broken: &AtomicBool) {
        com();
        let (client, capture, event) = match open(pid) {
            Ok(opened) => opened,
            Err(e) => {
                let _ = ready.send(Err(e.to_string()));
                return;
            }
        };
        let _ = ready.send(Ok(()));
        while !stop.load(Relaxed) {
            // a packet every 10 ms or so; the limit only lets `stop` be seen
            if unsafe { WaitForSingleObject(event, 100) } == WAIT_OBJECT_0 && read(&capture, &mut tx, &mut both).is_err() {
                broken.store(true, Relaxed);
                break;
            }
        }
        unsafe {
            let _ = client.Stop();
            let _ = CloseHandle(event);
        }
    }

    /// The packets waiting, pushed as mono and as left and right; silence where Windows says so.
    fn read(capture: &IAudioCaptureClient, tx: &mut rtrb::Producer<f32>, both: &mut rtrb::Producer<[f32; 2]>) -> windows::core::Result<()> {
        unsafe {
            while capture.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0, 0);
                capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let silent = data.is_null() || flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                for i in 0..frames as usize {
                    let frame = if silent { [0.0; 2] } else { *data.cast::<[f32; 2]>().add(i) };
                    let (mono, stereo) = pick(&frame, (0, 1));
                    // full: analysis is behind; drop rather than wait
                    let _ = tx.push(mono);
                    let _ = both.push(stereo);
                }
                capture.ReleaseBuffer(frames)?;
            }
        }
        Ok(())
    }

    /// Says when Windows has made the client (on a thread of its own).
    #[implement(IActivateAudioInterfaceCompletionHandler)]
    struct Activated(mpsc::Sender<()>);

    impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
        fn ActivateCompleted(&self, _: Ref<'_, IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
            let _ = self.0.send(());
            Ok(())
        }
    }

    /// A loopback client on process `pid` and its children, started; each packet is signalled
    /// on the event.
    fn open(pid: u32) -> windows::core::Result<(IAudioClient, IAudioCaptureClient, HANDLE)> {
        let params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 { ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS { TargetProcessId: pid, ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE } },
        };
        let mut blob = PROPVARIANT::default();
        let format = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_IEEE_FLOAT, nChannels: 2, nSamplesPerSec: RATE, nAvgBytesPerSec: RATE * 8, nBlockAlign: 8, wBitsPerSample: 32, cbSize: 0 };
        unsafe {
            let v = &mut *blob.Anonymous.Anonymous;
            v.vt = VT_BLOB;
            v.Anonymous.blob = BLOB { cbSize: size_of_val(&params) as u32, pBlobData: (&raw const params).cast_mut().cast() };
            let (done, finished) = mpsc::channel();
            let handler: IActivateAudioInterfaceCompletionHandler = Activated(done).into();
            let operation = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&raw const blob), &handler)?;
            finished.recv_timeout(Duration::from_secs(5)).map_err(|_| Error::from(ERROR_TIMEOUT.to_hresult()))?;
            let (mut result, mut activated) = (HRESULT::default(), None::<IUnknown>);
            operation.GetActivateResult(&mut result, &mut activated)?;
            result.ok()?;
            let client: IAudioClient = activated.ok_or_else(Error::empty)?.cast()?;
            let flags = AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            // 20 ms of room, as Microsoft's sample asks; the packets come each engine period
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 200_000, 0, &format, None)?;
            let event = CreateEventW(None, false, false, None)?;
            client.SetEventHandle(event)?;
            let capture: IAudioCaptureClient = client.GetService()?;
            client.Start()?;
            Ok((client, capture, event))
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod elsewhere {
    use super::App;

    /// One app's audio needs macOS or Windows for now (PipeWire comes later).
    pub fn apps() -> Vec<App> {
        Vec::new()
    }

    pub fn processes(_: &str) -> Vec<u32> {
        Vec::new()
    }

    pub struct Tap {
        pub device: String,
        pub processes: Vec<u32>,
    }

    impl Tap {
        pub fn new(_: &str, _: &str) -> Result<Self, String> {
            Err("listening to one app works on macOS and Windows for now".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_fold_into_their_app_and_daemons_stay_out() {
        let chrome = "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Versions/1/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper";
        let apps = group([
            (chrome.to_string(), true),
            ("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string(), false),
            ("/Applications/Spotify.app/Contents/MacOS/Spotify".to_string(), false),
            ("/System/Applications/Music.app/Contents/MacOS/Music".to_string(), false),
            ("/usr/libexec/audiomxd".to_string(), true),
            ("/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter".to_string(), false),
        ]);
        let shown: Vec<(&str, bool)> = apps.iter().map(|a| (a.name.as_str(), a.playing)).collect();
        assert_eq!(shown, [("Google Chrome", true), ("Music", false), ("Spotify", false)], "playing first, then by name");
        assert_eq!(app_of(chrome), Some("/Applications/Google Chrome.app"));
        assert_eq!(app_of("/usr/libexec/audiomxd"), None);
    }

    #[test]
    fn on_windows_an_app_is_its_executable_and_the_systems_stay_out() {
        let chrome = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
        let apps = group([
            (r"C:\Users\dj\AppData\Roaming\Spotify\Spotify.exe".to_string(), false),
            (chrome.to_string(), false),
            (chrome.to_string(), true),
            (r"C:\WINDOWS\explorer.exe".to_string(), true),
            (r"C:\Windows\System32\svchost.exe".to_string(), true),
            // a process Velo may not look into
            (String::new(), true),
        ]);
        let shown: Vec<(&str, bool)> = apps.iter().map(|a| (a.name.as_str(), a.playing)).collect();
        assert_eq!(shown, [("chrome", true), ("Spotify", false)], "playing first, then by name");
        assert_eq!(app_of(chrome), Some(chrome));
    }

    #[test]
    #[cfg(any(target_os = "macos", windows))]
    fn the_apps_list_never_fails() {
        // whatever runs on this machine: each app named, none of them us
        for app in apps() {
            assert!(!app.name.is_empty() && !app.id.is_empty(), "{app:?}");
        }
        assert!(Tap::new("com.example.nothing-here", "Nothing").is_err());
    }
}
