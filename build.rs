//! Windows only: embed the app icon and version info into `branchline.exe`, so Explorer and
//! shortcuts show it. Elsewhere this does nothing; a missing resource compiler is only a warning.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app-icon/branchline.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app-icon/branchline.ico")
        .set("ProductName", "Branchline")
        .set("FileDescription", "Branchline — visual Git client")
        .set("LegalCopyright", "MIT OR Apache-2.0")
        .set("OriginalFilename", "branchline.exe")
        .set("InternalName", "branchline");
    if let Err(e) = res.compile() {
        println!("cargo:warning=branchline.exe built without its icon: {e}");
    }
}
