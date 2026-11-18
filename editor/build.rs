//! Icon and version information for plume-editor.exe.

fn main() {
    println!("cargo:rerun-if-changed=../installer/plume.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") && !cfg!(windows) {
        res.set_windres_path("x86_64-w64-mingw32-windres");
        res.set_ar_path("x86_64-w64-mingw32-ar");
    }
    res.set_icon("../installer/plume.ico")
        .set("ProductName", "Plume")
        .set("FileDescription", "Plume widget editor")
        .set("CompanyName", "JoaoVictorCoder")
        .set("LegalCopyright", "Copyright (c) 2026 JoaoVictorCoder. MIT License.")
        .set("OriginalFilename", "plume-editor.exe");
    if let Err(e) = res.compile() {
        panic!("could not embed Windows resources: {e}");
    }
}
