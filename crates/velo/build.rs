//! On Windows, gives velo.exe its icon in Explorer and on shortcuts (the window's own comes from
//! assets/icons/png). The icon is the brand pack's (assets/icons/windows/velo.ico).
//! From Ravune (03cb268), trimmed for Velo.

fn main() {
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons/windows/velo.ico");
        println!("cargo:rerun-if-changed={}", icon.display());
        winresource::WindowsResource::new().set_icon(&icon.to_string_lossy()).compile().expect("embed the icon (needs the Windows SDK's rc.exe)");
    }
}
