
fn main() {
    #[cfg(target_os = "windows")]
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/ruby.ico");
        res.compile().unwrap();
    }
}
