//! Windows only: embed the app icon and version info into `gitcraft.exe`, so Explorer and
//! shortcuts show it. Elsewhere this does nothing; a missing resource compiler is only a warning.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app-icon/gitcraft.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app-icon/gitcraft.ico")
        .set("ProductName", "gitcraft")
        .set("FileDescription", "gitcraft")
        .set("LegalCopyright", "MIT OR Apache-2.0")
        .set("OriginalFilename", "gitcraft.exe")
        .set("InternalName", "gitcraft");
    if let Err(e) = res.compile() {
        println!("cargo:warning=gitcraft.exe built without its icon: {e}");
    }
}
