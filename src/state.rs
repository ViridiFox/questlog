use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::quest::Quest;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuestState {
    pub last_completed: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppState(pub HashMap<String, QuestState>);

impl AppState {
    pub fn key(quest: &Quest) -> String {
        format!("{}.{}", quest.game_id, quest.name)
    }
}

pub fn state_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("questlog")
        .join("state.json")
}

pub fn load_state(quests: &mut [Quest]) -> Result<()> {
    let path = state_path();
    if !path.exists() {
        return Ok(());
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read state at {}", path.display()))?;
    let state: AppState = serde_json::from_str(&contents).context("failed to parse state.json")?;

    for q in quests {
        if let Some(state) = state.0.get(&AppState::key(q)) {
            q.with_state(state);
        }
    }

    Ok(())
}

pub fn save_state(quests: &[Quest]) -> Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create state dir {}", parent.display()))?;
    }

    let state = quests
        .iter()
        .map(|q| (AppState::key(q), q.as_state()))
        .collect();

    let contents =
        serde_json::to_string_pretty(&AppState(state)).context("failed to serialize state")?;
    std::fs::write(&path, contents)
        .with_context(|| format!("failed to write state to {}", path.display()))?;
    Ok(())
}
