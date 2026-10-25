fn main() {
    // Embed the app icon in snapt.exe; other systems take it from the package.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        #[cfg(windows)]
        {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/icons/icon.ico");
            if let Err(err) = res.compile() {
                println!("cargo:warning=failed to embed the icon: {err}");
            }
        }
    }
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
}
