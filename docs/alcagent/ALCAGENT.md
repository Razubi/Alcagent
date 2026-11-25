# `alcagent` CLI & TUI Reference

`alcagent` is the operator binary for Project Alcagent: it launches either the interactive
terminal observer or a headless simulation run, using the [`sumo`](../sumo/SUMO_ENGINE.md) engine for
every world/game function. Engine docs are maintained under `docs/sumo/` in this repository.

> The **complete** gameplay, systems and controls manual is
> [`ALCAGENT_GAMEPLAY.md`](ALCAGENT_GAMEPLAY.md). This file is the concise CLI quick reference.

---

## Build and run

```powershell
cargo build --workspace
cargo run --                  # interactive TUI (default)
cargo run -- observe          # same, explicitly
cargo run -- observe --no-tui # 30-day headless report instead of TUI
cargo run -- run 365          # headless 365-day run (run with no number = 30 days)
cargo run -- --help           # full clap help
cargo run -- docs             # documentation pointer
```

Executable: `target\debug\alcagent.exe` (Windows). For long runs use
`cargo run --release -- run 3650`. Logging: `$env:RUST_LOG='sumo=debug'`.

## Commands

| Command | Effect |
|---|---|
| *(none)* / `observe` | Interactive ratatui TUI; `--no-tui` falls back to a 30-day headless report. |
| `run [DAYS]` | Headless run at extreme speed (default **30** days), prints a report, always saves. |
| `docs` | Prints `See agents.md (directive) and docs/ (index, design, systems, reports)`. |

## Global flags (before or after the command)

| Flag | Default | Effect |
|---|---|---|
| `--seed <u64>` | `0xA1C_0A_6E_17_0` | Reproducible world seed; ignored when `--load` is used. |
| `--speed <u32>` | `60` | TUI ticks/second (`0` is an error; clamped 1–4000). Ignored by headless `run`. |
| `--no-tui` | off | `observe` becomes a headless 30-day report (CI-friendly). |
| `--load <DIR>` | none | Resume from a save directory; overrides the seed. |
| `--save-dir <DIR>` | `saves/default` | Save/output directory. |
| `--autosave-days <N>` | `7` | Autosave cadence in simulated days; `0` disables. |

## Headless report (example, abbreviated)

```
=== Alcagent headless run ===
Seed:            0xa1c0a6e170
Sim time:        Day 0005 00:00
Days simulated:  5
Population:      25 / 80
Cash (major):    253378.00
Ticks/sec:       104        (actually days/sec — legacy label)
Recent events (last 10): …
```

## TUI essentials

- **Views:** `1`–`9` = Overview, Prisoner, Gang, Staff, Warden, Financial, Event Timeline,
  Social Network, Prison Map; `0`/`i` = Incident Reports; `a`/`A` = Decision Audit.
- **Playback:** `Space` pause, `+`/`-` speed, arrows/`hjkl` cursor, `q` quit.
- **Policy:** `p` mode, `N`/`E`/`K` band, `L`/`U` or `x` lockdown, `[ ]` security, `, .`
  privileges, `; '` rehab funding, `b` intake pause.
- **Gameplay:** `t` program, `r`/`R` enroll/drop, `f`/`F` contraband case, `m`/`o`/`d` filters.
- **Saves:** `s` save, `Ctrl-L` load, `--autosave-days` cadence.
- Full key-by-key effects are in [`ALCAGENT_GAMEPLAY.md`](ALCAGENT_GAMEPLAY.md#2-tui-keybindings-complete).

## Troubleshooting

| Symptom | Fix |
|---|---|
| Garbled TUI / `^[[A` arrows | Use Windows Terminal or a modern console; CI should use `--no-tui`. |
| Wrong binary | `cargo run -p alcagent -- <args>`. |
| "No save found" | Point `--load` at a directory containing `world.bin`. |
| Same seed, different result | A loaded save overrides the CLI seed. |
| Old save won't load | Save format is v3; regenerate or migrate older saves. |
