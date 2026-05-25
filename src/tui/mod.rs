pub mod ui;

use crate::config::{RawConfig, load_config};
use crate::config_edit::{self, GameSpec, QuestSpec};
use crate::quest::{Quest, build_quests, sort_quests};
use crate::state::AppState;
use anyhow::Result;
use chrono::Utc;
use chrono_tz::Tz;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::Duration;

// ── Modal state ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ModalKind {
    AddQuest,
    EditQuest { original_name: String },
    DeleteQuest { name: String, game_id: String },
    AddGame,
    DeleteGame { game_id: String, game_name: String },
}

#[derive(Debug, Clone)]
pub struct Modal {
    pub kind: ModalKind,
    /// Input field values in order: see ModalKind for semantics.
    pub fields: Vec<String>,
    /// Index of the focused field.
    pub focused: usize,
    /// Error message shown at the bottom of the modal.
    pub error: Option<String>,
}

impl Modal {
    fn new_add_quest(game_id: &str) -> Self {
        Self {
            kind: ModalKind::AddQuest,
            fields: vec![game_id.to_string(), String::new(), "daily".to_string()],
            focused: if game_id.is_empty() { 0 } else { 1 },
            error: None,
        }
    }

    fn new_edit_quest(name: &str, reset: &str) -> Self {
        Self {
            kind: ModalKind::EditQuest {
                original_name: name.to_string(),
            },
            fields: vec![name.to_string(), reset.to_string()],
            focused: 0,
            error: None,
        }
    }

    fn new_delete_quest(name: &str, game_id: &str) -> Self {
        Self {
            kind: ModalKind::DeleteQuest {
                name: name.to_string(),
                game_id: game_id.to_string(),
            },
            fields: vec![],
            focused: 0,
            error: None,
        }
    }

    fn new_add_game() -> Self {
        Self {
            kind: ModalKind::AddGame,
            fields: vec![String::new(), String::new()],
            focused: 0,
            error: None,
        }
    }

    fn new_delete_game(game_id: &str, game_name: &str) -> Self {
        Self {
            kind: ModalKind::DeleteGame {
                game_id: game_id.to_string(),
                game_name: game_name.to_string(),
            },
            fields: vec![],
            focused: 0,
            error: None,
        }
    }

    fn field_count(&self) -> usize {
        self.fields.len()
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub quests: Vec<Quest>,
    pub state: AppState,
    pub selected_tab: usize,
    pub selected_quest: usize,
    pub group_by_game: bool,
    pub sort_done_last: bool,
    pub show_help: bool,
    /// Sorted (game_id, display_name) pairs sourced from config — includes
    /// games that have no quests yet.
    pub games: Vec<(String, String)>,
    pub modal: Option<Modal>,
    /// Non-fatal status message shown at the bottom of the screen.
    pub status_msg: Option<String>,
}

fn games_from_config(config: &RawConfig) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = config
        .games
        .iter()
        .map(|(id, g)| (id.clone(), g.name.clone()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

impl App {
    pub fn new(mut quests: Vec<Quest>, state: AppState, config: &RawConfig) -> Self {
        sort_quests(&mut quests);
        Self {
            quests,
            state,
            selected_tab: 0,
            selected_quest: 0,
            group_by_game: false,
            sort_done_last: false,
            show_help: false,
            games: games_from_config(config),
            modal: None,
            status_msg: None,
        }
    }

    fn tab_count(&self) -> usize {
        1 + self.games.len()
    }

    pub fn visible_quests(&self) -> Vec<&Quest> {
        if self.selected_tab == 0 {
            self.quests.iter().collect()
        } else {
            let game_id = &self.games[self.selected_tab - 1].0;
            self.quests
                .iter()
                .filter(|q| &q.game_id == game_id)
                .collect()
        }
    }

    /// Game ID for the currently selected tab (None = "All" tab).
    fn current_game_id(&self) -> Option<&str> {
        if self.selected_tab == 0 {
            None
        } else {
            Some(&self.games[self.selected_tab - 1].0)
        }
    }

    fn selected_quest_info(&self) -> Option<(&str, &str)> {
        let visible = self.visible_quests();
        visible
            .get(self.selected_quest)
            .map(|q| (q.game_id.as_str(), q.name.as_str()))
    }

    fn mark_selected_complete(&mut self) {
        self.with_selected_quest(|state, game_id, quest_name| {
            state.mark_complete(game_id, quest_name, Utc::now());
        });
    }

    fn mark_selected_incomplete(&mut self) {
        self.with_selected_quest(|state, game_id, quest_name| {
            state.mark_incomplete(game_id, quest_name);
        });
    }

    fn with_selected_quest(&mut self, f: impl FnOnce(&mut AppState, &str, &str)) {
        let visible: Vec<usize> = if self.selected_tab == 0 {
            (0..self.quests.len()).collect()
        } else {
            let game_id = &self.games[self.selected_tab - 1].0;
            self.quests
                .iter()
                .enumerate()
                .filter(|(_, q)| &q.game_id == game_id)
                .map(|(i, _)| i)
                .collect()
        };
        if let Some(&idx) = visible.get(self.selected_quest) {
            let (game_id, name) = {
                let q = &self.quests[idx];
                (q.game_id.clone(), q.name.clone())
            };
            f(&mut self.state, &game_id, &name);
        }
    }

    /// Reload quests from config after a mutation.
    fn reload_quests(&mut self) {
        match load_config().and_then(|c| {
            let games = games_from_config(&c);
            build_quests(&c).map(|qs| (qs, games))
        }) {
            Ok((mut qs, games)) => {
                sort_quests(&mut qs);
                self.quests = qs;
                self.games = games;
                // Clamp selections.
                let tab_count = self.tab_count();
                if self.selected_tab >= tab_count {
                    self.selected_tab = tab_count.saturating_sub(1);
                }
                let visible_len = self.visible_quests().len();
                if self.selected_quest >= visible_len.max(1) {
                    self.selected_quest = visible_len.saturating_sub(1);
                }
            }
            Err(e) => {
                self.status_msg = Some(format!("Failed to reload config: {e}"));
            }
        }
    }

    // ── modal helpers ────────────────────────────────────────────────────────

    fn open_add_quest_modal(&mut self) {
        let game_id = self.current_game_id().unwrap_or("").to_string();
        self.modal = Some(Modal::new_add_quest(&game_id));
    }

    fn open_edit_quest_modal(&mut self) {
        if let Some((game_id, quest_name)) = self.selected_quest_info() {
            // Find the reset label for pre-filling.
            let reset = self
                .quests
                .iter()
                .find(|q| q.game_id == game_id && q.name == quest_name)
                .map(|q| q.reset_schedule_label())
                .unwrap_or_default();
            let quest_name = quest_name.to_string();
            self.modal = Some(Modal::new_edit_quest(&quest_name, &reset));
        }
    }

    fn open_delete_quest_modal(&mut self) {
        if let Some((game_id, quest_name)) = self.selected_quest_info() {
            let (game_id, quest_name) = (game_id.to_string(), quest_name.to_string());
            self.modal = Some(Modal::new_delete_quest(&quest_name, &game_id));
        }
    }

    fn open_add_game_modal(&mut self) {
        self.modal = Some(Modal::new_add_game());
    }

    fn open_delete_game_modal(&mut self) {
        if let Some(game_id) = self.current_game_id() {
            let game_name = self
                .games
                .iter()
                .find(|(id, _)| id == game_id)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| game_id.to_string());
            let game_id = game_id.to_string();
            self.modal = Some(Modal::new_delete_game(&game_id, &game_name));
        }
    }

    fn submit_modal(&mut self) {
        let modal = match self.modal.take() {
            Some(m) => m,
            None => return,
        };

        let result = match &modal.kind {
            ModalKind::AddQuest => {
                let game_id = modal.fields[0].trim().to_string();
                let name = modal.fields[1].trim().to_string();
                let reset = modal.fields[2].trim().to_string();
                if game_id.is_empty() {
                    self.modal = Some(Modal {
                        error: Some("Game ID cannot be empty.".into()),
                        ..modal
                    });
                    return;
                }
                if name.is_empty() {
                    self.modal = Some(Modal {
                        error: Some("Quest name cannot be empty.".into()),
                        ..modal
                    });
                    return;
                }
                config_edit::add_quest(&QuestSpec {
                    game_id,
                    name: name.clone(),
                    reset,
                })
                .map(|_| format!("Added quest '{name}'."))
            }
            ModalKind::EditQuest { original_name } => {
                let new_name = modal.fields[0].trim().to_string();
                let reset = modal.fields[1].trim().to_string();
                if new_name.is_empty() {
                    self.modal = Some(Modal {
                        error: Some("Quest name cannot be empty.".into()),
                        ..modal
                    });
                    return;
                }
                let (game_id, _) = self
                    .selected_quest_info()
                    .map(|(g, n)| (g.to_string(), n.to_string()))
                    .unwrap_or_default();
                let orig = original_name.clone();
                config_edit::update_quest(
                    &game_id,
                    &orig,
                    &QuestSpec {
                        game_id: game_id.clone(),
                        name: new_name.clone(),
                        reset,
                    },
                )
                .map(|_| format!("Updated quest '{new_name}'."))
            }
            ModalKind::DeleteQuest { name, game_id } => {
                let (name, game_id) = (name.clone(), game_id.clone());
                config_edit::remove_quest(&game_id, &name)
                    .map(|_| format!("Deleted quest '{name}'."))
            }
            ModalKind::AddGame => {
                let id = modal.fields[0].trim().to_string();
                let name = modal.fields[1].trim().to_string();
                if id.is_empty() || name.is_empty() {
                    self.modal = Some(Modal {
                        error: Some("Game ID and name cannot be empty.".into()),
                        ..modal
                    });
                    return;
                }
                config_edit::add_game(&GameSpec {
                    id: id.clone(),
                    name: name.clone(),
                    timezone: None,
                    reset_time: None,
                    reset_day: None,
                })
                .map(|_| format!("Added game '{id}' ({name})."))
            }
            ModalKind::DeleteGame { game_id, .. } => {
                let game_id = game_id.clone();
                config_edit::remove_game(&game_id).map(|_| format!("Deleted game '{game_id}'."))
            }
        };

        match result {
            Ok(msg) => {
                self.status_msg = Some(msg);
                self.reload_quests();
            }
            Err(e) => {
                self.modal = Some(Modal {
                    error: Some(e.to_string()),
                    ..modal
                });
            }
        }
    }
}

// ── event loop ────────────────────────────────────────────────────────────────

pub fn run(quests: Vec<Quest>, state: AppState, config: &RawConfig, tz: Tz) -> Result<AppState> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(quests, state, config);

    loop {
        terminal.draw(|f| {
            ui::draw(f, &app, tz);
        })?;

        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
        {
            // ── modal input ──────────────────────────────────────────────
            if let Some(ref mut modal) = app.modal {
                match key.code {
                    KeyCode::Esc => {
                        app.modal = None;
                    }
                    KeyCode::Enter => {
                        // On confirmation modals (no fields) Enter confirms.
                        // On form modals, Tab moves to next field; Enter on last field submits.
                        if modal.field_count() == 0 {
                            app.submit_modal();
                        } else if modal.focused + 1 < modal.field_count() {
                            modal.focused += 1;
                        } else {
                            app.submit_modal();
                        }
                    }
                    KeyCode::Tab if modal.field_count() > 0 => {
                        modal.focused = (modal.focused + 1) % modal.field_count();
                    }
                    KeyCode::BackTab if modal.field_count() > 0 => {
                        let n = modal.field_count();
                        modal.focused = (modal.focused + n - 1) % n;
                    }
                    KeyCode::Backspace => {
                        if let Some(field) = modal.fields.get_mut(modal.focused) {
                            field.pop();
                            modal.error = None;
                        }
                    }
                    KeyCode::Char(c) => {
                        // Ctrl-U clears the current field.
                        if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'u' {
                            if let Some(field) = modal.fields.get_mut(modal.focused) {
                                field.clear();
                                modal.error = None;
                            }
                        } else if !key.modifiers.contains(KeyModifiers::CONTROL)
                            && !key.modifiers.contains(KeyModifiers::ALT)
                            && let Some(field) = modal.fields.get_mut(modal.focused)
                        {
                            field.push(c);
                            modal.error = None;
                        }
                    }
                    _ => {}
                }
                continue;
            }

            // ── normal input ─────────────────────────────────────────────
            if app.show_help {
                app.show_help = false;
                continue;
            }

            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('?') => app.show_help = true,
                KeyCode::Char('s') => app.sort_done_last = !app.sort_done_last,
                KeyCode::Char('g') if app.selected_tab == 0 => {
                    app.group_by_game = !app.group_by_game;
                }
                KeyCode::Tab => {
                    app.selected_tab = (app.selected_tab + 1) % app.tab_count();
                    app.selected_quest = 0;
                }
                KeyCode::BackTab => {
                    let n = app.tab_count();
                    app.selected_tab = (app.selected_tab + n - 1) % n;
                    app.selected_quest = 0;
                }
                KeyCode::Up | KeyCode::Char('k') if app.selected_quest > 0 => {
                    app.selected_quest -= 1;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = app.visible_quests().len();
                    if len > 0 && app.selected_quest < len - 1 {
                        app.selected_quest += 1;
                    }
                }
                KeyCode::Char(' ') | KeyCode::Enter => {
                    app.mark_selected_complete();
                    app.status_msg = None;
                }
                KeyCode::Char('u') => {
                    app.mark_selected_incomplete();
                    app.status_msg = None;
                }
                // ── CRUD ─────────────────────────────────────────────────
                KeyCode::Char('a') => {
                    app.open_add_quest_modal();
                }
                KeyCode::Char('e') => {
                    app.open_edit_quest_modal();
                }
                KeyCode::Char('d') => {
                    app.open_delete_quest_modal();
                }
                KeyCode::Char('A') => {
                    app.open_add_game_modal();
                }
                KeyCode::Char('D') => {
                    app.open_delete_game_modal();
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(app.state)
}
