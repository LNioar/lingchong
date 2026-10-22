use rusqlite::Connection;

/// 初始化数据库表结构。幂等。
pub fn init_db(conn: &Connection) -> rusqlite::Result<()> {
    // 开启外键约束，让 hot_embeddings 的 ON DELETE CASCADE 生效
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS hot_items (
            id            TEXT PRIMARY KEY,
            platform      TEXT NOT NULL,
            rank          INTEGER NOT NULL,
            title         TEXT NOT NULL,
            hot_value     INTEGER,
            url           TEXT NOT NULL,
            tags          TEXT NOT NULL DEFAULT '[]',
            summary       TEXT,
            first_seen    INTEGER NOT NULL,
            last_seen     INTEGER NOT NULL,
            source_trust  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_hot_platform ON hot_items(platform);
        CREATE INDEX IF NOT EXISTS idx_hot_first_seen ON hot_items(first_seen);

        CREATE TABLE IF NOT EXISTS announcements (
            hot_id TEXT PRIMARY KEY,
            announced_at INTEGER NOT NULL,
            FOREIGN KEY (hot_id) REFERENCES hot_items(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS hot_embeddings (
            id         TEXT PRIMARY KEY,
            embedding  BLOB NOT NULL,
            platform   TEXT NOT NULL,
            first_seen INTEGER NOT NULL,
            FOREIGN KEY (id) REFERENCES hot_items(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS fetch_logs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            platform    TEXT NOT NULL,
            started_at  INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            status      TEXT NOT NULL,
            item_count  INTEGER NOT NULL DEFAULT 0,
            error       TEXT
        );

        CREATE TABLE IF NOT EXISTS profile (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS feedback (
            id      INTEGER PRIMARY KEY AUTOINCREMENT,
            hot_id  TEXT NOT NULL,
            action  TEXT NOT NULL,
            at      INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS pet_state (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}
