//! 应用配置。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub fetch: FetchConfig,
    pub llm: LlmConfig,
    pub tts: TtsConfig,
    pub pet: deskpet_core::PetProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub quiet_hours: Vec<String>,
    pub min_announce_interval_min: u32,
    pub announce_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FetchConfig {
    pub default_interval_min: u32,
    pub rsshub_endpoint: String,
    pub platforms: Vec<PlatformConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformConfig {
    pub id: String,
    pub enabled: bool,
    pub interval_min: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    // 显式指定字段级默认：TOML 中缺失该键时反序列化为 None，
    // 而非沿用 LlmConfig::default() 的 Some("text-embedding-3-small")。
    // 这保证了"用户清空向量模型 → 保存为 None → 重新加载仍为 None"的往返一致性。
    #[serde(default = "none_embedding_model")]
    pub embedding_model: Option<String>,
}

fn none_embedding_model() -> Option<String> {
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    pub provider: String,
    pub voice: String,
    pub enabled: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            quiet_hours: vec!["23:00-08:00".into()],
            min_announce_interval_min: 45,
            announce_threshold: 0.6,
        }
    }
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            default_interval_min: 15,
            rsshub_endpoint: "https://rsshub.app".into(),
            platforms: vec![
                PlatformConfig { id: "zhihu".into(), enabled: true, interval_min: 10 },
                PlatformConfig { id: "bilibili".into(), enabled: true, interval_min: 15 },
                PlatformConfig { id: "weibo".into(), enabled: true, interval_min: 20 },
                PlatformConfig { id: "rsshub".into(), enabled: true, interval_min: 30 },
                PlatformConfig { id: "douyin".into(), enabled: true, interval_min: 30 },
                PlatformConfig { id: "xiaohongshu".into(), enabled: true, interval_min: 30 },
            ],
        }
    }
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self { id: String::new(), enabled: true, interval_min: 15 }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_base: "https://api.openai.com/v1".into(),
            api_key: std::env::var("DESKPET_LLM_KEY").unwrap_or_default(),
            model: "gpt-4o-mini".into(),
            embedding_model: Some("text-embedding-3-small".into()),
        }
    }
}

impl LlmConfig {
    /// 是否启用 embedding：需要有 api_key，且 embedding_model 非空。
    /// 把 embedding_model 设为空字符串即可关闭（例如 DeepSeek 不提供 embeddings 接口）。
    pub fn embedding_enabled(&self) -> bool {
        !self.api_key.trim().is_empty()
            && self.embedding_model.as_ref().map_or(false, |m| !m.trim().is_empty())
    }
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            provider: "edge".into(),
            voice: "zh-CN-XiaoxiaoNeural".into(),
            enabled: true,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            fetch: FetchConfig::default(),
            llm: LlmConfig::default(),
            tts: TtsConfig::default(),
            pet: deskpet_core::PetProfile::default(),
        }
    }
}

impl AppConfig {
    /// 配置文件路径（应用数据目录下 config.toml）。
    pub fn path() -> PathBuf {
        crate::paths::data_dir().join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        log::info!("[config] loading from {}", path.display());
        match std::fs::read_to_string(&path) {
            Ok(s) => match toml::from_str(&s) {
                Ok(cfg) => cfg,
                Err(e) => {
                    // 解析失败：打印错误并使用默认配置，但**绝不覆盖**用户文件，
                    // 避免因缺少新字段导致 api_key 等被清空。
                    log::warn!(
                        "[config] parse failed: {e}. Using defaults WITHOUT overwriting {}",
                        path.display()
                    );
                    Self::default()
                }
            },
            Err(_) => {
                // 文件不存在：写入默认配置
                let cfg = Self::default();
                cfg.save();
                cfg
            }
        }
    }

    pub fn save(&self) {
        if let Err(error) = self.try_save() {
            log::warn!("[config] save failed: {error}");
        }
    }

    pub fn try_save(&self) -> Result<(), String> {
        self.save_to(&Self::path())
    }

    fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        let contents = toml::to_string_pretty(self).map_err(|_| "配置序列化失败")?;
        let temporary = path.with_extension("toml.tmp");
        std::fs::write(&temporary, contents).map_err(|_| "配置写入失败，请检查应用存储空间")?;
        std::fs::rename(&temporary, &path).map_err(|_| "配置保存失败，请重试")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pet_profile_defaults_for_legacy_config_and_round_trips() {
        let mut config: AppConfig = toml::from_str("[general]\nmin_announce_interval_min = 10").unwrap();
        assert_eq!(config.pet, deskpet_core::PetProfile::default());
        config.pet.name = "小火".into();
        config.pet.personality = "沉稳好奇".into();
        config.pet.speaking_style = "温和自然".into();
        config.pet.background = "来自森林的伙伴".into();
        config.llm.embedding_model = None;
        let stored: AppConfig = toml::from_str(&toml::to_string_pretty(&config).unwrap()).unwrap();
        assert_eq!(stored.pet, config.pet);
        assert!(stored.llm.embedding_model.is_none());
    }

    #[test]
    fn saving_replaces_existing_config_and_round_trips_key() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("deskpet-config-{}-{unique}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut config = AppConfig::default();
        config.llm.api_key = "test-only-first-key".into();
        config.save_to(&path).unwrap();
        config.llm.api_key = "test-only-replacement-key".into();
        config.llm.model = "test-only-model".into();
        config.save_to(&path).unwrap();
        let stored: AppConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(stored.llm, config.llm);
        assert!(!path.with_extension("toml.tmp").exists());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn missing_embedding_model_field_deserializes_to_none() {
        // TOML 中没有 embedding_model 键（即 None 被序列化后的结果），
        // 反序列化后应为 None，而非被结构体默认值回填为 Some("text-embedding-3-small")。
        let toml_str = r#"
[llm]
api_base = "https://api.deepseek.com"
api_key = ""
model = "deepseek-flash"
"#;
        let cfg: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(
            cfg.llm.embedding_model.is_none(),
            "expected None, got {:?}",
            cfg.llm.embedding_model
        );
    }

    #[test]
    fn explicit_embedding_model_field_round_trips() {
        let toml_str = r#"
[llm]
api_base = "https://api.example.com/v1"
api_key = ""
model = "gpt-4o-mini"
embedding_model = "text-embedding-3-small"
"#;
        let cfg: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.llm.embedding_model.as_deref(), Some("text-embedding-3-small"));
    }
}
