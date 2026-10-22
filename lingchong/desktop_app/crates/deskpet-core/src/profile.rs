use serde::{Deserialize, Serialize};

/// 宠物本身的身份和表达偏好，与用户兴趣画像独立。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PetProfile {
    pub name: String,
    pub personality: String,
    pub speaking_style: String,
    pub background: String,
}

impl Default for PetProfile {
    fn default() -> Self {
        Self {
            name: "基尔兽".into(),
            personality: "活泼、好奇、友善".into(),
            speaking_style: "自然亲切，有自己的想法，表达方式灵活".into(),
            background: String::new(),
        }
    }
}

/// 用户画像中的一条兴趣。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interest {
    /// 兴趣主题，如 "AI/大模型"
    pub topic: String,
    /// 权重 0.0 ~ 1.0
    pub weight: f32,
    /// 末次命中时间戳（秒）
    pub last_hit: i64,
    /// 主题的 embedding 向量，由 EmbeddingProvider 生成并持久化。
    /// None 表示尚未计算，需在 Agent 周期前补算。
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
}

/// 时段偏好。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulePref {
    /// 勿扰时段，如 "23:00-08:00"
    pub quiet_hours: Vec<String>,
    /// 两次主动播报之间的最小间隔（分钟）
    pub min_announce_interval_min: u32,
}

impl Default for SchedulePref {
    fn default() -> Self {
        Self {
            quiet_hours: vec!["23:00-08:00".to_string()],
            min_announce_interval_min: 45,
        }
    }
}

/// 用户画像。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserProfile {
    pub interests: Vec<Interest>,
    /// LLM 生成的人物描述，用于口播语气定制
    pub persona: String,
    pub schedule: SchedulePref,
    /// 末次更新时间戳
    pub updated_at: i64,
}

impl UserProfile {
    /// 每日全局衰减，避免旧兴趣永久置顶。
    pub fn decay_weights(&mut self, factor: f32) {
        for i in &mut self.interests {
            i.weight = (i.weight * factor).clamp(0.0, 1.0);
        }
    }

    /// 调整某主题权重。
    pub fn adjust_topic(&mut self, topic: &str, delta: f32, now: i64) {
        if let Some(i) = self.interests.iter_mut().find(|i| i.topic == topic) {
            i.weight = (i.weight + delta).clamp(0.0, 1.0);
            i.last_hit = now;
        } else if delta > 0.0 {
            self.interests.push(Interest {
                topic: topic.to_string(),
                weight: delta.clamp(0.0, 1.0),
                last_hit: now,
                embedding: None,
            });
        }
    }

    /// 找出尚未计算 embedding 的兴趣主题列表。
    pub fn topics_without_embedding(&self) -> Vec<String> {
        self.interests
            .iter()
            .filter(|i| i.embedding.is_none())
            .map(|i| i.topic.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_with(topic: &str, weight: f32) -> UserProfile {
        let mut p = UserProfile::default();
        p.interests.push(Interest {
            topic: topic.into(),
            weight,
            last_hit: 0,
            embedding: None,
        });
        p
    }

    #[test]
    fn adjust_topic_positive_clamps_to_one() {
        let mut p = profile_with("AI", 0.95);
        p.adjust_topic("AI", 0.12, 100);
        assert_eq!(p.interests[0].weight, 1.0);
        assert_eq!(p.interests[0].last_hit, 100);
    }

    #[test]
    fn adjust_topic_negative_clamps_to_zero() {
        let mut p = profile_with("AI", 0.05);
        p.adjust_topic("AI", -0.10, 100);
        assert_eq!(p.interests[0].weight, 0.0);
    }

    #[test]
    fn adjust_topic_creates_new_when_positive() {
        let mut p = UserProfile::default();
        p.adjust_topic("数码", 0.20, 50);
        assert_eq!(p.interests.len(), 1);
        assert_eq!(p.interests[0].topic, "数码");
        assert!((p.interests[0].weight - 0.20).abs() < 1e-6);
    }

    #[test]
    fn adjust_topic_does_not_create_when_negative() {
        let mut p = UserProfile::default();
        p.adjust_topic("数码", -0.10, 50);
        assert!(p.interests.is_empty());
    }

    #[test]
    fn decay_reduces_weights() {
        let mut p = profile_with("AI", 1.0);
        p.decay_weights(0.99);
        assert!((p.interests[0].weight - 0.99).abs() < 1e-6);
        // 连续衰减
        p.decay_weights(0.99);
        assert!((p.interests[0].weight - 0.9801).abs() < 1e-5);
    }

    #[test]
    fn topics_without_embedding_filters() {
        let mut p = UserProfile::default();
        p.interests.push(Interest { topic: "AI".into(), weight: 0.5, last_hit: 0, embedding: Some(vec![1.0]) });
        p.interests.push(Interest { topic: "数码".into(), weight: 0.3, last_hit: 0, embedding: None });
        let missing = p.topics_without_embedding();
        assert_eq!(missing, vec!["数码".to_string()]);
    }

    #[test]
    fn feedback_delta_values() {
        use crate::types::FeedbackAction;
        assert_eq!(FeedbackAction::Like.delta(), 0.12);
        assert_eq!(FeedbackAction::Click.delta(), 0.08);
        assert_eq!(FeedbackAction::Ignore.delta(), -0.03);
        assert_eq!(FeedbackAction::Dismiss.delta(), -0.05);
        assert_eq!(FeedbackAction::Dislike.delta(), -0.10);
    }
}
