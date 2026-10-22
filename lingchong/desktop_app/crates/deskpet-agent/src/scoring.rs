//! 兴趣匹配打分算法。
//!
//! score = 0.6*sim + 0.2*heat + 0.15*fresh + 0.05*penalty
//! 其中 sim 用热点 embedding 与兴趣 embedding 的最大余弦相似度。
//!
//! sim 只有在一个兴趣持有真实向量时才参与计算。embedding 不可用时（未配置
//! embedding_model、服务端不支持 /embeddings、补算失败等），sim 恒为 0，
//! 总分上限只剩 0.4，因此必须用 [`effective_threshold`] 把配置阈值按可用分量折算，
//! 否则自动播报永远达不到阈值。

use chrono::Utc;
use rusqlite::Connection;

use deskpet_core::{HotItem, UserProfile};
use deskpet_storage::{FeedbackRepo, vector};

/// 兴趣相似度权重。
pub const W_SIM: f32 = 0.6;
/// 热度权重。
pub const W_HEAT: f32 = 0.2;
/// 新鲜度权重。
pub const W_FRESH: f32 = 0.15;
/// 反馈惩罚权重。
pub const W_PENALTY: f32 = 0.05;

/// 单条打分。
/// 热点 embedding 从 hot_embeddings 表读（由 collector 写入真实向量）；
/// 兴趣 embedding 从 Interest.embedding 字段读（由 orchestrator 补算并持久化）。
/// `sim_available` 为 false 时跳过相似度分量（此时占位向量只是噪声，不应参与）。
pub fn score(conn: &Connection, item: &HotItem, profile: &UserProfile, sim_available: bool) -> f32 {
    // 1. 兴趣相似度（取各兴趣中最高的）
    let sim = if sim_available { interest_similarity(conn, item, profile) } else { 0.0 };

    // 2. 热度归一化（同平台内用固定上限近似）
    let heat = normalize_hot(item.hot_value);

    // 3. 新鲜度
    let hours = (Utc::now().timestamp() - item.first_seen) as f32 / 3600.0;
    let fresh = 1.0 / (1.0 + hours * 0.1);

    // 4. 近期被忽略惩罚
    let penalty = if FeedbackRepo::was_recently_ignored(conn, &item.id, 3 * 86400).unwrap_or(false) {
        0.3
    } else {
        1.0
    };

    W_SIM * sim + W_HEAT * heat + W_FRESH * fresh + W_PENALTY * penalty
}

/// 兴趣向量是否可用于相似度打分：至少有一个有权重且已持久化真实向量的兴趣。
pub fn has_usable_embeddings(profile: &UserProfile) -> bool {
    profile.interests.iter().any(|i| {
        i.weight > 0.0 && i.embedding.as_ref().map_or(false, |e| !e.is_empty())
    })
}

/// 按当前可用分量把配置阈值折算成等效阈值。
///
/// 配置里的 0.6 是"满分 1.0 的 60%"。当 sim 不可用时满分只有 0.4，
/// 直接与 0.6 比较在数学上不可能成立，因此折算为 0.6 * 0.4 = 0.24。
pub fn effective_threshold(configured: f32, sim_available: bool) -> f32 {
    let scale = if sim_available { 1.0 } else { W_HEAT + W_FRESH + W_PENALTY };
    (configured * scale).clamp(0.0, 1.0)
}

/// 兴趣相似度：只统计持有真实向量的兴趣，并按全局最大权重归一化。
///
/// 权重只表达兴趣之间的相对强弱，不能当作绝对折扣：兴趣初始权重为 0.5 且每日
/// 衰减 0.99，若直接乘权重，即使完美命中 sim 也只有 0.5 左右，阈值同样不可达。
fn interest_similarity(conn: &Connection, item: &HotItem, profile: &UserProfile) -> f32 {
    let max_weight = profile.interests.iter().map(|i| i.weight).fold(0.0f32, f32::max);
    if max_weight <= 0.0 {
        return 0.0;
    }
    let he = hot_embedding(conn, &item.id, &item.title);
    profile.interests.iter()
        .filter_map(|i| {
            // 缺失真实向量的兴趣不参与：占位向量与真实向量维度不同，余弦恒为 0
            let ie = i.embedding.as_ref().filter(|e| !e.is_empty())?;
            if i.weight <= 0.0 {
                return None;
            }
            Some(vector::cosine(&he, ie).max(0.0) * (i.weight / max_weight))
        })
        .fold(0.0f32, f32::max)
}

/// 读取热点 embedding：优先从 hot_embeddings 表取真实向量，
/// 表中无记录则用标题生成占位向量（降级，保证打分不崩）。
fn hot_embedding(conn: &Connection, id: &str, fallback_title: &str) -> Vec<f32> {
    use rusqlite::params;
    let stmt = conn.prepare("SELECT embedding FROM hot_embeddings WHERE id = ?1").ok();
    if let Some(mut s) = stmt {
        if let Ok(mut rows) = s.query(params![id]) {
            if let Ok(Some(r)) = rows.next() {
                let blob: Vec<u8> = r.get(0).unwrap_or_default();
                if !blob.is_empty() {
                    return vector::decode(&blob);
                }
            }
        }
    }
    placeholder_embedding(fallback_title)
}

/// 占位 embedding：EmbeddingProvider 不可用时降级用，与 collector 中保持一致。
pub fn placeholder_embedding(title: &str) -> Vec<f32> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    title.hash(&mut h);
    let mut s = h.finish();
    let mut v = vec![0.0f32; 32];
    for x in v.iter_mut() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *x = ((s >> 33) as f32) / (u32::MAX as f32) * 2.0 - 1.0;
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 { for x in v.iter_mut() { *x /= norm; } }
    v
}

/// 热度归一化到 0~1。各平台热度量级差异大，这里用 log 压缩。
fn normalize_hot(hot: Option<u64>) -> f32 {
    match hot {
        None => 0.5,
        Some(v) if v == 0 => 0.0,
        Some(v) => {
            let l = (v as f64).ln();
            (l / 20.0).min(1.0) as f32
        }
    }
}

/// 打分阈值（有兴趣画像时）。
pub const SCORE_THRESHOLD: f32 = 0.6;

/// 冷启动阈值（无兴趣画像时）：只用 heat+fresh，阈值降低让热点能播出。
pub const COLD_START_THRESHOLD: f32 = 0.3;

#[cfg(test)]
mod tests {
    use super::*;
    use deskpet_core::{HotItem, Interest, UserProfile, TrustLevel};
    use deskpet_storage::schema;
    use deskpet_storage::vector;
    use deskpet_storage::HotRepo;
    use rusqlite::Connection;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        schema::init_db(&c).unwrap();
        c
    }

    fn make_hot(id: &str, title: &str, hot: Option<u64>, first_seen: i64) -> HotItem {
        HotItem {
            id: id.into(),
            platform: "zhihu".into(),
            rank: 1,
            title: title.into(),
            hot_value: hot,
            url: format!("https://example.com/{id}"),
            tags: vec![],
            summary: None,
            first_seen,
            last_seen: first_seen,
            source_trust: TrustLevel::Direct,
        }
    }

    /// 单位向量（维度=2），便于控制余弦相似度。
    fn unit_emb(x: f32, y: f32) -> Vec<f32> {
        let n = (x * x + y * y).sqrt();
        vec![x / n, y / n]
    }

    #[test]
    fn score_zero_when_no_interests() {
        let conn = mem_db();
        let item = make_hot("h1", "AI新闻", Some(1_000_000), Utc::now().timestamp());
        HotRepo::upsert(&conn, &item).unwrap();
        // 热点 embedding 为 (1,0)
        HotRepo::set_embedding(&conn, "h1", "zhihu", item.first_seen, &unit_emb(1.0, 0.0)).unwrap();
        let profile = UserProfile::default(); // 无兴趣
        let s = score(&conn, &item, &profile, true);
        // sim=0，但 heat+fresh+penalty 仍有分，应在 (0, 阈值) 之间
        assert!(s > 0.0 && s < SCORE_THRESHOLD);
    }

    #[test]
    fn score_high_when_interest_matches_embedding() {
        let conn = mem_db();
        let item = make_hot("h2", "AI新闻", Some(5_000_000), Utc::now().timestamp());
        HotRepo::upsert(&conn, &item).unwrap();
        // 热点向量 (1,0)
        HotRepo::set_embedding(&conn, "h2", "zhihu", item.first_seen, &unit_emb(1.0, 0.0)).unwrap();
        let mut profile = UserProfile::default();
        // 兴趣向量与热点完全同向 (1,0)，余弦=1.0
        profile.interests.push(Interest {
            topic: "AI".into(),
            weight: 1.0,
            last_hit: 0,
            embedding: Some(unit_emb(1.0, 0.0)),
        });
        let s = score(&conn, &item, &profile, true);
        // sim=1.0 → 0.6*1.0 = 0.6，加上 heat/fresh/penalty 应超过阈值
        assert!(s >= SCORE_THRESHOLD, "score={s} should be >= threshold");
    }

    #[test]
    fn score_low_when_interest_orthogonal() {
        let conn = mem_db();
        let item = make_hot("h3", "体育新闻", Some(1_000_000), Utc::now().timestamp());
        HotRepo::upsert(&conn, &item).unwrap();
        HotRepo::set_embedding(&conn, "h3", "zhihu", item.first_seen, &unit_emb(1.0, 0.0)).unwrap();
        let mut profile = UserProfile::default();
        // 兴趣向量正交 (0,1)，余弦=0
        profile.interests.push(Interest {
            topic: "体育".into(),
            weight: 1.0,
            last_hit: 0,
            embedding: Some(unit_emb(0.0, 1.0)),
        });
        let s = score(&conn, &item, &profile, true);
        // sim≈0，分数主要靠 heat/fresh，应低于匹配情况
        assert!(s < SCORE_THRESHOLD);
    }

    #[test]
    fn score_penalized_when_recently_ignored() {
        let conn = mem_db();
        let now = Utc::now().timestamp();
        let item = make_hot("h4", "被忽略的新闻", Some(5_000_000), now);
        HotRepo::upsert(&conn, &item).unwrap();
        HotRepo::set_embedding(&conn, "h4", "zhihu", now, &unit_emb(1.0, 0.0)).unwrap();
        // 插入一条近期 ignore 反馈
        deskpet_storage::FeedbackRepo::insert(&conn, "h4", deskpet_core::FeedbackAction::Ignore).unwrap();

        let mut profile = UserProfile::default();
        profile.interests.push(Interest {
            topic: "AI".into(),
            weight: 1.0,
            last_hit: 0,
            embedding: Some(unit_emb(1.0, 0.0)),
        });

        // 有 ignore 惩罚的分数
        let s_penalized = score(&conn, &item, &profile, true);

        // 删掉 ignore 反馈再打分
        conn.execute("DELETE FROM feedback WHERE hot_id = 'h4'", []).unwrap();
        let s_clean = score(&conn, &item, &profile, true);

        assert!(s_penalized < s_clean, "penalized={s_penalized} should be < clean={s_clean}");
    }

    #[test]
    fn score_decreases_with_age() {
        let conn = mem_db();
        let now = Utc::now().timestamp();
        let fresh = make_hot("h5", "新鲜新闻", Some(2_000_000), now);
        let old = make_hot("h6", "旧闻", Some(2_000_000), now - 7200); // 2 小时前
        HotRepo::upsert(&conn, &fresh).unwrap();
        HotRepo::upsert(&conn, &old).unwrap();
        HotRepo::set_embedding(&conn, "h5", "zhihu", now, &unit_emb(1.0, 0.0)).unwrap();
        HotRepo::set_embedding(&conn, "h6", "zhihu", now - 7200, &unit_emb(1.0, 0.0)).unwrap();
        let mut profile = UserProfile::default();
        profile.interests.push(Interest {
            topic: "AI".into(), weight: 1.0, last_hit: 0,
            embedding: Some(unit_emb(1.0, 0.0)),
        });
        let s_fresh = score(&conn, &fresh, &profile, true);
        let s_old = score(&conn, &old, &profile, true);
        assert!(s_fresh > s_old, "fresh={s_fresh} should be > old={s_old}");
    }

    #[test]
    fn normalize_hot_handles_none_and_zero() {
        assert_eq!(normalize_hot(None), 0.5);
        assert_eq!(normalize_hot(Some(0)), 0.0);
        // 大热值应接近 1.0
        assert!(normalize_hot(Some(10_000_000)) > 0.8);
    }

    #[test]
    fn effective_threshold_is_scaled_when_sim_unavailable() {
        assert!((effective_threshold(0.6, true) - 0.6).abs() < 1e-6);
        let scaled = effective_threshold(0.6, false);
        assert!((scaled - 0.24).abs() < 1e-5, "scaled={scaled}");
    }

    /// 没有 embedding 时，靠热度+新鲜度也应能过折算后的阈值（修复前永远达不到 0.6）。
    #[test]
    fn embedding_less_profile_can_still_announce() {
        let conn = mem_db();
        let item = make_hot("h7", "刚出炉的热点", Some(5_000_000), Utc::now().timestamp());
        HotRepo::upsert(&conn, &item).unwrap();
        let mut profile = UserProfile::default();
        profile.interests.push(Interest {
            topic: "游戏".into(), weight: 0.5, last_hit: 0, embedding: None,
        });
        assert!(!has_usable_embeddings(&profile));

        let s = score(&conn, &item, &profile, false);
        let threshold = effective_threshold(SCORE_THRESHOLD, false);
        assert!(s >= threshold, "score={s} threshold={threshold}");
    }

    /// 兴趣权重 0.5 属于常态（初始值 + 每日衰减），归一化后完美命中仍应达到原阈值。
    #[test]
    fn weak_interest_weight_still_reaches_threshold() {
        let conn = mem_db();
        let item = make_hot("h8", "AI 新模型发布", Some(5_000_000), Utc::now().timestamp());
        HotRepo::upsert(&conn, &item).unwrap();
        HotRepo::set_embedding(&conn, "h8", "zhihu", item.first_seen, &unit_emb(1.0, 0.0)).unwrap();
        let mut profile = UserProfile::default();
        profile.interests.push(Interest {
            topic: "AI".into(), weight: 0.5, last_hit: 0, embedding: Some(unit_emb(1.0, 0.0)),
        });
        assert!(has_usable_embeddings(&profile));

        let s = score(&conn, &item, &profile, true);
        assert!(s >= SCORE_THRESHOLD, "score={s} should be >= {SCORE_THRESHOLD}");
    }

    /// 兴趣没有真实向量时不应拿占位向量凑分数，即使 sim_available 误传为 true。
    #[test]
    fn interest_without_vector_contributes_nothing() {
        let conn = mem_db();
        let item = make_hot("h9", "AI 新模型发布", Some(5_000_000), Utc::now().timestamp());
        HotRepo::upsert(&conn, &item).unwrap();
        HotRepo::set_embedding(&conn, "h9", "zhihu", item.first_seen, &unit_emb(1.0, 0.0)).unwrap();
        let mut profile = UserProfile::default();
        profile.interests.push(Interest {
            topic: "AI".into(), weight: 1.0, last_hit: 0, embedding: None,
        });

        let with_sim = score(&conn, &item, &profile, true);
        let without_sim = score(&conn, &item, &profile, false);
        assert!((with_sim - without_sim).abs() < 1e-6, "sim 不应凭空产生：{with_sim} vs {without_sim}");
    }

    #[test]
    fn placeholder_embedding_is_normalized() {
        let v = placeholder_embedding("test title");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "norm={norm} should be ~1.0");
        assert_eq!(v.len(), 32);
    }

    #[test]
    fn cosine_identical_vectors_is_one() {
        let a = unit_emb(1.0, 0.0);
        assert!((vector::cosine(&a, &a) - 1.0).abs() < 1e-4);
    }
}
