//! The program shared with other apps: on this computer through Spout (Windows: a DX11 texture
//! other apps open by its handle, found by name in Spout's shared memory, as the Spout SDK does
//! it), or on the network through NDI (from the runtime the user installed, never bundled). The
//! render thread reads the output back full size; a thread of its own sends each frame.

/// Where the program goes besides the outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Share {
    Spout,
    Ndi,
}

impl Share {
    /// Whether this computer can share this way at all (Spout is Windows only).
    pub fn available(self) -> bool {
        match self {
            Share::Spout => cfg!(windows),
            Share::Ndi => true,
        }
    }
}

/// What sends frames: BGRA8 pixels, `stride` bytes a row.
pub trait Sender: Send {
    fn send(&mut self, pixels: &[u8], size: [u32; 2], stride: u32) -> Result<(), String>;
}

/// A sender called `name` (other apps list it by that name).
pub fn open(kind: Share, name: &str) -> Result<Box<dyn Sender>, String> {
    match kind {
        #[cfg(windows)]
        Share::Spout => Ok(Box::new(spout::Spout::new(name))),
        #[cfg(not(windows))]
        Share::Spout => Err("Spout is Windows only".into()),
        Share::Ndi => ndi::Ndi::new(name).map(|n| Box::new(n) as Box<dyn Sender>),
    }
}

#[cfg(windows)]
pub mod spout {
    //! Spout 2's protocol (leadedge/Spout2, SpoutSenderNames): "SpoutSenderNames" holds the
    //! senders' names, 256 bytes each, sorted, ended by an empty one; a map named after each
    //! sender holds its texture's share handle, size and format; "ActiveSenderName" the one a
    //! receiver picks by default. Each map has a mutex, its name and "_mutex".

    use windows::Win32::Foundation::{CloseHandle, HANDLE, HMODULE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_RESOURCE_MISC_SHARED, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
        ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::IDXGIResource;
    use windows::Win32::System::Memory::{CreateFileMappingA, FILE_MAP_ALL_ACCESS, MEMORY_BASIC_INFORMATION, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile, VirtualQuery};
    use windows::Win32::System::Threading::{CreateMutexA, ReleaseMutex, WaitForSingleObject};
    use windows::core::{Interface, PCSTR};

    /// Spout's longest name (with its nul), and how many senders it lists by default.
    const NAME: usize = 256;
    const SENDERS: usize = 64;
    /// SharedTextureInfo: handle, width, height, format, usage, a 256-byte description, partner id.
    const INFO: usize = 280;

    /// One of Spout's shared memory maps, with its mutex.
    pub struct Map {
        file: HANDLE,
        view: MEMORY_MAPPED_VIEW_ADDRESS,
        mutex: HANDLE,
        len: usize,
    }

    impl Map {
        /// Opens the map `name` (or makes it, `size` bytes, if nobody has).
        pub fn new(name: &str, size: usize) -> Result<Self, String> {
            let cname = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
            let mutex_name = std::ffi::CString::new(format!("{name}_mutex")).map_err(|e| e.to_string())?;
            // SAFETY: plain Win32 calls on names that live through them; every handle is closed in Drop
            unsafe {
                let file = CreateFileMappingA(INVALID_HANDLE_VALUE, None, PAGE_READWRITE, 0, size as u32, PCSTR(cname.as_ptr().cast())).map_err(|e| format!("{name}: {e}"))?;
                let view = MapViewOfFile(file, FILE_MAP_ALL_ACCESS, 0, 0, 0);
                if view.Value.is_null() {
                    let _ = CloseHandle(file);
                    return Err(format!("{name}: can't map it"));
                }
                // made by another app first, it may be smaller than we'd make it
                let mut region = MEMORY_BASIC_INFORMATION::default();
                VirtualQuery(Some(view.Value), &mut region, std::mem::size_of::<MEMORY_BASIC_INFORMATION>());
                let mutex = CreateMutexA(None, false, PCSTR(mutex_name.as_ptr().cast())).map_err(|e| format!("{name}: {e}"))?;
                Ok(Self { file, view, mutex, len: region.RegionSize })
            }
        }

        /// Its bytes while `f` runs, with its mutex held (as Spout waits: 67 ms at most).
        pub fn with<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> Option<R> {
            // SAFETY: the view is mapped for `len` bytes until Drop; the mutex guards it
            unsafe {
                if WaitForSingleObject(self.mutex, 67) != WAIT_OBJECT_0 {
                    return None;
                }
                let out = f(std::slice::from_raw_parts_mut(self.view.Value.cast::<u8>(), self.len));
                let _ = ReleaseMutex(self.mutex);
                Some(out)
            }
        }
    }

    impl Drop for Map {
        fn drop(&mut self) {
            // SAFETY: made in new, closed once here
            unsafe {
                let _ = UnmapViewOfFile(self.view);
                let _ = CloseHandle(self.file);
                let _ = CloseHandle(self.mutex);
            }
        }
    }

    // SAFETY: the handles and the view belong to this process and are only used through &self
    // with the mutex held, or dropped
    unsafe impl Send for Map {}

    /// The names in "SpoutSenderNames".
    pub fn names(buffer: &[u8]) -> Vec<String> {
        buffer.chunks(NAME).take(SENDERS).map(|n| n.split(|b| *b == 0).next().unwrap_or_default()).take_while(|n| !n.is_empty()).map(|n| String::from_utf8_lossy(n).into_owned()).collect()
    }

    /// `names` back into "SpoutSenderNames", sorted as Spout keeps them, an empty one after.
    pub fn write_names(buffer: &mut [u8], names: &[String]) {
        let mut names: Vec<&String> = names.iter().collect();
        names.sort();
        names.dedup();
        let room = (buffer.len() / NAME).min(SENDERS);
        for (i, slot) in buffer.chunks_mut(NAME).take(room).enumerate() {
            slot.fill(0);
            if let Some(name) = names.get(i) {
                let bytes = &name.as_bytes()[..name.len().min(NAME - 1)];
                slot[..bytes.len()].copy_from_slice(bytes);
            }
        }
    }

    pub struct Spout {
        name: String,
        device: Option<(ID3D11Device, ID3D11DeviceContext)>,
        texture: Option<(ID3D11Texture2D, [u32; 2])>,
        names: Option<Map>,
        info: Option<Map>,
        active: Option<Map>,
    }

    // SAFETY: the DX11 device, its context and texture are made and used on the one thread the
    // sender is sent to
    unsafe impl Send for Spout {}

    impl Spout {
        pub fn new(name: &str) -> Self {
            Self { name: name.to_string(), device: None, texture: None, names: None, info: None, active: None }
        }

        /// A DX11 device of our own, its texture shared by handle at `size`, and our name and its
        /// details where receivers look.
        fn start(&mut self, size: [u32; 2]) -> Result<(), String> {
            if self.device.is_none() {
                let (mut device, mut context) = (None, None);
                // SAFETY: out pointers to locals
                unsafe { D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)) }
                    .map_err(|e| format!("DirectX 11: {e}"))?;
                self.device = device.zip(context);
            }
            let Some((device, _)) = &self.device else { return Err("DirectX 11: no device".into()) };
            let desc = D3D11_TEXTURE2D_DESC {
                Width: size[0],
                Height: size[1],
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32,
            };
            let mut texture = None;
            // SAFETY: a described texture, no initial data
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.map_err(|e| format!("DirectX 11: {e}"))?;
            let texture = texture.ok_or("DirectX 11: no texture")?;
            // SAFETY: a shared texture has a DXGI resource and its legacy share handle
            let handle = unsafe { texture.cast::<IDXGIResource>().and_then(|r| r.GetSharedHandle()) }.map_err(|e| format!("DirectX 11: {e}"))?;
            if self.names.is_none() {
                let names = Map::new("SpoutSenderNames", SENDERS * NAME)?;
                // our name, or name_1, name_2… if it's taken
                let wanted = self.name.clone();
                let mine = names
                    .with(|b| {
                        let mut list = self::names(b);
                        let mine = std::iter::once(wanted.clone()).chain((1..).map(|i| format!("{wanted}_{i}"))).find(|n| !list.contains(n)).unwrap_or_default();
                        list.push(mine.clone());
                        write_names(b, &list);
                        mine
                    })
                    .ok_or("Spout's list of senders is busy")?;
                self.name = mine;
                self.names = Some(names);
                self.info = Some(Map::new(&self.name, INFO)?);
                let active = Map::new("ActiveSenderName", NAME)?;
                let name = self.name.clone();
                active.with(|b| {
                    let n = name.len().min(NAME - 1);
                    let all = NAME.min(b.len());
                    b[..all].fill(0);
                    b[..n].copy_from_slice(&name.as_bytes()[..n]);
                });
                self.active = Some(active);
            }
            // the texture's details: handle (as 32 bits, as Spout keeps it), size, format, usage,
            // then the app's path
            let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
            if let Some(info) = &self.info {
                info.with(|b| {
                    let all = INFO.min(b.len());
                    b[..all].fill(0);
                    let fields = [handle.0 as usize as u32, size[0], size[1], DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32, 0];
                    for (i, v) in fields.iter().enumerate() {
                        b[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
                    }
                    let path = &exe.as_bytes()[..exe.len().min(255)];
                    b[20..20 + path.len()].copy_from_slice(path);
                });
            }
            self.texture = Some((texture, size));
            Ok(())
        }
    }

    impl super::Sender for Spout {
        fn send(&mut self, pixels: &[u8], size: [u32; 2], stride: u32) -> Result<(), String> {
            if self.texture.as_ref().is_none_or(|(_, s)| *s != size) {
                self.start(size)?;
            }
            if pixels.len() < (stride * size[1]) as usize {
                return Ok(());
            }
            if let (Some((_, context)), Some((texture, _))) = (&self.device, &self.texture) {
                // SAFETY: the pixels cover `stride` × height bytes, as checked
                unsafe {
                    context.UpdateSubresource(texture, 0, None, pixels.as_ptr().cast(), stride, 0);
                    context.Flush();
                }
            }
            Ok(())
        }
    }

    impl Drop for Spout {
        fn drop(&mut self) {
            // out of the list, and no longer the active one
            let name = self.name.clone();
            if let Some(names) = &self.names {
                names.with(|b| {
                    let list: Vec<String> = self::names(b).into_iter().filter(|n| *n != name).collect();
                    write_names(b, &list);
                });
            }
            if let Some(active) = &self.active {
                active.with(|b| {
                    if b.starts_with(name.as_bytes()) && b.get(name.len()) == Some(&0) {
                        b.fill(0);
                    }
                });
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn names_are_kept_sorted_and_ended_as_spout_keeps_them() {
            let mut buffer = vec![0u8; SENDERS * NAME];
            write_names(&mut buffer, &["Velo".into(), "Arena".into()]);
            assert_eq!(names(&buffer), ["Arena", "Velo"]);
            assert_eq!(buffer[2 * NAME], 0, "an empty name ends the list");
            write_names(&mut buffer, &["Arena".into()]);
            assert_eq!(names(&buffer), ["Arena"]);
        }

        /// A running Velo sharing through Spout (`VELO_DEMO_SHARE=spout`): listed as Velo, its
        /// picture not black. Run by hand (`--ignored`) while it runs.
        #[test]
        #[ignore]
        fn a_running_velo_is_listed_with_its_picture() {
            use windows::Win32::Graphics::Direct3D11::{D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_STAGING};
            let list = Map::new("SpoutSenderNames", SENDERS * NAME).unwrap();
            let all = list.with(|b| names(b)).unwrap();
            assert!(all.contains(&"Velo".to_string()), "senders: {all:?}");
            let info = Map::new("Velo", INFO).unwrap();
            let f: Vec<u32> = info.with(|b| (0..3).map(|i| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())).collect()).unwrap();
            let (mut device, mut context) = (None, None);
            unsafe { D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)) }.unwrap();
            let (device, context) = (device.unwrap(), context.unwrap());
            let mut shared: Option<ID3D11Texture2D> = None;
            unsafe { device.OpenSharedResource(HANDLE(f[0] as usize as *mut std::ffi::c_void), &mut shared) }.unwrap();
            let desc = D3D11_TEXTURE2D_DESC {
                Width: f[1],
                Height: f[2],
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut staging = None;
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }.unwrap();
            let staging = staging.unwrap();
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            let lit = unsafe {
                context.CopyResource(&staging, &shared.unwrap());
                context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).unwrap();
                let row = std::slice::from_raw_parts(mapped.pData.cast::<u8>().add((f[2] / 2) as usize * mapped.RowPitch as usize), (f[1] * 4) as usize);
                let lit = row.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30).count();
                context.Unmap(&staging, 0);
                lit
            };
            eprintln!("Velo: {} x {}, {lit} lit pixels across the middle", f[1], f[2]);
            assert!(lit > 0, "its picture isn't black");
        }

        /// What a Spout receiver does: the name in the list, the handle in the sender's map, the
        /// texture opened by it on a device of its own, the pixels sent.
        #[test]
        fn a_receiver_finds_the_sender_and_reads_its_pixels() {
            use super::super::Sender;
            use windows::Win32::Graphics::Direct3D11::{D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_STAGING};
            let size = [64u32, 32];
            let stride = 256u32;
            // a gradient: blue across, green down
            let pixels: Vec<u8> = (0..size[1]).flat_map(|y| (0..stride / 4).flat_map(move |x| [(x * 4) as u8, (y * 8) as u8, 200, 255])).collect();
            let mut sender = Spout::new("Velo test");
            if let Err(e) = sender.send(&pixels, size, stride) {
                eprintln!("no DirectX 11 here: skipped ({e})");
                return;
            }
            let name = sender.name.clone();
            let list = Map::new("SpoutSenderNames", SENDERS * NAME).unwrap();
            assert!(list.with(|b| names(b)).unwrap().contains(&name), "{name} is listed");
            let info = Map::new(&name, INFO).unwrap();
            let fields: Vec<u32> = info.with(|b| (0..4).map(|i| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())).collect()).unwrap();
            assert_eq!((fields[1], fields[2], fields[3]), (64, 32, DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32));
            // the receiver's own device opens the texture by its handle and copies it out
            let (mut device, mut context) = (None, None);
            unsafe { D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)) }.unwrap();
            let (device, context) = (device.unwrap(), context.unwrap());
            let mut shared: Option<ID3D11Texture2D> = None;
            unsafe { device.OpenSharedResource(HANDLE(fields[0] as usize as *mut std::ffi::c_void), &mut shared) }.unwrap();
            let shared = shared.unwrap();
            let desc = D3D11_TEXTURE2D_DESC {
                Width: 64,
                Height: 32,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut staging = None;
            unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }.unwrap();
            let staging = staging.unwrap();
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            let read = unsafe {
                context.CopyResource(&staging, &shared);
                context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).unwrap();
                let row = |y: usize| std::slice::from_raw_parts(mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize), 64 * 4).to_vec();
                let rows = (row(0), row(31));
                context.Unmap(&staging, 0);
                rows
            };
            assert_eq!(&read.0[..8], &pixels[..8], "the first pixels");
            assert_eq!(&read.1[40..48], &pixels[31 * 256 + 40..31 * 256 + 48], "a pixel of the last row");
            // gone once it's dropped
            drop(sender);
            assert!(!list.with(|b| names(b)).unwrap().contains(&name));
        }
    }
}

pub mod ndi {
    //! NDI from the runtime NDI Tools installs (Processing.NDI.Lib.x64.dll on Windows, libndi
    //! elsewhere), loaded when sharing starts: a sender, and a frame of BGRA each time.

    use std::ffi::{CString, c_char, c_void};

    #[repr(C)]
    struct SendCreate {
        name: *const c_char,
        groups: *const c_char,
        clock_video: bool,
        clock_audio: bool,
    }

    #[repr(C)]
    struct VideoFrame {
        xres: i32,
        yres: i32,
        four_cc: u32,
        frame_rate_n: i32,
        frame_rate_d: i32,
        picture_aspect_ratio: f32,
        frame_format_type: i32,
        timecode: i64,
        data: *const u8,
        line_stride_in_bytes: i32,
        metadata: *const c_char,
        timestamp: i64,
    }

    /// 'B' 'G' 'R' 'A', progressive, a timecode NDI makes up.
    const BGRA: u32 = u32::from_le_bytes(*b"BGRA");
    const PROGRESSIVE: i32 = 1;
    const SYNTHESIZE: i64 = i64::MAX;

    type Create = unsafe extern "C" fn(*const SendCreate) -> *mut c_void;
    type SendVideo = unsafe extern "C" fn(*mut c_void, *const VideoFrame);
    type Destroy = unsafe extern "C" fn(*mut c_void);

    pub struct Ndi {
        send: SendVideo,
        destroy: Destroy,
        instance: *mut c_void,
        _name: CString,
    }

    // SAFETY: an NDI sender may be used from any one thread at a time; this one is only ever
    // used by the thread it's sent to
    unsafe impl Send for Ndi {}

    /// Where the runtime is: the folder its installer names, else the system's search. Loaded
    /// once and kept: NDI's own threads outlive its senders, and unloading under them crashes.
    fn library() -> Result<&'static libloading::Library, String> {
        static LOADED: std::sync::OnceLock<libloading::Library> = std::sync::OnceLock::new();
        if let Some(lib) = LOADED.get() {
            return Ok(lib);
        }
        #[cfg(windows)]
        let tries: Vec<std::path::PathBuf> = ["NDI_RUNTIME_DIR_V6", "NDI_RUNTIME_DIR_V5"]
            .iter()
            .filter_map(std::env::var_os)
            .map(std::path::PathBuf::from)
            // where NDI 6 installs, for a Velo started before it was installed
            .chain(std::env::var_os("ProgramFiles").map(|p| std::path::Path::new(&p).join(r"NDI\NDI 6 Runtime\v6")))
            .map(|d| d.join("Processing.NDI.Lib.x64.dll"))
            .chain(std::iter::once("Processing.NDI.Lib.x64.dll".into()))
            .collect();
        #[cfg(target_os = "macos")]
        let tries: Vec<std::path::PathBuf> = vec!["/usr/local/lib/libndi.dylib".into(), "libndi.dylib".into()];
        #[cfg(all(unix, not(target_os = "macos")))]
        let tries: Vec<std::path::PathBuf> = vec!["libndi.so.6".into(), "libndi.so.5".into()];
        // SAFETY: loading the NDI runtime runs its initializers, as any app using NDI does
        let lib = tries.iter().find_map(|p| unsafe { libloading::Library::new(p) }.ok()).ok_or_else(|| "NDI isn't installed: get NDI Tools (or the NDI Runtime) from ndi.video".to_string())?;
        Ok(LOADED.get_or_init(|| lib))
    }

    impl Ndi {
        pub fn new(name: &str) -> Result<Self, String> {
            let lib = library()?;
            let name = CString::new(name).map_err(|e| e.to_string())?;
            // SAFETY: the NDI runtime's own functions, with the signatures of its headers
            unsafe {
                let initialize = *lib.get::<unsafe extern "C" fn() -> bool>(b"NDIlib_initialize\0").map_err(|e| e.to_string())?;
                if !initialize() {
                    return Err("NDI can't run on this processor".into());
                }
                let create = *lib.get::<Create>(b"NDIlib_send_create\0").map_err(|e| e.to_string())?;
                let send = *lib.get::<SendVideo>(b"NDIlib_send_send_video_v2\0").map_err(|e| e.to_string())?;
                let destroy = *lib.get::<Destroy>(b"NDIlib_send_destroy\0").map_err(|e| e.to_string())?;
                let settings = SendCreate { name: name.as_ptr(), groups: std::ptr::null(), clock_video: false, clock_audio: false };
                let instance = create(&settings);
                if instance.is_null() {
                    return Err("NDI didn't make a sender".into());
                }
                Ok(Self { send, destroy, instance, _name: name })
            }
        }
    }

    impl super::Sender for Ndi {
        fn send(&mut self, pixels: &[u8], size: [u32; 2], stride: u32) -> Result<(), String> {
            if pixels.len() < (stride * size[1]) as usize {
                return Ok(());
            }
            let frame = VideoFrame {
                xres: size[0] as i32,
                yres: size[1] as i32,
                four_cc: BGRA,
                frame_rate_n: 60_000,
                frame_rate_d: 1000,
                picture_aspect_ratio: size[0] as f32 / size[1].max(1) as f32,
                frame_format_type: PROGRESSIVE,
                timecode: SYNTHESIZE,
                data: pixels.as_ptr(),
                line_stride_in_bytes: stride as i32,
                metadata: std::ptr::null(),
                timestamp: 0,
            };
            // SAFETY: a live sender and a frame whose pixels outlive the (synchronous) call
            unsafe { (self.send)(self.instance, &frame) };
            Ok(())
        }
    }

    impl Drop for Ndi {
        fn drop(&mut self) {
            // SAFETY: made in new, destroyed once
            unsafe { (self.destroy)(self.instance) };
        }
    }

    #[cfg(test)]
    mod tests {
        use std::ffi::{c_char, c_void};

        /// What an NDI receiver gets from the first source whose name holds `name`: its size and
        /// its middle row, BGRA. None without the runtime, or if nothing comes in 10 s.
        fn receive(name: &str) -> Option<([u32; 2], Vec<u8>)> {
            #[repr(C)]
            struct FindCreate {
                show_local_sources: bool,
                groups: *const c_char,
                extra_ips: *const c_char,
            }
            #[repr(C)]
            #[derive(Clone, Copy)]
            struct Source {
                name: *const c_char,
                address: *const c_char,
            }
            #[repr(C)]
            struct RecvCreate {
                source: Source,
                color_format: i32,
                bandwidth: i32,
                allow_video_fields: bool,
                recv_name: *const c_char,
            }
            const BGRX_BGRA: i32 = 0;
            const HIGHEST: i32 = 100;
            const VIDEO: i32 = 1;
            let lib = super::library().ok()?;
            // SAFETY: the NDI runtime's own functions, with the signatures of its headers
            unsafe {
                let initialize = *lib.get::<unsafe extern "C" fn() -> bool>(b"NDIlib_initialize\0").ok()?;
                let find_create = *lib.get::<unsafe extern "C" fn(*const FindCreate) -> *mut c_void>(b"NDIlib_find_create_v2\0").ok()?;
                let wait = *lib.get::<unsafe extern "C" fn(*mut c_void, u32) -> bool>(b"NDIlib_find_wait_for_sources\0").ok()?;
                let current = *lib.get::<unsafe extern "C" fn(*mut c_void, *mut u32) -> *const Source>(b"NDIlib_find_get_current_sources\0").ok()?;
                let find_destroy = *lib.get::<unsafe extern "C" fn(*mut c_void)>(b"NDIlib_find_destroy\0").ok()?;
                let recv_create = *lib.get::<unsafe extern "C" fn(*const RecvCreate) -> *mut c_void>(b"NDIlib_recv_create_v3\0").ok()?;
                let capture = *lib.get::<unsafe extern "C" fn(*mut c_void, *mut super::VideoFrame, *mut c_void, *mut c_void, u32) -> i32>(b"NDIlib_recv_capture_v2\0").ok()?;
                let free = *lib.get::<unsafe extern "C" fn(*mut c_void, *const super::VideoFrame)>(b"NDIlib_recv_free_video_v2\0").ok()?;
                let recv_destroy = *lib.get::<unsafe extern "C" fn(*mut c_void)>(b"NDIlib_recv_destroy\0").ok()?;
                if !initialize() {
                    return None;
                }
                let finder = find_create(&FindCreate { show_local_sources: true, groups: std::ptr::null(), extra_ips: std::ptr::null() });
                let mut source = None;
                for _ in 0..10 {
                    wait(finder, 1000);
                    let mut count = 0;
                    let all = current(finder, &mut count);
                    let all = if all.is_null() { &[][..] } else { std::slice::from_raw_parts(all, count as usize) };
                    let names: Vec<String> = all.iter().map(|s| std::ffi::CStr::from_ptr(s.name).to_string_lossy().into_owned()).collect();
                    eprintln!("sources: {names:?}");
                    if let Some(i) = names.iter().position(|n| n.contains(name)) {
                        source = Some(all[i]);
                        break;
                    }
                }
                // the receiver keeps a copy of the source; the finder that owns it can go after
                let receiver = source.map(|source| recv_create(&RecvCreate { source, color_format: BGRX_BGRA, bandwidth: HIGHEST, allow_video_fields: false, recv_name: std::ptr::null() }));
                find_destroy(finder);
                let receiver = receiver.filter(|r| !r.is_null())?;
                let mut got = None;
                for _ in 0..10 {
                    let mut frame: super::VideoFrame = std::mem::zeroed();
                    if capture(receiver, &mut frame, std::ptr::null_mut(), std::ptr::null_mut(), 1000) == VIDEO {
                        let (w, h) = (frame.xres as u32, frame.yres as u32);
                        let row = std::slice::from_raw_parts(frame.data.add((h / 2) as usize * frame.line_stride_in_bytes as usize), (w * 4) as usize).to_vec();
                        free(receiver, &frame);
                        got = Some(([w, h], row));
                        break;
                    }
                }
                recv_destroy(receiver);
                got
            }
        }

        /// A sender of our own, found and received: the size sent, the color sent (NDI compresses,
        /// so near it). Needs the NDI runtime, and the firewall may ask: run by hand (`--ignored`).
        #[test]
        #[ignore]
        fn a_receiver_gets_what_is_sent() {
            use super::super::Sender;
            let Ok(mut sender) = super::Ndi::new("Velo test") else {
                return eprintln!("no NDI runtime: skipped");
            };
            let (size, stride) = ([64u32, 32], 256u32);
            let pixels = [40u8, 120, 220, 255].repeat((stride / 4 * size[1]) as usize);
            let done = std::sync::atomic::AtomicBool::new(false);
            let got = std::thread::scope(|s| {
                s.spawn(|| {
                    while !done.load(std::sync::atomic::Ordering::Relaxed) {
                        sender.send(&pixels, size, stride).unwrap();
                        std::thread::sleep(std::time::Duration::from_millis(16));
                    }
                });
                let got = receive("(Velo test)");
                done.store(true, std::sync::atomic::Ordering::Relaxed);
                got
            });
            let (got, row) = got.expect("our sender is found and its frames come");
            assert_eq!(got, size);
            for (a, b) in row[..4].iter().zip(&pixels[..4]) {
                assert!(a.abs_diff(*b) <= 8, "sent {:?}, got {:?}", &pixels[..4], &row[..4]);
            }
        }

        /// A running Velo sharing through NDI (`VELO_DEMO_SHARE=ndi`): found, its picture not
        /// black. Run by hand (`--ignored`) while it runs.
        #[test]
        #[ignore]
        fn a_running_velo_is_received() {
            let (size, row) = receive("(Velo)").expect("Velo is found and its frames come");
            let lit = row.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30).count();
            eprintln!("Velo: {} x {}, {lit} lit pixels across the middle", size[0], size[1]);
            assert!(lit > 0, "its picture isn't black");
        }

        #[test]
        fn a_frame_is_laid_out_as_ndis_headers_say() {
            // NDIlib_video_frame_v2_t on 64 bits: 72 bytes, the timecode at 32, the data at 40
            assert_eq!(std::mem::size_of::<super::VideoFrame>(), 72);
            assert_eq!(std::mem::offset_of!(super::VideoFrame, timecode), 32);
            assert_eq!(std::mem::offset_of!(super::VideoFrame, data), 40);
            assert_eq!(std::mem::offset_of!(super::VideoFrame, metadata), 56);
            assert_eq!(std::mem::size_of::<super::SendCreate>(), 24);
        }
    }
}
