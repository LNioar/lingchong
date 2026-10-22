//! RSSHub 聚合源 Adapter（兜底 + 覆盖抖音/小红书等不自采平台）。
//!
//! - `RsshubAdapter`：多路由聚合（platform_id = "rsshub"），一次抓取所有路由。
//! - `RsshubRouteAdapter`：单路由适配器，platform_id = 路由平台名（如 "douyin"），
//!   用于让抖音/小红书拥有独立调度周期，也用于直采失败时的兜底。
//!
//! 默认路由：/zhihu/hotlist, /bilibili/popular/all, /weibo/search/hot,
//!          /douyin/hot, /xiaohongshu/hot

use async_trait::async_trait;
use deskpet_core::{HotItem, TrustLevel};
use std::time::Duration;

use crate::adapter::{build_item, FetchContext, FetchError, HotAdapter};

/// RSSHub 路由配置。
#[derive(Clone)]
pub struct RsshubRoute {
    /// 该路由对应展示的平台名
    pub platform: &'static str,
    /// RSSHub 路径，如 "/zhihu/hotlist"
    pub path: &'static str,
}

/// 默认路由表（兜底用，覆盖直采平台 + 不自采平台）。
pub fn default_routes() -> Vec<RsshubRoute> {
    vec![
        RsshubRoute { platform: "zhihu", path: "/zhihu/hotlist" },
        RsshubRoute { platform: "bilibili", path: "/bilibili/popular/all" },
        RsshubRoute { platform: "weibo", path: "/weibo/search/hot" },
        RsshubRoute { platform: "douyin", path: "/douyin/hot" },
        RsshubRoute { platform: "xiaohongshu", path: "/xiaohongshu/hot" },
    ]
}

/// 多路由聚合适配器。
pub struct RsshubAdapter {
    pub endpoint: String,
    pub routes: Vec<RsshubRoute>,
}

impl RsshubAdapter {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self { endpoint: endpoint.into(), routes: default_routes() }
    }

    /// 抓取指定平台的单条 RSSHub 路由（兜底用）。
    pub async fn fetch_route(
        &self,
        ctx: &FetchContext,
        platform: &str,
    ) -> std::result::Result<Vec<HotItem>, FetchError> {
        let route = self
            .routes
            .iter()
            .find(|r| r.platform == platform)
            .ok_or_else(|| FetchError::Parse(format!("no rsshub route for {platform}")))?;
        let url = format!("{}{}", self.endpoint.trim_end_matches('/'), route.path);
        fetch_rss(&ctx.client, &url, route.platform, ctx.now).await
    }
}

#[async_trait]
impl HotAdapter for RsshubAdapter {
    fn platform_id(&self) -> &'static str { "rsshub" }
    fn default_interval(&self) -> Duration { Duration::from_secs(30 * 60) }

    async fn fetch(&self, ctx: &FetchContext) -> std::result::Result<Vec<HotItem>, FetchError> {
        let mut all = Vec::new();
        for route in &self.routes {
            let url = format!("{}{}", self.endpoint.trim_end_matches('/'), route.path);
            match fetch_rss(&ctx.client, &url, route.platform, ctx.now).await {
                Ok(items) => all.extend(items),
                Err(e) => {
                    // 单路由失败不影响其他
                    log::warn!("[rsshub/{}] {}", route.platform, e);
                }
            }
        }
        if all.is_empty() {
            return Err(FetchError::Parse("all rsshub routes failed".into()));
        }
        Ok(all)
    }
}

/// 单路由适配器：让抖音/小红书等平台拥有独立 platform_id 与调度周期。
pub struct RsshubRouteAdapter {
    pub endpoint: String,
    pub route: RsshubRoute,
}

impl RsshubRouteAdapter {
    pub fn new(endpoint: impl Into<String>, route: RsshubRoute) -> Self {
        Self { endpoint: endpoint.into(), route }
    }
}

#[async_trait]
impl HotAdapter for RsshubRouteAdapter {
    fn platform_id(&self) -> &'static str { self.route.platform }
    fn default_interval(&self) -> Duration { Duration::from_secs(30 * 60) }

    async fn fetch(&self, ctx: &FetchContext) -> std::result::Result<Vec<HotItem>, FetchError> {
        let url = format!("{}{}", self.endpoint.trim_end_matches('/'), self.route.path);
        fetch_rss(&ctx.client, &url, self.route.platform, ctx.now).await
    }
}

/// 简易 RSS 2.0 解析：用正则提取 <item><title>...</title><link>...</link></item>。
async fn fetch_rss(
    client: &reqwest::Client,
    url: &str,
    platform: &str,
    now: i64,
) -> std::result::Result<Vec<HotItem>, FetchError> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(FetchError::HttpStatus(resp.status().as_u16()));
    }
    let text = resp.text().await?;
    let mut out = Vec::new();
    let items: Vec<&str> = text.split("<item>").skip(1).collect();
    for (i, block) in items.iter().enumerate() {
        let title = extract_tag(block, "title");
        let link = extract_tag(block, "link");
        if title.is_empty() { continue; }
        let item = build_item(platform, (i + 1) as u32, &title, &link, now, TrustLevel::Aggregated);
        out.push(item);
    }
    Ok(out)
}

fn extract_tag(block: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    if let (Some(s), Some(e)) = (block.find(&open), block.find(&close)) {
        block[s + open.len()..e].trim().to_string()
    } else {
        String::new()
    }
}
