fn main() {
    // Give spry.exe its icon. Skipped quietly when no resource compiler is installed.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/spry.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=no exe icon: {e}");
        }
    }
}
