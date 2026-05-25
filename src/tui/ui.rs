use crate::quest::Quest;
use crate::state::AppState;
use crate::tui::{App, Modal, ModalKind};
use chrono::Utc;
use chrono_tz::Tz;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, Padding, Paragraph, Row, Table, TableState, Tabs,
        Wrap,
    },
};

pub fn draw(f: &mut Frame, app: &App, tz: Tz) {
    let quests = &app.quests;
    let state = &app.state;

    let tab_titles: Vec<Line> = std::iter::once(Line::from("Overview"))
        .chain(app.games.iter().map(|(_, name)| Line::from(name.clone())))
        .collect();

    let has_status = app.status_msg.is_some();
    let main_constraints = if has_status {
        vec![
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Length(3), Constraint::Min(0)]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(main_constraints)
        .split(f.area());

    let tabs = Tabs::new(tab_titles)
        .select(app.selected_tab)
        .block(Block::default().borders(Borders::ALL).title("questlog"))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, chunks[0]);

    let visible_quests: Vec<&Quest> = if app.selected_tab == 0 {
        quests.iter().collect()
    } else {
        let game_id = &app.games[app.selected_tab - 1].0;
        quests.iter().filter(|q| &q.game_id == game_id).collect()
    };

    let now = Utc::now();

    if app.selected_tab == 0 && app.group_by_game {
        draw_grouped(
            f,
            chunks[1],
            &visible_quests,
            state,
            now,
            app.sort_done_last,
            tz,
        );
    } else {
        draw_list(
            f,
            chunks[1],
            &visible_quests,
            state,
            now,
            app.selected_quest,
            None,
            app.sort_done_last,
            tz,
        );
    }

    if has_status {
        let msg = app.status_msg.as_deref().unwrap_or("");
        let p = Paragraph::new(msg).style(Style::default().fg(Color::Cyan));
        f.render_widget(p, chunks[2]);
    }

    if app.show_help {
        draw_help(f);
    }

    if let Some(ref modal) = app.modal {
        draw_modal(f, modal);
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
        draw_list(
            f,
            chunks[i],
            &game_quests,
            state,
            now,
            0,
            Some(title),
            sort_done_last,
            tz,
        );
    }
}

#[allow(clippy::too_many_arguments)]
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
            let meta = if dim {
                base.add_modifier(Modifier::DIM)
            } else {
                base
            };
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
        [
            Constraint::Min(20),
            Constraint::Length(w_schedule as u16),
            Constraint::Length(20),
        ],
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
        ("u", "Mark quest incomplete"),
        ("j / ↓", "Move down"),
        ("k / ↑", "Move up"),
        ("Tab", "Next tab"),
        ("Shift-Tab", "Previous tab"),
        ("g", "Toggle group-by-game (overview)"),
        ("s", "Toggle sort done to end"),
        ("a", "Add quest (on game tab)"),
        ("e", "Edit selected quest"),
        ("d", "Delete selected quest"),
        ("A", "Add game"),
        ("D", "Delete current game (on game tab)"),
        ("?", "Toggle this help"),
        ("q", "Quit"),
    ];

    let key_col = BINDS.iter().map(|(k, _)| k.len()).max().unwrap_or(0) as u16;
    let inner_width = key_col + 2 + 35;
    let popup_width = inner_width + 4;
    let popup_height = BINDS.len() as u16 + 4;

    let area = centered_rect(popup_width, popup_height, f.area());

    let lines: Vec<Line> = BINDS
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(
                    format!("{:>width$}", key, width = key_col as usize),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::raw(*desc),
            ])
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Keybindings ")
        .title_alignment(Alignment::Center)
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(Color::Reset));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .style(Style::default().fg(Color::White).bg(Color::Reset));

    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}

// ── modal rendering ───────────────────────────────────────────────────────────

fn draw_modal(f: &mut Frame, modal: &Modal) {
    match &modal.kind {
        ModalKind::DeleteQuest { name, .. } => {
            draw_confirm_modal(
                f,
                " Delete Quest ",
                &format!("Delete quest '{name}'?"),
                modal,
            );
        }
        ModalKind::DeleteGame { game_name, .. } => {
            draw_confirm_modal(
                f,
                " Delete Game ",
                &format!("Delete game '{game_name}' and ALL its quests?"),
                modal,
            );
        }
        ModalKind::AddQuest | ModalKind::EditQuest { .. } => {
            let (title, labels): (&str, &[(&str, usize)]) =
                if matches!(modal.kind, ModalKind::AddQuest) {
                    (" Add Quest ", &[("Game ID", 0), ("Name", 1), ("Reset", 2)])
                } else {
                    (" Edit Quest ", &[("Name", 0), ("Reset", 1)])
                };
            draw_form_modal(f, title, labels, modal);
        }
        ModalKind::AddGame => {
            draw_form_modal(
                f,
                " Add Game ",
                &[("Game ID", 0), ("Display Name", 1)],
                modal,
            );
        }
    }
}

fn draw_confirm_modal(f: &mut Frame, title: &str, message: &str, modal: &Modal) {
    const POPUP_BG: Color = Color::Reset;
    const WIDTH: u16 = 62;

    let has_err = modal.error.is_some();
    let height = 2 /* borders */ + 1 /* blank */ + 2 /* message */ + 1 /* blank */ + 1 /* hint */
        + if has_err { 2 } else { 0 };
    let area = centered_rect(WIDTH, height, f.area());

    f.render_widget(Clear, area);

    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(message, Style::default().fg(Color::White))),
        Line::from(""),
        Line::from(Span::styled(
            "Enter  confirm    Esc  cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    if let Some(ref err) = modal.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            err.as_str(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center)
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(POPUP_BG));

    let p = Paragraph::new(lines)
        .block(block)
        .style(Style::default().fg(Color::White).bg(POPUP_BG))
        .wrap(Wrap { trim: false });

    f.render_widget(p, area);
}

fn draw_form_modal(f: &mut Frame, title: &str, labels: &[(&str, usize)], modal: &Modal) {
    const POPUP_BG: Color = Color::Reset;
    const INACTIVE_BG: Color = Color::Black;
    const WIDTH: u16 = 64;

    // Height: border(2) + per-field(label 1 + input 1 + gap 1 = 3) + hint(1) + bottom padding(1)
    // + optional error(2)
    let has_err = modal.error.is_some();
    let height = 2 + (labels.len() as u16) * 3 + 1 + 1 + if has_err { 2 } else { 0 };
    let area = centered_rect(WIDTH, height, f.area());

    f.render_widget(Clear, area);

    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center)
        .style(Style::default().bg(POPUP_BG));
    f.render_widget(outer_block, area);

    // Work inside the border, with 1-cell horizontal padding.
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });

    // Build row constraints: label + input + gap for each field, then hint, then optional error.
    let mut row_constraints: Vec<Constraint> = labels
        .iter()
        .flat_map(|_| {
            [
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
        })
        .collect();
    row_constraints.push(Constraint::Length(1)); // hint
    if has_err {
        row_constraints.push(Constraint::Length(1)); // blank
        row_constraints.push(Constraint::Length(1)); // error text
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(inner);

    for (i, (label, field_idx)) in labels.iter().enumerate() {
        let base = i * 3;
        let focused = modal.focused == *field_idx;

        // ── label ───────────────────────────────────────────────────────────
        let label_style = if focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        f.render_widget(Paragraph::new(*label).style(label_style), rows[base]);

        // ── input ────────────────────────────────────────────────────────────
        let value = modal
            .fields
            .get(*field_idx)
            .map(|s| s.as_str())
            .unwrap_or("");

        let box_rect = rows[base + 1];

        // Fill the row background first so the box has a solid colour.
        f.render_widget(
            Block::default().style(Style::default().bg(INACTIVE_BG)),
            box_rect,
        );

        let content: Line = if focused {
            Line::from(vec![Span::styled(
                format!(" {value} "),
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .add_modifier(Modifier::BOLD),
            )])
        } else {
            Line::from(Span::styled(
                format!(" {value}"),
                Style::default().fg(Color::Gray).bg(INACTIVE_BG),
            ))
        };
        f.render_widget(Paragraph::new(content), box_rect);
    }

    // ── hint row ─────────────────────────────────────────────────────────────
    let hint_idx = labels.len() * 3;
    let hint = Line::from(vec![
        Span::styled(
            "Tab",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" next field  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" submit  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "Esc",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(hint), rows[hint_idx]);

    // ── error row ────────────────────────────────────────────────────────────
    if let Some(ref err) = modal.error {
        let err_idx = hint_idx + 2;
        if err_idx < rows.len() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    err.as_str(),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                rows[err_idx],
            );
        }
    }
}
