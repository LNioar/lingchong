//! 日志初始化：同时输出到 stderr 和轮转文件。
//!
//! - 文件日志位于数据目录 `deskpet.log`，超过 10MB 轮转，保留 5 个备份。
//! - 日志级别由 `RUST_LOG` 环境变量控制，默认 `info`。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{LevelFilter, Metadata, Record};

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
const MAX_BACKUPS: usize = 5;

struct RotatingFileLogger {
    path: PathBuf,
    file: Mutex<File>,
    stderr_level: LevelFilter,
}

impl RotatingFileLogger {
    fn new(path: PathBuf, stderr_level: LevelFilter) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { path, file: Mutex::new(file), stderr_level })
    }

    /// 若当前日志文件超过上限，执行轮转：deskpet.log → deskpet.log.1 → ... → deskpet.log.5（删除）
    fn rotate_if_needed(&self) {
        let file = match self.file.lock() {
            Ok(f) => f,
            Err(_) => return,
        };
        let metadata = match file.metadata() {
            Ok(m) => m,
            Err(_) => return,
        };
        if metadata.len() < MAX_FILE_SIZE {
            return;
        }
        drop(file); // 释放锁后再操作文件

        // 轮转备份：deskpet.log.N-1 → deskpet.log.N
        for i in (1..=MAX_BACKUPS).rev() {
            let src = self.backup_path(i - 1);
            let dst = self.backup_path(i);
            if src.exists() {
                let _ = fs::rename(&src, &dst);
            }
        }
        // 主日志 → .1
        let _ = fs::rename(&self.path, self.backup_path(1));
        // 重新打开主日志文件
        if let Ok(mut f) = self.file.lock() {
            if let Ok(new_file) = OpenOptions::new().create(true).append(true).open(&self.path) {
                *f = new_file;
            }
        }
    }

    fn backup_path(&self, n: usize) -> PathBuf {
        if n == 0 {
            self.path.clone()
        } else {
            let mut p = self.path.clone().into_os_string();
            p.push(format!(".{n}"));
            PathBuf::from(p)
        }
    }
}

impl log::Log for RotatingFileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.stderr_level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} [{}] {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            record.level(),
            record.args()
        );
        // 写文件
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(line.as_bytes());
            let _ = f.flush();
        }
        // 写 stderr
        eprint!("{line}");
        // 轮转检查
        self.rotate_if_needed();
    }

    fn flush(&self) {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.flush();
        }
    }
}

/// 初始化日志：同时输出到 stderr 和数据目录下的轮转日志文件。
pub fn init_logging() {
    let log_path = log_file_path();
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(LevelFilter::Info);

    match RotatingFileLogger::new(log_path.clone(), level) {
        Ok(logger) => {
            if log::set_boxed_logger(Box::new(logger)).is_ok() {
                log::set_max_level(level);
                eprintln!("[logging] writing logs to {}", log_path.display());
            }
        }
        Err(e) => {
            // 文件日志初始化失败，降级为 env_logger（仅 stderr）
            eprintln!("[logging] file logger init failed ({e}), falling back to stderr-only");
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
        }
    }
}

fn log_file_path() -> PathBuf {
    crate::paths::data_dir().join("deskpet.log")
}

/// 返回日志文件路径（供外部查询）。
pub fn log_path() -> PathBuf {
    log_file_path()
}

#[allow(dead_code)]
fn _assert_path_send(_p: &Path) {}
