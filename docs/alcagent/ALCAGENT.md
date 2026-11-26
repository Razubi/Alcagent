# `alcagent` Launch & Observer Reference

`alcagent` is the **only binary** in the workspace (`cargo run` selects it via `default-members`).
It launches the graphical egui/eframe observer by default, the ratatui debug observer with `--tui`,
or a headless run, using the [`sumo`](../sumo/SUMO_ENGINE.md) engine for every world/game function.
Engine docs are maintained under `docs/sumo/` in this repository.

> The **complete** gameplay and systems manual is
> [`ALCAGENT_GAMEPLAY.md`](ALCAGENT_GAMEPLAY.md). This file is the concise launch/CLI reference.

---

## Build and run

```powershell
cargo build --workspace
cargo run --                  # graphical observer at the Main Menu (default)
cargo run -- observe          # same, explicitly
cargo run -- --tui            # ratatui debug observer
cargo run -- run 365          # headless 365-day run (run with no number = 30 days)
cargo run -- --help           # full clap help
cargo run -- docs             # documentation pointer
```

Executable: `target\debug\alcagent.exe` (Windows). There is no separate GUI binary (`alc-gui` is a
library adapter). For long runs use `cargo run --release -- run 3650`. Logging:
`$env:RUST_LOG='sumo=debug'`.

## Commands

| Command / flag | Effect |
|---|---|
| *(none)* / `observe` | Graphical egui/eframe observer at the **Main Menu**: seed field + **Start** (new world), Load Existing Save, Settings, Quit. Add `--tui` for the ratatui debug observer. |
| `run [DAYS]` | Headless run at extreme speed (default **30** days), prints a report, always saves. |
| `docs` | Prints `See agents.md (directive) and docs/ (see docs/README.md)`. |

## Launch flags (observer-only flags are rejected by `run`/`docs`)

| Flag | Default | Effect |
|---|---|---|
| `--tui` | off | Open the ratatui debug observer instead of the graphical observer. Its key map is in the in-app `?` help overlay. Observer path only. |
| `--no-tui` | off | Legacy alias for a short headless 30-day report (CI-friendly); not the main UI switch. Prefer `--tui` and `run`. Mutually exclusive with `--tui`; observer path only. |
| `--seed <u64>` | saved preference / `0xA1C_0A_6E_17_0` | Reproducible world seed; ignored when `--load` is used. In the GUI, the saved preference is used unless this flag is supplied. |
| `--speed <u32>` | saved preference / `60` | Absolute RealTime ticks/second; must be in `1..=4000` or the CLI exits with an error, even with headless `run`. Used only by observers; GUI inherits saved preference unless supplied. |
| `--load <DIR>` | none | Resume from a save directory; overrides the seed. In GUI mode the requested save opens directly; without it the Main Menu offers a save browser. |
| `--save-dir <DIR>` | saved preference / `saves/default` | Save/output directory. GUI inherits saved preference unless supplied. |
| `--autosave-days <N>` | saved preference / `7` | Autosave cadence in simulated days; `0` disables. GUI inherits saved preference unless supplied. |

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

## Observer controls

### Graphical observer (default)

Button/menu-only — **no hidden keyboard shortcuts**. The Main Menu offers a seed field with **Start**
(new world), plus Load Existing Save, Settings and Quit. The save browser uses
`alc_persistence::list_saves`/`probe_save_dir` and
shows each directory's version, compatibility status, population and simulation time. The world
map is a layered campus-style view with zones, roads, facilities and pawns, plus explicit on-screen
camera controls (Fit/Reset and zoom). Observer preferences are stored separately at
`saves/alc-gui/settings.json`; they do not change world saves (v5 directories with `world.bin`).

### Terminal debug observer (`--tui`)

Keyboard-driven; the same key map is shown in the in-app `?` help overlay.

- **Views:** `1`–`9` = Overview, Prisoner, Gang, Staff, Warden, Financial, Event Timeline,
  Social Network, World Map; `0` = Incident Reports; `A` = Decision Audit.
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

Both frontends render read-only snapshots and validated controls from `sumo::Sim`, with shared
display models from `sumo::graphics`; neither mutates arbitrary world state.

## Troubleshooting

| Symptom | Fix |
|---|---|
| Garbled TUI / `^[[A` arrows | Use Windows Terminal or a modern console; CI should use `cargo run -- run N` (legacy `--no-tui` still gives a short report). |
| Looking for a GUI binary | There is only `alcagent`; use `cargo run` (or `cargo run -p alcagent -- <args>`). `alc-gui` is a library adapter. |
| GUI shortcuts not working | The graphical observer is button/menu-only; use `--tui` for the keyboard-driven observer. |
| "No save found" | Point `--load` at a directory containing `world.bin`, or use the GUI save browser. |
| Same seed, different result | A loaded save overrides the CLI seed. |
| Old save won't load | Current save format is v5; regenerate or migrate older saves. |
