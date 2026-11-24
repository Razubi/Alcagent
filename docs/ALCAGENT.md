# `alcagent` User Guide

`alcagent` is the command-line operator for Project Alcagent. It launches either an
interactive terminal observer (TUI) or a headless simulation run. It is built from
`crates/alcagent` and wraps the [`sumo`](SUMO_API.md) engine.

---

## Build and run

```powershell
cargo build --workspace

cargo run --                 # interactive TUI (default)
cargo run -- observe         # same thing, explicitly
cargo run -- run 365         # headless 365-day run
cargo run -- --help          # complete clap help
cargo run -- docs            # one-line pointer to project docs
```

The executable is `target\debug\alcagent.exe` on Windows. For long headless runs prefer
`cargo run --release -- run 3650` (release profile uses LTO and codegen-units=1). Logging uses
`RUST_LOG` (e.g. `$env:RUST_LOG='sumo=debug'; cargo run -- run 5`).

---

## CLI reference

```
alcagent [OPTIONS] [COMMAND]
```

Commands (global options may appear before or after the command):

| Command | Effect |
|---|---|
| *(none)* | Same as `observe` — opens the interactive TUI. |
| `observe` | Interactive ratatui observer; with `--no-tui`, runs a 30-day headless report instead. |
| `run <DAYS>` | Headless run for `DAYS` simulated days at extreme speed, then prints a report and saves. |
| `docs` | Prints: `See agents.md (directive) and Documentation/ (index, design, systems, reports)` |

Global options:

| Flag | Default | Meaning |
|---|---|---|
| `--seed <u64>` | `0xA1C_0A_6E_17_0` | World-generation seed. The same seed reproduces the same institution. Ignored when loading a save. |
| `--speed <u32>` | `60` | TUI tick rate in simulation ticks per wall-clock second (clamped 1–4000). **Ignored by headless `run`**, which forces extreme speed. |
| `--no-tui` | off | Disable the TUI; `observe` falls back to a 30-day headless report. Useful in CI/non-interactive shells. |
| `--load <PATH>` | none | Load simulation state from a save directory before running. With a loaded save, `run <DAYS>` adds to the world's already-simulated days. |
| `--save-dir <PATH>` | `saves/default` | Directory for manual/auto saves. |
| `--autosave-days <u64>` | `7` | Autosave interval in simulated days (`0` disables). |

Verbatim `alcagent --help` description:

> Autonomous Living Prison Simulation
>
> With no arguments (or `observe`) an interactive ratatui observer opens.
>
> TUI controls:
>   q / Q / Esc / Ctrl-C   quit
>   Space                  pause / resume
>   + / =                  speed up
>   - / _                  speed down
>   1-9                    switch observation view
>   Up/Down/Left/Right or k/j/h/l   move the selection cursor
>
> Use `run <days>` for a headless soak run, or `--no-tui` for a short report.

---

## Headless runs

Example (`cargo run -- run 5 --save-dir saves/example`, abbreviated output):

```
=== Alcagent headless run ===
Seed:            0xa1c0a6e170
Sim time:        Day 0005 00:00
Days simulated:  5
Population:      25
Capacity:        80
Guards:          4
Facilities:      6
Cash (major):    253378.00
Events logged:   452
Save dir:        saves/example
Real elapsed:    48.10ms
Ticks/sec:       104

Recent events (last 10):
  [Day 0005 00:00] Argument — Argument between 19 and 23
  [Day 0005 00:00] Rumor — Rumor exchanged between 10 and 11
  ...
```

Notes:

- The seed is printed in hex. `Cash (major)` is the institution's ledger in major currency units.
- **`Ticks/sec` is actually days per second** (a legacy label), and when using `--load` it is
  inflated by the days already simulated before the run.
- Every `run` **always writes a save** to `--save-dir` on completion, even without autosave.
- Headless speed is fixed at `Extreme` (30 simulated minutes per tick); `--speed` is ignored.
- With a 1-year run this typically completes in ~1 second on a modern machine; a 730-day run
  is used by the test suite as a soak/regression.

---

## Interactive TUI

The observer is a read-only ratatui view over the engine: it cannot corrupt the simulation by
editing state directly. Startup opens the alternate screen and raw mode; shutdown always
restores the terminal. The loop renders at ~60 FPS and ticks at `--speed` ticks/second
(internal safety: max 5,000 ticks per frame; stalled accumulators are discarded).

### Keybindings

| Key | Action |
|---|---|
| `q`, `Q`, `Esc`, `Ctrl-C` | Quit |
| `Space` | Pause / resume the observer tick loop |
| `+` / `=` | Double tick speed (max 4000 tps) |
| `-` / `_` | Halve tick speed (min 1 tps) |
| `1`–`9` (in-flight: `0`, `A`) | Switch observation view (resets scroll) |
| `↑` `↓` `←` `→` / `k` `j` `h` `l` | Move the selection cursor (held keys repeat) |
| `s` / `S` | Manual save to `--save-dir` (status shows `Saved to <path>` or the error) |
| `L` / `U` | Enable / lift lockdown |
| `p` / `P` | Cycle policy mode: Deterministic → Council → Shadow → Deterministic |
| `N` / `E` / `K` | Set policy band: Normal / Enhanced / Lockdown (in-flight) |
| `[` / `]` | Decrease / increase security level by 0.05 (in-flight) |
| `,` / `.` | Decrease / increase privileges by 0.05 (in-flight) |
| `;` / `'` | Decrease / increase rehab funding by 0.1 (in-flight) |
| `b` / `B` | Toggle manual intake pause (in-flight) |

The in-app help bar lists the common controls; several of the policy keys above are only shown
in the Warden view's "Policy Controls" panel. The `PAUSED`/`RUNNING` badge refers to the tick
loop, *not* to policy mode. A `LOCKDOWN` badge appears in the status bar when lockdown is active.

> **In-flight (2026-11-24):** a gameplay wave is adding the policy keys, the `0`/`A` views,
> incident-record/audit views and a construction table on the map. The keybindings above marked
> "in-flight" may change before the wave lands; the list is kept for early adopters.

### The nine views (plus in-flight additions)

| Key | View | What it shows |
|---|---|---|
| 1 | Institution Overview | Sim time/days, population & capacity, guards/facilities, gangs, events, security/privileges/economy bars, lockdown, recent 10 events, warden/institution panel and facilities table. |
| 2 | Prisoner | Selectable prisoner list; profile (age, class, sentence, gang, reputation, health/mental/hope), 12 traits, 25 need-deficit bars, goals/schedule, top skills, memory summary. |
| 3 | Gang | Leader, members, reputation/resources, culture, formation time, hierarchy, rivalries, alliances, disputes and goals (or an empty-state hint). |
| 4 | Staff | Roster (rank/post); detail: authority, shift, task, credibility, duty-condition bars (competence, morale, fatigue), disposition (stress, loyalty, integrity) and duty log. |
| 5 | Warden | Name/philosophy, attention, reports received, 11 leadership traits, pending actions. **In-flight:** "Policy Controls" panel (band, lockdown, security/privileges bars, rehab funding, intake state). |
| 6 | Financial | Treasury, health index, funding per prisoner/day, crowding threshold, paged ledger of daily accounts. |
| 7 | Event Timeline | Newest-first events; detail pane with kind, time, id, confidence and causal chains (`a → b`). |
| 8 | Social Network | Dyad counts and a paged table of relationships sorted by bond strength (friendship, trust, fear, hostility, rivalry, bond/threat scores). |
| 9 | Prison Map | ASCII schematic: perimeter/tower count, facilities by kind coloured by condition, guard-post counts and facilities table. **In-flight:** construction-queue table (ID, project, %, days). (A live graphical map fed by `sim.map_frame()` is planned; the current view is schematic.) |
| 0 / A (in-flight) | Incident Reports / Decision Audit | Structured incident records (severity colour, OPEN/RES, cause/consequence) and decision audit records (actor, source, validation colour, reason). |

Views have narrow-layout fallbacks (compact tab labels/help) and are safe on sparse or very
large worlds (2,000+ prisoners).

---

## Saves and autosave

- Default directory: `saves/default` (created on first save). Override with `--save-dir`.
- Files: `world.bin` (versioned bincode — world + engine state) and `world.debug.json` (human-readable debug sidecar). Format version is currently **3**; saves from earlier versions must be regenerated/migrated.
- Autosave every 7 simulated days by default; `--autosave-days 0` disables it.
- Resume with `cargo run -- --load saves/default observe` (or `run 30`). Loading ignores
  `--seed`: the world's original seed comes from the save.
- TUI manual save: press `s`. Headless `run` always saves at the end.

To inspect a save: `type saves\default\world.debug.json` (PowerShell) or open it in an editor.

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| TUI looks garbled / arrow keys print `^[[A` | Use Windows Terminal or a modern console; raw mode requires a real console. CI should use `--no-tui`. |
| `cargo run` builds the wrong crate | Use the explicit form `cargo run -p alcagent -- <args>`. |
| "no save found" on `--load` | Check the path contains `world.bin`; it must be a directory, not a file. |
| Different results with same seed | Ensure you did not accidentally `--load` a different world; loaded saves override the seed. |
| Terminal left in raw mode after a crash | Reset with `(Get-Process -Id $PID)` is not applicable — just run `mode con` or `reset` in a fresh shell; normal shutdown always restores it. |

## See also

- [`SUMO_API.md`](SUMO_API.md) — embed the engine in your own Rust code.
- [`../Documentation/Project/PROJECT.md`](../Documentation/Project/PROJECT.md) — project status and known gaps.
