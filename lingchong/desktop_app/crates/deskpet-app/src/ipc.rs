//! Tauri IPC 命令（前端 ↔ 后端）。

use serde::Serialize;
use std::sync::Arc;

use tauri::Manager;

use deskpet_core::{FeedbackAction, UserProfile};

use crate::runtime::AppState;
use crate::settings::{ConfigDto, ConfigUpdate, SettingsUpdate, SettingsView};

#[tauri::command]
pub async fn get_config(state: tauri::State<'_, Arc<AppState>>) -> Result<ConfigDto, String> {
    Ok(ConfigDto::from(&*state.config.lock().await))
}

#[tauri::command]
pub async fn save_config(state: tauri::State<'_, Arc<AppState>>, config: ConfigUpdate) -> Result<bool, String> {
    let mut candidate = state.config.lock().await.clone();
    config.apply(&mut candidate)?;
    candidate.try_save()?;
    state.reload_config().await?;
    Ok(true)
}

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, Arc<AppState>>) -> Result<SettingsView, String> {
    let config = state.config.lock().await;
    Ok(SettingsView::from(&*config))
}

#[tauri::command]
pub async fn save_settings(
    state: tauri::State<'_, Arc<AppState>>,
    settings: SettingsUpdate,
) -> Result<SettingsView, String> {
    let mut config = state.config.lock().await.clone();
    settings.apply(&mut config)?;
    config.try_save()?;
    state.reload_config().await?;
    Ok(SettingsView::from(&config))
}

#[derive(Serialize)]
pub struct HotItemDto {
    pub id: String,
    pub platform: String,
    pub rank: u32,
    pub title: String,
    pub hot_value: Option<u64>,
    pub url: String,
    pub tags: Vec<String>,
    pub first_seen: i64,
}

impl From<deskpet_core::HotItem> for HotItemDto {
    fn from(h: deskpet_core::HotItem) -> Self {
        Self {
            id: h.id,
            platform: h.platform,
            rank: h.rank,
            title: h.title,
            hot_value: h.hot_value,
            url: h.url,
            tags: h.tags,
            first_seen: h.first_seen,
        }
    }
}

#[tauri::command]
pub async fn get_hot_list(
    state: tauri::State<'_, Arc<AppState>>,
    limit: Option<u32>,
    platform: Option<String>,
) -> Result<Vec<HotItemDto>, String> {
    let limit = limit.unwrap_or(50);
    let p = platform.clone();
    let state = state.inner().clone();
    let items = tokio::task::spawn_blocking(move || state.list_hot(limit, p.as_deref()))
        .await
        .map_err(|e| e.to_string())?;
    Ok(items.into_iter().map(HotItemDto::from).collect())
}

#[tauri::command]
pub async fn get_profile(state: tauri::State<'_, Arc<AppState>>) -> Result<UserProfile, String> {
    state.get_profile().await
}

#[tauri::command]
pub async fn update_profile(
    state: tauri::State<'_, Arc<AppState>>,
    mut profile: UserProfile,
) -> Result<bool, String> {
    crate::settings::validate_quiet_hours(&profile.schedule.quiet_hours)?;
    if profile.schedule.min_announce_interval_min < 1 { return Err("播报间隔至少为 1 分钟".into()); }
    let mut topics = std::collections::HashSet::new();
    for interest in &mut profile.interests {
        interest.topic = interest.topic.trim().into();
        if interest.topic.is_empty() || !interest.weight.is_finite() || !topics.insert(interest.topic.clone()) {
            return Err("兴趣主题不能为空、重复，权重必须为有效数字".into());
        }
        interest.weight = interest.weight.clamp(0.0, 1.0);
    }
    profile.updated_at = chrono::Utc::now().timestamp();
    state.save_profile(&profile).await?;
    Ok(true)
}

#[tauri::command]
pub async fn feedback(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    hot_id: String,
    action: String,
) -> Result<bool, String> {
    let action = match action.as_str() {
        "click" => FeedbackAction::Click,
        "like" => FeedbackAction::Like,
        "ignore" => FeedbackAction::Ignore,
        "dismiss" => FeedbackAction::Dismiss,
        "dislike" => FeedbackAction::Dislike,
        _ => return Err("invalid action".into()),
    };
    // 记录反馈 + 按热点标签回写画像权重 + 落库（形成学习闭环）
    state.apply_feedback(&hot_id, action).await;
    // 顺手让桌宠给出情绪反馈（气泡在独立窗口，桌宠这边需要同步情绪）
    let mood = match action {
        FeedbackAction::Like => "happy",
        FeedbackAction::Dislike => "bored",
        _ => "idle",
    };
    crate::gui::set_pet_mood(&app, mood);
    Ok(true)
}

#[tauri::command]
pub async fn trigger_agent(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    // 用户主动触发，跳过勿扰/静默/间隔检查
    // Bound the complete cycle, including waiting for another agent cycle.
    // A slow/unreachable model service must not leave the mobile button disabled forever.
    crate::gui::show_status(&app, "正在联网刷新热点……", "curious");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(45),
        state.refresh_and_run_agent(),
    )
    .await;
    match result {
        Ok(Ok(Some(announcement))) => {
            crate::gui::announce_to_ui(&app, Some(announcement));
            Ok(true)
        }
        Ok(Ok(None)) => {
            crate::gui::show_status(&app, "本次刷新暂无未播报的新热点，稍后再试。", "idle");
            Ok(true)
        }
        other => {
            let message = match other {
                Ok(Err(message)) => message,
                _ => "联网刷新或生成回复超时，请稍后重试。".into(),
            };
            crate::gui::show_status(&app, &message, "idle");
            Err(message)
        }
    }
}

#[tauri::command]
pub async fn silence_for(
    state: tauri::State<'_, Arc<AppState>>,
    minutes: u32,
) -> Result<bool, String> {
    state.scheduler.silence_for(minutes).await;
    Ok(true)
}

#[tauri::command]
pub async fn tts_speak(
    state: tauri::State<'_, Arc<AppState>>,
    text: String,
) -> Result<bool, String> {
    state.speak(&text).await;
    Ok(true)
}

/// 热重载配置文件（勿扰时段、采集间隔等可热更新项）。
#[tauri::command]
pub async fn reload_config(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    state.reload_config().await.map(|_| true)
}

/// 用系统默认浏览器打开外部链接（http/https）。
///
/// Tauri 默认不允许 webview 新建窗口，`<a target="_blank">` 点了没反应，
/// 所以统一走系统命令打开，避免把宠物窗口本身导航走。
#[tauri::command]
pub async fn open_url(url: String) -> Result<bool, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http/https url allowed".into());
    }
    open_in_browser(&url).map_err(|e| e.to_string())?;
    Ok(true)
}

fn open_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

/// 隐藏气泡窗口（气泡自动超时/点击关闭时调用）。
#[tauri::command]
pub async fn hide_bubble(app: tauri::AppHandle) -> Result<bool, String> {
    if let Some(w) = app.get_webview_window("bubble") {
        let _ = w.hide();
    }
    Ok(true)
}

/// 气泡窗口加载完成后回拉最近一次播报内容（避免 show 与 listen 的竞态丢事件）。
#[tauri::command]
pub async fn bubble_ready() -> Result<Option<serde_json::Value>, String> {
    Ok(crate::gui::LAST_BUBBLE.lock().ok().and_then(|g| g.clone()))
}
