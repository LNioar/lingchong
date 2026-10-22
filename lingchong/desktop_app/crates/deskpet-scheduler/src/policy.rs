//! 主动播报打扰策略。

use chrono::{Local, Timelike};

#[derive(Debug, Clone)]
pub struct AnnouncePolicy {
    /// 勿扰时段，如 "23:00-08:00"
    pub quiet_hours: Vec<String>,
    /// 两次播报最小间隔（分钟）
    pub min_interval_min: u32,
    /// 全局静默截止时间戳
    pub silence_until: i64,
}

impl Default for AnnouncePolicy {
    fn default() -> Self {
        Self {
            quiet_hours: vec!["23:00-08:00".into()],
            min_interval_min: 45,
            silence_until: 0,
        }
    }
}

impl AnnouncePolicy {
    /// 判断当前是否处于勿扰时段（使用系统当前时间）。
    pub fn in_quiet_hours(&self) -> bool {
        let now = Local::now();
        let cur = now.hour() * 60 + now.minute();
        self.in_quiet_hours_at(cur)
    }

    /// 判断给定分钟数（0..=1439）是否处于勿扰时段，便于单元测试。
    pub fn in_quiet_hours_at(&self, cur_minutes: u32) -> bool {
        for range in &self.quiet_hours {
            if let Some((a, b)) = parse_range(range) {
                if a <= b {
                    if cur_minutes >= a && cur_minutes < b { return true; }
                } else {
                    // 跨天，如 23:00-08:00
                    if cur_minutes >= a || cur_minutes < b { return true; }
                }
            }
        }
        false
    }

    /// 综合判断是否允许播报。
    pub fn can_announce(&self, now_ts: i64, last_announce_ts: Option<i64>) -> bool {
        if self.in_quiet_hours() { return false; }
        if now_ts < self.silence_until { return false; }
        if let Some(last) = last_announce_ts {
            if (now_ts - last) < (self.min_interval_min as i64) * 60 {
                return false;
            }
        }
        true
    }

    /// 静默模式下判断是否允许播报（不依赖系统时间，便于测试）。
    /// `cur_minutes` 为当日分钟数，用于勿扰时段判断。
    pub fn can_announce_at(&self, now_ts: i64, last_announce_ts: Option<i64>, cur_minutes: u32) -> bool {
        if self.in_quiet_hours_at(cur_minutes) { return false; }
        if now_ts < self.silence_until { return false; }
        if let Some(last) = last_announce_ts {
            if (now_ts - last) < (self.min_interval_min as i64) * 60 {
                return false;
            }
        }
        true
    }

    /// 设置静默 N 分钟。
    pub fn silence_for(&mut self, minutes: u32, now_ts: i64) {
        self.silence_until = now_ts + (minutes as i64) * 60;
    }
}

/// 解析 "HH:MM-HH:MM" 为 (分钟, 分钟)。
fn parse_range(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.split('-');
    let a = parts.next()?;
    let b = parts.next()?;
    let pa = parse_hhmm(a)?;
    let pb = parse_hhmm(b)?;
    Some((pa, pb))
}

fn parse_hhmm(s: &str) -> Option<u32> {
    let mut p = s.split(':');
    let h: u32 = p.next()?.parse().ok()?;
    let m: u32 = p.next()?.parse().ok()?;
    Some(h * 60 + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> AnnouncePolicy {
        AnnouncePolicy {
            quiet_hours: vec!["23:00-08:00".into()],
            min_interval_min: 45,
            silence_until: 0,
        }
    }

    // ---------- 勿扰时段 ----------

    #[test]
    fn quiet_hours_cross_day_before_midnight() {
        let p = policy();
        // 23:30 属于跨天勿扰 (23:00-08:00)
        assert!(p.in_quiet_hours_at(23 * 60 + 30));
    }

    #[test]
    fn quiet_hours_cross_day_after_midnight() {
        let p = policy();
        // 07:00 属于勿扰
        assert!(p.in_quiet_hours_at(7 * 60));
        // 08:00 刚过，不属于（区间右开）
        assert!(!p.in_quiet_hours_at(8 * 60));
    }

    #[test]
    fn quiet_hours_outside_range() {
        let p = policy();
        // 12:00 不在勿扰时段
        assert!(!p.in_quiet_hours_at(12 * 60));
    }

    #[test]
    fn quiet_hours_same_day_range() {
        let p = AnnouncePolicy {
            quiet_hours: vec!["09:00-12:00".into()],
            ..Default::default()
        };
        assert!(p.in_quiet_hours_at(10 * 60));
        assert!(!p.in_quiet_hours_at(8 * 60));
        assert!(!p.in_quiet_hours_at(12 * 60));
    }

    #[test]
    fn quiet_hours_boundary_inclusive_left() {
        let p = policy();
        // 23:00 正好开始，属于
        assert!(p.in_quiet_hours_at(23 * 60));
    }

    // ---------- 最小间隔 ----------

    #[test]
    fn min_interval_blocks_too_soon() {
        let p = policy();
        // 在非勿扰时段测试，min_interval=45min
        let now = 1_000_000;
        // 上一次播报在 30 分钟前 → 间隔不足
        assert!(!p.can_announce_at(now, Some(now - 30 * 60), 12 * 60));
    }

    #[test]
    fn min_interval_allows_after_enough_time() {
        let p = policy();
        let now = 1_000_000;
        // 上一次在 50 分钟前 → 可以
        assert!(p.can_announce_at(now, Some(now - 50 * 60), 12 * 60));
    }

    #[test]
    fn min_interval_first_announce_allowed() {
        let p = policy();
        // 无历史播报 → 允许
        assert!(p.can_announce_at(1_000_000, None, 12 * 60));
    }

    // ---------- 静默 ----------

    #[test]
    fn silence_blocks_announce() {
        let mut p = policy();
        let now = 1_000_000;
        p.silence_for(60, now); // 静默 60 分钟
        // 静默期间不允许
        assert!(!p.can_announce_at(now + 30 * 60, None, 12 * 60));
        // 静默结束后允许
        assert!(p.can_announce_at(now + 61 * 60, None, 12 * 60));
    }

    // ---------- 综合 ----------

    #[test]
    fn quiet_hours_overrides_other_conditions() {
        let p = policy();
        let now = 1_000_000;
        // 即使满足间隔和无静默，勿扰时段也不允许
        assert!(!p.can_announce_at(now, Some(now - 50 * 60), 1 * 60)); // 01:00 在勿扰
    }

    #[test]
    fn can_announce_allows_when_everything_clear() {
        let p = policy();
        let now = 1_000_000;
        // 非勿扰、无静默、间隔足够
        assert!(p.can_announce_at(now, Some(now - 60 * 60), 12 * 60));
    }
}
