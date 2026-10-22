//! Headless 运行模式：无 GUI，把热点采集与 Agent 播报输出到日志/终端。
//!
//! 用于沙箱内验证核心逻辑编译与运行，也可作为无桌面环境的后台服务。

use std::sync::Arc;
use std::time::Duration;

use crate::config::AppConfig;
use crate::runtime::AppState;

pub async fn run() {
    crate::logging::init_logging();
    log::info!("DeskPet starting in headless mode...");

    let config = AppConfig::load();
    let state = Arc::new(AppState::new(config));

    // 启动配置文件监听（热重载）
    state.start_config_watcher();

    // 启动调度器（采集定时任务）
    state.scheduler.start().await;

    // 立即采集一次所有平台
    for p in ["zhihu", "bilibili", "weibo", "rsshub", "douyin", "xiaohongshu"] {
        let outcome = state.fetch_platform(p).await;
        log::info!("[headless] fetch {} -> {} ({} items)", p, outcome.status, outcome.item_count);
    }

    // 主循环：定时跑 Agent
    loop {
        if let Some(ann) = state.run_agent_cycle().await {
            log::info!("[headless] ANNOUNCE: {}", ann.script);
            log::info!("[headless] link: {}", ann.source_url);
            state.speak(&ann.script).await;
        } else {
            log::info!("[headless] nothing to announce this cycle");
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}
