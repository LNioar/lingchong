//! edge-tts Provider：通过调用 `edge-tts` CLI 合成语音并播放。
//!
//! 生产环境建议实现 WebSocket 协议直接调微软在线 TTS，
//! 骨架阶段复用成熟 CLI，零成本可用。
//!
//! 安装：`pip install edge-tts`

use async_trait::async_trait;
use deskpet_core::{DeskPetError, Result};
use log::warn;

use crate::TtsProvider;

pub struct EdgeTts {
    pub voice: String,
}

impl EdgeTts {
    pub fn new(voice: impl Into<String>) -> Self {
        Self { voice: voice.into() }
    }
}

#[async_trait]
impl TtsProvider for EdgeTts {
    async fn speak(&self, text: &str) -> Result<()> {
        // 检查 edge-tts 是否可用
        let check = tokio::process::Command::new("edge-tts")
            .arg("--version")
            .output()
            .await;
        if check.is_err() {
            warn!("[tts] edge-tts not installed, falling back to text-only");
            return Err(DeskPetError::Tts("edge-tts not installed".into()));
        }

        // 合成到临时文件（用时间戳+随机数避免并发冲突）
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = std::env::temp_dir()
            .join(format!("deskpet_{}_{}.mp3", std::process::id(), nanos));
        let out = tokio::process::Command::new("edge-tts")
            .args(["--voice", &self.voice, "--text", text])
            .arg("--write-media")
            .arg(&tmp)
            .output()
            .await
            .map_err(|e| DeskPetError::Tts(e.to_string()))?;

        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).to_string();
            return Err(DeskPetError::Tts(err));
        }

        // 播放：不同播放器参数不同
        let player = find_player().await;
        if let Some(p) = player {
            let mut cmd = match p {
                "mpv" => {
                    let mut c = tokio::process::Command::new(p);
                    c.arg("--no-video").arg(&tmp);
                    c
                }
                "ffplay" => {
                    let mut c = tokio::process::Command::new(p);
                    c.arg("-nodisp").arg("-autoexit").arg(&tmp);
                    c
                }
                "cvlc" => {
                    let mut c = tokio::process::Command::new(p);
                    c.arg("--no-video").arg("--play-and-exit").arg(&tmp);
                    c
                }
                _ => {
                    let mut c = tokio::process::Command::new(p);
                    c.arg(&tmp);
                    c
                }
            };
            let _ = cmd.status().await;
        } else {
            warn!("[tts] no audio player found, audio saved to {:?}", tmp);
        }
        let _ = tokio::fs::remove_file(&tmp).await;
        Ok(())
    }
}

async fn find_player() -> Option<&'static str> {
    for p in ["mpv", "ffplay", "cvlc"] {
        if tokio::process::Command::new(p)
            .arg("--version")
            .output()
            .await
            .is_ok()
        {
            return Some(p);
        }
    }
    None
}
