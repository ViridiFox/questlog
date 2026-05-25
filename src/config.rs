use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ResetSpec {
    Shorthand(String),
    Single(ResetRuleRaw),
    Multiple(Vec<ResetRuleRaw>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResetRuleRaw {
    #[serde(rename = "type")]
    pub kind: String,
    pub time: Option<String>,
    pub day: Option<String>,
    pub minutes: Option<u64>,
    pub hours: Option<u64>,
    pub days: Option<u64>,
    pub weeks: Option<u64>,
    pub anchor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuestConfig {
    pub name: String,
    pub reset: ResetSpec,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GameConfig {
    pub name: String,
    pub timezone: Option<String>,
    pub reset_time: Option<String>,
    pub reset_day: Option<String>,
    #[serde(default)]
    pub quests: Vec<QuestConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Defaults {
    pub reset_time: Option<String>,
    pub reset_day: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawConfig {
    pub timezone: Option<String>,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub games: HashMap<String, GameConfig>,
}

/// Resolved defaults for a game context.
#[derive(Debug, Clone)]
pub struct ResolvedDefaults {
    pub timezone: String,
    pub reset_time: String,
    pub reset_day: String,
}

impl RawConfig {
    pub fn resolved_defaults_for(&self, game: &GameConfig) -> ResolvedDefaults {
        ResolvedDefaults {
            timezone: game
                .timezone
                .clone()
                .or_else(|| self.timezone.clone())
                .unwrap_or_else(|| "UTC".to_string()),
            reset_time: game
                .reset_time
                .clone()
                .or_else(|| self.defaults.reset_time.clone())
                .unwrap_or_else(|| "00:00".to_string()),
            reset_day: game
                .reset_day
                .clone()
                .or_else(|| self.defaults.reset_day.clone())
                .unwrap_or_else(|| "monday".to_string()),
        }
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("questlog")
        .join("config.toml")
}

pub fn load_config() -> Result<RawConfig> {
    let path = config_path();
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    let config: RawConfig = toml::from_str(&contents).context("failed to parse config.toml")?;
    Ok(config)
}

const DEFAULT_CONFIG: &str = r#"# questlog configuration

# Global timezone (IANA name, e.g. "Europe/Berlin"). Defaults to UTC.
# timezone = "UTC"

# Global default reset time and day used when a quest/game doesn't specify one.
[defaults]
# reset_time = "00:00"
# reset_day  = "monday"

# ---------------------------------------------------------------------------
# Reset types
# ---------------------------------------------------------------------------
#
# Shorthands — inherit reset_time / reset_day / timezone from game or defaults:
#
#   reset = "daily"
#   reset = "weekly"
#
# daily — resets at a fixed time each day:
#
#   reset = { type = "daily", time = "08:00" }
#
# weekly — resets at a fixed time on a given weekday:
#
#   reset = { type = "weekly", day = "tuesday", time = "16:00" }
#
# interval — resets a fixed duration after the last completion:
#
#   reset = { type = "interval", minutes = 30 }
#   reset = { type = "interval", hours = 4 }
#   reset = { type = "interval", days = 1 }
#   reset = { type = "interval", weeks = 1 }
#   (fields are additive: hours = 1, minutes = 30  =>  90 minutes)
#
# schedule — resets on a repeating clock-aligned period:
#
#   reset = { type = "schedule", minutes = 15 }          # every 15 min, epoch-aligned
#   reset = { type = "schedule", hours = 2 }             # every 2 hours, epoch-aligned
#   reset = { type = "schedule", hours = 12, anchor = "2026-01-01T08:00:00+01:00" }
#
# Multiple rules — available when ANY rule fires, resets at the soonest next reset:
#
#   reset = [
#     { type = "daily", time = "08:00" },
#     { type = "daily", time = "20:00" },
#   ]
#
# ---------------------------------------------------------------------------
# Games
# ---------------------------------------------------------------------------
#
# [games.<id>]
# name       = "Display Name"   # shown in TUI and CLI output
# timezone   = "Europe/London"  # overrides global timezone for this game
# reset_time = "16:00"          # default time for daily/weekly resets
# reset_day  = "tuesday"        # default day for weekly resets
#
# [[games.<id>.quests]]
# name  = "Quest name"
# reset = "daily"               # any reset value from above
#
# ---------------------------------------------------------------------------
# Example
# ---------------------------------------------------------------------------
#
# [games.ffxiv]
# name = "Final Fantasy XIV"
# timezone = "Europe/London"
# reset_time = "16:00"
# reset_day  = "tuesday"
#
# [[games.ffxiv.quests]]
# name = "Daily roulettes"
# reset = "daily"
#
# [[games.ffxiv.quests]]
# name = "Weekly raid"
# reset = "weekly"
#
# [[games.ffxiv.quests]]
# name = "Chaos recipe"
# reset = { type = "interval", hours = 24 }
#
# [[games.ffxiv.quests]]
# name = "Timed node"
# reset = { type = "schedule", hours = 12, anchor = "2026-01-01T08:00:00+01:00" }
"#;

pub fn load_or_create_config() -> Result<RawConfig> {
    let path = config_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config dir {}", parent.display()))?;
        }
        std::fs::write(&path, DEFAULT_CONFIG)
            .with_context(|| format!("failed to write default config to {}", path.display()))?;
        eprintln!("Created default config at {}", path.display());
    }
    load_config()
}
