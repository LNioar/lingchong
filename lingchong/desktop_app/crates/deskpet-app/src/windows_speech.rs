//! Windows GUI speech only. Existing Android/headless providers stay unchanged.
use crate::{config::TtsConfig, gui::LAST_BUBBLE, runtime::AppState};
use deskpet_tts::windows_process::execute;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    process::Command,
    sync::{oneshot, watch},
};

#[derive(Default)]
pub struct SpeechController {
    sequence: AtomicU64,
    active: Mutex<Option<Session>>,
    gate: Arc<tokio::sync::Mutex<()>>,
}

struct Session {
    id: String,
    cancel: watch::Sender<bool>,
    presented: Option<oneshot::Sender<()>>,
}

pub fn stop(app: &AppHandle, id: Option<&str>) {
    if let Some(controller) = app.try_state::<SpeechController>() {
        if let Ok(mut active) = controller.active.lock() {
            if active
                .as_ref()
                .is_some_and(|session| id.is_none() || id == Some(session.id.as_str()))
            {
                if let Some(session) = active.take() {
                    let _ = session.cancel.send(true);
                }
            }
        }
    }
}

/// Prepare a task before showing the bubble. It cannot synthesize until the
/// bubble acknowledges painting this specific id (including startup recovery).
pub fn prepare(app: &AppHandle, mut payload: Value, speak: bool) -> Value {
    stop(app, None);
    let controller = app.state::<SpeechController>();
    let id = format!(
        "{}-{}",
        std::process::id(),
        controller.sequence.fetch_add(1, Ordering::Relaxed)
    );
    payload["bubble_id"] = json!(id);
    let enabled = app
        .state::<Arc<AppState>>()
        .config
        .try_lock()
        .map(|config| config.tts.enabled && config.tts.provider == "edge")
        .unwrap_or(true);
    let text = deskpet_tts::spoken_prefix(payload["script"].as_str().unwrap_or("")).to_owned();
    if !speak || !enabled || text.trim().is_empty() {
        return payload;
    }
    payload["speech_state"] = json!("queued");
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (present_tx, present_rx) = oneshot::channel();
    *controller.active.lock().unwrap() = Some(Session {
        id: id.clone(),
        cancel: cancel_tx,
        presented: Some(present_tx),
    });
    let handle = app.clone();
    let gate = controller.gate.clone();
    tauri::async_runtime::spawn(async move {
        let _single_voice = gate.lock().await;
        let result = run(&handle, &id, &text, present_rx, cancel_rx).await;
        #[cfg(debug_assertions)]
        let smoke_result = if std::env::args().any(|arg| arg == "--speech-smoke-test") {
            Some(json!({"success":result.is_ok(),"error":result.as_ref().err(),"bubble_id":id,
                "bubble_visible":handle.get_webview_window("bubble").and_then(|bubble| bubble.is_visible().ok())}))
        } else { None };
        match result {
            Ok(()) => transition(&handle, &id, "completed", None),
            Err(error) if error == "cancelled" => transition(&handle, &id, "cancelled", None),
            Err(error) => {
                log::warn!("[windows-tts] {error}");
                transition(&handle, &id, "failed", Some(&error));
            }
        }
        let controller = handle.state::<SpeechController>();
        if let Ok(mut active) = controller.active.lock() {
            if active.as_ref().is_some_and(|session| session.id == id) {
                active.take();
            }
        };
        #[cfg(debug_assertions)]
        if let Some(result) = smoke_result {
            let _ = std::fs::write(crate::paths::data_dir().join("result.json"), result.to_string());
            handle.exit(if result["success"] == true { 0 } else { 1 });
        }
    });
    payload
}

pub fn presented(app: &AppHandle, id: &str) {
    let controller = app.state::<SpeechController>();
    if let Ok(mut active) = controller.active.lock() {
        if let Some(session) = active.as_mut().filter(|session| session.id == id) {
            if let Some(sender) = session.presented.take() {
                let _ = sender.send(());
            }
        }
    };
}

fn transition(app: &AppHandle, id: &str, phase: &str, error: Option<&str>) {
    if let Ok(mut last) = LAST_BUBBLE.lock() {
        if let Some(payload) = last
            .as_mut()
            .filter(|p| p["bubble_id"].as_str() == Some(id))
        {
            payload["speech_state"] = json!(phase);
            payload["speech_error"] = json!(error);
            // Cache and event share the same id. Reopening never restarts speech.
            if let Some(bubble) = app.get_webview_window("bubble") {
                let _ = bubble.emit(
                    "bubble:speech",
                    json!({"bubble_id":id,"speech_state":phase,"speech_error":error}),
                );
            }
        }
    }
}

fn runtime_path(
    app: &AppHandle,
    config: Option<&str>,
    variable: &str,
    relative: &str,
) -> Result<PathBuf, String> {
    let explicit = config
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os(variable).map(PathBuf::from));
    if let Some(path) = explicit {
        return path
            .is_file()
            .then_some(path)
            .ok_or_else(|| format!("{variable} 指定的文件不存在"));
    }
    if let Ok(dir) = app.path().resource_dir() {
        let path = dir.join("tts").join(relative);
        if path.is_file() {
            return Ok(path);
        }
    }
    // A dedicated project-local runtime; never depend on the running Demo server.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if cfg!(debug_assertions) {
        let path = root.join("resources/tts").join(relative);
        if path.is_file() {
            return Ok(path);
        }
        if relative == "python/python.exe" {
            let path = root.join("../../demos/edge-tts/.venv/Scripts/python.exe");
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    Err(format!(
        "缺少语音运行时 {relative}，请运行 scripts/prepare-windows-tts.ps1"
    ))
}

async fn run(
    app: &AppHandle,
    id: &str,
    text: &str,
    presented: oneshot::Receiver<()>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), String> {
    if *cancel.borrow() {
        return Err("cancelled".into());
    }
    tokio::select! {
        ready = tokio::time::timeout(Duration::from_secs(10), presented) => {
            if !matches!(ready, Ok(Ok(()))) { return Err("气泡尚未就绪，已跳过语音".into()); }
        }
        _ = cancel.changed() => return Err("cancelled".into()),
    }
    let state = app.state::<Arc<AppState>>().inner().clone();
    let config: TtsConfig = state.config.lock().await.tts.clone();
    if !config.enabled || config.provider != "edge" || *cancel.borrow() {
        return Err("cancelled".into());
    }
    let edge = config.edge.unwrap_or_default();
    let live_config = state.config.clone();
    let enabled: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        live_config
            .try_lock()
            .map(|config| config.tts.enabled && config.tts.provider == "edge")
            .unwrap_or(true)
    });
    let python = runtime_path(
        app,
        edge.python_path.as_deref(),
        "DESKPET_TTS_PYTHON",
        "python/python.exe",
    )?;
    let mpv = runtime_path(
        app,
        edge.mpv_path.as_deref(),
        "DESKPET_TTS_MPV",
        "mpv/mpv.exe",
    )?;
    let dir = crate::paths::data_dir().join("tts/jobs").join(id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|_| "无法创建语音临时目录")?;
    let result = async {
        let script = dir.join("worker.py");
        let audio = dir.join("speech.mp3");
        tokio::fs::write(&script, include_str!("../resources/tts/edge_worker.py"))
            .await
            .map_err(|_| "无法准备语音合成脚本")?;
        transition(app, id, "synthesizing", None);
        let input = serde_json::to_vec(
            &json!({"text":text,"voice":config.voice,"proxy":edge.proxy,"output":audio}),
        )
        .map_err(|_| "语音请求无效")?;
        let mut command = Command::new(python);
        command.arg(&script).env("PYTHONUTF8", "1");
        execute(
            &mut command,
            Some(input),
            &mut cancel,
            enabled.clone(),
            Duration::from_secs(90),
        )
        .await?;
        let size = tokio::fs::metadata(&audio)
            .await
            .map_err(|_| "未生成音频")?
            .len();
        if size == 0 {
            return Err("合成返回空音频".into());
        }
        if *cancel.borrow() {
            return Err("cancelled".into());
        }
        transition(app, id, "playing", None);
        let mut command = Command::new(mpv);
        command
            .args([
                "--no-config",
                "--load-scripts=no",
                "--ytdl=no",
                "--no-video",
                "--audio-display=no",
                "--no-terminal",
                "--input-terminal=no",
                "--msg-level=all=no",
                "--",
            ])
            .arg(&audio);
        execute(
            &mut command,
            None,
            &mut cancel,
            enabled,
            Duration::from_secs(300),
        )
        .await
    }
    .await;
    // Only the generated task directory is removed; never the data root.
    if let Err(error) = tokio::fs::remove_dir_all(&dir).await {
        log::warn!("[windows-tts] temporary cleanup: {error}");
    }
    result
}
