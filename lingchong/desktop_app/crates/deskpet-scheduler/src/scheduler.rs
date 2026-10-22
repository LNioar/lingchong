//! 轻量调度器：定时触发采集与 Agent 唤醒。

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use chrono::Utc;
use log::info;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::policy::AnnouncePolicy;

/// 调度器持有的回调。
pub type FetchFn = Arc<dyn Fn(String) + Send + Sync>;
pub type AgentFn = Arc<dyn Fn() + Send + Sync>;
/// 每日维护回调（画像衰减 + 数据清理等）。
pub type DailyFn = Arc<dyn Fn() + Send + Sync>;

pub struct Scheduler {
    /// 各平台采集间隔（用 std Mutex，因初始化在 runtime 内，不能用 tokio Mutex 的 blocking_lock）。
    fetch_interval: Arc<StdMutex<HashMap<String, Duration>>>,
    fetch_cb: FetchFn,
    agent_interval: Arc<StdMutex<Duration>>,
    /// Agent 回调；None 时由外部驱动（GUI/headless 自行轮询），调度器不启动 agent 任务。
    agent_cb: Option<AgentFn>,
    /// 每日维护回调；None 时不启动日级任务。
    daily_cb: Option<DailyFn>,
    policy: Mutex<AnnouncePolicy>,
    last_announce: Mutex<Option<i64>>,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl Scheduler {
    pub fn new(fetch_cb: FetchFn, agent_cb: Option<AgentFn>) -> Self {
        Self {
            fetch_interval: Arc::new(StdMutex::new(HashMap::new())),
            fetch_cb,
            agent_interval: Arc::new(StdMutex::new(Duration::from_secs(45 * 60))),
            agent_cb,
            daily_cb: None,
            policy: Mutex::new(AnnouncePolicy::default()),
            last_announce: Mutex::new(None),
            handles: Mutex::new(Vec::new()),
        }
    }

    /// 注册每日维护回调（画像衰减、数据清理等），在每日 00:00 触发。
    pub fn set_daily_cb(&mut self, cb: DailyFn) {
        self.daily_cb = Some(cb);
    }

    pub fn set_fetch_interval(&mut self, platform: &str, interval: Duration) {
        self.fetch_interval.lock().unwrap().insert(platform.to_string(), interval);
    }

    pub fn set_agent_interval(&mut self, interval: Duration) {
        *self.agent_interval.lock().unwrap() = interval;
    }

    /// 热更新单个平台采集间隔（立即生效，下一轮 tick 使用新间隔）。
    pub async fn update_fetch_interval(&self, platform: &str, interval: Duration) {
        self.fetch_interval.lock().unwrap().insert(platform.to_string(), interval);
    }

    /// 热更新 Agent 轮询间隔。
    pub async fn update_agent_interval(&self, interval: Duration) {
        *self.agent_interval.lock().unwrap() = interval;
    }

    /// 热更新打扰策略（勿扰时段 + 最小播报间隔）。
    pub async fn update_policy(&self, quiet_hours: Vec<String>, min_interval_min: u32) {
        let mut p = self.policy.lock().await;
        p.quiet_hours = quiet_hours;
        p.min_interval_min = min_interval_min;
    }

    pub async fn start(&self) {
        let mut handles = self.handles.lock().await;

        // 采集任务：每轮读取当前间隔，支持热更新
        let fetch_cb = self.fetch_cb.clone();
        let intervals = self.fetch_interval.clone();
        let platforms: Vec<String> = {
            let m = intervals.lock().unwrap();
            m.keys().cloned().collect()
        };
        for platform in platforms {
            let platform = platform.clone();
            let cb = fetch_cb.clone();
            let intervals = intervals.clone();
            let handle = tokio::spawn(async move {
                loop {
                    let interval = {
                        let m = intervals.lock().unwrap();
                        *m.get(&platform).unwrap_or(&Duration::from_secs(900))
                    };
                    tokio::time::sleep(interval).await;
                    info!("[scheduler] fetch tick: {}", platform);
                    cb(platform.clone());
                }
            });
            handles.push(handle);
        }

        // Agent 任务：仅当 agent_cb 存在时启动（None 时由外部驱动）
        if let Some(agent_cb) = self.agent_cb.clone() {
            let agent_interval = self.agent_interval.clone();
            let agent_handle = tokio::spawn(async move {
                loop {
                    let interval = *agent_interval.lock().unwrap();
                    tokio::time::sleep(interval).await;
                    info!("[scheduler] agent tick");
                    agent_cb();
                }
            });
            handles.push(agent_handle);
        }

        // 每日维护任务：先睡到次日 00:00，之后每 24h 触发一次
        if let Some(daily_cb) = self.daily_cb.clone() {
            let daily_handle = tokio::spawn(async move {
                loop {
                    if let Some(secs) = secs_until_next_midnight() {
                        tokio::time::sleep(Duration::from_secs(secs)).await;
                    }
                    info!("[scheduler] daily tick (decay + cleanup)");
                    daily_cb();
                    // 之后每 24 小时一次
                    tokio::time::sleep(Duration::from_secs(86400)).await;
                }
            });
            handles.push(daily_handle);
        }
    }

    pub async fn mark_announced(&self) {
        let mut la = self.last_announce.lock().await;
        *la = Some(Utc::now().timestamp());
    }

    pub async fn can_announce(&self) -> bool {
        let now = Utc::now().timestamp();
        let last = *self.last_announce.lock().await;
        self.policy.lock().await.can_announce(now, last)
    }

    pub async fn silence_for(&self, minutes: u32) {
        let now = Utc::now().timestamp();
        self.policy.lock().await.silence_for(minutes, now);
    }
}

/// 计算距离本地次日 00:00 的秒数。
fn secs_until_next_midnight() -> Option<u64> {
    use chrono::Local;
    let now = Local::now();
    let tomorrow = (now + chrono::Duration::days(1))
        .date_naive()
        .and_hms_opt(0, 0, 0)?;
    let now_naive = now.naive_local();
    let delta = tomorrow - now_naive;
    Some(delta.num_seconds().max(0) as u64)
}
