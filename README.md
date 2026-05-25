# questlog

A CLI/TUI app for tracking recurring tasks (dailies, weeklies, custom intervals) across multiple games.

## Installation

```
cargo install --path .
```

## Usage

```
questlog              # launch TUI
questlog list         # print status of all quests
questlog edit         # open config in $EDITOR
questlog done         # mark a quest complete
```

### `list`

```
questlog list [--game <id|name>] [--sort-done-last]
```

Prints a table of all quests with their status, reset schedule, and next reset time. `--game` is case-insensitive and matches either the game ID or display name.

### `done`

```
questlog done <name> --game <id>
```

Marks a quest complete from the command line.

## TUI

### Keybindings

| Key | Action |
|---|---|
| `space` / `enter` | Mark quest complete |
| `u` | Mark quest incomplete |
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Tab` | Next tab |
| `Shift-Tab` | Previous tab |
| `g` | Toggle group-by-game (overview) |
| `s` | Toggle sort done to end |
| `?` | Show keybinding help |
| `q` | Quit |

## Configuration

Config file: `~/.config/questlog/config.toml`

Created automatically with commented documentation on first run. Open it with `questlog edit`.

```toml
timezone = "Europe/Berlin"

[defaults]
reset_time = "00:00"
reset_day  = "monday"

[games.ffxiv]
name = "Final Fantasy XIV"
timezone = "Europe/London"
reset_time = "16:00"
reset_day  = "tuesday"

[[games.ffxiv.quests]]
name = "Daily roulettes"
reset = "daily"

[[games.ffxiv.quests]]
name = "Weekly raid"
reset = "weekly"

[[games.ffxiv.quests]]
name = "Chaos recipe"
reset = { type = "interval", hours = 24 }

[[games.ffxiv.quests]]
name = "Timed node"
reset = { type = "schedule", hours = 12, anchor = "2026-01-01T08:00:00+01:00" }
```

### Reset types

| Type | Description |
|---|---|
| `"daily"` | Resets daily at `reset_time` |
| `"weekly"` | Resets weekly on `reset_day` at `reset_time` |
| `{ type = "daily", time = "08:00" }` | Daily at a specific time |
| `{ type = "weekly", day = "tuesday", time = "16:00" }` | Weekly on a specific day/time |
| `{ type = "interval", hours = 4 }` | Resets N duration after last completion |
| `{ type = "schedule", minutes = 15 }` | Fixed repeating clock period, epoch-aligned |
| `{ type = "schedule", hours = 12, anchor = "..." }` | Fixed period with custom alignment |

`interval` fields are additive — `hours = 1, minutes = 30` means 90 minutes. Accepts `minutes`, `hours`, `days`, `weeks`.

Multiple rules can be combined as an array — the quest is available when any rule fires:

```toml
reset = [
  { type = "daily", time = "08:00" },
  { type = "daily", time = "20:00" },
]
```

### Default resolution

For `time`, `day`, and `timezone`, the value is resolved in this order:

1. Quest-level field
2. Game-level default
3. Global `[defaults]`
4. Hardcoded fallback: `00:00`, `monday`, `UTC`

## State

State file: `~/.local/share/questlog/state.json`

Tracks the last completion time per quest, keyed by `<game_id>.<quest_name>`. Deleting a key resets that quest to never-completed.
