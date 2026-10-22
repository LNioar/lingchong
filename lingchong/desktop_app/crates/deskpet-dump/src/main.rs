// 导出 hot_items 为 JSON，供效果预览页使用。
use deskpet_storage::{init_db, HotRepo};
use rusqlite::Connection;

fn main() {
    let db_path = data_path();
    let conn = Connection::open(&db_path).expect("open db");
    init_db(&conn).unwrap();
    // 各平台 Top10
    let mut all = Vec::new();
    for p in ["zhihu", "bilibili"] {
        let items = HotRepo::list(&conn, 10, Some(p)).unwrap_or_default();
        all.extend(items);
    }
    println!("{}", serde_json::to_string_pretty(&all).unwrap());
}

fn data_path() -> std::path::PathBuf {
    if let Some(d) = dirs::data_dir() {
        return d.join("deskpet").join("deskpet.db");
    }
    std::path::PathBuf::from("deskpet.db")
}
