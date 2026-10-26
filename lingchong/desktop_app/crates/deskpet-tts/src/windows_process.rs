//! Cancellable, hidden Windows processes, isolated from any GUI or database.
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::watch,
};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    },
};

struct ProcessJob(usize);
impl ProcessJob {
    fn attach(child: &Child) -> Result<Self, String> {
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err("无法创建语音进程管理器".into());
            }
            let job = Self(handle as usize);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(
                    handle,
                    child.raw_handle().ok_or("语音进程句柄无效")? as _,
                ) == 0
            {
                return Err("无法隔离语音进程".into());
            }
            Ok(job)
        }
    }
}
impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0 as _);
        }
    }
}

pub async fn execute(
    command: &mut Command,
    input: Option<Vec<u8>>,
    cancel: &mut watch::Receiver<bool>,
    enabled: Arc<dyn Fn() -> bool + Send + Sync>,
    timeout: Duration,
) -> Result<(), String> {
    if *cancel.borrow() {
        return Err("cancelled".into());
    }
    command
        .creation_flags(0x08000000)
        .kill_on_drop(true)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    let mut child = command
        .spawn()
        .map_err(|_| "无法启动语音运行时，请检查 Python/edge-tts/mpv".to_string())?;
    let _job = match ProcessJob::attach(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
    };
    let stderr = child.stderr.take();
    let diagnostic = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(mut stderr) = stderr {
            // Keep a bounded diagnostic while draining the entire pipe.
            let mut chunk = [0; 1024];
            loop {
                match stderr.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let keep = count.min(2048 - bytes.len());
                        bytes.extend_from_slice(&chunk[..keep]);
                    }
                }
            }
        }
        bytes
    });
    if let Some(bytes) = input {
        let mut stdin = child.stdin.take().ok_or("无法打开语音输入管道")?;
        let write = tokio::time::timeout(Duration::from_secs(3), async {
            stdin.write_all(&bytes).await?;
            stdin.shutdown().await
        })
        .await;
        if !matches!(write, Ok(Ok(()))) {
            let _ = child.kill().await;
            let _ = child.wait().await;
            diagnostic.abort();
            return Err("语音输入写入失败".into());
        }
    }
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    let result = loop {
        tokio::select! {
            status = child.wait() => break status.map_err(|_| "语音进程等待失败".to_string())
                .and_then(|status| if status.success() { Ok(()) } else { Err("语音进程执行失败，请检查网络、音色及播放器".into()) }),
            _ = cancel.changed() => break Err("cancelled".into()),
            _ = &mut deadline => break Err("语音处理超时".into()),
            _ = interval.tick() => if *cancel.borrow() || !enabled() { break Err("cancelled".into()); },
        }
    };
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let bytes = tokio::time::timeout(Duration::from_secs(2), diagnostic)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    if result
        .as_ref()
        .err()
        .is_some_and(|error| error != "cancelled")
    {
        log::warn!("[windows-tts] child: {}", String::from_utf8_lossy(&bytes));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sleeping() -> Command {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ]);
        command
    }

    #[tokio::test]
    async fn successful_and_failed_exit_codes_are_distinct() {
        let (_tx, mut rx) = watch::channel(false);
        let mut success = Command::new("cmd.exe");
        success.args(["/C", "exit", "0"]);
        assert!(execute(
            &mut success,
            None,
            &mut rx,
            Arc::new(|| true),
            Duration::from_secs(5)
        )
        .await
        .is_ok());
        let mut failure = Command::new("cmd.exe");
        failure.args(["/C", "exit", "7"]);
        assert!(execute(
            &mut failure,
            None,
            &mut rx,
            Arc::new(|| true),
            Duration::from_secs(5)
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn close_cancel_and_disable_reap_the_process() {
        let (tx, mut rx) = watch::channel(false);
        let cancel = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            tx.send(true).unwrap();
        });
        let result = execute(
            &mut sleeping(),
            None,
            &mut rx,
            Arc::new(|| true),
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(result.unwrap_err(), "cancelled");
        cancel.await.unwrap();
        let (_tx, mut rx) = watch::channel(false);
        assert_eq!(
            execute(
                &mut sleeping(),
                None,
                &mut rx,
                Arc::new(|| false),
                Duration::from_secs(5)
            )
            .await
            .unwrap_err(),
            "cancelled"
        );
    }

    #[tokio::test]
    async fn process_timeout_and_pre_cancel_are_bounded() {
        let (_tx, mut rx) = watch::channel(false);
        assert!(execute(
            &mut sleeping(),
            None,
            &mut rx,
            Arc::new(|| true),
            Duration::from_millis(200)
        )
        .await
        .unwrap_err()
        .contains("超时"));
        let (_tx, mut rx) = watch::channel(true);
        let mut invalid = Command::new("this-file-must-not-be-spawned.exe");
        assert_eq!(
            execute(
                &mut invalid,
                None,
                &mut rx,
                Arc::new(|| true),
                Duration::from_secs(1)
            )
            .await
            .unwrap_err(),
            "cancelled"
        );
    }

    #[tokio::test]
    #[ignore = "online smoke test; prepare the portable Windows speech resources first"]
    async fn prepared_edge_runtime_synthesizes_plays_and_stops() {
        let resources = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../deskpet-app/resources/tts");
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../target/tts-smoke-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("speech.mp3");
        let input = format!(
            r#"{{"text":"你好，这是一条桌宠语音测试。","voice":"zh-CN-XiaoxiaoNeural","output":{},"proxy":null}}"#,
            serde_json::to_string(&audio.to_string_lossy()).unwrap()
        );
        let (_tx, mut rx) = watch::channel(false);
        let mut worker = Command::new(resources.join("python/python.exe"));
        worker
            .arg(resources.join("edge_worker.py"))
            .env("PYTHONUTF8", "1");
        let synthesis = execute(
            &mut worker,
            Some(input.into_bytes()),
            &mut rx,
            Arc::new(|| true),
            Duration::from_secs(90),
        )
        .await;
        assert!(synthesis.is_ok(), "synthesis failed: {synthesis:?}");
        assert!(std::fs::metadata(&audio).unwrap().len() > 0);
        let player = || {
            let mut command = Command::new(resources.join("mpv/mpv.exe"));
            command
                .args([
                    "--no-config",
                    "--load-scripts=no",
                    "--ytdl=no",
                    "--no-video",
                    "--no-terminal",
                    "--msg-level=all=no",
                    "--",
                ])
                .arg(&audio);
            command
        };
        assert!(execute(
            &mut player(),
            None,
            &mut rx,
            Arc::new(|| true),
            Duration::from_secs(30)
        )
        .await
        .is_ok());
        let (tx, mut rx) = watch::channel(false);
        let stop = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let _ = tx.send(true);
        });
        assert_eq!(
            execute(
                &mut player(),
                None,
                &mut rx,
                Arc::new(|| true),
                Duration::from_secs(30)
            )
            .await
            .unwrap_err(),
            "cancelled"
        );
        stop.await.unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
