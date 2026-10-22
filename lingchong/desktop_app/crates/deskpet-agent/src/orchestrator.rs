//! Agent 编排核心：挑出值得播报的热点，并生成口播文案。

use std::sync::Arc;

use log::{info, warn};
use rusqlite::Connection;
use tokio::sync::Mutex;

use deskpet_core::{EmbeddingProvider, HotItem, Mood, PetProfile, UserProfile};
use deskpet_storage::{AnnouncementRepo, HotRepo, ProfileRepo};

use crate::llm::{ChatMessage, LlmProvider};
use crate::scoring::{score, SCORE_THRESHOLD, COLD_START_THRESHOLD};

/// Agent 输出。
#[derive(Debug, Clone)]
pub struct AnnounceResult {
    pub hot: HotItem,
    pub script: String,
    pub mood_hint: Mood,
    pub source_url: String,
}

pub struct AgentOrchestrator {
    llm: Arc<dyn LlmProvider>,
    /// embedding 提供方，用于补算兴趣向量
    embedder: Arc<dyn EmbeddingProvider>,
    db: Arc<Mutex<Connection>>,
    last_cycle_at: i64,
    /// 有兴趣画像时的打分阈值，从配置读取（默认 0.6）
    score_threshold: f32,
    pet: PetProfile,
}

impl AgentOrchestrator {
    pub fn new(llm: Arc<dyn LlmProvider>, embedder: Arc<dyn EmbeddingProvider>, db: Arc<Mutex<Connection>>) -> Self {
        Self { llm, embedder, db, last_cycle_at: 0, score_threshold: SCORE_THRESHOLD, pet: PetProfile::default() }
    }

    /// 从配置设置打分阈值
    pub fn set_score_threshold(&mut self, threshold: f32) {
        self.score_threshold = threshold;
    }

    pub fn set_pet_profile(&mut self, pet: PetProfile) {
        self.pet = pet;
    }

    /// 热更新 LLM 提供方（配置重载时调用）。
    pub fn set_llm(&mut self, llm: Arc<dyn LlmProvider>) {
        self.llm = llm;
    }

    /// 热更新 Embedding 提供方（配置重载时调用）。
    pub fn set_embedder(&mut self, embedder: Arc<dyn EmbeddingProvider>) {
        self.embedder = embedder;
    }

    /// 执行一个 Agent 周期。
    pub async fn run_cycle(&mut self) -> Option<AnnounceResult> {
        let since = if self.last_cycle_at == 0 {
            chrono::Utc::now().timestamp() - 86400
        } else {
            self.last_cycle_at
        };
        self.last_cycle_at = chrono::Utc::now().timestamp();
        self.run_cycle_inner(since, false, None).await
    }

    /// 手动召唤：回看 24 小时，跳过自动播报门槛，仍排除 dislike 的热点。
    pub async fn run_cycle_force(&mut self) -> Option<AnnounceResult> {
        let since = chrono::Utc::now().timestamp() - 86400;
        self.run_cycle_inner(since, true, None).await
    }

    /// Only consider items present in the current successful network refresh.
    pub async fn run_cycle_refreshed(&mut self, ids: &[String]) -> Option<AnnounceResult> {
        self.run_cycle_inner(0, true, Some(ids)).await
    }

    async fn run_cycle_inner(&mut self, since: i64, manual: bool, refreshed_ids: Option<&[String]>) -> Option<AnnounceResult> {

        // 同步读取候选与画像（短暂加锁，不跨 await）
        let (candidates, mut profile) = {
            let conn = self.db.lock().await;
            let raw = if let Some(ids) = refreshed_ids {
                ids.iter().filter_map(|id| HotRepo::get(&conn, id).ok().flatten()).collect()
            } else { HotRepo::recent_since(&conn, since).unwrap_or_default() };
            // 硬排除：近期被 dislike 的热点 3 天内不再入选候选
            let candidates: Vec<HotItem> = raw
                .into_iter()
                .filter(|it| {
                    !AnnouncementRepo::contains(&conn, &it.id).unwrap_or(true)
                    && !deskpet_storage::FeedbackRepo::was_recently_disliked(&conn, &it.id, 3 * 86400)
                        .unwrap_or(false)
                })
                .collect();
            let profile = ProfileRepo::load(&conn).unwrap_or_default();
            (candidates, profile)
        };

        if candidates.is_empty() {
            info!("[agent] no new candidates (or all disliked-filtered)");
            return None;
        }

        // 补算缺失的兴趣 embedding 并持久化（不持 DB 锁）
        self.ensure_interest_embeddings(&mut profile).await;

        // 打分（同步，需 conn）
        let mut scored: Vec<(f32, HotItem)> = {
            let conn = self.db.lock().await;
            candidates
                .into_iter()
                .map(|it| (score(&conn, &it, &profile), it))
                .collect()
        };
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let (top_score, top_item) = scored.remove(0);
        // 冷启动：无兴趣画像时用较低阈值，让热点能播出，通过反馈逐步学习
        let threshold = if profile.interests.is_empty() { COLD_START_THRESHOLD } else { self.score_threshold };
        info!("[agent] top candidate score={:.3} title={} (threshold={:.1})", top_score, top_item.title, threshold);
        if !manual && top_score < threshold {
            info!("[agent] below threshold, skip");
            return None;
        }

        let script = match self.compose_script(&top_item, &profile).await {
            Ok(s) if !s.trim().is_empty() => {
                info!("[agent] announce ready ({} chars): {}", s.chars().count(), s);
                s
            }
            other => {
                // LLM 失败或返回空白 → 降级模板文案，保证不崩、且一定非空
                match other {
                    Err(e) => warn!("[agent] LLM failed ({}), fallback to template", e),
                    Ok(_) => warn!("[agent] LLM returned blank, fallback to template"),
                }
                template_script(&top_item, &profile)
            }
        };
        {
            let conn = self.db.lock().await;
            if let Err(error) = AnnouncementRepo::record(&conn, &top_item.id) {
                warn!("[agent] cannot record announcement: {error}");
                return None;
            }
        }
        Some(AnnounceResult {
            hot: top_item.clone(),
            script,
            mood_hint: if top_score > 0.75 { Mood::Excited } else { Mood::Curious },
            source_url: top_item.url.clone(),
        })
    }

    /// 为画像中缺失 embedding 的兴趣主题补算向量并持久化。
    /// 失败则保留 None，打分时降级为占位向量。
    async fn ensure_interest_embeddings(&self, profile: &mut UserProfile) {
        let topics = profile.topics_without_embedding();
        if topics.is_empty() {
            return;
        }
        info!("[agent] computing embeddings for {} interests", topics.len());
        match self.embedder.embed(&topics).await {
            Ok(embs) if embs.len() == topics.len() => {
                for (topic, emb) in topics.iter().zip(embs.into_iter()) {
                    if let Some(i) = profile.interests.iter_mut().find(|i| &i.topic == topic) {
                        i.embedding = Some(emb);
                    }
                }
                // 持久化
                let conn = self.db.lock().await;
                if let Err(e) = ProfileRepo::save(&conn, profile) {
                    warn!("[agent] persist interest embeddings failed: {e}");
                }
            }
            Ok(embs) => {
                warn!("[agent] interest embedding count mismatch ({} != {}), skip", embs.len(), topics.len());
            }
            Err(e) => {
                warn!("[agent] interest embedding failed ({}), will use placeholder at scoring", e);
            }
        }
    }

    /// 生成口播文案。LLM 调用期间不持有 DB 连接。
    async fn compose_script(
        &self,
        item: &HotItem,
        profile: &UserProfile,
    ) -> Result<String, String> {
        info!("[agent] compose_script invoked (marker=compose-v2)");
        let persona = if profile.persona.is_empty() { synthesize_persona(profile) } else { profile.persona.clone() };
        let system = format!(
            "你是一只陪伴主人的宠物。\n\
             宠物名称：{}\n\
             宠物性格：{}\n\
             表达偏好：{}\n\
             背景设定：{}\n\
             主人画像：{persona}\n\
             \n\
             现在给你一条热点信息，请用你自己的口吻对主人说点什么。\n\
             结合宠物画像表达，回复长短和句式由你自由决定，自然就好。\n\
             \n\
             注意事项：\n\
             - 只根据下面给出的信息作答，不要请求更多信息，不要使用任何工具\n\
             - 不要编造事实\n\
             - 记得附上原文链接，方便主人查看\n\
             - 直接说要讲的内容即可，不用加前置说明或过渡语",
            self.pet.name, self.pet.personality, self.pet.speaking_style, self.pet.background
        );

        let messages = vec![
            ChatMessage { role: "system".into(), content: system },
            ChatMessage {
                role: "user".into(),
                content: format!(
                    "热点标题：{}\n平台：{}\n链接：{}\n\n来，用你自己的方式跟主人聊聊这条热点吧：",
                    item.title, item.platform, item.url
                ),
            },
        ];

        // 不使用 function calling：部分模型（如 DeepSeek）在 tools 模式下会返回空 content
        // 或过渡语。这里单次请求，直接取正文。
        let resp = self.llm.chat(&messages, None).await.map_err(|e| e.to_string())?;
        let content = resp.content.unwrap_or_default();
        if is_usable_script(&content) {
            return Ok(content.trim().to_string());
        }

        // 正文是过渡语/空 → 带更强指令重试一次
        warn!("[agent] suspicious llm output ({}), retrying with stricter instruction", content.trim());
        let mut retry = messages.clone();
        retry.push(ChatMessage { role: "assistant".into(), content: content.clone() });
        retry.push(ChatMessage {
            role: "user".into(),
            content: "直接用宠物口吻对主人说点什么吧，记得附上链接：".into(),
        });
        let r2 = self.llm.chat(&retry, None).await.map_err(|e| e.to_string())?;
        let c2 = r2.content.unwrap_or_default();
        if is_usable_script(&c2) {
            return Ok(c2.trim().to_string());
        }

        let preview: String = c2.trim().chars().take(80).collect();
        Err(format!("no usable script (retry output={preview:?})"))
    }
}

/// 判断 LLM 输出是否为可用文案（过滤空串与「让我看看详情」类过渡语）。
fn is_usable_script(s: &str) -> bool {
    let t = s.trim();
    let n = t.chars().count();
    if n < 8 {
        return false;
    }
    // 过短且含过渡语特征 → 判定不可用
    if n < 30 {
        let lower = t.to_lowercase();
        const BAD: [&str; 10] = [
            "看看详情", "查看详情", "让我看看", "我先看", "稍等", "让我查",
            "详情", "look up", "let me", "check the",
        ];
        if BAD.iter().any(|b| lower.contains(b)) {
            return false;
        }
    }
    true
}

/// 从画像兴趣合成人物描述（persona 为空时用），
/// 让 LLM 有个性化上下文来定制口吻。
fn synthesize_persona(profile: &UserProfile) -> String {
    if profile.interests.is_empty() {
        return "一位普通的桌面用户，兴趣待探索".to_string();
    }
    let top: Vec<&str> = {
        let mut sorted: Vec<_> = profile.interests.iter().collect();
        sorted.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
        sorted.into_iter().take(3).map(|i| i.topic.as_str()).collect()
    };
    format!("一位关注「{}」的用户", top.join("、"))
}

/// LLM 不可用时的降级模板文案。
/// 仍包含热点标题、与主人兴趣的关联、原文链接。
fn template_script(item: &HotItem, profile: &UserProfile) -> String {
    let related = if profile.interests.is_empty() {
        String::from("这条热度挺高，瞄一眼？")
    } else {
        // 找出与热点 tags 重合的兴趣主题
        let hits: Vec<&str> = profile.interests
            .iter()
            .filter(|i| item.tags.iter().any(|t| t == &i.topic))
            .map(|i| i.topic.as_str())
            .collect();
        if hits.is_empty() {
            format!("「{}」好像跟你关注的领域沾点边～", profile.interests[0].topic)
        } else {
            format!("这条跟你看好的「{}」相关！", hits.join("、"))
        }
    };
    format!(
        "主人～{}上了{}热榜第{}！{}\n原文：{}",
        item.title, item.platform, item.rank, related, item.url
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskpet_core::{DeskPetError, EmbeddingProvider, Interest};
    use crate::llm::LlmResponse;
    use crate::functions;
    use deskpet_storage::schema;

    fn mem_db() -> Arc<Mutex<Connection>> {
        let c = Connection::open_in_memory().unwrap();
        schema::init_db(&c).unwrap();
        Arc::new(Mutex::new(c))
    }

    fn unconfigured_llm() -> Arc<dyn LlmProvider> {
        Arc::new(crate::llm::OpenAiCompat::new(crate::llm::LlmConfig {
            api_base: "http://127.0.0.1:1/v1".into(), api_key: " ".into(),
            model: "unused".into(), embedding_model: None,
        }))
    }

    #[tokio::test]
    async fn pet_profile_is_used_in_prompt_and_can_be_replaced() {
        struct RecordingLlm(Arc<std::sync::Mutex<String>>);
        #[async_trait::async_trait]
        impl LlmProvider for RecordingLlm {
            async fn chat(&self, messages: &[ChatMessage], _: Option<&[serde_json::Value]>) -> deskpet_core::Result<LlmResponse> {
                *self.0.lock().unwrap() = messages[0].content.clone();
                Ok(LlmResponse { content: Some("这是根据真实热点自由表达的一段内容，原文：https://example.com/hot".into()), function_call: None })
            }
        }
        let recorded = Arc::new(std::sync::Mutex::new(String::new()));
        let mut agent = AgentOrchestrator::new(Arc::new(RecordingLlm(recorded.clone())), Arc::new(MockEmbedder), mem_db());
        let pet = PetProfile { name: "小火".into(), personality: "沉稳".into(), speaking_style: "自然细腻".into(), background: "来自森林".into() };
        agent.set_pet_profile(pet.clone());
        let item = make_hot("pet-prompt", "热点", &[], "https://example.com/hot");
        let mut user = UserProfile::default();
        user.persona = "喜欢科技的主人".into();
        agent.compose_script(&item, &user).await.unwrap();
        let prompt = recorded.lock().unwrap().clone();
        for value in [&pet.name, &pet.personality, &pet.speaking_style, &pet.background, &user.persona] {
            assert!(prompt.contains(value));
        }
        assert!(!prompt.contains("正确示例"));
        assert!(!prompt.contains("1-2 句"));
        agent.set_pet_profile(PetProfile { name: "小雨".into(), ..pet });
        agent.compose_script(&item, &user).await.unwrap();
        let prompt = recorded.lock().unwrap().clone();
        assert!(prompt.contains("小雨"));
        assert!(!prompt.contains("小火"));
    }

    #[tokio::test]
    async fn missing_key_uses_real_hot_template_without_http() {
        let llm = unconfigured_llm();
        let error = llm.chat(&[], None).await.unwrap_err().to_string();
        assert!(error.contains("API Key not configured"));
        let db = mem_db();
        let item = make_hot("without-key", "真实热点标题", &[], "https://example.com/hot");
        HotRepo::upsert(&*db.lock().await, &item).unwrap();
        let mut agent = AgentOrchestrator::new(llm, Arc::new(MockEmbedder), db);
        let result = agent.run_cycle_force().await.unwrap();
        assert_eq!(result.hot.id, item.id);
        assert!(result.script.contains(&item.title));
        assert!(result.script.contains(&item.url));
    }

    #[tokio::test]
    async fn manual_ignores_auto_threshold_but_respects_dislike() {
        let db = mem_db();
        let mut item = make_hot("low-score", "低分真实热点", &[], "https://example.com/low");
        item.hot_value = Some(0);
        let mut profile = UserProfile::default();
        profile.interests.push(Interest { topic: "AI".into(), weight: 0.0, last_hit: 0, embedding: None });
        {
            let conn = db.lock().await;
            HotRepo::upsert(&conn, &item).unwrap();
            ProfileRepo::save(&conn, &profile).unwrap();
        }
        let mut agent = AgentOrchestrator::new(unconfigured_llm(), Arc::new(MockEmbedder), db.clone());
        agent.set_score_threshold(1.0);
        assert!(agent.run_cycle().await.is_none());
        let cursor = agent.last_cycle_at;
        assert!(agent.run_cycle_force().await.is_some());
        assert_eq!(agent.last_cycle_at, cursor);
        deskpet_storage::FeedbackRepo::insert(&*db.lock().await, &item.id, deskpet_core::FeedbackAction::Dislike).unwrap();
        assert!(agent.run_cycle_force().await.is_none());
    }

    #[tokio::test]
    async fn manual_with_empty_database_does_not_invent_hot_news() {
        let mut agent = AgentOrchestrator::new(unconfigured_llm(), Arc::new(MockEmbedder), mem_db());
        assert!(agent.run_cycle_force().await.is_none());
    }

    #[tokio::test]
    async fn refreshed_cycle_excludes_cache_and_remembers_announcements_after_restart() {
        let db = mem_db();
        let cached = make_hot("cache-only", "旧缓存", &[], "https://example.com/old");
        let mut fresh = make_hot("fresh", "本次网络结果", &[], "https://example.com/fresh");
        // It may be an existing hot item seen again today, not newly inserted today.
        fresh.first_seen -= 2 * 86400;
        {
            let conn = db.lock().await;
            HotRepo::upsert(&conn, &cached).unwrap();
            HotRepo::upsert(&conn, &fresh).unwrap();
        }
        let mut agent = AgentOrchestrator::new(unconfigured_llm(), Arc::new(MockEmbedder), db.clone());
        assert!(agent.run_cycle_refreshed(&[]).await.is_none());
        let ids = vec![fresh.id.clone()];
        assert_eq!(agent.run_cycle_refreshed(&ids).await.unwrap().hot.id, fresh.id);
        assert!(agent.run_cycle_refreshed(&ids).await.is_none());
        let mut restarted = AgentOrchestrator::new(unconfigured_llm(), Arc::new(MockEmbedder), db.clone());
        assert!(restarted.run_cycle_refreshed(&ids).await.is_none());
        assert!(restarted.run_cycle_refreshed(&[cached.id]).await.is_some());
    }

    fn make_hot(id: &str, title: &str, tags: &[&str], url: &str) -> HotItem {
        HotItem {
            id: id.into(),
            platform: "zhihu".into(),
            rank: 1,
            title: title.into(),
            hot_value: Some(3_000_000),
            url: url.into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            summary: None,
            first_seen: chrono::Utc::now().timestamp(),
            last_seen: chrono::Utc::now().timestamp(),
            source_trust: deskpet_core::TrustLevel::Direct,
        }
    }

    #[test]
    fn template_script_contains_title_url_and_interest() {
        let item = make_hot("h1", "DeepSeek发布", &["AI"], "https://example.com/x");
        let mut profile = UserProfile::default();
        profile.interests.push(Interest {
            topic: "AI".into(), weight: 0.9, last_hit: 0, embedding: None,
        });
        let s = template_script(&item, &profile);
        assert!(s.contains("DeepSeek发布"));
        assert!(s.contains("https://example.com/x"));
        assert!(s.contains("AI"));
    }

    #[test]
    fn template_script_no_interests_still_works() {
        let item = make_hot("h2", "某明星离婚", &[], "https://example.com/y");
        let s = template_script(&item, &UserProfile::default());
        assert!(s.contains("某明星离婚"));
        assert!(s.contains("https://example.com/y"));
    }

    #[test]
    fn synthesize_persona_from_interests() {
        let mut p = UserProfile::default();
        p.interests.push(Interest { topic: "AI".into(), weight: 0.9, last_hit: 0, embedding: None });
        p.interests.push(Interest { topic: "数码".into(), weight: 0.5, last_hit: 0, embedding: None });
        let s = synthesize_persona(&p);
        assert!(s.contains("AI"));
    }

    /// 验证 fetch_hot_detail 工具能从本地库取详情（可溯源，不编造）。
    #[tokio::test]
    async fn execute_tool_fetches_real_detail() {
        let db = mem_db();
        let item = make_hot("h3", "DeepSeek桌面版", &["AI"], "https://zhihu.com/q/1");
        {
            let conn = db.lock().await;
            HotRepo::upsert(&conn, &item).unwrap();
        }
        let args = serde_json::json!({ "hot_id": "h3" });
        let conn = db.lock().await;
        let detail = functions::execute_tool("fetch_hot_detail", &args, &conn);
        assert!(detail.is_some());
        let d = detail.unwrap();
        assert_eq!(d["title"], "DeepSeek桌面版");
        assert_eq!(d["url"], "https://zhihu.com/q/1");
    }

    /// mock LLM：返回固定文案，验证 compose_script 走通对话路径。
    struct MockLlm { reply: String }
    #[async_trait::async_trait]
    impl LlmProvider for MockLlm {
        async fn chat(&self, _m: &[ChatMessage], _t: Option<&[serde_json::Value]>) -> deskpet_core::Result<LlmResponse> {
            Ok(LlmResponse { content: Some(self.reply.clone()), function_call: None })
        }
    }

    /// mock embedder：返回零向量。
    struct MockEmbedder;
    #[async_trait::async_trait]
    impl EmbeddingProvider for MockEmbedder {
        async fn embed(&self, texts: &[String]) -> deskpet_core::Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![0.0f32; 8]).collect())
        }
    }

    /// mock LLM 失败时验证降级到模板文案。
    struct FailingLlm;
    #[async_trait::async_trait]
    impl LlmProvider for FailingLlm {
        async fn chat(&self, _m: &[ChatMessage], _t: Option<&[serde_json::Value]>) -> deskpet_core::Result<LlmResponse> {
            Err(DeskPetError::Llm("no api key".into()))
        }
    }

    #[tokio::test]
    async fn compose_falls_back_to_template_when_llm_fails() {
        let db = mem_db();
        let item = make_hot("h4", "DeepSeek桌面版", &["AI"], "https://zhihu.com/q/4");
        {
            let conn = db.lock().await;
            HotRepo::upsert(&conn, &item).unwrap();
        }
        let llm: Arc<dyn LlmProvider> = Arc::new(FailingLlm);
        let emb: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbedder);
        let mut orch = AgentOrchestrator::new(llm, emb, db.clone());
        // 直接调 compose_script 应失败，再验证模板
        let profile = UserProfile::default();
        let result = orch.compose_script(&item, &profile).await;
        assert!(result.is_err());
        let tmpl = template_script(&item, &profile);
        assert!(tmpl.contains("DeepSeek桌面版"));
    }

    #[tokio::test]
    async fn compose_returns_llm_content_on_success() {
        let db = mem_db();
        let item = make_hot("h5", "DeepSeek桌面版", &["AI"], "https://zhihu.com/q/5");
        {
            let conn = db.lock().await;
            HotRepo::upsert(&conn, &item).unwrap();
        }
        let llm: Arc<dyn LlmProvider> = Arc::new(MockLlm {
            reply: "主人～DeepSeek桌面版发布啦，跟你爱的AI有关哦！原文：https://zhihu.com/q/5".into(),
        });
        let emb: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbedder);
        let orch = AgentOrchestrator::new(llm, emb, db);
        let profile = UserProfile::default();
        let s = orch.compose_script(&item, &profile).await.unwrap();
        assert!(s.contains("DeepSeek"));
        assert!(s.contains("https://zhihu.com/q/5"));
    }
}
