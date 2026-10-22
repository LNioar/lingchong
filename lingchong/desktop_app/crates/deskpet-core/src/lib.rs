//! DeskPet 核心领域模型与公共类型定义。
//!
//! 本 crate 不依赖任何具体实现（HTTP、SQLite、Tauri 等），
//! 仅提供跨模块共享的数据结构、错误类型与 trait 契约。

pub mod error;
pub mod mood;
pub mod profile;
pub mod trust;
pub mod types;
pub mod embedding;

pub use error::{DeskPetError, Result};
pub use mood::Mood;
pub use profile::{Interest, PetProfile, SchedulePref, UserProfile};
pub use trust::TrustLevel;
pub use types::{FeedbackAction, HotItem, PlatformId};
pub use embedding::EmbeddingProvider;
