//! On macOS: Syphon's framework (third_party/syphon: its server, client and messaging, none of its
//! OpenGL or Metal parts) and Velo's bridge to it (src/syphon_bridge.m), compiled in, so the
//! program can go to other apps on the Mac (share.rs, Share::Syphon). Nothing elsewhere.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/syphon");
    println!("cargo:rerun-if-changed={}", root.display());
    println!("cargo:rerun-if-changed=src/syphon_bridge.m");
    // its headers import each other as a framework's (<Syphon/…>): that folder, in OUT_DIR, of
    // links to them (not copies: #import takes a copy for another header, defined twice)
    let include = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("include");
    let framework = include.join("Syphon");
    std::fs::create_dir_all(&framework).expect("OUT_DIR");
    for header in ["SyphonServerBase.h", "SyphonClientBase.h"] {
        let link = framework.join(header);
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(root.join(header).canonicalize().expect("Syphon's headers"), &link).expect("a link in OUT_DIR");
    }
    let mut build = cc::Build::new();
    let mut sources: Vec<_> = std::fs::read_dir(&root).expect("third_party/syphon").flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "m" || e == "c")).collect();
    sources.sort();
    build.files(sources).file("src/syphon_bridge.m");
    build.include(&root).include(&include).flag("-fobjc-arc").flag("-include").flag(root.join("Syphon_Prefix.pch").to_str().expect("a path")).warnings(false);
    build.compile("velo_syphon");
    for framework in ["Foundation", "IOSurface", "AppKit"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
