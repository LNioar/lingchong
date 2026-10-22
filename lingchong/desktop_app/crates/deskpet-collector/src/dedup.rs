//! 去重逻辑：精确去重 + 语义近邻去重。

use deskpet_core::HotItem;
use rusqlite::Connection;

use deskpet_storage::{HotRepo};

/// 去重结果。
#[derive(Debug)]
pub enum DedupResult {
    /// 全新热点
    New,
    /// 已存在（精确 id 命中），只刷新 last_seen
    AlreadyExists,
    /// 与已有热点语义相同（同一事件），关联到已有 id
    SameEventAs(String),
}

/// 对单条热点做去重判定。
/// - `emb_fn`: 计算该热点 embedding 的函数（通常调 LLM embedding 接口）。
pub fn dedup(
    conn: &Connection,
    item: &HotItem,
    emb: Option<&[f32]>,
    semantic_threshold: f32,
) -> DedupResult {
    // 1. 精确去重
    if HotRepo::get(conn, &item.id).ok().flatten().is_some() {
        return DedupResult::AlreadyExists;
    }
    // 2. 语义去重（24h 窗口内）
    if let Some(e) = emb {
        let since = item.first_seen - 24 * 3600;
        if let Ok(Some((id, _))) = HotRepo::nearest_neighbor(conn, e, since, semantic_threshold) {
            return DedupResult::SameEventAs(id);
        }
    }
    DedupResult::New
}

/// 占位 embedding：在没有 LLM embedding 时，用一个基于标题的简单向量。
/// 仅在 EmbeddingProvider 不可用或调用失败时降级使用。
pub fn placeholder_embedding(title: &str) -> Vec<f32> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    title.hash(&mut h);
    let seed = h.finish();
    // 32 维伪随机向量
    let mut v = vec![0.0f32; 32];
    let mut s = seed;
    for x in v.iter_mut() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *x = ((s >> 33) as f32) / (u32::MAX as f32) * 2.0 - 1.0;
    }
    // 归一化
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() { *x /= norm; }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskpet_core::{HotItem, TrustLevel};
    use deskpet_storage::schema;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        schema::init_db(&c).unwrap();
        c
    }

    fn make_item(platform: &str, title: &str, now: i64) -> HotItem {
        HotItem {
            id: HotItem::make_id(platform, title),
            platform: platform.to_string(),
            rank: 1,
            title: title.to_string(),
            hot_value: None,
            url: format!("https://example.com/{title}"),
            tags: vec![],
            summary: None,
            first_seen: now,
            last_seen: now,
            source_trust: TrustLevel::Direct,
        }
    }

    #[test]
    fn dedup_new_item() {
        let conn = mem_db();
        let item = make_item("zhihu", "DeepSeek桌面版发布", 1000);
        let emb = vec![1.0, 0.0, 0.0];
        assert!(matches!(dedup(&conn, &item, Some(&emb), 0.85), DedupResult::New));
    }

    #[test]
    fn dedup_exact_already_exists() {
        let conn = mem_db();
        let item = make_item("zhihu", "DeepSeek桌面版发布", 1000);
        let emb = vec![1.0, 0.0, 0.0];
        HotRepo::upsert(&conn, &item).unwrap();
        HotRepo::set_embedding(&conn, &item.id, &item.platform, 1000, &emb).unwrap();
        assert!(matches!(dedup(&conn, &item, Some(&emb), 0.85), DedupResult::AlreadyExists));
    }

    /// 验证：当真实 embedding 让两条不同平台标题在语义上相近时，
    /// nearest_neighbor 会命中并把第二条关联为同一事件（SameEventAs）。
    /// 这证明真实 embedding 接入后语义去重链路生效。
    #[test]
    fn dedup_same_event_across_platforms() {
        let conn = mem_db();
        // A: 知乎 "DeepSeek桌面版发布"
        let a = make_item("zhihu", "DeepSeek桌面版发布", 1000);
        let emb_a = vec![1.0, 0.0, 0.0];
        HotRepo::upsert(&conn, &a).unwrap();
        HotRepo::set_embedding(&conn, &a.id, &a.platform, 1000, &emb_a).unwrap();
        // B: B站 "DeepSeek Harness 桌面版正式发布"（不同 id，但 embedding 相同 → cosine=1.0）
        let b = make_item("bilibili", "DeepSeek Harness 桌面版正式发布", 1000);
        let emb_b = vec![1.0, 0.0, 0.0];
        match dedup(&conn, &b, Some(&emb_b), 0.85) {
            DedupResult::SameEventAs(id) => assert_eq!(id, a.id),
            other => panic!("expected SameEventAs, got {other:?}"),
        }
    }

    /// 向量不相似时不应误判为同事件。
    #[test]
    fn dedup_unrelated_not_same_event() {
        let conn = mem_db();
        let a = make_item("zhihu", "某明星离婚", 1000);
        let emb_a = vec![1.0, 0.0, 0.0];
        HotRepo::upsert(&conn, &a).unwrap();
        HotRepo::set_embedding(&conn, &a.id, &a.platform, 1000, &emb_a).unwrap();
        let b = make_item("bilibili", "DeepSeek发布", 1000);
        let emb_b = vec![0.0, 1.0, 0.0]; // 正交 → cosine=0
        assert!(matches!(dedup(&conn, &b, Some(&emb_b), 0.85), DedupResult::New));
    }

    /// 无 embedding 时不做语义去重，直接返回 New（除非精确命中）。
    #[test]
    fn dedup_no_embedding_returns_new() {
        let conn = mem_db();
        let a = make_item("zhihu", "新闻A", 1000);
        let emb_a = vec![1.0, 0.0, 0.0];
        HotRepo::upsert(&conn, &a).unwrap();
        HotRepo::set_embedding(&conn, &a.id, &a.platform, 1000, &emb_a).unwrap();
        // 不同标题、无 embedding → New
        let b = make_item("bilibili", "完全不同新闻", 1000);
        assert!(matches!(dedup(&conn, &b, None, 0.85), DedupResult::New));
    }

    /// 超过 24h 窗口的相似热点不做语义去重。
    #[test]
    fn dedup_outside_24h_window_is_new() {
        let conn = mem_db();
        let old_time = 1000;
        let a = make_item("zhihu", "DeepSeek发布", old_time);
        let emb_a = vec![1.0, 0.0, 0.0];
        HotRepo::upsert(&conn, &a).unwrap();
        HotRepo::set_embedding(&conn, &a.id, &a.platform, old_time, &emb_a).unwrap();
        // 25 小时后再来一条相同 embedding 的热点
        let new_time = old_time + 25 * 3600;
        let b = make_item("bilibili", "DeepSeek发布新消息", new_time);
        let emb_b = vec![1.0, 0.0, 0.0];
        assert!(matches!(dedup(&conn, &b, Some(&emb_b), 0.85), DedupResult::New));
    }

    /// 精确去重优先于语义去重：同 id 即使向量不同也返回 AlreadyExists。
    #[test]
    fn dedup_exact_takes_priority_over_semantic() {
        let conn = mem_db();
        let a = make_item("zhihu", "同一条新闻", 1000);
        HotRepo::upsert(&conn, &a).unwrap();
        // 用完全不同的向量再次去重同 id
        let different_emb = vec![0.0, 1.0, 0.0];
        assert!(matches!(dedup(&conn, &a, Some(&different_emb), 0.85), DedupResult::AlreadyExists));
    }

    #[test]
    fn placeholder_embedding_is_normalized() {
        let v = placeholder_embedding("任意标题");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "norm={norm}");
        assert_eq!(v.len(), 32);
    }
}
