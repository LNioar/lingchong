//! TTS 抽象层。

pub mod edge_tts;

pub use edge_tts::EdgeTts;

use async_trait::async_trait;
use deskpet_core::Result;

#[async_trait]
pub trait TtsProvider: Send + Sync {
    async fn speak(&self, text: &str) -> Result<()>;
}
