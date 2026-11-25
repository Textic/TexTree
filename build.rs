fn main() {
    println!("cargo:rerun-if-changed=assets/textree.ico");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Windows defaults to a 1 MB main-thread stack, which debug builds of
        // GPUI / gpui-kit can exceed during component layout and rendering.
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = tauri_winres::WindowsResource::new();
        // GPUI's Windows platform layer loads resource ID 1 for window/taskbar HICON.
        res.set_icon_with_id("assets/textree.ico", "1");
        res.compile().expect("failed to compile Windows icon resource");
    }
}
