//! 热点采集 Adapter 抽象。

use async_trait::async_trait;
use deskpet_core::{DeskPetError, HotItem};
use std::fmt;
use std::time::Duration;

/// 限频配置（token bucket 参数）。
#[derive(Debug, Clone)]
pub struct RateLimit {
    /// 每个周期最多请求数
    pub max_requests: u32,
    /// 周期长度
    pub per: Duration,
}

impl Default for RateLimit {
    fn default() -> Self {
        Self { max_requests: 1, per: Duration::from_secs(60) }
    }
}

/// 采集上下文。每个 Adapter 通过它拿统一的 HTTP client 与时间。
#[derive(Clone)]
pub struct FetchContext {
    pub client: reqwest::Client,
    pub now: i64,
}

/// 采集错误。
#[derive(Debug)]
pub enum FetchError {
    Network(String),
    HttpStatus(u16),
    Parse(String),
    RateLimited,
    Blocked,
}

impl From<FetchError> for DeskPetError {
    fn from(e: FetchError) -> Self {
        match e {
            FetchError::Network(s) => DeskPetError::Network(s),
            FetchError::HttpStatus(c) => DeskPetError::HttpStatus(c),
            FetchError::Parse(s) => DeskPetError::Parse(s),
            FetchError::RateLimited => DeskPetError::RateLimited,
            FetchError::Blocked => DeskPetError::Blocked,
        }
    }
}

impl From<reqwest::Error> for FetchError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            FetchError::Network(e.to_string())
        } else if let Some(s) = e.status() {
            FetchError::HttpStatus(s.as_u16())
        } else {
            FetchError::Network(e.to_string())
        }
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::Network(s) => write!(f, "network: {s}"),
            FetchError::HttpStatus(c) => write!(f, "http status: {c}"),
            FetchError::Parse(s) => write!(f, "parse: {s}"),
            FetchError::RateLimited => write!(f, "rate limited"),
            FetchError::Blocked => write!(f, "blocked by platform"),
        }
    }
}

/// 热点采集 Adapter 契约。新增平台只需实现此 trait。
#[async_trait]
pub trait HotAdapter: Send + Sync {
    /// 平台唯一标识，如 "zhihu"。
    fn platform_id(&self) -> &'static str;

    /// 默认采集周期。
    fn default_interval(&self) -> Duration { Duration::from_secs(15 * 60) }

    /// 限频配置。
    fn rate_limit(&self) -> RateLimit { RateLimit::default() }

    /// 抓取并返回标准化热点列表。
    async fn fetch(&self, ctx: &FetchContext) -> std::result::Result<Vec<HotItem>, FetchError>;
}

/// 便捷构造函数：把 (platform, title) 转成 HotItem 骨架。
pub fn build_item(platform: &str, rank: u32, title: &str, url: &str, now: i64, trust: deskpet_core::TrustLevel) -> HotItem {
    HotItem {
        id: HotItem::make_id(platform, title),
        platform: platform.to_string(),
        rank,
        title: title.to_string(),
        hot_value: None,
        url: url.to_string(),
        tags: vec![],
        summary: None,
        first_seen: now,
        last_seen: now,
        source_trust: trust,
    }
}
