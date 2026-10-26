fn main() {
    // GNU otherwise auto-exports dependency symbols from the mobile cdylib
    // target on Windows and can exceed PE's 65535-export limit. Windows uses
    // the rlib executable entry; Android still exports its mobile entry normally.
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("windows-gnu")) {
        println!("cargo:rustc-cdylib-link-arg=-Wl,--exclude-all-symbols");
    }
    #[cfg(feature = "tauri")]
    tauri_build::build();
}
