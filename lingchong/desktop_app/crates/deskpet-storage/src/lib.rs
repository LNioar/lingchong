//! 本地存储层：SQLite（结构化数据）+ 轻量向量检索。
//!
//! 为减少依赖，向量库暂用 SQLite BLOB 存 embedding 并做暴力余弦，
//! 数据量在万级以下完全够用；后续可替换为 LanceDB。

pub mod schema;
pub mod repo;
pub mod vector;

pub use repo::{AnnouncementRepo, HotRepo, ProfileRepo, FeedbackRepo, PetStateRepo, FetchLogRepo};
pub use schema::init_db;
