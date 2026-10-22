//! Agent 编排层：画像读取、候选打分、深检索、生成口播文案。

pub mod orchestrator;
pub mod llm;
pub mod scoring;
pub mod functions;

pub use orchestrator::{AgentOrchestrator, AnnounceResult};
pub use llm::{LlmProvider, ChatMessage, FunctionCall, LlmConfig, OpenAiCompat};
