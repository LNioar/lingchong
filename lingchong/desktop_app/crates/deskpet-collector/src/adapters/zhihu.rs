//! 知乎热榜 Adapter。
//!
//! 接口：https://api.zhihu.com/topstory/hot-lists/total?limit=50

use async_trait::async_trait;
use deskpet_core::{HotItem, TrustLevel};
use serde_json::Value;
use std::time::Duration;

use crate::adapter::{build_item, FetchContext, FetchError, HotAdapter};

pub struct ZhihuAdapter;

#[async_trait]
impl HotAdapter for ZhihuAdapter {
    fn platform_id(&self) -> &'static str { "zhihu" }
    fn default_interval(&self) -> Duration { Duration::from_secs(10 * 60) }

    async fn fetch(&self, ctx: &FetchContext) -> std::result::Result<Vec<HotItem>, FetchError> {
        let url = "https://api.zhihu.com/topstory/hot-lists/total?limit=50";
        let resp = ctx.client
            .get(url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(FetchError::HttpStatus(status.as_u16()));
        }
        let json: Value = resp.json().await?;
        let data = json["data"].as_array().ok_or_else(|| FetchError::Parse("no data".into()))?;

        let mut out = Vec::with_capacity(data.len());
        for (i, v) in data.iter().enumerate() {
            let target = &v["target"];
            let title = target["title"].as_str().unwrap_or_default().to_string();
            if title.is_empty() { continue; }
            let id = target["id"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| target["id"].as_i64().map(|n| n.to_string()))
                .unwrap_or_default();
            let url = format!("https://www.zhihu.com/question/{id}");
            let detail = v["detail_text"].as_str().unwrap_or_default();
            let hot = parse_hot_value(detail);
            let mut item = build_item("zhihu", (i + 1) as u32, &title, &url, ctx.now, TrustLevel::Direct);
            item.hot_value = hot;
            out.push(item);
        }
        Ok(out)
    }
}

/// 解析知乎 detail_text 中的热度，如 "1234 万热度" -> 12340000。
fn parse_hot_value(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() { return None; }
    // 取第一个数字段
    let num_str: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let num: f64 = num_str.parse().ok()?;
    let mult = if s.contains('万') { 10_000.0 } else { 1.0 };
    Some((num * mult) as u64)
}
