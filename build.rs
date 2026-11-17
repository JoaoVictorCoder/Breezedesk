//! Embeds the icon, version information and application manifest into
//! plume.exe. Antivirus heuristics distrust executables without them.

fn main() {
    println!("cargo:rerun-if-changed=installer/plume.ico");
    println!("cargo:rerun-if-changed=installer/plume.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    // Cross-compiling from Linux uses MinGW's resource compiler.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") && !cfg!(windows) {
        res.set_windres_path("x86_64-w64-mingw32-windres");
        res.set_ar_path("x86_64-w64-mingw32-ar");
    }
    res.set_icon("installer/plume.ico")
        .set_manifest_file("installer/plume.manifest")
        .set("ProductName", "Plume")
        .set("FileDescription", "Plume desktop widgets")
        .set("CompanyName", "JoaoVictorCoder")
        .set("LegalCopyright", "Copyright (c) 2026 JoaoVictorCoder. MIT License.")
        .set("OriginalFilename", "plume.exe")
        .set("InternalName", "plume");
    if let Err(e) = res.compile() {
        panic!("could not embed Windows resources: {e}");
    }
}
