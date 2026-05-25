use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuestState {
    pub last_completed: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppState(pub HashMap<String, QuestState>);

impl AppState {
    pub fn key(game_id: &str, quest_name: &str) -> String {
        format!("{}.{}", game_id, quest_name)
    }

    pub fn last_completed(&self, game_id: &str, quest_name: &str) -> Option<DateTime<Utc>> {
        self.0.get(&Self::key(game_id, quest_name))?.last_completed
    }

    pub fn mark_complete(&mut self, game_id: &str, quest_name: &str, at: DateTime<Utc>) {
        self.0
            .entry(Self::key(game_id, quest_name))
            .or_default()
            .last_completed = Some(at);
    }

    pub fn mark_incomplete(&mut self, game_id: &str, quest_name: &str) {
        if let Some(entry) = self.0.get_mut(&Self::key(game_id, quest_name)) {
            entry.last_completed = None;
        }
    }
}

pub fn state_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("questlog")
        .join("state.json")
}

pub fn load_state() -> Result<AppState> {
    let path = state_path();
    if !path.exists() {
        return Ok(AppState::default());
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read state at {}", path.display()))?;
    let state: AppState = serde_json::from_str(&contents).context("failed to parse state.json")?;
    Ok(state)
}

pub fn save_state(state: &AppState) -> Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create state dir {}", parent.display()))?;
    }
    let contents = serde_json::to_string_pretty(state).context("failed to serialize state")?;
    std::fs::write(&path, contents)
        .with_context(|| format!("failed to write state to {}", path.display()))?;
    Ok(())
}
