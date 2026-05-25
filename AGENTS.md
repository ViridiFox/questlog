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
  main.rs     # clap dispatch → tui::run or CLI commands
  config.rs   # deserialize ~/.config/questlog/config.toml, resolve defaults
  state.rs    # load/save ~/.local/share/questlog/state.json
  quest.rs    # ResetRule variants, availability logic
  tui/
    mod.rs    # ratatui + crossterm app loop, event handling
    ui.rs     # tab bar + quest list layout
```

## Key Domain Facts

- **Config**: `~/.config/questlog/config.toml` — created with commented docs on first run
- **State**: `~/.local/share/questlog/state.json` — keyed by `<game_id>.<quest_name>`
- **Availability**: `now >= min(next_reset_time across all rules)` — multi-rule quests are available when any rule fires
- **Reset types**: `daily`, `weekly`, `interval` (completion-anchored), `schedule` (clock-anchored)
- **Default resolution**: quest-level → game-level → `[defaults]` → hardcoded (`00:00`, `monday`, `UTC`)
- `interval` duration fields are additive (`hours = 1, minutes = 30` = 90 min)

## Dependencies

- TUI: `ratatui` + `crossterm`
- Date/time: `chrono` + `chrono-tz` (IANA timezone support via `iana-time-zone`)
- CLI: `clap` with derive feature
- Config: `toml` + `serde`; State: `serde_json`
