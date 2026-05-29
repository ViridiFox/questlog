use crate::config::{GameConfig, QuestConfig, RawConfig, ResetRuleRaw, ResetSpec};
use anyhow::{Result, bail};
use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Utc, Weekday};
use chrono_tz::Tz;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub enum ResetRule {
    Daily {
        time: NaiveTime,
        tz: Tz,
    },
    Weekly {
        day: Weekday,
        time: NaiveTime,
        tz: Tz,
    },
    Interval {
        duration: Duration,
    },
    Schedule {
        period: Duration,
        anchor: DateTime<Utc>,
    },
}

#[derive(Debug, Clone)]
pub struct Quest {
    pub game_id: String,
    pub game_name: String,
    pub name: String,
    pub rules: Vec<ResetRule>,
    pub reset_spec: ResetSpec,
}

/// Sort key for reset interval length: schedule < interval < daily < weekly
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResetClass {
    Schedule,
    Interval,
    Daily,
    Weekly,
}

impl Quest {
    pub fn reset_class(&self) -> ResetClass {
        // Use the "broadest" rule's class for sorting purposes
        let mut cls = ResetClass::Schedule;
        for rule in &self.rules {
            let c = match rule {
                ResetRule::Schedule { .. } => ResetClass::Schedule,
                ResetRule::Interval { .. } => ResetClass::Interval,
                ResetRule::Daily { .. } => ResetClass::Daily,
                ResetRule::Weekly { .. } => ResetClass::Weekly,
            };
            if c > cls {
                cls = c;
            }
        }
        cls
    }

    /// Next reset time for a given rule, given the last completion time.
    /// For interval rules this is completion-anchored.
    /// For clock-anchored rules (daily/weekly/schedule) this returns the most
    /// recent past tick, so callers can compare it against last_completed.
    pub fn last_reset(
        rule: &ResetRule,
        last_completed: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> DateTime<Utc> {
        match rule {
            ResetRule::Interval { duration } => {
                // Becomes available at last_completed + duration.
                let base = last_completed.unwrap_or(now - *duration - Duration::seconds(1));
                base + *duration
            }
            ResetRule::Daily { time, tz } => prev_daily(*time, *tz, now),
            ResetRule::Weekly { day, time, tz } => prev_weekly(*day, *time, *tz, now),
            ResetRule::Schedule { period, anchor } => prev_schedule(*period, *anchor, now),
        }
    }

    /// Next upcoming reset time for display purposes.
    pub fn next_reset_time(
        rule: &ResetRule,
        last_completed: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> DateTime<Utc> {
        match rule {
            ResetRule::Interval { duration } => {
                let base = last_completed.unwrap_or(now - *duration - Duration::seconds(1));
                base + *duration
            }
            ResetRule::Daily { time, tz } => next_daily(*time, *tz, now),
            ResetRule::Weekly { day, time, tz } => next_weekly(*day, *time, *tz, now),
            ResetRule::Schedule { period, anchor } => next_schedule(*period, *anchor, now),
        }
    }

    /// A quest is available when, for any rule, the last reset has passed
    /// more recently than the last completion.
    pub fn is_available(&self, last_completed: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
        self.rules.iter().any(|r| {
            let reset = Self::last_reset(r, last_completed, now);
            match r {
                ResetRule::Interval { .. } => now >= reset,
                _ => last_completed.is_none_or(|lc| lc < reset),
            }
        })
    }

    /// Returns the soonest upcoming reset time across all rules.
    pub fn next_available(
        &self,
        last_completed: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> DateTime<Utc> {
        self.rules
            .iter()
            .map(|r| Self::next_reset_time(r, last_completed, now))
            .min()
            .unwrap_or(now)
    }

    /// Next reset formatted in the given timezone.
    pub fn format_next_available(
        &self,
        last_completed: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
        tz: Tz,
    ) -> String {
        let next = self.next_available(last_completed, now);
        next.with_timezone(&tz)
            .format("%Y-%m-%d %H:%M %Z")
            .to_string()
    }

    /// Human-readable label describing the reset schedule, e.g. "daily 16:00", "every 4h".
    pub fn reset_schedule_label(&self) -> String {
        if self.rules.len() == 1 {
            rule_label(&self.rules[0])
        } else {
            self.rules
                .iter()
                .map(rule_label)
                .collect::<Vec<_>>()
                .join(", ")
        }
    }

    /// Value to prefill in the reset field of the edit modal.
    ///
    /// For shorthands this is the bare string (`"daily"` / `"weekly"`).
    /// For structured specs this is an inline TOML table or array literal
    /// that `config_edit::parse_reset_value` can round-trip.
    pub fn reset_edit_value(&self) -> String {
        match &self.reset_spec {
            ResetSpec::Shorthand(s) => s.clone(),
            ResetSpec::Single(raw) => raw_to_inline(raw),
            ResetSpec::Multiple(raws) => {
                let items: Vec<String> = raws.iter().map(raw_to_inline).collect();
                format!("[{}]", items.join(", "))
            }
        }
    }
}

/// Serialize a `ResetRuleRaw` back to an inline TOML table string, e.g.
/// `{ type = "interval", hours = 4 }`.
fn raw_to_inline(raw: &ResetRuleRaw) -> String {
    let mut parts = vec![format!("type = \"{}\"", raw.kind)];
    if let Some(ref t) = raw.time {
        parts.push(format!("time = \"{}\"", t));
    }
    if let Some(ref d) = raw.day {
        parts.push(format!("day = \"{}\"", d));
    }
    if let Some(v) = raw.minutes {
        parts.push(format!("minutes = {}", v));
    }
    if let Some(v) = raw.hours {
        parts.push(format!("hours = {}", v));
    }
    if let Some(v) = raw.days {
        parts.push(format!("days = {}", v));
    }
    if let Some(v) = raw.weeks {
        parts.push(format!("weeks = {}", v));
    }
    if let Some(ref a) = raw.anchor {
        parts.push(format!("anchor = \"{}\"", a));
    }
    format!("{{ {} }}", parts.join(", "))
}

fn rule_label(rule: &ResetRule) -> String {
    match rule {
        ResetRule::Daily { time, .. } => format!("daily {}", time.format("%H:%M")),
        ResetRule::Weekly { day, time, .. } => {
            format!("weekly {day:?} {}", time.format("%H:%M")).to_lowercase()
        }
        ResetRule::Interval { duration } => format_duration(*duration),
        ResetRule::Schedule { period, .. } => format!("every {}", format_duration(*period)),
    }
}

fn format_duration(d: Duration) -> String {
    let total = d.num_minutes();
    if total % (60 * 24 * 7) == 0 {
        let w = total / (60 * 24 * 7);
        format!("{}w", w)
    } else if total % (60 * 24) == 0 {
        let days = total / (60 * 24);
        format!("{}d", days)
    } else if total % 60 == 0 {
        let hours = total / 60;
        format!("{}h", hours)
    } else {
        format!("{}m", total)
    }
}

fn prev_daily(time: NaiveTime, tz: Tz, now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now.with_timezone(&tz);
    let today_reset = tz
        .from_local_datetime(&local.date_naive().and_time(time))
        .earliest()
        .unwrap()
        .with_timezone(&Utc);
    if now >= today_reset {
        today_reset
    } else {
        let yesterday = local.date_naive().pred_opt().unwrap();
        tz.from_local_datetime(&yesterday.and_time(time))
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }
}

fn prev_weekly(day: Weekday, time: NaiveTime, tz: Tz, now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now.with_timezone(&tz);
    let today = local.date_naive();
    let days_since = (today.weekday().num_days_from_monday() as i64
        - day.num_days_from_monday() as i64)
        .rem_euclid(7);
    let candidate_date = today - Duration::days(days_since);
    let candidate = tz
        .from_local_datetime(&candidate_date.and_time(time))
        .earliest()
        .unwrap()
        .with_timezone(&Utc);
    if now >= candidate {
        candidate
    } else {
        let prev_date = candidate_date - Duration::days(7);
        tz.from_local_datetime(&prev_date.and_time(time))
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }
}

fn prev_schedule(period: Duration, anchor: DateTime<Utc>, now: DateTime<Utc>) -> DateTime<Utc> {
    let period_secs = period.num_seconds();
    if period_secs <= 0 {
        return now;
    }
    let elapsed = (now - anchor).num_seconds();
    let periods_passed = if elapsed >= 0 {
        elapsed / period_secs
    } else {
        (elapsed - period_secs + 1) / period_secs
    };
    anchor + Duration::seconds(periods_passed * period_secs)
}

fn next_daily(time: NaiveTime, tz: Tz, now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now.with_timezone(&tz);
    let today_reset = tz
        .from_local_datetime(&local.date_naive().and_time(time))
        .earliest()
        .unwrap();
    if now < today_reset {
        today_reset.with_timezone(&Utc)
    } else {
        let tomorrow = local.date_naive().succ_opt().unwrap();
        tz.from_local_datetime(&tomorrow.and_time(time))
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }
}

fn next_weekly(day: Weekday, time: NaiveTime, tz: Tz, now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now.with_timezone(&tz);
    let today = local.date_naive();
    let days_until = (day.num_days_from_monday() as i64
        - today.weekday().num_days_from_monday() as i64)
        .rem_euclid(7);
    let candidate_date = today + Duration::days(days_until);
    let candidate = tz
        .from_local_datetime(&candidate_date.and_time(time))
        .earliest()
        .unwrap()
        .with_timezone(&Utc);
    if now < candidate {
        candidate
    } else {
        let next_date = candidate_date + Duration::days(7);
        tz.from_local_datetime(&next_date.and_time(time))
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }
}

fn next_schedule(period: Duration, anchor: DateTime<Utc>, now: DateTime<Utc>) -> DateTime<Utc> {
    let period_secs = period.num_seconds();
    if period_secs <= 0 {
        return now;
    }
    let elapsed = (now - anchor).num_seconds();
    let periods_passed = if elapsed >= 0 {
        elapsed / period_secs
    } else {
        (elapsed - period_secs + 1) / period_secs
    };
    let last_tick = anchor + Duration::seconds(periods_passed * period_secs);
    if now >= last_tick {
        last_tick + period
    } else {
        last_tick
    }
}

fn parse_time(s: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(s, "%H:%M:%S"))
        .map_err(|_| anyhow::anyhow!("invalid time '{}' — expected HH:MM", s))
}

fn parse_weekday(s: &str) -> Result<Weekday> {
    match s.to_lowercase().as_str() {
        "monday" | "mon" => Ok(Weekday::Mon),
        "tuesday" | "tue" => Ok(Weekday::Tue),
        "wednesday" | "wed" => Ok(Weekday::Wed),
        "thursday" | "thu" => Ok(Weekday::Thu),
        "friday" | "fri" => Ok(Weekday::Fri),
        "saturday" | "sat" => Ok(Weekday::Sat),
        "sunday" | "sun" => Ok(Weekday::Sun),
        _ => bail!("unknown weekday '{}'", s),
    }
}

fn parse_tz(s: &str) -> Result<Tz> {
    Tz::from_str(s).map_err(|_| anyhow::anyhow!("unknown timezone '{}'", s))
}

fn rule_from_raw(
    raw: &ResetRuleRaw,
    default_time: &str,
    default_day: &str,
    default_tz: &str,
) -> Result<ResetRule> {
    match raw.kind.as_str() {
        "daily" => {
            let time_str = raw.time.as_deref().unwrap_or(default_time);
            let time = parse_time(time_str)?;
            let tz = parse_tz(default_tz)?;
            Ok(ResetRule::Daily { time, tz })
        }
        "weekly" => {
            let time_str = raw.time.as_deref().unwrap_or(default_time);
            let day_str = raw.day.as_deref().unwrap_or(default_day);
            let time = parse_time(time_str)?;
            let day = parse_weekday(day_str)?;
            let tz = parse_tz(default_tz)?;
            Ok(ResetRule::Weekly { day, time, tz })
        }
        "interval" => {
            let total_minutes = raw.minutes.unwrap_or(0)
                + raw.hours.unwrap_or(0) * 60
                + raw.days.unwrap_or(0) * 60 * 24
                + raw.weeks.unwrap_or(0) * 60 * 24 * 7;
            if total_minutes == 0 {
                bail!(
                    "interval reset requires at least one duration field (minutes/hours/days/weeks)"
                );
            }
            Ok(ResetRule::Interval {
                duration: Duration::minutes(total_minutes as i64),
            })
        }
        "schedule" => {
            let total_minutes = raw.minutes.unwrap_or(0) + raw.hours.unwrap_or(0) * 60;
            if total_minutes == 0 {
                bail!("schedule reset requires minutes or hours");
            }
            let period = Duration::minutes(total_minutes as i64);
            let anchor = if let Some(ref a) = raw.anchor {
                DateTime::parse_from_rfc3339(a)
                    .map_err(|_| anyhow::anyhow!("invalid anchor timestamp '{}'", a))?
                    .with_timezone(&Utc)
            } else {
                DateTime::UNIX_EPOCH
            };
            Ok(ResetRule::Schedule { period, anchor })
        }
        other => bail!("unknown reset type '{}'", other),
    }
}

pub fn build_quests(config: &RawConfig) -> Result<Vec<Quest>> {
    let mut quests = Vec::new();

    for (game_id, game) in &config.games {
        let defaults = config.resolved_defaults_for(game);

        for quest_cfg in &game.quests {
            let rules = build_rules(
                quest_cfg,
                game,
                &defaults.reset_time,
                &defaults.reset_day,
                &defaults.timezone,
            )?;
            quests.push(Quest {
                game_id: game_id.clone(),
                game_name: game.name.clone(),
                name: quest_cfg.name.clone(),
                rules,
                reset_spec: quest_cfg.reset.clone(),
            });
        }
    }

    Ok(quests)
}

fn build_rules(
    quest: &QuestConfig,
    _game: &GameConfig,
    default_time: &str,
    default_day: &str,
    default_tz: &str,
) -> Result<Vec<ResetRule>> {
    match &quest.reset {
        ResetSpec::Shorthand(s) => {
            let raw = match s.as_str() {
                "daily" => ResetRuleRaw {
                    kind: "daily".to_string(),
                    time: None,
                    day: None,
                    minutes: None,
                    hours: None,
                    days: None,
                    weeks: None,
                    anchor: None,
                },
                "weekly" => ResetRuleRaw {
                    kind: "weekly".to_string(),
                    time: None,
                    day: None,
                    minutes: None,
                    hours: None,
                    days: None,
                    weeks: None,
                    anchor: None,
                },
                other => bail!("unknown reset shorthand '{}'", other),
            };
            Ok(vec![rule_from_raw(
                &raw,
                default_time,
                default_day,
                default_tz,
            )?])
        }
        ResetSpec::Single(raw) => Ok(vec![rule_from_raw(
            raw,
            default_time,
            default_day,
            default_tz,
        )?]),
        ResetSpec::Multiple(raws) => raws
            .iter()
            .map(|r| rule_from_raw(r, default_time, default_day, default_tz))
            .collect(),
    }
}

pub fn sort_quests(quests: &mut [Quest]) {
    quests.sort_by(|a, b| {
        a.reset_class()
            .cmp(&b.reset_class())
            .then_with(|| a.name.cmp(&b.name))
    });
}
