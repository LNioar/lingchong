//! Shared entry point for the desktop executable and Tauri's mobile library.

mod config;
mod logging;
mod paths;
mod runtime;

#[cfg(feature = "tauri")]
mod gui;
#[cfg(all(feature = "tauri", target_os = "windows"))]
mod windows_speech;
#[cfg(feature = "tauri")]
mod ipc;
#[cfg(any(feature = "tauri", test))]
mod settings;

#[cfg(not(feature = "tauri"))]
mod headless;

#[cfg(feature = "tauri")]
#[cfg_attr(
    any(target_os = "android", target_os = "ios"),
    tauri::mobile_entry_point
)]
pub fn run() {
    gui::run();
}

#[cfg(not(feature = "tauri"))]
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(headless::run());
}
