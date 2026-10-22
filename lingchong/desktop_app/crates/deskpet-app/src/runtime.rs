//! 运行时状态：把 storage / collector / scheduler / agent / tts 组装到一起。

use std::sync::Arc;
use std::time::Duration;

use rusqlite::Connection;
use tokio::sync::Mutex;

use deskpet_agent::{AgentOrchestrator, LlmProvider, OpenAiCompat, LlmConfig as AgentLlmConfig};
use deskpet_collector::{
    adapters::{BilibiliAdapter, RsshubAdapter, RsshubRouteAdapter, WeiboAdapter, ZhihuAdapter, default_routes},
    AdapterManager, HotAdapter,
};
use deskpet_core::{EmbeddingProvider, FeedbackAction, UserProfile};
use deskpet_scheduler::Scheduler;
use deskpet_storage::{init_db, FeedbackRepo, FetchLogRepo, HotRepo, ProfileRepo};
use deskpet_tts::{EdgeTts, TtsProvider};

use crate::config::AppConfig;

/// 无 embedder 时的占位实现：始终返回错误，
/// ensure_interest_embeddings 会捕获并降级为占位向量。
struct NoopEmbedder;

#[async_trait::async_trait]
impl EmbeddingProvider for NoopEmbedder {
    async fn embed(&self, _texts: &[String]) -> deskpet_core::Result<Vec<Vec<f32>>> {
        Err(deskpet_core::DeskPetError::Llm("embedding disabled (no api key)".into()))
    }
}

/// 全局运行时状态。
pub struct AppState {
    pub config: Arc<Mutex<AppConfig>>,
    pub db: Arc<Mutex<Connection>>,
    pub collector: Arc<AdapterManager>,
    pub agent: Mutex<AgentOrchestrator>,
    pub scheduler: Arc<Scheduler>,
    pub tts: Option<Box<dyn TtsProvider>>,
    manual_refresh: Mutex<()>,
}

impl AppState {
    /// 初始化数据库、注册 Adapter、构造 Agent 等。
    pub fn new(config: AppConfig) -> Self {
        // 数据库
        let db_path = crate::paths::data_dir().join("deskpet.db");
        let conn = Connection::open(&db_path).expect("open db");
        init_db(&conn).expect("init db");
        let db = Arc::new(Mutex::new(conn));

        // LLM / Embedding（同一实例，实现 LlmProvider + EmbeddingProvider 两个 trait）
        let llm_config = AgentLlmConfig {
            api_base: config.llm.api_base.clone(),
            api_key: config.llm.api_key.clone(),
            model: config.llm.model.clone(),
            embedding_model: config.llm.embedding_model.clone(),
        };
        // 启动时打印 LLM 配置（key 脱敏），方便排查配置是否生效
        let masked_key = if config.llm.api_key.is_empty() {
            "(empty)".to_string()
        } else {
            "(configured)".to_string()
        };
        log::info!(
            "[runtime] LLM config: base={} model={} embedding={:?} key={}",
            config.llm.api_base,
            config.llm.model,
            config.llm.embedding_model,
            masked_key
        );
        log::info!("[runtime] BUILD_MARKER=llm-trace-v1");
        // 有 API Key 且配置了 embedding_model 才启用 embedding，否则降级占位向量
        let provider = Arc::new(OpenAiCompat::new(llm_config));
        let llm: Arc<dyn LlmProvider> = provider.clone();
        // embedding 仅在 api_key 非空且 embedding_model 非空时启用
        let embedder: Option<Arc<dyn EmbeddingProvider>> = if config.llm.embedding_enabled() {
            Some(provider.clone())
        } else {
            log::info!("[runtime] embedding disabled (api_key empty or embedding_model unset), placeholder fallback");
            None
        };

        // Collector：注入 embedding 提供方
        let mut collector = AdapterManager::with_embedder(db.clone(), embedder.clone());
        collector.register(Arc::new(ZhihuAdapter));
        collector.register(Arc::new(BilibiliAdapter));
        collector.register(Arc::new(WeiboAdapter));
        // RSSHub 多路由聚合（platform_id = "rsshub"）
        let rsshub = Arc::new(RsshubAdapter::new(&config.fetch.rsshub_endpoint));
        collector.register(rsshub.clone());
        // 抖音/小红书：通过 RSSHub 单路由适配器独立注册，拥有独立调度周期
        for route in default_routes() {
            if route.platform == "douyin" || route.platform == "xiaohongshu" {
                collector.register(Arc::new(RsshubRouteAdapter::new(&config.fetch.rsshub_endpoint, route)));
            }
        }
        // 直采失败时用 RSSHub 对应路由兜底
        collector.set_rsshub(rsshub);
        let collector = Arc::new(collector);

        // Agent
        let embedder_for_agent = embedder.unwrap_or_else(|| {
            // 无 embedder 时用一个空实现占位（ensure_interest_embeddings 会优雅跳过）
            Arc::new(NoopEmbedder) as Arc<dyn EmbeddingProvider>
        });
        let mut orch = AgentOrchestrator::new(llm, embedder_for_agent, db.clone());
        orch.set_score_threshold(config.general.announce_threshold);
        orch.set_pet_profile(config.pet.clone());
        let agent = Mutex::new(orch);

        // Scheduler：回调里用 collector
        let live_config = Arc::new(Mutex::new(config.clone()));
        let collector_for_fetch = collector.clone();
        let config_for_fetch = live_config.clone();
        let fetch_cb: deskpet_scheduler::FetchFn = Arc::new(move |platform| {
            let collector = collector_for_fetch.clone();
            let config = config_for_fetch.clone();
            tokio::spawn(async move {
                let enabled = config.lock().await.fetch.platforms.iter().any(|p| p.id == platform && p.enabled);
                if !enabled { return; }
                let _ = collector.fetch_one(&platform).await;
            });
        });

        // Agent 由外部 run_agent_cycle 驱动（GUI/headless），调度器不启动 agent 任务
        let mut scheduler = Scheduler::new(fetch_cb, None);
        for id in ["zhihu", "bilibili", "weibo", "rsshub", "douyin", "xiaohongshu"] {
            let minutes = config.fetch.platforms.iter().find(|p| p.id == id)
                .map(|p| p.interval_min).unwrap_or(config.fetch.default_interval_min);
            scheduler.set_fetch_interval(id, Duration::from_secs(minutes.max(1) as u64 * 60));
        }
        scheduler.set_agent_interval(Duration::from_secs(config.general.min_announce_interval_min as u64 * 60));

        // 每日维护：画像权重衰减（0.99）+ 数据生命周期清理
        let db_for_daily = db.clone();
        let daily_cb: deskpet_scheduler::DailyFn = Arc::new(move || {
            let db = db_for_daily.clone();
            tokio::spawn(async move {
                let conn = db.lock().await;
                // 1. 画像权重衰减
                let mut profile = ProfileRepo::load(&conn).unwrap_or_default();
                profile.decay_weights(0.99);
                if let Err(e) = ProfileRepo::save(&conn, &profile) {
                    log::warn!("[daily] decay save failed: {e}");
                } else {
                    log::info!("[daily] weights decayed, {} interests", profile.interests.len());
                }
                // 2. 过期热点 + 向量清理（7 天）
                if let Err(e) = HotRepo::purge_expired(&conn) {
                    log::warn!("[daily] purge hot_items failed: {e}");
                }
                // 3. 孤儿向量清理（hot_items 已删除但 embedding 残留）
                match HotRepo::purge_orphan_embeddings(&conn) {
                    Ok(n) if n > 0 => log::info!("[daily] purged {n} orphan embeddings"),
                    Ok(_) => {}
                    Err(e) => log::warn!("[daily] purge orphan embeddings failed: {e}"),
                }
                // 4. 采集日志清理（30 天）
                if let Err(e) = FetchLogRepo::purge_expired(&conn) {
                    log::warn!("[daily] purge fetch_logs failed: {e}");
                }
                // 5. 反馈记录清理（30 天）
                match FeedbackRepo::purge_expired(&conn) {
                    Ok(n) if n > 0 => log::info!("[daily] purged {n} expired feedback"),
                    Ok(_) => {}
                    Err(e) => log::warn!("[daily] purge feedback failed: {e}"),
                }
                log::info!("[daily] maintenance complete");
            });
        });
        scheduler.set_daily_cb(daily_cb);

        let scheduler = Arc::new(scheduler);

        // TTS
        let tts: Option<Box<dyn TtsProvider>> = if config.tts.enabled {
            Some(Box::new(EdgeTts::new(&config.tts.voice)))
        } else {
            None
        };

        Self {
            config: live_config,
            db,
            collector,
            agent,
            scheduler,
            tts,
            manual_refresh: Mutex::new(()),
        }
    }

    /// 执行一次 Agent 周期，若决定播报则返回文案。
    pub async fn run_agent_cycle(&self) -> Option<deskpet_agent::AnnounceResult> {
        if !self.scheduler.can_announce().await {
            return None;
        }
        let mut agent = self.agent.lock().await;
        let result = agent.run_cycle().await;
        drop(agent);
        if result.is_some() {
            self.scheduler.mark_announced().await;
        }
        result
    }

    /// 强制执行一次 Agent 周期（跳过勿扰/静默/间隔检查，回看24h）。
    /// 用于"立即播报"等用户主动触发的场景。
    pub async fn run_agent_cycle_force(&self) -> Option<deskpet_agent::AnnounceResult> {
        let mut agent = self.agent.lock().await;
        let result = agent.run_cycle_force().await;
        drop(agent);
        if result.is_some() {
            self.scheduler.mark_announced().await;
        }
        result
    }

    /// A manual request uses only the results of its own bounded network refresh.
    pub async fn refresh_and_run_agent(&self) -> Result<Option<deskpet_agent::AnnounceResult>, String> {
        let _refresh = self.manual_refresh.try_lock().map_err(|_| "正在联网刷新，请稍候再试".to_string())?;
        let platforms: Vec<String> = self.config.lock().await.fetch.platforms.iter()
            .filter(|p| p.enabled).map(|p| p.id.clone()).collect();
        if platforms.is_empty() { return Err("请先在设置中启用至少一个采集平台".into()); }
        let mut tasks = tokio::task::JoinSet::new();
        for platform in platforms {
            let collector = self.collector.clone();
            tasks.spawn(async move {
                tokio::time::timeout(Duration::from_secs(20), collector.fetch_one_force(&platform)).await
            });
        }
        let mut fetched_ids = std::collections::HashSet::new();
        let mut succeeded = false;
        while let Some(result) = tasks.join_next().await {
            if let Ok(Ok(outcome)) = result {
                if outcome.status == "ok" {
                    succeeded = true;
                    fetched_ids.extend(outcome.fetched_ids);
                }
            }
        }
        if !succeeded { return Err("本次联网刷新失败，请检查网络或采集源后重试；没有复用旧热点。".into()); }
        let ids: Vec<String> = fetched_ids.into_iter().collect();
        let result = self.agent.lock().await.run_cycle_refreshed(&ids).await;
        if result.is_some() { self.scheduler.mark_announced().await; }
        Ok(result)
    }

    /// 没有可播报热点时，由 LLM 生成一句自然回应（不弹固定文案）。
    pub async fn idle_reply(&self) -> String {
        self.agent.lock().await.compose_idle_reply().await
    }

    /// 触发一次指定平台的采集。
    pub async fn fetch_platform(&self, platform: &str) -> deskpet_collector::FetchOutcome {
        if !self.config.lock().await.fetch.platforms.iter().any(|p| p.id == platform && p.enabled) {
            return deskpet_collector::FetchOutcome {
                fetched_ids: Vec::new(),
                platform: platform.into(), status: "disabled".into(), item_count: 0,
                duration_ms: 0, error: None,
            };
        }
        self.collector.fetch_one(platform).await
    }

    /// 取热点列表。
    pub fn list_hot(&self, limit: u32, platform: Option<&str>) -> Vec<deskpet_core::HotItem> {
        let conn = self.db.blocking_lock();
        HotRepo::list(&conn, limit, platform).unwrap_or_default()
    }

    /// 读取画像。
    pub async fn get_profile(&self) -> Result<UserProfile, String> {
        let conn = self.db.lock().await;
        ProfileRepo::load(&conn).map_err(|_| "读取用户画像失败".into())
    }

    /// 保存画像。
    pub async fn save_profile(&self, profile: &UserProfile) -> Result<(), String> {
        let conn = self.db.lock().await;
        ProfileRepo::save(&conn, profile).map_err(|_| "保存用户画像失败".into())
    }

    /// 处理用户对某热点的反馈：记录反馈 + 按热点标签调整画像权重 + 落库。
    /// 返回受影响的兴趣主题数。
    pub async fn apply_feedback(&self, hot_id: &str, action: FeedbackAction) -> usize {
        let now = chrono::Utc::now().timestamp();
        let delta = action.delta();
        let mut affected = 0usize;

        let conn = self.db.lock().await;
        // 1. 记录反馈
        if let Err(e) = FeedbackRepo::insert(&conn, hot_id, action) {
            log::warn!("[feedback] insert failed: {e}");
        }
        // 2. 取热点 tags 作为兴趣主题来源
        let tags = match HotRepo::get(&conn, hot_id) {
            Ok(Some(item)) => item.tags,
            _ => {
                log::info!("[feedback] hot item {hot_id} not found, skip profile adjust");
                drop(conn);
                return 0;
            }
        };
        // 3. 调整画像权重
        let mut profile = ProfileRepo::load(&conn).unwrap_or_default();
        if tags.is_empty() {
            log::info!("[feedback] hot item has no tags, cannot map to interest");
        } else {
            for tag in &tags {
                profile.adjust_topic(tag, delta, now);
                affected += 1;
            }
            log::info!(
                "[feedback] {action:?} on {hot_id}, adjusted {affected} topics by {delta:+.2}"
            );
        }
        // 4. 落库
        if let Err(e) = ProfileRepo::save(&conn, &profile) {
            log::warn!("[feedback] profile save failed: {e}");
        }
        affected
    }

    /// 每日画像权重衰减（避免旧兴趣永久置顶）。
    pub async fn daily_decay(&self) {
        let conn = self.db.lock().await;
        let mut profile = ProfileRepo::load(&conn).unwrap_or_default();
        profile.decay_weights(0.99);
        if let Err(e) = ProfileRepo::save(&conn, &profile) {
            log::warn!("[decay] save failed: {e}");
        } else {
            log::info!("[decay] weights decayed by 0.99, {} interests", profile.interests.len());
        }
    }

    /// 播报语音。
    pub async fn speak(&self, text: &str) {
        if let Some(tts) = &self.tts {
            if let Err(e) = tts.speak(text).await {
                log::warn!("[tts] {}", e);
            }
        }
    }

    /// 热重载配置：从磁盘重新读取 config.toml，将可热更新的项应用到运行时。
    ///
    /// 可热更新：勿扰时段、最小播报间隔、各平台采集间隔、Agent 轮询间隔、
    /// LLM/Embedding 实例（api_base/api_key/model/embedding_model 变化时重建）、
    /// 打分阈值。
    /// 不可热更新（需重启）：TTS 实例、已注册的 Adapter 列表。
    /// 返回 Ok(()) 表示重载成功。
    pub async fn reload_config(&self) -> Result<(), String> {
        let mut active_config = self.config.lock().await;
        let path = AppConfig::path();
        let new_cfg: AppConfig = match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).map_err(|e| format!("parse config: {e}"))?,
            Err(e) => return Err(format!("read config: {e}")),
        };

        // 1. 更新打扰策略（立即生效）
        self.scheduler
            .update_policy(
                new_cfg.general.quiet_hours.clone(),
                new_cfg.general.min_announce_interval_min,
            )
            .await;
        log::info!(
            "[reload] policy updated: quiet_hours={:?}, min_interval={}min",
            new_cfg.general.quiet_hours,
            new_cfg.general.min_announce_interval_min
        );

        // 2. 更新各平台采集间隔（下一轮 tick 生效）
        for p in &new_cfg.fetch.platforms {
                self.scheduler
                    .update_fetch_interval(&p.id, Duration::from_secs(p.interval_min as u64 * 60))
                    .await;
        }
        // 3. 更新 Agent 轮询间隔
        self.scheduler
            .update_agent_interval(Duration::from_secs(
                new_cfg.general.min_announce_interval_min as u64 * 60,
            ))
            .await;

        // 4. 热更新 LLM / Embedding（若配置变化）
        let llm_changed = active_config.llm != new_cfg.llm;
        if llm_changed {
            let new_llm_config = AgentLlmConfig {
                api_base: new_cfg.llm.api_base.clone(),
                api_key: new_cfg.llm.api_key.clone(),
                model: new_cfg.llm.model.clone(),
                embedding_model: new_cfg.llm.embedding_model.clone(),
            };
            let provider = Arc::new(OpenAiCompat::new(new_llm_config));
            let new_llm: Arc<dyn LlmProvider> = provider.clone();
            let new_embedder: Arc<dyn EmbeddingProvider> = if new_cfg.llm.embedding_enabled() {
                provider.clone()
            } else {
                Arc::new(NoopEmbedder)
            };

            // 更新 Agent 的 llm + embedder
            {
                let mut agent = self.agent.lock().await;
                agent.set_llm(new_llm);
                agent.set_embedder(new_embedder.clone());
                agent.set_score_threshold(new_cfg.general.announce_threshold);
            }
            // 更新 Collector 的 embedder
            let collector_embedder: Option<Arc<dyn EmbeddingProvider>> = if new_cfg.llm.embedding_enabled() {
                Some(provider.clone() as Arc<dyn EmbeddingProvider>)
            } else {
                None
            };
            self.collector.set_embedder(collector_embedder).await;

            let masked = if new_cfg.llm.api_key.is_empty() {
                "(empty)".to_string()
            } else {
                "(configured)".to_string()
            };
            log::info!(
                "[reload] LLM updated: base={} model={} key={}",
                new_cfg.llm.api_base, new_cfg.llm.model, masked
            );
        }

        // 5. 更新阈值 + 内存配置快照
        {
            let mut agent = self.agent.lock().await;
            agent.set_score_threshold(new_cfg.general.announce_threshold);
            agent.set_pet_profile(new_cfg.pet.clone());
        }
        *active_config = new_cfg;

        log::info!("[reload] config reloaded from {}", path.display());
        Ok(())
    }

    /// 启动配置文件监听：每 5 秒检查 config.toml 的修改时间，
    /// 若发生变化则自动热重载。无需手动触发。
    pub fn start_config_watcher(self: &Arc<Self>) {
        let state = self.clone();
        tokio::spawn(async move {
            let path = AppConfig::path();
            let mut last_mtime: Option<std::time::SystemTime> = None;
            loop {
                if let Ok(meta) = std::fs::metadata(&path) {
                    if let Ok(mtime) = meta.modified() {
                        if last_mtime.map_or(true, |t| mtime != t) {
                            if last_mtime.is_some() {
                                log::info!("[watcher] config changed, reloading...");
                                if let Err(e) = state.reload_config().await {
                                    log::warn!("[watcher] reload failed: {e}");
                                }
                            }
                            last_mtime = Some(mtime);
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }
}

// 让 HotAdapter trait 在本模块可见（避免未使用警告）
#[allow(dead_code)]
fn _adapter_used(_a: &dyn HotAdapter) {}
