//! LLM Provider 抽象与 OpenAI 兼容实现。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use deskpet_core::{DeskPetError, EmbeddingProvider, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    /// embedding 模型（可与对话模型不同）
    pub embedding_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub function_call: Option<FunctionCall>,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// 对话补全，支持 function calling。
    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
    ) -> Result<LlmResponse>;
}

/// OpenAI 兼容实现（OpenAI / 通义 / Kimi / DeepSeek / 本地 vLLM 等）。
pub struct OpenAiCompat {
    client: reqwest::Client,
    config: LlmConfig,
}

impl OpenAiCompat {
    pub fn new(config: LlmConfig) -> Self {
        // 构建失败时回退到默认 client（永不 panic，保证 LLM 调用路径仍能走通并返回错误）
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client, config }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompat {
    async fn chat(&self, messages: &[ChatMessage], tools: Option<&[Value]>) -> Result<LlmResponse> {
        if self.config.api_key.trim().is_empty() {
            // No configured agent service: let the orchestrator use its local template.
            return Err(DeskPetError::Llm("API Key not configured; use template".into()));
        }
        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
        });
        if let Some(t) = tools {
            body["tools"] = Value::Array(t.to_vec());
        }
        let url = format!("{}/chat/completions", self.config.api_base.trim_end_matches('/'));
        log::info!("[llm] >>> POST {url} (model={})", self.config.model);
        log::info!("[llm] >>> request body: {}", body);
        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                log::warn!("[llm] >>> request failed: {e}");
                DeskPetError::Llm(e.to_string())
            })?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        log::info!("[llm] <<< status={status} body={text}");
        if !status.is_success() {
            return Err(DeskPetError::Llm(format!("{status}: {text}")));
        }
        let json: Value = serde_json::from_str(&text).map_err(|e| DeskPetError::Llm(e.to_string()))?;
        let choice = &json["choices"][0]["message"];
        let content = choice["content"].as_str().map(|s| s.to_string());
        let function_call = choice.get("tool_calls")
            .and_then(|tc| tc.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| {
                let name = c["function"]["name"].as_str()?.to_string();
                let args: Value = serde_json::from_str(c["function"]["arguments"].as_str()?).ok()?;
                Some(FunctionCall { name, arguments: args })
            });
        // 没有任何正文且没有工具调用 → 打印原始响应，便于排查（不必开 debug）
        let empty_content = content.as_deref().map_or(true, |s| s.trim().is_empty());
        if empty_content && function_call.is_none() {
            log::warn!("[llm] empty content & no tool call, raw response: {}", json);
        }
        Ok(LlmResponse { content, function_call })
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiCompat {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let model = self.config.embedding_model.clone().unwrap_or_else(|| self.config.model.clone());
        let body = serde_json::json!({ "model": model, "input": texts });
        let resp = self.client
            .post(format!("{}/embeddings", self.config.api_base.trim_end_matches('/')))
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| DeskPetError::Llm(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(DeskPetError::Llm(format!("embedding http {}", resp.status())));
        }
        let json: Value = resp.json().await.map_err(|e| DeskPetError::Llm(e.to_string()))?;
        let data = json["data"].as_array().ok_or_else(|| DeskPetError::Llm("no data".into()))?;
        let mut out = Vec::with_capacity(data.len());
        for d in data {
            let emb: Vec<f32> = d["embedding"]
                .as_array()
                .ok_or_else(|| DeskPetError::Llm("bad embedding".into()))?
                .iter()
                .filter_map(|v| v.as_f64().map(|x| x as f32))
                .collect();
            out.push(emb);
        }
        Ok(out)
    }
}
