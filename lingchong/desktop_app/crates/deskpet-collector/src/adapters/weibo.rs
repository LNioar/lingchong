//! 微博热搜 Adapter。
//!
//! 接口：https://m.weibo.cn/... （移动端半公开接口）
//! 可靠性：对瞬时错误（网络/超时/5xx/限流）进行最多 3 次重试，指数退避。

use async_trait::async_trait;
use deskpet_core::{HotItem, TrustLevel};
use serde_json::Value;
use std::time::Duration;

use crate::adapter::{build_item, FetchContext, FetchError, HotAdapter};

pub struct WeiboAdapter;

#[async_trait]
impl HotAdapter for WeiboAdapter {
    fn platform_id(&self) -> &'static str { "weibo" }
    fn default_interval(&self) -> Duration { Duration::from_secs(20 * 60) }

    async fn fetch(&self, ctx: &FetchContext) -> std::result::Result<Vec<HotItem>, FetchError> {
        const MAX_ATTEMPTS: u32 = 3;
        const BASE_DELAY: u64 = 1; // 秒

        let mut last_err: Option<FetchError> = None;
        for attempt in 1..=MAX_ATTEMPTS {
            match fetch_weibo(ctx).await {
                Ok(items) => return Ok(items),
                Err(e) => {
                    let retryable = is_retryable(&e);
                    last_err = Some(e);
                    if !retryable || attempt == MAX_ATTEMPTS {
                        break;
                    }
                    let delay = Duration::from_secs(BASE_DELAY * 2u64.pow(attempt - 1));
                    log::info!("[weibo] attempt {attempt} failed, retrying in {:?}", delay);
                    tokio::time::sleep(delay).await;
                }
            }
        }
        Err(last_err.unwrap_or_else(|| FetchError::Network("fetch_weibo produced no result".into())))
    }
}

/// 判断错误是否可重试：网络错误/限流/5xx 服务端错误。
fn is_retryable(e: &FetchError) -> bool {
    match e {
        FetchError::Network(_) | FetchError::RateLimited => true,
        FetchError::HttpStatus(c) => *c >= 500,
        _ => false,
    }
}

async fn fetch_weibo(ctx: &FetchContext) -> std::result::Result<Vec<HotItem>, FetchError> {
    // 移动端半公开接口
    let url = "https://m.weibo.cn/api/container/getIndex?containerid=106003type%3D25%26t%3D3%26disable_hot%3D1%26filter_type%3Drealtimehot";
    let resp = ctx.client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) AppleWebKit/605.1.15")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(FetchError::HttpStatus(resp.status().as_u16()));
    }
    let json: Value = resp.json().await?;
    let cards = json["data"]["cards"].as_array().ok_or_else(|| FetchError::Parse("no cards".into()))?;
    let card_group = cards
        .iter()
        .find_map(|c| c["card_group"].as_array())
        .ok_or_else(|| FetchError::Parse("no card_group".into()))?;

    let mut out = Vec::with_capacity(card_group.len());
    for (i, v) in card_group.iter().enumerate() {
        let desc = v["desc"].as_str().unwrap_or_default().to_string();
        if desc.is_empty() { continue; }
        let scheme = v["scheme"].as_str().unwrap_or_default().to_string();
        let hot = v["desc_extr"].as_u64();
        let mut item = build_item("weibo", (i + 1) as u32, &desc, &scheme, ctx.now, TrustLevel::Direct);
        item.hot_value = hot;
        out.push(item);
    }
    Ok(out)
}
