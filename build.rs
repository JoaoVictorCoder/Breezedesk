//! Embeds the icon, version information and application manifest into
//! breezedesk.exe. Antivirus heuristics distrust executables without them.

fn main() {
    println!("cargo:rerun-if-changed=installer/breezedesk.ico");
    println!("cargo:rerun-if-changed=installer/breezedesk.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    // Cross-compiling from Linux uses MinGW's resource compiler.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") && !cfg!(windows) {
        res.set_windres_path("x86_64-w64-mingw32-windres");
        res.set_ar_path("x86_64-w64-mingw32-ar");
    }
    res.set_icon("installer/breezedesk.ico")
        .set_manifest_file("installer/breezedesk.manifest")
        .set("ProductName", "Breezedesk")
        .set("FileDescription", "Breezedesk desktop widgets")
        .set("CompanyName", "JoaoVictorCoder")
        .set("LegalCopyright", "Copyright (c) 2026 JoaoVictorCoder. MIT License.")
        .set("OriginalFilename", "breezedesk.exe")
        .set("InternalName", "breezedesk");
    if let Err(e) = res.compile() {
        panic!("could not embed Windows resources: {e}");
    }
}
