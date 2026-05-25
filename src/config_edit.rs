//! Structure-preserving config mutations using `toml_edit`.
//!
//! All functions read the config file, apply the requested change, and write it
//! back in one atomic step (write to a temp file then rename).  Comments and
//! existing formatting are preserved.

use anyhow::{bail, Context, Result};
use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::config::config_path;

// ── helpers ──────────────────────────────────────────────────────────────────

fn read_doc() -> Result<DocumentMut> {
    let path = config_path();
    let src = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    src.parse::<DocumentMut>()
        .context("failed to parse config.toml as TOML document")
}

fn write_doc(doc: &DocumentMut) -> Result<()> {
    let path = config_path();
    // Write to a sibling temp file then rename for atomicity.
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string())
        .with_context(|| format!("failed to write temp config to {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("failed to replace config at {}", path.display()))?;
    Ok(())
}

fn games_table_mut(doc: &mut DocumentMut) -> &mut Table {
    if !doc.contains_key("games") {
        doc["games"] = Item::Table(Table::new());
    }
    doc["games"].as_table_mut().expect("games must be a table")
}

// ── games ────────────────────────────────────────────────────────────────────

pub struct GameSpec {
    pub id: String,
    pub name: String,
    pub timezone: Option<String>,
    pub reset_time: Option<String>,
    pub reset_day: Option<String>,
}

/// Add a new `[games.<id>]` block.  Fails if the ID already exists.
pub fn add_game(spec: &GameSpec) -> Result<()> {
    let mut doc = read_doc()?;
    {
        let games = games_table_mut(&mut doc);
        if games.contains_key(&spec.id) {
            bail!("game '{}' already exists in config", spec.id);
        }
        let mut gt = Table::new();
        gt["name"] = value(spec.name.as_str());
        if let Some(ref tz) = spec.timezone {
            gt["timezone"] = value(tz.as_str());
        }
        if let Some(ref rt) = spec.reset_time {
            gt["reset_time"] = value(rt.as_str());
        }
        if let Some(ref rd) = spec.reset_day {
            gt["reset_day"] = value(rd.as_str());
        }
        // Ensure quests array exists so quests can be appended later.
        gt["quests"] = Item::Value(toml_edit::Value::Array(Array::new()));
        games.insert(&spec.id, Item::Table(gt));
    }
    write_doc(&doc)
}

/// Update fields on an existing `[games.<id>]` block.
pub fn update_game(spec: &GameSpec) -> Result<()> {
    let mut doc = read_doc()?;
    {
        let games = games_table_mut(&mut doc);
        let game = games
            .get_mut(&spec.id)
            .and_then(|i| i.as_table_mut())
            .with_context(|| format!("game '{}' not found in config", spec.id))?;
        game["name"] = value(spec.name.as_str());
        set_or_remove(game, "timezone", spec.timezone.as_deref());
        set_or_remove(game, "reset_time", spec.reset_time.as_deref());
        set_or_remove(game, "reset_day", spec.reset_day.as_deref());
    }
    write_doc(&doc)
}

fn set_or_remove(table: &mut Table, key: &str, val: Option<&str>) {
    if let Some(v) = val {
        table[key] = value(v);
    } else {
        table.remove(key);
    }
}

/// Remove a `[games.<id>]` block (and all its quests).
pub fn remove_game(game_id: &str) -> Result<()> {
    let mut doc = read_doc()?;
    {
        let games = games_table_mut(&mut doc);
        if !games.contains_key(game_id) {
            bail!("game '{}' not found in config", game_id);
        }
        games.remove(game_id);
    }
    write_doc(&doc)
}

// ── quests ───────────────────────────────────────────────────────────────────

pub struct QuestSpec {
    pub game_id: String,
    pub name: String,
    /// Reset spec as a TOML inline value string, e.g. `"daily"` or
    /// `{ type = "interval", hours = 4 }`.  We accept the two common forms:
    /// - bare shorthand string like `daily` or `weekly`
    /// - TOML inline table literal (the user supplies it verbatim)
    pub reset: String,
}

/// Returns true if a quest with `name` already exists in the game table,
/// handling both array-of-tables and inline-array forms.
fn quest_exists_in_game(game: &Table, name: &str) -> bool {
    match game.get("quests") {
        Some(Item::ArrayOfTables(aot)) => aot.iter().any(|t| {
            t.get("name").and_then(|v| v.as_str()) == Some(name)
        }),
        Some(Item::Value(v)) => v.as_array().map_or(false, |arr| {
            arr.iter().any(|entry| {
                entry.as_inline_table()
                    .and_then(|t| t.get("name"))
                    .and_then(|v| v.as_str())
                    == Some(name)
            })
        }),
        _ => false,
    }
}

/// Append a new quest to a game's quests.
///
/// Handles both representations that appear in real configs:
/// - `[[games.<id>.quests]]` — array of tables (most common; what users write by hand)
/// - `quests = [...]` — inline array (what `add_game` writes for new games)
pub fn add_quest(spec: &QuestSpec) -> Result<()> {
    let mut doc = read_doc()?;
    {
        let games = games_table_mut(&mut doc);
        let game = games
            .get_mut(&spec.game_id)
            .and_then(|i| i.as_table_mut())
            .with_context(|| format!("game '{}' not found in config", spec.game_id))?;

        // Check for duplicate name across both storage forms.
        if quest_exists_in_game(game, &spec.name) {
            bail!(
                "quest '{}' already exists in game '{}'",
                spec.name,
                spec.game_id
            );
        }

        let reset_val = parse_reset_value(&spec.reset)?;

        match game.get("quests") {
            // ── array of tables: [[games.<id>.quests]] ───────────────────────
            Some(Item::ArrayOfTables(_)) => {
                let mut entry = Table::new();
                entry["name"] = value(spec.name.as_str());
                entry["reset"] = Item::Value(reset_val);
                game["quests"]
                    .as_array_of_tables_mut()
                    .expect("matched above")
                    .push(entry);
            }
            // ── inline array: quests = [...] ─────────────────────────────────
            Some(Item::Value(_)) => {
                let mut quest_inline = toml_edit::InlineTable::new();
                quest_inline.insert("name", spec.name.as_str().into());
                quest_inline.insert("reset", reset_val);
                game["quests"]
                    .as_value_mut()
                    .and_then(|v| v.as_array_mut())
                    .context("'quests' key exists but is not an array")?
                    .push(toml_edit::Value::InlineTable(quest_inline));
            }
            // ── key absent: create as array of tables ────────────────────────
            None => {
                let mut entry = Table::new();
                entry["name"] = value(spec.name.as_str());
                entry["reset"] = Item::Value(reset_val);
                let mut aot = toml_edit::ArrayOfTables::new();
                aot.push(entry);
                game["quests"] = Item::ArrayOfTables(aot);
            }
            _ => bail!("unexpected TOML structure for 'quests' in game '{}'", spec.game_id),
        }
    }
    write_doc(&doc)
}

/// Update the reset spec (and optionally rename) a quest.
pub fn update_quest(game_id: &str, quest_name: &str, new_spec: &QuestSpec) -> Result<()> {
    let mut doc = read_doc()?;
    {
        let games = games_table_mut(&mut doc);
        let game = games
            .get_mut(game_id)
            .and_then(|i| i.as_table_mut())
            .with_context(|| format!("game '{}' not found in config", game_id))?;

        let reset_val = parse_reset_value(&new_spec.reset)?;

        match game.get_mut("quests") {
            Some(Item::ArrayOfTables(aot)) => {
                let entry = aot
                    .iter_mut()
                    .find(|t| t.get("name").and_then(|v| v.as_str()) == Some(quest_name))
                    .with_context(|| format!("quest '{}' not found in game '{}'", quest_name, game_id))?;
                entry["name"] = value(new_spec.name.as_str());
                entry["reset"] = Item::Value(reset_val);
            }
            Some(Item::Value(v)) => {
                let arr = v.as_array_mut().context("'quests' is not an array")?;
                let idx = arr
                    .iter()
                    .position(|entry| {
                        entry.as_inline_table()
                            .and_then(|t| t.get("name"))
                            .and_then(|v| v.as_str())
                            == Some(quest_name)
                    })
                    .with_context(|| format!("quest '{}' not found in game '{}'", quest_name, game_id))?;
                let mut qt = toml_edit::InlineTable::new();
                qt.insert("name", new_spec.name.as_str().into());
                qt.insert("reset", reset_val);
                arr.replace(idx, toml_edit::Value::InlineTable(qt));
            }
            _ => bail!("game '{}' has no quests", game_id),
        }
    }
    write_doc(&doc)
}

/// Remove a quest by name from a game.
pub fn remove_quest(game_id: &str, quest_name: &str) -> Result<()> {
    let mut doc = read_doc()?;
    {
        let games = games_table_mut(&mut doc);
        let game = games
            .get_mut(game_id)
            .and_then(|i| i.as_table_mut())
            .with_context(|| format!("game '{}' not found in config", game_id))?;

        match game.get_mut("quests") {
            Some(Item::ArrayOfTables(aot)) => {
                let idx = aot
                    .iter()
                    .position(|t| t.get("name").and_then(|v| v.as_str()) == Some(quest_name))
                    .with_context(|| format!("quest '{}' not found in game '{}'", quest_name, game_id))?;
                aot.remove(idx);
            }
            Some(Item::Value(v)) => {
                let arr = v.as_array_mut().context("'quests' is not an array")?;
                let idx = arr
                    .iter()
                    .position(|entry| {
                        entry.as_inline_table()
                            .and_then(|t| t.get("name"))
                            .and_then(|v| v.as_str())
                            == Some(quest_name)
                    })
                    .with_context(|| format!("quest '{}' not found in game '{}'", quest_name, game_id))?;
                arr.remove(idx);
            }
            _ => bail!("game '{}' has no quests", game_id),
        }
    }
    write_doc(&doc)
}

// ── reset value parsing ───────────────────────────────────────────────────────

/// Accept either a bare shorthand (`daily`, `weekly`) or an inline TOML table
/// literal such as `{ type = "interval", hours = 4 }`.
fn parse_reset_value(s: &str) -> Result<toml_edit::Value> {
    let trimmed = s.trim();
    // Shorthands are plain strings.
    if trimmed == "daily" || trimmed == "weekly" {
        return Ok(trimmed.into());
    }
    // Try parsing as an inline table wrapped in a synthetic key assignment so
    // toml_edit can handle it.
    let synthetic = format!("x = {}", trimmed);
    let parsed: DocumentMut = synthetic
        .parse()
        .with_context(|| format!("invalid reset spec '{}' — expected a shorthand (\"daily\", \"weekly\") or an inline TOML table like {{ type = \"interval\", hours = 4 }}", s))?;
    parsed["x"]
        .as_value()
        .cloned()
        .context("reset spec parsed but produced no value")
}
