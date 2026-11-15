//! The Windows exe carries RustyShelf's icon (icon.ico, made from renderer/app-icon.png) as its resource 1, for
//! Explorer, the taskbar and appicon.rs.
fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=icon.ico");
    #[cfg(windows)]
    {
        // fail rather than ship an exe without its icon
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
            && embed_resource::compile("app.rc", embed_resource::NONE).manifest_required().is_err()
        {
            panic!("could not embed the app icon (app.rc): no resource compiler found");
        }
    }
}
