mod config;
mod config_edit;
mod quest;
mod state;
mod tui;

use anyhow::{Context, Result};
use chrono::Utc;
use chrono_tz::Tz;
use clap::{Parser, Subcommand};
use std::str::FromStr;

fn system_tz() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|s| Tz::from_str(&s).ok())
        .unwrap_or(chrono_tz::UTC)
}

#[derive(Parser)]
#[command(name = "questlog", about = "Track recurring game quests")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Print status of all quests
    List {
        /// Filter by game ID
        #[arg(long)]
        game: Option<String>,
        /// Sort completed quests to the end
        #[arg(long)]
        sort_done_last: bool,
    },
    /// List all configured games
    ListGames,
    /// Open the config file in $EDITOR
    Edit,
    /// Mark a quest as complete
    Done {
        /// Quest name
        name: String,
        /// Game ID
        #[arg(long)]
        game: String,
    },
    /// Add a new game to the config
    AddGame {
        /// Game ID (used in config keys and --game flags)
        #[arg(long)]
        id: String,
        /// Display name
        #[arg(long)]
        name: String,
        /// IANA timezone (e.g. "Europe/Berlin")
        #[arg(long)]
        timezone: Option<String>,
        /// Default reset time for daily/weekly quests (HH:MM)
        #[arg(long)]
        reset_time: Option<String>,
        /// Default reset day for weekly quests
        #[arg(long)]
        reset_day: Option<String>,
    },
    /// Update an existing game's metadata
    UpdateGame {
        /// Game ID to update
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: String,
        /// IANA timezone (omit to clear)
        #[arg(long)]
        timezone: Option<String>,
        /// Default reset time (omit to clear)
        #[arg(long)]
        reset_time: Option<String>,
        /// Default reset day (omit to clear)
        #[arg(long)]
        reset_day: Option<String>,
    },
    /// Remove a game and all its quests from the config
    RemoveGame {
        /// Game ID to remove
        #[arg(long)]
        id: String,
    },
    /// Add a quest to a game in the config
    AddQuest {
        /// Quest name
        #[arg(long)]
        name: String,
        /// Game ID
        #[arg(long)]
        game: String,
        /// Reset spec: "daily", "weekly", or an inline TOML table
        /// e.g. '{ type = "interval", hours = 4 }'
        #[arg(long)]
        reset: String,
    },
    /// Update a quest's name and/or reset spec
    UpdateQuest {
        /// Current quest name
        #[arg(long)]
        name: String,
        /// Game ID
        #[arg(long)]
        game: String,
        /// New quest name (defaults to current name)
        #[arg(long)]
        new_name: Option<String>,
        /// New reset spec
        #[arg(long)]
        reset: String,
    },
    /// Remove a quest from the config
    RemoveQuest {
        /// Quest name
        #[arg(long)]
        name: String,
        /// Game ID
        #[arg(long)]
        game: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = config::load_or_create_config()?;
    let mut quests = quest::build_quests(&config)?;
    quest::sort_quests(&mut quests);
    let mut app_state = state::load_state()?;
    let tz = system_tz();

    match cli.command {
        None => {
            let new_state = tui::run(quests, app_state, &config, tz)?;
            state::save_state(&new_state)?;
        }
        Some(Command::List {
            game,
            sort_done_last,
        }) => {
            let now = Utc::now();
            let filter = game.as_deref().map(|g| g.to_lowercase());

            struct Row {
                status: &'static str,
                available: bool,
                game: String,
                name: String,
                schedule: String,
                next: String,
            }

            let mut rows: Vec<Row> = quests
                .iter()
                .filter(|q| {
                    filter.as_deref().is_none_or(|g| {
                        q.game_id.to_lowercase() == g || q.game_name.to_lowercase() == g
                    })
                })
                .map(|q| {
                    let last = app_state.last_completed(&q.game_id, &q.name);
                    let available = q.is_available(last, now);
                    Row {
                        status: if available { "available" } else { "done" },
                        available,
                        game: q.game_name.clone(),
                        name: q.name.clone(),
                        schedule: q.reset_schedule_label(),
                        next: q.format_next_available(last, now, tz),
                    }
                })
                .collect();

            if rows.is_empty() {
                return Ok(());
            }

            if sort_done_last {
                rows.sort_by_key(|r| !r.available);
            }

            let w_status = rows.iter().map(|r| r.status.len()).max().unwrap();
            let w_game = rows.iter().map(|r| r.game.len()).max().unwrap();
            let w_name = rows.iter().map(|r| r.name.len()).max().unwrap();
            let w_schedule = rows.iter().map(|r| r.schedule.len()).max().unwrap();

            for r in &rows {
                println!(
                    "{:<w_status$}  {:<w_game$}  {:<w_name$}  {:>w_schedule$}  {}",
                    r.status, r.game, r.name, r.schedule, r.next,
                );
            }
        }
        Some(Command::Edit) => {
            let path = config::config_path();
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
            std::process::Command::new(&editor)
                .arg(&path)
                .status()
                .with_context(|| format!("failed to launch editor '{}'", editor))?;
        }
        Some(Command::ListGames) => {
            let mut games: Vec<(&str, &str, usize)> = config
                .games
                .iter()
                .map(|(id, g)| {
                    let count = quests.iter().filter(|q| q.game_id == *id).count();
                    (id.as_str(), g.name.as_str(), count)
                })
                .collect();
            games.sort_by_key(|(id, _, _)| *id);
            if games.is_empty() {
                return Ok(());
            }
            let w_id = games.iter().map(|(id, _, _)| id.len()).max().unwrap();
            let w_name = games.iter().map(|(_, name, _)| name.len()).max().unwrap();
            for (id, name, count) in &games {
                println!(
                    "{:<w_id$}  {:<w_name$}  {} quest{}",
                    id,
                    name,
                    count,
                    if *count == 1 { "" } else { "s" }
                );
            }
        }
        Some(Command::Done { name, game }) => {
            let found = quests.iter().any(|q| q.game_id == game && q.name == name);
            if !found {
                anyhow::bail!("quest '{}' not found in game '{}'", name, game);
            }
            app_state.mark_complete(&game, &name, Utc::now());
            state::save_state(&app_state)?;
            let display_name = quests
                .iter()
                .find(|q| q.game_id == game)
                .map(|q| q.game_name.as_str())
                .unwrap_or(&game);
            println!("Marked '{}/{}' as complete.", display_name, name);
        }
        Some(Command::AddGame {
            id,
            name,
            timezone,
            reset_time,
            reset_day,
        }) => {
            config_edit::add_game(&config_edit::GameSpec {
                id: id.clone(),
                name: name.clone(),
                timezone,
                reset_time,
                reset_day,
            })?;
            println!("Added game '{id}' ({name}) to config.");
        }
        Some(Command::UpdateGame {
            id,
            name,
            timezone,
            reset_time,
            reset_day,
        }) => {
            config_edit::update_game(&config_edit::GameSpec {
                id: id.clone(),
                name: name.clone(),
                timezone,
                reset_time,
                reset_day,
            })?;
            println!("Updated game '{id}'.");
        }
        Some(Command::RemoveGame { id }) => {
            config_edit::remove_game(&id)?;
            println!("Removed game '{id}' from config.");
        }
        Some(Command::AddQuest { name, game, reset }) => {
            config_edit::add_quest(&config_edit::QuestSpec {
                game_id: game.clone(),
                name: name.clone(),
                reset,
            })?;
            println!("Added quest '{name}' to game '{game}'.");
        }
        Some(Command::UpdateQuest {
            name,
            game,
            new_name,
            reset,
        }) => {
            let effective_name = new_name.as_deref().unwrap_or(&name);
            config_edit::update_quest(
                &game,
                &name,
                &config_edit::QuestSpec {
                    game_id: game.clone(),
                    name: effective_name.to_string(),
                    reset,
                },
            )?;
            println!("Updated quest '{name}' in game '{game}'.");
        }
        Some(Command::RemoveQuest { name, game }) => {
            config_edit::remove_quest(&game, &name)?;
            println!("Removed quest '{name}' from game '{game}'.");
        }
    }

    Ok(())
}
