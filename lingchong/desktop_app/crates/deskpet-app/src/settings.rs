//! Settings exposed to the UI. Stored API keys are never sent back to JavaScript.
use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, GeneralConfig, FetchConfig, TtsConfig};

#[derive(Serialize)]
pub struct ConfigDto {
    #[serde(flatten)]
    pub config: AppConfig,
    pub has_api_key: bool,
}

impl From<&AppConfig> for ConfigDto {
    fn from(config: &AppConfig) -> Self {
        let has_api_key = !config.llm.api_key.is_empty();
        let mut config = config.clone();
        config.llm.api_key.clear();
        Self { config, has_api_key }
    }
}

#[derive(Deserialize)]
pub struct ConfigUpdate {
    pub general: GeneralConfig,
    pub fetch: FetchConfig,
    pub llm: LlmUpdate,
    pub tts: TtsConfig,
    #[serde(default)]
    pub pet: Option<deskpet_core::PetProfile>,
    #[serde(default)]
    pub clear_api_key: bool,
}

#[derive(Deserialize)]
pub struct LlmUpdate {
    pub api_base: String,
    pub api_key: Option<String>,
    pub model: String,
    pub embedding_model: Option<String>,
}

pub fn validate_quiet_hours(hours: &[String]) -> Result<(), String> {
    fn valid_time(time: &str) -> bool {
        let bytes = time.as_bytes();
        bytes.len() == 5 && bytes[2] == b':'
            && [0, 1, 3, 4].iter().all(|&i| bytes[i].is_ascii_digit())
            && time[..2].parse::<u8>().is_ok_and(|h| h < 24)
            && time[3..].parse::<u8>().is_ok_and(|m| m < 60)
    }
    for hours in hours {
        let (start, end) = hours.split_once('-').ok_or("勿扰时间应为 HH:MM-HH:MM")?;
        if !valid_time(start) || !valid_time(end) {
            return Err("勿扰时间应为有效的 HH:MM-HH:MM".into());
        }
    }
    Ok(())
}

impl ConfigUpdate {
    pub fn apply(mut self, config: &mut AppConfig) -> Result<(), String> {
        validate_quiet_hours(&self.general.quiet_hours)?;
        if self.general.min_announce_interval_min < 1 {
            return Err("播报间隔至少为 1 分钟".into());
        }
        if !self.general.announce_threshold.is_finite() || !(0.0..=1.0).contains(&self.general.announce_threshold) {
            return Err("播报阈值必须在 0 到 1 之间".into());
        }
        if self.fetch.default_interval_min < 1 || self.fetch.platforms.iter().any(|p| p.interval_min < 1) {
            return Err("采集间隔至少为 1 分钟".into());
        }
        let mut ids = std::collections::HashSet::new();
        for platform in &self.fetch.platforms {
            if !["zhihu", "bilibili", "weibo", "rsshub", "douyin", "xiaohongshu"].contains(&platform.id.as_str()) || !ids.insert(&platform.id) {
                return Err("采集平台无效或重复".into());
            }
        }
        let endpoint = reqwest::Url::parse(self.fetch.rsshub_endpoint.trim()).map_err(|_| "RSSHub 地址无效")?;
        if !matches!(endpoint.scheme(), "https" | "http") || endpoint.host_str().is_none() {
            return Err("RSSHub 地址必须为 http 或 https 地址".into());
        }
        if self.tts.voice.trim().is_empty() {
            return Err("请选择语音音色".into());
        }
        let key = if self.clear_api_key { Some(String::new()) }
            else { self.llm.api_key.filter(|key| !key.trim().is_empty()) };
        let mut candidate = config.clone();
        SettingsUpdate {
            api_base: self.llm.api_base, api_key: key,
            model: self.llm.model, embedding_model: self.llm.embedding_model,
            pet: self.pet,
        }.apply(&mut candidate)?;
        candidate.general = self.general;
        candidate.fetch = self.fetch;
        candidate.fetch.rsshub_endpoint = candidate.fetch.rsshub_endpoint.trim().trim_end_matches('/').into();
        if self.tts.edge.is_none() {
            self.tts.edge = candidate.tts.edge.clone();
        }
        candidate.tts = self.tts;
        *config = candidate;
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub api_base: String,
    pub model: String,
    pub embedding_model: Option<String>,
    pub api_key_configured: bool,
    pub pet: deskpet_core::PetProfile,
}

impl From<&AppConfig> for SettingsView {
    fn from(config: &AppConfig) -> Self {
        Self {
            api_base: config.llm.api_base.clone(),
            model: config.llm.model.clone(),
            embedding_model: config.llm.embedding_model.clone(),
            api_key_configured: !config.llm.api_key.is_empty(),
            pet: config.pet.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub api_base: String,
    pub model: String,
    pub embedding_model: Option<String>,
    // null/missing keeps the saved key; an explicit empty string clears it.
    pub api_key: Option<String>,
    #[serde(default)]
    pub pet: Option<deskpet_core::PetProfile>,
}

impl SettingsUpdate {
    pub fn apply(self, config: &mut AppConfig) -> Result<(), String> {
        let api_base = self.api_base.trim().trim_end_matches('/');
        let url = reqwest::Url::parse(api_base).map_err(|_| "请输入有效的 API 地址")?;
        if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
            return Err("API 地址必须以 https:// 或 http:// 开头".into());
        }
        if !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
            return Err("API 地址不能包含账号密码、查询参数或片段".into());
        }
        let model = self.model.trim();
        if model.is_empty() {
            return Err("请填写模型名称".into());
        }
        let pet = self.pet.map(normalize_pet).transpose()?;
        config.llm.api_base = api_base.to_owned();
        config.llm.model = model.to_owned();
        config.llm.embedding_model = self.embedding_model
            .map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
        if let Some(key) = self.api_key {
            config.llm.api_key = key.trim().to_owned();
        }
        if let Some(pet) = pet { config.pet = pet; }
        Ok(())
    }
}

fn normalize_pet(mut pet: deskpet_core::PetProfile) -> Result<deskpet_core::PetProfile, String> {
    pet.name = pet.name.trim().into();
    pet.personality = pet.personality.trim().into();
    pet.speaking_style = pet.speaking_style.trim().into();
    pet.background = pet.background.trim().into();
    if pet.name.is_empty() || pet.name.chars().count() > 80 {
        return Err("宠物名称不能为空，且不超过 80 个字符".into());
    }
    if [&pet.personality, &pet.speaking_style, &pet.background].iter().any(|value| value.chars().count() > 4000) {
        return Err("宠物画像的单项描述不能超过 4000 个字符".into());
    }
    Ok(pet)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_update(config: &AppConfig) -> ConfigUpdate {
        serde_json::from_value(serde_json::to_value(ConfigDto::from(config)).unwrap()).unwrap()
    }

    #[test]
    fn pet_profile_updates_through_both_settings_apis_and_old_clients_keep_it() {
        let mut config = AppConfig::default();
        let mut patch = full_update(&config);
        patch.pet.as_mut().unwrap().name = " 小火 ".into();
        patch.pet.as_mut().unwrap().personality = " 沉稳 ".into();
        patch.apply(&mut config).unwrap();
        assert_eq!(config.pet.name, "小火");
        assert_eq!(config.pet.personality, "沉稳");
        update(None).apply(&mut config).unwrap();
        assert_eq!(config.pet.name, "小火");
        let mut patch = update(None);
        let mut pet = config.pet.clone();
        pet.speaking_style = "自由自然".into();
        patch.pet = Some(pet);
        patch.apply(&mut config).unwrap();
        assert_eq!(config.pet.speaking_style, "自由自然");
        let before = serde_json::to_value(&config).unwrap();
        let mut patch = full_update(&config);
        patch.pet.as_mut().unwrap().name = " ".into();
        assert!(patch.apply(&mut config).is_err());
        assert_eq!(serde_json::to_value(&config).unwrap(), before);
    }

    #[test]
    fn full_config_never_returns_secret_and_blank_keeps_it() {
        let mut config = AppConfig::default();
        config.llm.api_key = "desktop-test-secret".into();
        let view = serde_json::to_value(ConfigDto::from(&config)).unwrap();
        assert_eq!(view["llm"]["api_key"], "");
        assert_eq!(view["has_api_key"], true);
        assert!(!view.to_string().contains("desktop-test-secret"));
        let mut patch = full_update(&config);
        patch.general.quiet_hours = vec!["23:30-07:15".into()];
        patch.fetch.platforms[0].enabled = false;
        patch.apply(&mut config).unwrap();
        assert_eq!(config.llm.api_key, "desktop-test-secret");
        assert!(!config.fetch.platforms[0].enabled);
        assert_eq!(config.general.quiet_hours, vec!["23:30-07:15"]);
        let mut patch = full_update(&config);
        patch.clear_api_key = true;
        patch.apply(&mut config).unwrap();
        assert!(config.llm.api_key.is_empty());
    }

    #[test]
    fn full_validation_is_atomic_and_accepts_midnight_ranges() {
        let mut config = AppConfig::default();
        let before = serde_json::to_value(&config).unwrap();
        for hours in ["24:00-08:00", "23:60-08:00", "9:00-08:00", "零点-08:00"] {
            let mut patch = full_update(&config);
            patch.general.quiet_hours = vec![hours.into()];
            assert!(patch.apply(&mut config).is_err());
        }
        for kind in 0..5 {
            let mut patch = full_update(&config);
            match kind {
                0 => patch.general.min_announce_interval_min = 0,
                1 => patch.general.announce_threshold = f32::NAN,
                2 => patch.fetch.platforms[0].interval_min = 0,
                3 => patch.fetch.platforms.push(patch.fetch.platforms[0].clone()),
                _ => patch.fetch.rsshub_endpoint = "file:///invalid".into(),
            }
            assert!(patch.apply(&mut config).is_err());
            assert_eq!(serde_json::to_value(&config).unwrap(), before);
        }
        assert!(validate_quiet_hours(&[]).is_ok());
        assert!(validate_quiet_hours(&["23:00-08:00".into(), "00:00-00:30".into()]).is_ok());
    }

    #[test]
    fn legacy_settings_submission_preserves_edge_runtime() {
        let mut config = AppConfig::default();
        config.tts.edge = Some(crate::config::EdgeRuntimeConfig {
            python_path: Some("C:/portable/python.exe".into()),
            mpv_path: Some("C:/portable/mpv.exe".into()),
            proxy: Some("http://127.0.0.1:7890".into()),
        });
        let mut patch = full_update(&config);
        patch.tts.edge = None; // An old settings page only submits existing fields.
        patch.tts.voice = "zh-CN-YunxiNeural".into();
        patch.apply(&mut config).unwrap();
        assert_eq!(config.tts.edge.as_ref().unwrap().proxy.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(config.tts.voice, "zh-CN-YunxiNeural");
    }

    fn update(key: Option<&str>) -> SettingsUpdate {
        SettingsUpdate {
            api_base: " https://example.com/v1/ ".into(),
            model: " test-model ".into(),
            embedding_model: Some(" ".into()),
            api_key: key.map(str::to_owned),
            pet: None,
        }
    }

    #[test]
    fn blank_form_keeps_key_but_explicit_clear_removes_it() {
        let mut config = AppConfig::default();
        config.llm.api_key = "test-secret-never-expose".into();
        update(None).apply(&mut config).unwrap();
        assert_eq!(config.llm.api_key, "test-secret-never-expose");
        assert_eq!(config.llm.api_base, "https://example.com/v1");
        assert_eq!(config.llm.model, "test-model");
        assert!(config.llm.embedding_model.is_none());
        let view = serde_json::to_value(SettingsView::from(&config)).unwrap();
        assert_eq!(view["apiKeyConfigured"], true);
        assert!(view.get("apiKey").is_none());
        assert!(!view.to_string().contains("test-secret-never-expose"));
        update(Some("")).apply(&mut config).unwrap();
        assert!(config.llm.api_key.is_empty());
        update(Some(" replacement-key ")).apply(&mut config).unwrap();
        assert_eq!(config.llm.api_key, "replacement-key");
    }

    #[test]
    fn invalid_settings_do_not_mutate_existing_config() {
        let mut config = AppConfig::default();
        let original = config.llm.clone();
        for address in ["file:///tmp/api", "https://user:pass@example.com/v1", "https://example.com/v1?key=secret"] {
            let mut patch = update(Some("new-key"));
            patch.api_base = address.into();
            assert!(patch.apply(&mut config).is_err());
            assert_eq!(config.llm, original);
        }
        let mut patch = update(Some("new-key"));
        patch.model = " ".into();
        assert!(patch.apply(&mut config).is_err());
        assert_eq!(config.llm, original);
    }
}
