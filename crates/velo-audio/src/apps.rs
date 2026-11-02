//! One app's audio (docs/audio.md §1, "only Spotify"): on macOS 14.2+, a Core Audio process tap
//! on the app's processes (its helpers too: a browser plays from one) in a private aggregate
//! device, which opens like any input. The app keeps playing out loud. Elsewhere not yet.

/// An app that uses audio: what it's saved as, what it's shown as, whether it's playing.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    /// Its bundle on disk ("/Applications/Spotify.app"): every process inside it is the app's
    /// (a browser's helpers too).
    pub id: String,
    pub name: String,
    pub playing: bool,
}

/// The app an executable belongs to: its outermost `.app` bundle, if that's an application's
/// (in an Applications folder; not a daemon, not a system service).
pub fn app_of(executable: &str) -> Option<&str> {
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
                let name = std::path::Path::new(id).file_stem().map_or_else(|| id.to_string(), |s| s.to_string_lossy().into_owned());
                apps.push(App { id: id.to_string(), name, playing });
            }
        }
    }
    apps.sort_by(|a, b| b.playing.cmp(&a.playing).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    apps
}

#[cfg(target_os = "macos")]
pub use mac::{Tap, apps, processes};

#[cfg(not(target_os = "macos"))]
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

#[cfg(not(target_os = "macos"))]
mod elsewhere {
    use super::App;

    /// One app's audio needs macOS for now (WASAPI process loopback and PipeWire come later).
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
            Err("listening to one app works on macOS for now".into())
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
    #[cfg(target_os = "macos")]
    fn the_apps_list_never_fails() {
        // whatever runs on this machine: each app named, none of them us
        for app in apps() {
            assert!(!app.name.is_empty() && !app.id.is_empty(), "{app:?}");
        }
        assert!(Tap::new("com.example.nothing-here", "Nothing").is_err());
    }
}
