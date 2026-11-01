// Windows: embed the icon, version info and a manifest that asks for
// administrator rights at launch (netsh needs them to change addresses).
fn main() {
    println!("cargo:rerun-if-changed=../assets/icon.ico");
    println!("cargo:rerun-if-changed=packaging/rackip.manifest");
    // Resources are compiled on Windows itself (the release build); a
    // cross-`cargo check` from another OS skips them.
    let host_windows = std::env::var("HOST").map(|h| h.contains("windows")).unwrap_or(false);
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && host_windows {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../assets/icon.ico");
        res.set("ProductName", "RackIP");
        res.set("FileDescription", "RackIP");
        res.set_manifest_file("packaging/rackip.manifest");
        res.compile().expect("failed to embed Windows resources");
    }
}
