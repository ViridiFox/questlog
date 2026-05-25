use crate::quest::Quest;
use crate::state::AppState;
use chrono::Utc;
use chrono_tz::Tz;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Tabs,
    },
    Frame,
};

pub fn draw(
    f: &mut Frame,
    quests: &[Quest],
    state: &AppState,
    selected_tab: usize,
    selected_quest: usize,
    group_by_game: bool,
    sort_done_last: bool,
    show_help: bool,
    tz: Tz,
) {
    // Collect (game_id, game_name) pairs in stable sorted order by game_id.
    let game_ids: Vec<&str> = {
        let mut ids: Vec<&str> = quests.iter().map(|q| q.game_id.as_str()).collect();
        ids.sort();
        ids.dedup();
        ids
    };

    let tab_titles: Vec<Line> = std::iter::once(Line::from("Overview"))
        .chain(game_ids.iter().map(|id| {
            let display = quests
                .iter()
                .find(|q| q.game_id == *id)
                .map(|q| q.game_name.as_str())
                .unwrap_or(id);
            Line::from(display.to_owned())
        }))
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(f.area());

    let tabs = Tabs::new(tab_titles)
        .select(selected_tab)
        .block(Block::default().borders(Borders::ALL).title("questlog"))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, chunks[0]);

    let visible_quests: Vec<&Quest> = if selected_tab == 0 {
        quests.iter().collect()
    } else {
        let game_id = game_ids[selected_tab - 1];
        quests.iter().filter(|q| q.game_id == game_id).collect()
    };

    let now = Utc::now();

    if selected_tab == 0 && group_by_game {
        draw_grouped(f, chunks[1], &visible_quests, state, now, sort_done_last, tz);
    } else {
        draw_list(f, chunks[1], &visible_quests, state, now, selected_quest, None, sort_done_last, tz);
    }

    if show_help {
        draw_help(f);
    }
}

fn draw_grouped(
    f: &mut Frame,
    area: Rect,
    quests: &[&Quest],
    state: &AppState,
    now: chrono::DateTime<Utc>,
    sort_done_last: bool,
    tz: Tz,
) {
    let mut game_ids: Vec<&str> = quests.iter().map(|q| q.game_id.as_str()).collect();
    game_ids.sort();
    game_ids.dedup();

    if game_ids.is_empty() {
        return;
    }

    let constraints: Vec<Constraint> = game_ids.iter().map(|_| Constraint::Min(3)).collect();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (i, game_id) in game_ids.iter().enumerate() {
        let game_quests: Vec<&Quest> = quests
            .iter()
            .copied()
            .filter(|q| q.game_id == *game_id)
            .collect();
        let title = game_quests
            .first()
            .map(|q| q.game_name.as_str())
            .unwrap_or(game_id);
        draw_list(f, chunks[i], &game_quests, state, now, 0, Some(title), sort_done_last, tz);
    }
}

fn draw_list(
    f: &mut Frame,
    area: Rect,
    quests: &[&Quest],
    state: &AppState,
    now: chrono::DateTime<Utc>,
    selected: usize,
    title: Option<&str>,
    sort_done_last: bool,
    tz: Tz,
) {
    // Optionally reorder: available quests first, done quests last (stable).
    let ordered: Vec<&Quest> = if sort_done_last {
        let mut available: Vec<&Quest> = Vec::new();
        let mut done: Vec<&Quest> = Vec::new();
        for q in quests {
            if q.is_available(state.last_completed(&q.game_id, &q.name), now) {
                available.push(q);
            } else {
                done.push(q);
            }
        }
        available.into_iter().chain(done).collect()
    } else {
        quests.to_vec()
    };
    let rows: Vec<(bool, [String; 3])> = ordered
        .iter()
        .map(|q| {
            let last = state.last_completed(&q.game_id, &q.name);
            let available = q.is_available(last, now);
            let schedule = q.reset_schedule_label();
            (
                available,
                [
                    format!("{} {}", if available { "[ ]" } else { "[x]" }, q.name),
                    schedule,
                    q.format_next_available(last, now, tz),
                ],
            )
        })
        .collect();

    // Measure schedule column width for consistent alignment.
    let w_schedule = rows.iter().map(|(_, r)| r[1].len()).max().unwrap_or(8);

    let table_rows: Vec<Row> = rows
        .iter()
        .map(|(available, cols)| {
            let (fg, dim) = if *available {
                (Color::Green, false)
            } else {
                (Color::DarkGray, true)
            };
            let base = Style::default().fg(fg);
            let meta = if dim { base.add_modifier(Modifier::DIM) } else { base };
            Row::new(vec![
                Cell::from(cols[0].as_str()).style(base),
                Cell::from(format!("{:>width$}", cols[1], width = w_schedule)).style(meta),
                Cell::from(cols[2].as_str()).style(meta),
            ])
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title.unwrap_or("Quests"));

    let table = Table::new(
        table_rows,
        [Constraint::Min(20), Constraint::Length(w_schedule as u16), Constraint::Length(20)],
    )
    .block(block)
    .column_spacing(2)
    .row_highlight_style(
        Style::default()
            .add_modifier(Modifier::REVERSED)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("> ");

    let mut table_state = TableState::default();
    if !ordered.is_empty() {
        table_state.select(Some(selected.min(ordered.len() - 1)));
    }

    f.render_stateful_widget(table, area, &mut table_state);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

fn draw_help(f: &mut Frame) {
    const BINDS: &[(&str, &str)] = &[
        ("space / enter", "Mark quest complete"),
        ("u",             "Mark quest incomplete"),
        ("j / ↓",         "Move down"),
        ("k / ↑",         "Move up"),
        ("Tab",           "Next tab"),
        ("Shift-Tab",     "Previous tab"),
        ("g",             "Toggle group-by-game (overview)"),
        ("s",             "Toggle sort done to end"),
        ("?",             "Toggle this help"),
        ("q",             "Quit"),
    ];

    let key_col = BINDS.iter().map(|(k, _)| k.len()).max().unwrap_or(0) as u16;
    let inner_width = key_col + 2 + 30; // key + "  " + description
    let popup_width = inner_width + 4;   // borders + padding
    let popup_height = BINDS.len() as u16 + 4; // rows + title + borders + padding

    let area = centered_rect(popup_width, popup_height, f.area());

    let lines: Vec<Line> = BINDS
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(
                    format!("{:>width$}", key, width = key_col as usize),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::raw(*desc),
            ])
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Keybindings ")
        .title_alignment(Alignment::Center)
        .style(Style::default().bg(Color::DarkGray));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray));

    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}
