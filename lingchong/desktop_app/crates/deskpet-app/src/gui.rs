//! Tauri GUI 入口。
//!
//! 两个窗口：
//!   - `pet`：贴紧宠物本身的小窗口，无边框透明置顶，不承载气泡。
//!   - `bubble`：独立的气泡窗口，先建好并隐藏，播报时定位到宠物上方再显示。
//!
//! 需要系统依赖：webkit2gtk-4.1 / libgtk-3 / libsoup-3.0 / libayatana-appindicator3。
//! 编译：cargo build -p deskpet-app --features tauri

use std::sync::{Arc, Mutex};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    LogicalPosition, WebviewWindow,
};
use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder};

use crate::config::AppConfig;
use crate::ipc;
use crate::runtime::AppState;

/// 宠物窗口尺寸：够放下精灵图（单帧 90x106 × scale 2 = 180x212）并留 5px 边距。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const PET_W: f64 = 190.0;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const PET_H: f64 = 222.0;
/// 气泡窗口尺寸（固定，内容超出时内部裁剪）。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const BUBBLE_W: f64 = 300.0;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const BUBBLE_H: f64 = 180.0;
/// 气泡与宠物之间的间距。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const BUBBLE_GAP: f64 = 8.0;

/// 最近一次播报内容。气泡窗口加载完成后可回拉，避免 show 与 listen 的竞态丢事件。
pub(crate) static LAST_BUBBLE: Mutex<Option<serde_json::Value>> = Mutex::new(None);

/// 生成一个 32x32 的 RGBA 图标（橙色圆角方块 + 简易猫脸）。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn make_pet_icon() -> Image<'static> {
    const SIZE: usize = 32;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = (x as f32 - (SIZE as f32 / 2.0)).abs();
            let dy = (y as f32 - (SIZE as f32 / 2.0)).abs();
            let corner = dx > (SIZE as f32 / 2.0 - 2.0) && dy > (SIZE as f32 / 2.0 - 2.0);
            let idx = (y * SIZE + x) * 4;
            if corner {
                rgba[idx + 3] = 0; // 透明
            } else {
                // 橙色底 #FFB74D
                rgba[idx] = 0xFF;
                rgba[idx + 1] = 0xB7;
                rgba[idx + 2] = 0x4D;
                rgba[idx + 3] = 0xFF;
                // 眼睛（两个小黑点）
                let eye = ((x == 11 || x == 12) && (y == 13 || y == 14))
                    || ((x == 19 || x == 20) && (y == 13 || y == 14));
                if eye {
                    rgba[idx] = 0x33;
                    rgba[idx + 1] = 0x33;
                    rgba[idx + 2] = 0x33;
                }
                // 鼻子
                if (x >= 15 && x <= 16) && y == 18 {
                    rgba[idx] = 0xE6;
                    rgba[idx + 1] = 0x51;
                    rgba[idx + 2] = 0x00;
                }
            }
        }
    }
    Image::new(&rgba, SIZE as u32, SIZE as u32).to_owned()
}

/// 确保气泡窗口存在（创建后隐藏，后续复用）。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn ensure_bubble_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window("bubble") {
        return Ok(w);
    }
    WebviewWindowBuilder::new(app, "bubble", tauri::WebviewUrl::App("bubble.html".into()))
        .title("DeskPet Bubble")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .resizable(false)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .inner_size(BUBBLE_W, BUBBLE_H)
        .build()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn ensure_settings_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window("settings") {
        let was_visible = window.is_visible()?;
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
        if !was_visible { window.emit("settings:reload", ())?; }
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App("settings.html".into()))
        .title("DeskPet 设置")
        .inner_size(620.0, 740.0)
        .min_inner_size(480.0, 560.0)
        .resizable(true)
        .center()
        .build()?;
    let handle = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = handle.emit("settings:hide", ());
            let _ = handle.hide();
        }
    });
    Ok(window)
}

/// 把气泡定位到宠物正上方；顶部空间不足时改放到下方。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn place_bubble(app: &AppHandle, bubble: &WebviewWindow) {
    let Some(pet) = app.get_webview_window("pet") else { return; };
    let Ok(scale) = pet.scale_factor() else { return; };
    let (Ok(pos), Ok(size)) = (pet.outer_position(), pet.inner_size()) else { return; };
    let pos = pos.to_logical::<f64>(scale);
    let size = size.to_logical::<f64>(scale);

    let x = pos.x + (size.width - BUBBLE_W) / 2.0;
    let mut y = pos.y - BUBBLE_H - BUBBLE_GAP;
    if y < 0.0 {
        y = pos.y + size.height + BUBBLE_GAP;
    }
    let _ = bubble.set_position(LogicalPosition::new(x, y));
}

/// 让宠物切到指定情绪（通过事件通知宠物窗口）。
pub(crate) fn set_pet_mood(app: &AppHandle, mood: &str) {
    if let Some(pet) = app.get_webview_window("pet") {
        let _ = pet.emit("pet:mood", mood);
    }
}

/// 播报：显示气泡窗口并把内容推给它，同时让宠物进入对应情绪。
pub(crate) fn announce_to_ui(app: &AppHandle, ann: Option<deskpet_agent::AnnounceResult>) {
    let payload = match ann {
        Some(a) => serde_json::json!({
            "hot_id": a.hot.id,
            "title": a.hot.title,
            "url": a.source_url,
            "script": a.script,
            "mood_hint": a.mood_hint.as_str(),
        }),
        None => serde_json::json!({
            "hot_id": null,
            "title": "",
            "url": "",
            "script": "还没有可播报的热点，可能仍在采集，或候选已被标记为不感兴趣。请联网后稍等片刻再试。",
            "mood_hint": "idle",
        }),
    };

    publish_bubble(app, payload);
}

pub(crate) fn show_status(app: &AppHandle, message: &str, mood: &str) {
    publish_bubble(app, serde_json::json!({
        "hot_id": null, "title": "", "url": "", "script": message, "mood_hint": mood,
    }));
}

fn publish_bubble(app: &AppHandle, payload: serde_json::Value) {
    if let Ok(mut last) = LAST_BUBBLE.lock() {
        *last = Some(payload.clone());
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if let Ok(bubble) = ensure_bubble_window(app) {
        place_bubble(app, &bubble);
        let _ = bubble.show();
        let _ = bubble.emit("bubble:show", payload.clone());
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    let _ = app.emit("bubble:show", payload.clone());

    let mood = payload.get("mood_hint").and_then(|v| v.as_str()).unwrap_or("curious");
    set_pet_mood(app, mood);
}

pub fn run() {
    tauri::Builder::default()
        .setup(move |app| {
            // Android：最先注入 app 私有数据目录，随后 logging/config/db 全部落到该目录。
            // 桌面无需注入，paths 回退到系统数据目录，行为与之前一致。
            #[cfg(any(target_os = "android", target_os = "ios"))]
            {
                crate::paths::init(app.path().app_data_dir()?);
            }

            crate::logging::init_logging();
            let config = AppConfig::load();
            let state = Arc::new(AppState::new(config));
            app.manage(state.clone());
            let app_handle = app.handle().clone();

            #[cfg(any(target_os = "android", target_os = "ios"))]
            WebviewWindowBuilder::new(app, "pet", tauri::WebviewUrl::App("index.html".into()))
                .title("DeskPet")
                .build()?;

            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                let icon = make_pet_icon();
                // 宠物窗口（贴紧宠物，不承载气泡）
                let _pet = WebviewWindowBuilder::new(
                    app,
                    "pet",
                    tauri::WebviewUrl::App("index.html".into()),
                )
                .title("DeskPet")
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .always_on_top(true)
                .resizable(false)
                .skip_taskbar(true)
                .inner_size(PET_W, PET_H)
                .icon(icon.clone())?
                .build()?;

                // 气泡窗口：先建好并隐藏，等播报时再定位显示（避免每次创建导致首个事件丢失）
                let _ = ensure_bubble_window(&app_handle);

                // 托盘图标 + 菜单
                let show_item = MenuItemBuilder::with_id("show", "显示宠物").build(app)?;
                let hide_item = MenuItemBuilder::with_id("hide", "隐藏宠物").build(app)?;
                let agent_item = MenuItemBuilder::with_id("agent", "立即播报").build(app)?;
                let settings_item = MenuItemBuilder::with_id("settings", "设置").build(app)?;
                let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;
                let menu = MenuBuilder::new(app)
                    .items(&[&show_item, &hide_item, &agent_item, &settings_item, &quit_item])
                    .build()?;

                let _tray = TrayIconBuilder::with_id("deskpet-tray")
                    .icon(icon)
                    .tooltip("DeskPet · 桌面宠物")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| {
                        let win = app.get_webview_window("pet");
                        match event.id().as_ref() {
                            "show" => { if let Some(w) = &win { let _ = w.show(); let _ = w.set_focus(); } }
                            "hide" => { if let Some(w) = &win { let _ = w.hide(); } }
                            "agent" => {
                                let ah = app.clone();
                                tauri::async_runtime::spawn(async move {
                                    if let Some(state) = ah.try_state::<Arc<AppState>>() {
                                        let ann = state.run_agent_cycle_force().await;
                                        if let Some(ref a) = ann {
                                            state.speak(&a.script).await;
                                        }
                                        announce_to_ui(&ah, ann);
                                    }
                                });
                            }
                            "settings" => { if let Err(error) = ensure_settings_window(app) { log::warn!("open settings: {error}"); } }
                            "quit" => { app.exit(0); }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(|tray, event| {
                        use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
                        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                            let win = tray.app_handle().get_webview_window("pet");
                            if let Some(w) = &win {
                                if let Ok(true) = w.is_visible() {
                                    let _ = w.hide();
                                } else {
                                    let _ = w.show();
                                    let _ = w.set_focus();
                                }
                            }
                        }
                    })
                    .build(app)?;
            }

            // 后台线程跑调度器 + 采集 + Agent 周期 + 主动播报
            // 顺序：启动调度器 → 采集一轮 → 立即跑 Agent → 之后按间隔循环
            let state2 = state.clone();
            let app_handle2 = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                state2.start_config_watcher();
                state2.scheduler.start().await;

                // 先采一轮，等采集完成再跑 Agent
                for p in ["zhihu", "bilibili", "weibo", "rsshub", "douyin", "xiaohongshu"] {
                    let _ = state2.fetch_platform(p).await;
                }

                // 采集完成后立即跑第一次 Agent
                loop {
                    // 仅在真正有播报内容时才弹气泡/切情绪；
                    // None（勿扰、静默、无候选）时保持安静，避免每周期弹「暂时没有值得说的热点」。
                    if let Some(a) = state2.run_agent_cycle().await {
                        state2.speak(&a.script).await;
                        announce_to_ui(&app_handle2, Some(a));
                    }
                    let interval = state2.config.lock().await.general.min_announce_interval_min as u64 * 60;
                    tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::get_hot_list,
            ipc::get_profile,
            ipc::update_profile,
            ipc::feedback,
            ipc::trigger_agent,
            ipc::silence_for,
            ipc::tts_speak,
            ipc::reload_config,
            ipc::open_url,
            ipc::hide_bubble,
            ipc::bubble_ready,
            ipc::get_settings,
            ipc::save_settings,
            ipc::get_config,
            ipc::save_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
