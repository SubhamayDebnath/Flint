fn main() {
    // Embed the app icon in flint.exe (Explorer, Task Manager, installer shortcuts).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "Flint")
            .set("FileDescription", "Flint launcher")
            .set("CompanyName", "subhamay")
            .set("LegalCopyright", "Copyright (c) 2026 subhamay");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed icon: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
