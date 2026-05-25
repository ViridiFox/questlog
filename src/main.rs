mod config;
mod quest;
mod state;
mod tui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use chrono::Utc;
use chrono_tz::Tz;
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
            let new_state = tui::run(quests, app_state, tz)?;
            state::save_state(&new_state)?;
        }
        Some(Command::List { game, sort_done_last }) => {
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
                    filter.as_deref().map_or(true, |g| {
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

            let w_status   = rows.iter().map(|r| r.status.len()).max().unwrap();
            let w_game     = rows.iter().map(|r| r.game.len()).max().unwrap();
            let w_name     = rows.iter().map(|r| r.name.len()).max().unwrap();
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
    }

    Ok(())
}
