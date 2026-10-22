use thiserror::Error;

/// DeskPet 全局错误类型。
#[derive(Debug, Error)]
pub enum DeskPetError {
    #[error("网络错误: {0}")]
    Network(String),

    #[error("HTTP 状态码异常: {0}")]
    HttpStatus(u16),

    #[error("解析失败: {0}")]
    Parse(String),

    #[error("触发限频，需退避")]
    RateLimited,

    #[error("平台风控拦截")]
    Blocked,

    #[error("数据库错误: {0}")]
    Database(String),

    #[error("LLM 调用失败: {0}")]
    Llm(String),

    #[error("TTS 失败: {0}")]
    Tts(String),

    #[error("配置错误: {0}")]
    Config(String),

    #[error("其他错误: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, DeskPetError>;
