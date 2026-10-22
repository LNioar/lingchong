//! B站热门 Adapter。
//!
//! 接口：https://api.bilibili.com/x/web-interface/popular?ps=50

use async_trait::async_trait;
use deskpet_core::{HotItem, TrustLevel};
use serde_json::Value;
use std::time::Duration;

use crate::adapter::{build_item, FetchContext, FetchError, HotAdapter};

pub struct BilibiliAdapter;

#[async_trait]
impl HotAdapter for BilibiliAdapter {
    fn platform_id(&self) -> &'static str { "bilibili" }
    fn default_interval(&self) -> Duration { Duration::from_secs(15 * 60) }

    async fn fetch(&self, ctx: &FetchContext) -> std::result::Result<Vec<HotItem>, FetchError> {
        let url = "https://api.bilibili.com/x/web-interface/popular?ps=50&pn=1";
        let resp = ctx.client
            .get(url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(FetchError::HttpStatus(resp.status().as_u16()));
        }
        let json: Value = resp.json().await?;
        let code = json["code"].as_i64().unwrap_or(-1);
        if code != 0 {
            return Err(FetchError::Parse(format!("bilibili api code={code}")));
        }
        let list = json["data"]["list"].as_array().ok_or_else(|| FetchError::Parse("no list".into()))?;

        let mut out = Vec::with_capacity(list.len());
        for (i, v) in list.iter().enumerate() {
            let title = v["title"].as_str().unwrap_or_default().to_string();
            if title.is_empty() { continue; }
            let bvid = v["bvid"].as_str().unwrap_or_default();
            let url = format!("https://www.bilibili.com/video/{bvid}");
            let hot = v["stat"]["view"].as_u64();
            let mut item = build_item("bilibili", (i + 1) as u32, &title, &url, ctx.now, TrustLevel::Direct);
            item.hot_value = hot;
            if let Some(tname) = v["tname"].as_str() {
                item.tags.push(tname.to_string());
            }
            out.push(item);
        }
        Ok(out)
    }
}
