//! 数据目录抽象。
//!
//! 桌面平台使用系统数据目录（`dirs::data_dir()/deskpet`）；
//! Android 上 `dirs::data_dir()` 返回 None，需在启动早期由 Tauri
//! 注入 app 私有目录（`path::app_data_dir()`），见 `init()`。
//! 未注入时行为与改动前完全一致（桌面路径不变）。

use std::path::PathBuf;
use std::sync::OnceLock;

/// 外部注入的数据目录（Android）。OnceLock 保证只设置一次。
static INJECTED: OnceLock<PathBuf> = OnceLock::new();

/// 启动早期注入数据目录（Android 由 Tauri app_data_dir 调用）。
/// 桌面无需调用；重复调用静默忽略。
pub fn init(dir: PathBuf) {
    let _ = INJECTED.set(dir);
}

/// 应用数据目录。优先级：注入值 > 系统数据目录/deskpet > 当前目录。
pub fn data_dir() -> PathBuf {
    if let Some(d) = INJECTED.get() {
        std::fs::create_dir_all(d).ok();
        return d.clone();
    }
    let dir = dirs::data_dir()
        .map(|d| d.join("deskpet"))
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir).ok();
    dir
}
