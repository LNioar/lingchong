//! TTS 抽象层。

pub mod edge_tts;
#[cfg(target_os = "windows")]
pub mod windows_process;

pub use edge_tts::EdgeTts;

use async_trait::async_trait;
use deskpet_core::Result;

/// Speak only the text before the first HTTP(S) link, leaving display text intact.
pub fn spoken_prefix(text: &str) -> &str {
    let lower = text.to_ascii_lowercase();
    let end = lower.find("http://").into_iter()
        .chain(lower.find("https://"))
        .min()
        .unwrap_or(text.len());
    text[..end].trim()
}

#[cfg(test)]
mod text_tests {
    use super::spoken_prefix;

    #[test]
    fn stops_at_first_link_without_resuming_after_it() {
        assert_eq!(spoken_prefix("今日新闻。https://example.com 后文也不读"), "今日新闻。");
        assert_eq!(spoken_prefix("新闻 http://a.test 后文 https://b.test"), "新闻");
        assert_eq!(spoken_prefix("新闻 https://a.test 后文 http://b.test"), "新闻");
    }

    #[test]
    fn handles_leading_links_uppercase_and_plain_text() {
        assert_eq!(spoken_prefix(" HTTPS://example.com"), "");
        assert_eq!(spoken_prefix("中文说明 HTTP://example.com"), "中文说明");
        assert_eq!(spoken_prefix(" 普通中文内容。 "), "普通中文内容。");
        assert_eq!(spoken_prefix("介绍 HTTP 协议"), "介绍 HTTP 协议");
    }
}

#[async_trait]
pub trait TtsProvider: Send + Sync {
    async fn speak(&self, text: &str) -> Result<()>;
}
