use serde::{Deserialize, Serialize};

/// 平台标识。
pub type PlatformId = &'static str;

/// 数据可信度分级。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    /// 直采平台官方接口
    Direct,
    /// 经第三方聚合源获取
    Aggregated,
}

impl TrustLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrustLevel::Direct => "direct",
            TrustLevel::Aggregated => "aggregated",
        }
    }
}

/// 标准化热点条目。所有 Adapter 的输出都必须归一成此结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotItem {
    /// sha256(platform + normalized_title)
    pub id: String,
    pub platform: String,
    pub rank: u32,
    pub title: String,
    /// 平台原始热度值，仅同平台内可比
    pub hot_value: Option<u64>,
    pub url: String,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    /// 入库时间戳（秒）
    pub first_seen: i64,
    /// 末次出现时间戳（秒）
    pub last_seen: i64,
    pub source_trust: TrustLevel,
}

impl HotItem {
    /// 生成确定性 id：sha256(platform + "|" + title)。
    pub fn make_id(platform: &str, title: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(platform.as_bytes());
        h.update(b"|");
        h.update(title.trim().as_bytes());
        hex::encode(h.finalize())
    }
}

/// 用户对一次播报的反馈动作。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FeedbackAction {
    /// 点击原文 / 表示感兴趣
    Click,
    /// 点赞
    Like,
    /// 忽略（未交互自动消失）
    Ignore,
    /// 主动关闭气泡
    Dismiss,
    /// 不喜欢
    Dislike,
}

impl FeedbackAction {
    /// 反馈对相关兴趣主题的权重增量。
    /// 正值强化兴趣，负值弱化；dislike 最强负反馈。
    pub fn delta(&self) -> f32 {
        match self {
            FeedbackAction::Click => 0.08,
            FeedbackAction::Like => 0.12,
            FeedbackAction::Ignore => -0.03,
            FeedbackAction::Dismiss => -0.05,
            FeedbackAction::Dislike => -0.10,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            FeedbackAction::Click => "click",
            FeedbackAction::Like => "like",
            FeedbackAction::Ignore => "ignore",
            FeedbackAction::Dismiss => "dismiss",
            FeedbackAction::Dislike => "dislike",
        }
    }
}
