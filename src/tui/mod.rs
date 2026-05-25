pub mod ui;

use crate::quest::{Quest, sort_quests};
use crate::state::AppState;
use anyhow::Result;
use chrono::Utc;
use chrono_tz::Tz;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::Duration;

pub struct App {
    pub quests: Vec<Quest>,
    pub state: AppState,
    pub selected_tab: usize,
    pub selected_quest: usize,
    pub group_by_game: bool,
    pub sort_done_last: bool,
    pub show_help: bool,
    pub game_ids: Vec<String>,
}

impl App {
    pub fn new(mut quests: Vec<Quest>, state: AppState) -> Self {
        sort_quests(&mut quests);
        let mut game_ids: Vec<String> = quests.iter().map(|q| q.game_id.clone()).collect();
        game_ids.sort();
        game_ids.dedup();
        Self {
            quests,
            state,
            selected_tab: 0,
            selected_quest: 0,
            group_by_game: false,
            sort_done_last: false,
            show_help: false,
            game_ids,
        }
    }

    fn tab_count(&self) -> usize {
        1 + self.game_ids.len()
    }

    fn visible_quests(&self) -> Vec<&Quest> {
        if self.selected_tab == 0 {
            self.quests.iter().collect()
        } else {
            let game_id = &self.game_ids[self.selected_tab - 1];
            self.quests.iter().filter(|q| &q.game_id == game_id).collect()
        }
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
            let game_id = &self.game_ids[self.selected_tab - 1];
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
}

pub fn run(quests: Vec<Quest>, state: AppState, tz: Tz) -> Result<AppState> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(quests, state);

    loop {
        terminal.draw(|f| {
            ui::draw(
                f,
                &app.quests,
                &app.state,
                app.selected_tab,
                app.selected_quest,
                app.group_by_game,
                app.sort_done_last,
                app.show_help,
                tz,
            );
        })?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if app.show_help {
                    // any key closes help
                    app.show_help = false;
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('?') => app.show_help = true,
                    KeyCode::Char('s') => app.sort_done_last = !app.sort_done_last,
                    KeyCode::Char('g') => {
                        if app.selected_tab == 0 {
                            app.group_by_game = !app.group_by_game;
                        }
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
                    KeyCode::Up | KeyCode::Char('k') => {
                        if app.selected_quest > 0 {
                            app.selected_quest -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let len = app.visible_quests().len();
                        if len > 0 && app.selected_quest < len - 1 {
                            app.selected_quest += 1;
                        }
                    }
                    KeyCode::Char(' ') | KeyCode::Enter => {
                        app.mark_selected_complete();
                    }
                    KeyCode::Char('u') => {
                        app.mark_selected_incomplete();
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(app.state)
}
