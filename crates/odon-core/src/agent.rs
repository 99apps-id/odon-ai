use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Working hours for an agent's desk in the office.
///
/// `days` uses ISO weekday numbers (1 = Monday … 7 = Sunday). An empty list
/// means "every day", so an agent without a configured schedule is always on
/// shift instead of never being present.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSchedule {
    #[serde(default = "default_work_start")]
    pub start: String,
    #[serde(default = "default_work_end")]
    pub end: String,
    #[serde(default)]
    pub days: Vec<u8>,
    #[serde(default)]
    pub timezone: Option<String>,
}

fn default_work_start() -> String {
    "09:00".to_string()
}

fn default_work_end() -> String {
    "18:00".to_string()
}

impl Default for AgentSchedule {
    fn default() -> Self {
        Self {
            start: default_work_start(),
            end: default_work_end(),
            days: vec![1, 2, 3, 4, 5],
            timezone: None,
        }
    }
}

impl AgentSchedule {
    /// Whether `now` falls inside the configured window. `timezone` is an IANA
    /// name ("Asia/Jakarta"); an unknown or empty value falls back to the
    /// daemon's local timezone.
    pub fn is_on_shift(&self, now: DateTime<Utc>) -> bool {
        use chrono::{Datelike, Local, Timelike};
        let tz = self
            .timezone
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| value.parse::<chrono_tz::Tz>().ok());
        let local = match tz {
            Some(tz) => now.with_timezone(&tz).naive_local(),
            None => now.with_timezone(&Local).naive_local(),
        };
        if !self.days.is_empty() {
            let weekday = local.weekday().number_from_monday() as u8;
            if !self.days.contains(&weekday) {
                return false;
            }
        }
        let Some(start) = parse_hhmm(&self.start) else {
            return true;
        };
        let Some(end) = parse_hhmm(&self.end) else {
            return true;
        };
        let minutes = local.hour() * 60 + local.minute();
        if start <= end {
            minutes >= start && minutes < end
        } else {
            // Overnight shifts (e.g. 22:00 → 06:00).
            minutes >= start || minutes < end
        }
    }
}

fn parse_hhmm(value: &str) -> Option<u32> {
    let (hours, minutes) = value.trim().split_once(':')?;
    let hours: u32 = hours.parse().ok()?;
    let minutes: u32 = minutes.parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(hours * 60 + minutes)
}

/// A persisted, named agent with its own specialization, prompt, model and
/// working hours. Sessions reference it through `sessions.agent_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    /// Specialization label shown on the desk (Coding, F.A.T, Design, CRM…).
    pub role: String,
    /// Accent colour for the desk, as a CSS hex string.
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub system_prompt: String,
    /// `provider::model` or `odon-auto`.
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub schedule: AgentSchedule,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_model() -> String {
    "odon-auto".to_string()
}

fn default_enabled() -> bool {
    true
}

impl Agent {
    pub fn new(name: impl Into<String>, role: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: format!("agent_{}", Uuid::new_v4().simple()),
            name: name.into(),
            role: role.into(),
            color: "#557a95".to_string(),
            system_prompt: String::new(),
            model: default_model(),
            workspace_root: None,
            skills: Vec::new(),
            enabled: true,
            sort_order: 0,
            schedule: AgentSchedule::default(),
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn schedule(start: &str, end: &str, days: Vec<u8>, timezone: Option<&str>) -> AgentSchedule {
        AgentSchedule {
            start: start.to_string(),
            end: end.to_string(),
            days,
            timezone: timezone.map(str::to_string),
        }
    }

    #[test]
    fn timezone_is_applied_to_the_shift_window() {
        // 09:00–18:00 in Jakarta (UTC+7): 02:00 UTC is 09:00 WIB (on shift),
        // 11:30 UTC is 18:30 WIB (off shift) — regardless of the daemon clock.
        let sched = schedule("09:00", "18:00", vec![], Some("Asia/Jakarta"));
        assert!(sched
            .is_on_shift(Utc.with_ymd_and_hms(2026, 11, 5, 2, 0, 0).unwrap()));
        assert!(!sched
            .is_on_shift(Utc.with_ymd_and_hms(2026, 11, 5, 11, 30, 0).unwrap()));
    }

    #[test]
    fn timezone_day_boundary_uses_the_configured_zone() {
        // 22:00–06:00 overnight in New York: 02:00 UTC is 21:00 or 22:00 EST
        // depending on DST, so pick a moment that is clearly inside: 03:00 UTC
        // is 22:00 EST in winter — on shift; 14:00 UTC is 09:00 EST — off.
        let sched = schedule("22:00", "06:00", vec![], Some("America/New_York"));
        assert!(sched
            .is_on_shift(Utc.with_ymd_and_hms(2026, 1, 15, 3, 0, 0).unwrap()));
        assert!(!sched
            .is_on_shift(Utc.with_ymd_and_hms(2026, 1, 15, 14, 0, 0).unwrap()));
    }

    #[test]
    fn empty_or_unknown_timezone_falls_back_to_local() {
        let all_day = schedule("00:00", "23:59", vec![], None);
        assert!(all_day.is_on_shift(Utc::now()));
        let unknown = schedule("00:00", "23:59", vec![], Some("Mars/Olympus_Mons"));
        assert!(unknown.is_on_shift(Utc::now()));
        let blank = schedule("00:00", "23:59", vec![], Some("  "));
        assert!(blank.is_on_shift(Utc::now()));
    }

    #[test]
    fn day_filter_respects_the_configured_zone() {
        // Friday 20:00 UTC is Saturday 03:00 WIB: a Sat-only schedule matches
        // even though the UTC weekday is still Friday.
        let sat_only = schedule("00:00", "23:59", vec![6], Some("Asia/Jakarta"));
        assert!(sat_only.is_on_shift(Utc.with_ymd_and_hms(2026, 11, 6, 20, 0, 0).unwrap()));
        // 12:00 UTC Friday is 19:00 WIB Friday: no match.
        assert!(!sat_only.is_on_shift(Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap()));
    }
}
