//! AdapterManager：注册、限频、调度各 Adapter 并入库。
//!
//! 设计要点：rusqlite Connection 是 Send 但 !Sync，
//! 因此 &Connection 不能跨 await 持有。DB 操作全部同步、短暂加锁，
//! HTTP 异步抓取期间不持有 DB 连接。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use log::{info, warn};
use rusqlite::Connection;
use tokio::sync::Mutex;

use deskpet_core::{EmbeddingProvider, HotItem, Result, TrustLevel};
use deskpet_storage::{FetchLogRepo, HotRepo};

use crate::adapter::{FetchContext, HotAdapter, RateLimit};
use crate::adapters::RsshubAdapter;
use crate::dedup::{dedup, placeholder_embedding, DedupResult};

/// 采集结果统计。
pub struct FetchOutcome {
    /// IDs returned by this network request, including already stored items.
    pub fetched_ids: Vec<String>,
    pub platform: String,
    pub status: String,
    pub item_count: u32,
    pub duration_ms: u64,
    pub error: Option<String>,
}

pub struct AdapterManager {
    adapters: Vec<Arc<dyn HotAdapter>>,
    last_fetch: Mutex<HashMap<String, i64>>,
    client: reqwest::Client,
    semantic_threshold: f32,
    db: Arc<Mutex<Connection>>,
    /// 可选的 embedding 提供方；None 时降级为占位向量。
    /// 用 Mutex 包裹以支持运行时热替换（配置重载）。
    embedder: Mutex<Option<Arc<dyn EmbeddingProvider>>>,
    /// 可选的 RSSHub 适配器；用于直采失败时的兜底回退。
    rsshub: Option<Arc<RsshubAdapter>>,
}

impl AdapterManager {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self::with_embedder(db, None)
    }

    /// 带 embedding 提供方构造。
    pub fn with_embedder(db: Arc<Mutex<Connection>>, embedder: Option<Arc<dyn EmbeddingProvider>>) -> Self {
        // 构建失败时回退到默认 client，避免因 TLS 环境问题导致整个进程 panic
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; DeskPet/0.1)")
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            adapters: Vec::new(),
            last_fetch: Mutex::new(HashMap::new()),
            client,
            semantic_threshold: 0.85,
            db,
            embedder: Mutex::new(embedder),
            rsshub: None,
        }
    }

    /// 注入 RSSHub 适配器，用于直采失败时的兜底回退。
    pub fn set_rsshub(&mut self, rsshub: Arc<RsshubAdapter>) {
        self.rsshub = Some(rsshub);
    }

    /// 热更新 embedding 提供方（配置重载时调用）。
    pub async fn set_embedder(&self, embedder: Option<Arc<dyn EmbeddingProvider>>) {
        *self.embedder.lock().await = embedder;
    }

    pub fn register(&mut self, adapter: Arc<dyn HotAdapter>) {
        self.adapters.push(adapter);
    }

    pub fn platform_ids(&self) -> Vec<&'static str> {
        self.adapters.iter().map(|a| a.platform_id()).collect()
    }

    /// 执行单个平台的一次采集。HTTP 异步，DB 同步。
    pub async fn fetch_one(&self, platform: &str) -> FetchOutcome {
        self.fetch_one_inner(platform, false).await
    }

    /// An explicit user refresh bypasses the local scheduled-fetch cooldown.
    pub async fn fetch_one_force(&self, platform: &str) -> FetchOutcome {
        self.fetch_one_inner(platform, true).await
    }

    async fn fetch_one_inner(&self, platform: &str, force: bool) -> FetchOutcome {
        let start = Utc::now().timestamp_millis();
        let started_at = Utc::now().timestamp();

        let adapter = match self.adapters.iter().find(|a| a.platform_id() == platform) {
            Some(a) => a.clone(),
            None => {
                return FetchOutcome {
                    fetched_ids: Vec::new(),
                    platform: platform.to_string(),
                    status: "fail".into(),
                    item_count: 0,
                    duration_ms: 0,
                    error: Some(format!("adapter not found: {platform}")),
                };
            }
        };

        // 限频检查
        {
            let mut lf = self.last_fetch.lock().await;
            let rl: RateLimit = adapter.rate_limit();
            let min_interval = rl.per.as_secs() as i64 / rl.max_requests.max(1) as i64;
            let last = lf.get(platform).copied().unwrap_or(0);
            if !force && started_at - last < min_interval {
                return FetchOutcome {
                    fetched_ids: Vec::new(),
                    platform: platform.to_string(),
                    status: "skipped".into(),
                    item_count: 0,
                    duration_ms: (Utc::now().timestamp_millis() - start) as u64,
                    error: Some("rate limited".into()),
                };
            }
            lf.insert(platform.to_string(), started_at);
        }

        // 异步 HTTP 抓取（不持有 DB）
        let ctx = FetchContext { client: self.client.clone(), now: started_at };
        let result = adapter.fetch(&ctx).await;

        match result {
            Ok(items) => {
                let fetched_ids = items.iter().map(|item| item.id.clone()).collect();
                // 同步入库（短暂持有 DB）
                let inserted = self.upsert_items(items).await;
                let dur = (Utc::now().timestamp_millis() - start) as u64;
                {
                    let conn = self.db.lock().await;
                    let _ = FetchLogRepo::insert(&conn, platform, started_at, dur, "ok", inserted, None);
                }
                info!("[{}] fetched {} new items in {}ms", platform, inserted, dur);
                FetchOutcome { fetched_ids, platform: platform.to_string(), status: "ok".into(), item_count: inserted, duration_ms: dur, error: None }
            }
            Err(e) => {
                // RSSHub 兜底：直采失败时尝试对应 RSSHub 路由
                if let Some(rsshub) = &self.rsshub {
                    match rsshub.fetch_route(&ctx, platform).await {
                        Ok(items) => {
                            let fetched_ids = items.iter().map(|item| item.id.clone()).collect();
                            let inserted = self.upsert_items(items).await;
                            let dur = (Utc::now().timestamp_millis() - start) as u64;
                            {
                                let conn = self.db.lock().await;
                                let _ = FetchLogRepo::insert(&conn, platform, started_at, dur, "ok", inserted, Some("via rsshub fallback"));
                            }
                            info!("[{}] primary failed, rsshub fallback ok: {} items in {}ms", platform, inserted, dur);
                            return FetchOutcome { fetched_ids, platform: platform.to_string(), status: "ok".into(), item_count: inserted, duration_ms: dur, error: Some("via rsshub fallback".into()) };
                        }
                        Err(fe) => {
                            warn!("[{}] rsshub fallback also failed: {}", platform, fe);
                        }
                    }
                }
                let dur = (Utc::now().timestamp_millis() - start) as u64;
                let (status, err_msg): (&str, String) = match &e {
                    crate::adapter::FetchError::RateLimited => ("rate_limited", "rate limited".into()),
                    crate::adapter::FetchError::Blocked => ("blocked", "platform blocked".into()),
                    _ => ("fail", e.to_string()),
                };
                {
                    let conn = self.db.lock().await;
                    let _ = FetchLogRepo::insert(&conn, platform, started_at, dur, status, 0, Some(err_msg.as_str()));
                }
                warn!("[{}] fetch failed: {}", platform, err_msg);
                FetchOutcome { fetched_ids: Vec::new(), platform: platform.to_string(), status: status.into(), item_count: 0, duration_ms: dur, error: Some(err_msg) }
            }
        }
    }

    /// 去重 + 入库，返回新插入数量。
    /// embedding 在持锁前批量计算（HTTP 异步不持 DB 锁）。
    /// 入库前为无 tags 的条目做关键词规则打标，保证用户反馈能映射到兴趣主题。
    async fn upsert_items(&self, mut items: Vec<HotItem>) -> u32 {
        if items.is_empty() {
            return 0;
        }
        for item in &mut items {
            crate::tagging::fill_tags(item);
        }
        // 1. 批量计算 embedding（不持 DB 锁）
        let embeddings = self.batch_embeddings(&items).await;

        // 2. 锁库做精确去重 + 语义去重 + 入库
        let conn = self.db.lock().await;
        let mut inserted = 0u32;
        for (idx, item) in items.iter().enumerate() {
            let emb = &embeddings[idx];
            match dedup(&conn, item, Some(emb), self.semantic_threshold) {
                DedupResult::AlreadyExists => {
                    let mut it = item.clone();
                    it.first_seen = 0;
                    let _ = HotRepo::upsert(&conn, &it);
                }
                DedupResult::SameEventAs(_) | DedupResult::New => {
                    if HotRepo::upsert(&conn, item).is_ok() {
                        let _ = HotRepo::set_embedding(&conn, &item.id, &item.platform, item.first_seen, emb);
                        inserted += 1;
                    }
                }
            }
        }
        inserted
    }

    /// 批量为所有热点标题生成 embedding。
    /// 优先用真实 EmbeddingProvider；失败或无 provider 时降级占位向量。
    async fn batch_embeddings(&self, items: &[HotItem]) -> Vec<Vec<f32>> {
        // 默认占位
        let fallback: Vec<Vec<f32>> = items.iter().map(|i| placeholder_embedding(&i.title)).collect();
        let embedder = self.embedder.lock().await.clone();
        let Some(embedder) = embedder else {
            return fallback;
        };
        let titles: Vec<String> = items.iter().map(|i| i.title.clone()).collect();
        match embedder.embed(&titles).await {
            Ok(embs) if embs.len() == items.len() => embs,
            Ok(embs) => {
                warn!("[collector] embedding count mismatch ({} != {}), fallback to placeholder", embs.len(), items.len());
                fallback
            }
            Err(e) => {
                warn!("[collector] embedding failed ({}), fallback to placeholder", e);
                fallback
            }
        }
    }
}

#[allow(dead_code)]
fn _trust_used(_t: TrustLevel) {}
#[allow(dead_code)]
fn _result_used(_r: Result<()>) {}

#[cfg(test)]
mod refresh_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingAdapter(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl HotAdapter for CountingAdapter {
        fn platform_id(&self) -> &'static str { "test-refresh" }
        fn default_interval(&self) -> Duration { Duration::from_secs(600) }
        async fn fetch(&self, ctx: &FetchContext) -> std::result::Result<Vec<HotItem>, crate::adapter::FetchError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(vec![crate::adapter::build_item("test-refresh", 1, "真实抓取结果", "https://example.com/hot", ctx.now, TrustLevel::Direct)])
        }
    }

    #[tokio::test]
    async fn forced_refresh_calls_adapter_again_and_returns_existing_ids() {
        let connection = Connection::open_in_memory().unwrap();
        deskpet_storage::init_db(&connection).unwrap();
        let db = Arc::new(Mutex::new(connection));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut collector = AdapterManager::new(db);
        collector.register(Arc::new(CountingAdapter(calls.clone())));
        let first = collector.fetch_one("test-refresh").await;
        assert_eq!(first.status, "ok");
        assert_eq!(collector.fetch_one("test-refresh").await.status, "skipped");
        let second = collector.fetch_one_force("test-refresh").await;
        assert_eq!(second.status, "ok");
        assert_eq!(first.fetched_ids, second.fetched_ids);
        assert_eq!(second.fetched_ids.len(), 1);
        assert_eq!(second.item_count, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
