//! 调度器：定时触发采集与 Agent，管理勿扰与最小间隔。

pub mod scheduler;
pub mod policy;

pub use scheduler::{Scheduler, FetchFn, AgentFn, DailyFn};
pub use policy::AnnouncePolicy;
