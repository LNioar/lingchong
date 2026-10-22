//! Embedding Provider 抽象。
//!
//! 放在 core 层，让 collector 与 agent 都能依赖，
//! 而无需 collector 依赖 agent（避免耦合）。
//! OpenAiCompat 在 agent crate 实现此 trait。

use async_trait::async_trait;
use crate::Result;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// 批量生成文本 embedding。返回顺序与输入一致。
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}
