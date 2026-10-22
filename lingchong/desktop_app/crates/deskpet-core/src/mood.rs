use serde::{Deserialize, Serialize};

/// 宠物情绪状态。驱动前端动画切换。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mood {
    Idle,
    Happy,
    Curious,
    Excited,
    Sleepy,
    Bored,
}

impl Mood {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mood::Idle => "idle",
            Mood::Happy => "happy",
            Mood::Curious => "curious",
            Mood::Excited => "excited",
            Mood::Sleepy => "sleepy",
            Mood::Bored => "bored",
        }
    }
}

impl Default for Mood {
    fn default() -> Self {
        Mood::Idle
    }
}
