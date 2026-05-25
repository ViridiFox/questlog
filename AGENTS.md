# AGENTS.md

## Commands

```bash
cargo build          # compile
cargo run            # launch TUI
cargo run -- list    # print quest status table
cargo test           # run tests
cargo clippy         # lint
cargo install --path .  # install binary
```

No mise tasks configured — use `cargo` directly.

## Architecture

Single-binary Rust CLI/TUI:

```
src/
  main.rs        # clap dispatch → tui::run or CLI commands
  config.rs      # deserialize ~/.config/questlog/config.toml, resolve defaults
  config_edit.rs # structure-preserving config mutations via toml_edit (read→patch→atomic rename)
  state.rs       # load/save ~/.local/share/questlog/state.json
  quest.rs       # ResetRule variants, availability logic, reset_schedule_label()
  tui/
    mod.rs       # App struct, Modal state machine, ratatui event loop
    ui.rs        # draw(f, &app, tz) — tabs, quest table, modal overlays
```

## Key Domain Facts

- **Config**: `~/.config/questlog/config.toml` — created with commented docs on first run
- **State**: `~/.local/share/questlog/state.json` — keyed by `<game_id>.<quest_name>`
- **Availability**: `now >= min(next_reset_time across all rules)` — multi-rule quests are available when any rule fires
- **Reset types**: `daily`, `weekly`, `interval` (completion-anchored), `schedule` (clock-anchored)
- **Default resolution**: quest-level → game-level → `[defaults]` → hardcoded (`00:00`, `monday`, `UTC`)
- `interval` duration fields are additive (`hours = 1, minutes = 30` = 90 min)

## Config Editing (`config_edit.rs`)

All mutations use `toml_edit` to preserve comments and formatting. Writes are atomic (temp file + rename).

- `add_game` / `update_game` / `remove_game` — manage `[games.<id>]` blocks
- `add_quest` / `update_quest` / `remove_quest` — handle both TOML representations:
  - `[[games.<id>.quests]]` — array of tables (hand-written configs)
  - `quests = [...]` — inline array (written by `add_game`)
- `update_game`: omitting optional fields (`timezone`, `reset_time`, `reset_day`) **removes** them from config
- Reset spec for quests: bare string `"daily"` / `"weekly"`, or inline TOML table e.g. `{ type = "interval", hours = 4 }`

## TUI

- `App::new` takes `&RawConfig` — `games` list comes from config, not just quests (includes empty games)
- After any config mutation, call `app.reload_quests()` to re-parse config and re-sort; it clamps selections
- `tui::run` signature: `run(quests, state, &config, tz)` — config passed in for `App::new`
- `ui::draw` signature: `draw(f, &app, tz)` — single `&App` rather than individual fields
- Modal flow: `App::modal: Option<Modal>` — set via `open_*_modal()` helpers, submitted via `submit_modal()`
- Status messages (`app.status_msg`) add a 1-line footer row via a conditional layout constraint

## TUI Keybindings (for reference when editing ui.rs)

| Key | Action |
|-----|--------|
| `a` | Add quest (game tab only) |
| `e` | Edit selected quest |
| `d` | Delete selected quest |
| `A` | Add game |
| `D` | Delete current game (game tab only) |
| `g` | Toggle group-by-game |
| `s` | Toggle sort done to end |
| `?` | Help overlay |

Modal input: `Tab`/`Shift-Tab` cycle fields, `Enter` on last field submits, `Esc` cancels, `Ctrl-U` clears field.

## Dependencies

- TUI: `ratatui` + `crossterm`
- Date/time: `chrono` + `chrono-tz` (IANA timezone support via `iana-time-zone`)
- CLI: `clap` with derive feature
- Config: `toml` + `serde`; `toml_edit` for structure-preserving mutations; State: `serde_json`
