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
| `docs` | Prints `See agents.md (directive) and docs/ (see docs/README.md)`. |

## Global flags (before or after the command)

| Flag | Default | Effect |
|---|---|---|
| `--seed <u64>` | `0xA1C_0A_6E_17_0` | Reproducible world seed; ignored when `--load` is used. |
| `--speed <u32>` | `60` | TUI ticks/second; must be in `1..=4000` or the CLI exits with an error. Ignored by headless `run`. |
| `--no-tui` | off | `observe` becomes a headless 30-day report (CI-friendly). |
| `--load <DIR>` | none | Resume from a save directory; overrides the seed. |
| `--save-dir <DIR>` | `saves/default` | Save/output directory. |
| `--autosave-days <N>` | `7` | Autosave cadence in simulated days; `0` disables. |

## Headless report (example, abbreviated)

```
=== Alcagent headless run ===
Loaded from:    new world (seed 0xa1c0a6e170)
Save dir:       saves/default
Real elapsed:   1.23s
Ticks:          12345
Requested days: 365
Progressed days: 365
<sim.report() output>
Recent events (last 10): …
```

## TUI essentials

- **Views:** `1`–`9` = Overview, Prisoner, Gang, Staff, Warden, Financial, Event Timeline,
  Social Network, Prison Map; `0` = Incident Reports; `A` = Decision Audit.
- **Playback/navigation:** `Space` pause/resume, `+`/`-` speed, arrows/`hjkl` cursor,
  `q`/`Q`/`Esc`/`Ctrl-C` quit (`Esc` closes modal help/search first).
- **Search/help:** `/` free-text search (`Ctrl-W` clears), `?` modal help (`Esc`/`?` close),
  `Tab` pin, `n` notices, `V` auto-pause on critical incidents, `Ctrl-R` resolve critical incident.
- **Saves/policy:** `S` save, `Ctrl-L` load, `b`/`B` intake toggle, `p` policy mode,
  `N`/`E`/`K` policy band, `L` lock / `U` unlock outside Warden, `x`/`X` lockdown toggle outside
  Warden, `[ ]` security, `, .` privileges, `; '` rehab funding.
- **Construction (Warden):** `c` kind, `C` queue, `u`/`U` project cycle, `v` approve,
  `x`/`X` cancel/fail, `>` advance one day.
- **Gameplay:** Prisoner `H`/`G`/`J`/`Y` parole hearing/grant/deny/release; `t` rehab program
  cycle, `r`/`R` enroll/drop, `f`/`F` contraband case cycle/resolve, `m`/`o` incident filters,
  `d`/`D` decision validation filter.
- Full key-by-key effects are in [`ALCAGENT_GAMEPLAY.md`](ALCAGENT_GAMEPLAY.md#22-tui-keybindings-complete).

## Troubleshooting

| Symptom | Fix |
|---|---|
| Garbled TUI / `^[[A` arrows | Use Windows Terminal or a modern console; CI should use `--no-tui`. |
| Wrong binary | `cargo run -p alcagent -- <args>`. |
| "No save found" | Point `--load` at a directory containing `world.bin`. |
| Same seed, different result | A loaded save overrides the CLI seed. |
| Old save won't load | Current save format is v5; regenerate or migrate older saves. |
