//! Repository 层：对 SQLite 的所有读写封装。
//!
//! 所有方法接收 `&Connection`，调用方负责事务与并发控制。
//! （生产环境应改为 `Mutex<Connection>` 或连接池。）

use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json;

use deskpet_core::{FeedbackAction, HotItem, TrustLevel, UserProfile};
use crate::vector;

const HOT_RETENTION_DAYS: i64 = 7;
const LOG_RETENTION_DAYS: i64 = 30;
const FEEDBACK_RETENTION_DAYS: i64 = 30;

pub struct HotRepo;

pub struct AnnouncementRepo;
impl AnnouncementRepo {
    pub fn contains(conn: &Connection, hot_id: &str) -> rusqlite::Result<bool> {
        conn.query_row("SELECT EXISTS(SELECT 1 FROM announcements WHERE hot_id=?1)", params![hot_id], |row| row.get(0))
    }

    pub fn record(conn: &Connection, hot_id: &str) -> rusqlite::Result<()> {
        conn.execute("INSERT OR IGNORE INTO announcements(hot_id,announced_at) VALUES(?1,?2)", params![hot_id, Utc::now().timestamp()])?;
        Ok(())
    }
}

impl HotRepo {
    /// 插入或更新热点（已存在则刷新 last_seen）。
    pub fn upsert(conn: &Connection, item: &HotItem) -> rusqlite::Result<()> {
        let tags = serde_json::to_string(&item.tags).unwrap_or_default();
        conn.execute(
            r#"INSERT INTO hot_items
               (id, platform, rank, title, hot_value, url, tags, summary, first_seen, last_seen, source_trust)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
               ON CONFLICT(id) DO UPDATE SET last_seen=excluded.last_seen, rank=excluded.rank"#,
            params![
                item.id, item.platform, item.rank, item.title,
                item.hot_value, item.url, tags, item.summary,
                item.first_seen, item.last_seen, item.source_trust.as_str(),
            ],
        )?;
        Ok(())
    }

    /// 取某时间点之后的所有热点，用于 Agent 候选。
    pub fn recent_since(conn: &Connection, since_ts: i64) -> rusqlite::Result<Vec<HotItem>> {
        let mut stmt = conn.prepare(
            "SELECT id,platform,rank,title,hot_value,url,tags,summary,first_seen,last_seen,source_trust
             FROM hot_items WHERE first_seen >= ?1 ORDER BY first_seen DESC",
        )?;
        let rows = stmt.query_map(params![since_ts], row_to_hot)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 列表（主面板用）。
    pub fn list(conn: &Connection, limit: u32, platform: Option<&str>) -> rusqlite::Result<Vec<HotItem>> {
        let sql = if platform.is_some() {
            "SELECT id,platform,rank,title,hot_value,url,tags,summary,first_seen,last_seen,source_trust
             FROM hot_items WHERE platform = ?1 ORDER BY first_seen DESC LIMIT ?2"
        } else {
            "SELECT id,platform,rank,title,hot_value,url,tags,summary,first_seen,last_seen,source_trust
             FROM hot_items ORDER BY first_seen DESC LIMIT ?1"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = if let Some(p) = platform {
            stmt.query_map(params![p, limit], row_to_hot)?
        } else {
            stmt.query_map(params![limit], row_to_hot)?
        };
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 按 id 取单条。
    pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<HotItem>> {
        let mut stmt = conn.prepare(
            "SELECT id,platform,rank,title,hot_value,url,tags,summary,first_seen,last_seen,source_trust
             FROM hot_items WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            Ok(Some(row_to_hot(r)?))
        } else {
            Ok(None)
        }
    }

    /// 清理过期热点（默认保留 7 天）。
    /// 同时清理对应的 hot_embeddings（外键级联会自动处理，这里显式删除更稳妥）。
    pub fn purge_expired(conn: &Connection) -> rusqlite::Result<()> {
        let cutoff = Utc::now().timestamp() - HOT_RETENTION_DAYS * 86400;
        conn.execute("DELETE FROM hot_items WHERE first_seen < ?1", params![cutoff])?;
        conn.execute("DELETE FROM hot_embeddings WHERE first_seen < ?1", params![cutoff])?;
        Ok(())
    }

    /// 清理孤儿向量：hot_items 中已不存在的 embedding 记录。
    /// 即使外键级联未开启，也能回收无效向量数据。
    pub fn purge_orphan_embeddings(conn: &Connection) -> rusqlite::Result<u64> {
        let n = conn.execute(
            "DELETE FROM hot_embeddings WHERE id NOT IN (SELECT id FROM hot_items)",
            [],
        )?;
        Ok(n as u64)
    }

    /// 存 embedding。
    pub fn set_embedding(conn: &Connection, id: &str, platform: &str, first_seen: i64, emb: &[f32]) -> rusqlite::Result<()> {
        let blob = vector::encode(emb);
        conn.execute(
            "INSERT OR REPLACE INTO hot_embeddings (id, embedding, platform, first_seen) VALUES (?1,?2,?3,?4)",
            params![id, blob, platform, first_seen],
        )?;
        Ok(())
    }

    /// 在时间窗口内找最相似的热点 id（语义去重用）。
    pub fn nearest_neighbor(
        conn: &Connection,
        emb: &[f32],
        since_ts: i64,
        threshold: f32,
    ) -> rusqlite::Result<Option<(String, f32)>> {
        let mut stmt = conn.prepare(
            "SELECT id, embedding FROM hot_embeddings WHERE first_seen >= ?1",
        )?;
        let rows = stmt.query_map(params![since_ts], |r| {
            let id: String = r.get(0)?;
            let blob: Vec<u8> = r.get(1)?;
            Ok((id, blob))
        })?;
        let mut best: Option<(String, f32)> = None;
        for r in rows {
            let (id, blob) = r?;
            let other = vector::decode(&blob);
            let sim = vector::cosine(emb, &other);
            if sim >= threshold && best.as_ref().map(|b| sim > b.1).unwrap_or(true) {
                best = Some((id, sim));
            }
        }
        Ok(best)
    }
}

fn row_to_hot(r: &rusqlite::Row) -> rusqlite::Result<HotItem> {
    let tags_json: String = r.get(6)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let trust_str: String = r.get(10)?;
    let source_trust = match trust_str.as_str() {
        "direct" => TrustLevel::Direct,
        _ => TrustLevel::Aggregated,
    };
    Ok(HotItem {
        id: r.get(0)?,
        platform: r.get(1)?,
        rank: r.get(2)?,
        title: r.get(3)?,
        hot_value: r.get(4)?,
        url: r.get(5)?,
        tags,
        summary: r.get(7)?,
        first_seen: r.get(8)?,
        last_seen: r.get(9)?,
        source_trust,
    })
}

pub struct FetchLogRepo;

impl FetchLogRepo {
    pub fn insert(
        conn: &Connection,
        platform: &str,
        started_at: i64,
        duration_ms: u64,
        status: &str,
        item_count: u32,
        error: Option<&str>,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO fetch_logs (platform, started_at, duration_ms, status, item_count, error)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![platform, started_at, duration_ms as i64, status, item_count, error],
        )?;
        Ok(())
    }

    pub fn purge_expired(conn: &Connection) -> rusqlite::Result<()> {
        let cutoff = Utc::now().timestamp() - LOG_RETENTION_DAYS * 86400;
        conn.execute("DELETE FROM fetch_logs WHERE started_at < ?1", params![cutoff])?;
        Ok(())
    }
}

pub struct ProfileRepo;

impl ProfileRepo {
    const KEY: &'static str = "profile";

    pub fn load(conn: &Connection) -> rusqlite::Result<UserProfile> {
        let mut stmt = conn.prepare("SELECT value FROM profile WHERE key = ?1")?;
        let mut rows = stmt.query(params![Self::KEY])?;
        if let Some(r) = rows.next()? {
            let v: String = r.get(0)?;
            Ok(serde_json::from_str(&v).unwrap_or_default())
        } else {
            Ok(UserProfile::default())
        }
    }

    pub fn save(conn: &Connection, profile: &UserProfile) -> rusqlite::Result<()> {
        let v = serde_json::to_string(profile).unwrap_or_default();
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO profile (key, value, updated_at) VALUES (?1,?2,?3)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![Self::KEY, v, now],
        )?;
        Ok(())
    }
}

pub struct FeedbackRepo;

impl FeedbackRepo {
    pub fn insert(conn: &Connection, hot_id: &str, action: FeedbackAction) -> rusqlite::Result<()> {
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO feedback (hot_id, action, at) VALUES (?1,?2,?3)",
            params![hot_id, action.as_str(), now],
        )?;
        Ok(())
    }

    /// 某热点近期是否被忽略过（用于打分软惩罚）。
    pub fn was_recently_ignored(conn: &Connection, hot_id: &str, within_secs: i64) -> rusqlite::Result<bool> {
        let cutoff = Utc::now().timestamp() - within_secs;
        let mut stmt = conn.prepare(
            "SELECT 1 FROM feedback WHERE hot_id = ?1 AND action IN ('ignore','dismiss','dislike') AND at >= ?2 LIMIT 1",
        )?;
        let mut rows = stmt.query(params![hot_id, cutoff])?;
        Ok(rows.next()?.is_some())
    }

    /// 某热点近期是否被 dislike（用于硬排除：3 天内不再入选候选）。
    pub fn was_recently_disliked(conn: &Connection, hot_id: &str, within_secs: i64) -> rusqlite::Result<bool> {
        let cutoff = Utc::now().timestamp() - within_secs;
        let mut stmt = conn.prepare(
            "SELECT 1 FROM feedback WHERE hot_id = ?1 AND action = 'dislike' AND at >= ?2 LIMIT 1",
        )?;
        let mut rows = stmt.query(params![hot_id, cutoff])?;
        Ok(rows.next()?.is_some())
    }

    /// 清理过期反馈记录（默认保留 30 天）。
    pub fn purge_expired(conn: &Connection) -> rusqlite::Result<u64> {
        let cutoff = Utc::now().timestamp() - FEEDBACK_RETENTION_DAYS * 86400;
        let n = conn.execute("DELETE FROM feedback WHERE at < ?1", params![cutoff])?;
        Ok(n as u64)
    }
}

pub struct PetStateRepo;

impl PetStateRepo {
    pub fn get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
        let mut stmt = conn.prepare("SELECT value FROM pet_state WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(r) = rows.next()? {
            Ok(Some(r.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO pet_state (key, value, updated_at) VALUES (?1,?2,?3)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            params![key, value, now],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;

    fn mem_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        schema::init_db(&c).unwrap();
        c
    }

    #[test]
    fn disliked_hot_is_excluded_within_window() {
        let conn = mem_db();
        FeedbackRepo::insert(&conn, "h1", FeedbackAction::Dislike).unwrap();
        assert!(FeedbackRepo::was_recently_disliked(&conn, "h1", 86400).unwrap());
    }

    #[test]
    fn liked_hot_is_not_disliked() {
        let conn = mem_db();
        FeedbackRepo::insert(&conn, "h2", FeedbackAction::Like).unwrap();
        assert!(!FeedbackRepo::was_recently_disliked(&conn, "h2", 86400).unwrap());
    }

    #[test]
    fn ignored_counts_as_ignored_but_not_disliked() {
        let conn = mem_db();
        FeedbackRepo::insert(&conn, "h3", FeedbackAction::Ignore).unwrap();
        assert!(FeedbackRepo::was_recently_ignored(&conn, "h3", 86400).unwrap());
        assert!(!FeedbackRepo::was_recently_disliked(&conn, "h3", 86400).unwrap());
    }

    #[test]
    fn expired_dislike_not_counted() {
        let conn = mem_db();
        // 直接插入一条 4 天前的 dislike
        let old = Utc::now().timestamp() - 4 * 86400;
        conn.execute(
            "INSERT INTO feedback (hot_id, action, at) VALUES (?1, ?2, ?3)",
            params!["h4", "dislike", old],
        ).unwrap();
        // 3 天窗口内不算
        assert!(!FeedbackRepo::was_recently_disliked(&conn, "h4", 3 * 86400).unwrap());
    }

    fn make_hot(id: &str, first_seen: i64) -> HotItem {
        HotItem {
            id: id.to_string(),
            platform: "test".into(),
            rank: 1,
            title: id.to_string(),
            hot_value: None,
            url: format!("https://example.com/{id}"),
            tags: vec![],
            summary: None,
            first_seen,
            last_seen: first_seen,
            source_trust: TrustLevel::Direct,
        }
    }

    #[test]
    fn purge_expired_removes_old_hot_and_keeps_recent() {
        let conn = mem_db();
        let now = Utc::now().timestamp();
        let old = now - (HOT_RETENTION_DAYS + 1) * 86400;
        HotRepo::upsert(&conn, &make_hot("old", old)).unwrap();
        HotRepo::upsert(&conn, &make_hot("new", now)).unwrap();

        HotRepo::purge_expired(&conn).unwrap();

        assert!(HotRepo::get(&conn, "old").unwrap().is_none());
        assert!(HotRepo::get(&conn, "new").unwrap().is_some());
    }

    #[test]
    fn purge_orphan_embeddings_removes_dangling_vectors() {
        let conn = mem_db();
        let now = Utc::now().timestamp();
        // 插入一条热点及其向量
        HotRepo::upsert(&conn, &make_hot("alive", now)).unwrap();
        HotRepo::set_embedding(&conn, "alive", "test", now, &[0.1, 0.2]).unwrap();
        // 临时关闭 FK，构造一条孤儿向量（模拟 FK 未开启时的历史脏数据）
        conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        HotRepo::set_embedding(&conn, "orphan", "test", now, &[0.3, 0.4]).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

        let removed = HotRepo::purge_orphan_embeddings(&conn).unwrap();
        assert_eq!(removed, 1);

        // 孤儿向量被清理，存活的还在
        let alive = conn
            .query_row("SELECT count(*) FROM hot_embeddings WHERE id = 'alive'", [], |r| r.get::<_, i64>(0))
            .unwrap();
        let orphan = conn
            .query_row("SELECT count(*) FROM hot_embeddings WHERE id = 'orphan'", [], |r| r.get::<_, i64>(0))
            .unwrap();
        assert_eq!(alive, 1);
        assert_eq!(orphan, 0);
    }

    #[test]
    fn purge_expired_feedback_removes_old_records() {
        let conn = mem_db();
        let old = Utc::now().timestamp() - (FEEDBACK_RETENTION_DAYS + 1) * 86400;
        conn.execute(
            "INSERT INTO feedback (hot_id, action, at) VALUES (?1, ?2, ?3)",
            params!["h_old", "like", old],
        ).unwrap();
        FeedbackRepo::insert(&conn, "h_new", FeedbackAction::Dislike).unwrap();

        let removed = FeedbackRepo::purge_expired(&conn).unwrap();
        assert_eq!(removed, 1);

        // 旧记录被清理，新记录还在且仍被 was_recently_disliked 识别
        assert!(FeedbackRepo::was_recently_disliked(&conn, "h_new", 86400).unwrap());
    }

    #[test]
    fn purge_expired_fetch_logs_removes_old_logs() {
        let conn = mem_db();
        let now = Utc::now().timestamp();
        let old = now - (LOG_RETENTION_DAYS + 1) * 86400;
        FetchLogRepo::insert(&conn, "old", old, 100, "ok", 0, None).unwrap();
        FetchLogRepo::insert(&conn, "new", now, 100, "ok", 0, None).unwrap();

        FetchLogRepo::purge_expired(&conn).unwrap();

        let old_count: i64 = conn
            .query_row("SELECT count(*) FROM fetch_logs WHERE platform = 'old'", [], |r| r.get(0))
            .unwrap();
        let new_count: i64 = conn
            .query_row("SELECT count(*) FROM fetch_logs WHERE platform = 'new'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(old_count, 0);
        assert_eq!(new_count, 1);
    }
}
