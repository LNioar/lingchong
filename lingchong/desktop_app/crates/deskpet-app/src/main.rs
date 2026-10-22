// Windows GUI 模式：release 构建隐藏控制台窗口，只显示宠物 UI
// debug 构建保留控制台，方便查看日志调试
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    deskpet_app_lib::run();
}
