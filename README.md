# Project Alcagent — Autonomous Living Prison Simulation

A persistent, autonomous prison society in Rust. Dozens of independently simulated prisoners,
guards and an AI warden form an evolving institution: needs drift, gangs emerge from
relationships, conflicts have consequences, finances constrain the warden's choices, parole
and escapes change the population, and the world keeps developing whether or not anyone is
watching. No LLM is required to run it — the simulation is deterministic, seed-driven and
fully local.

> **Status (v0.05 / `0.5.0-beta.1`):** the workspace builds and tests green, including the
> 730-day three-seed chaos soak, a 365-day save/load continuation, deterministic council/shadow
> policy modes and both frontend suites. All 14 specs in `dev/ralph/specs` are complete:
> the spatial map, gang politics, AI orchestration/batching, long-run soak
> acceptance and release documentation are included in this candidate.
>
> **Frontends (current):** there is one binary, `alcagent`. Plain `cargo run` opens the graphical
> observer; `--tui` opens the ratatui debug observer. Both use the shared display models in
> `sumo::graphics` and read the world only through `sumo::Sim`.

---

## Quick start

Requirements: **Rust 1.85+** (edition 2024) — the project has been built with Rust 1.100.

```powershell
cargo build --workspace
cargo test --workspace

# Open the graphical observer (default; starts at the Main Menu)
cargo run
cargo run -- observe             # same, explicitly

# Open the ratatui debug observer
cargo run -- --tui

# Run a headless 1-year simulation and print a report
cargo run -- run 365

# CLI help (flags only; TUI keys are in the in-app ? overlay)
cargo run -- --help
```

`cargo run` works because the workspace `default-members` is `crates/alcagent`, the only binary.
The graphical observer opens at a Main Menu with a world-seed field and **Start** (new world),
**Load Existing Save**, **Settings** and **Quit**; it is button/menu-only and has no hidden keyboard
shortcuts. `--load <dir>` opens that save directly; without it, the menu offers a save browser.
`--seed`, `--speed`, `--save-dir` and `--autosave-days` work in both observers; in the GUI, omitted
values inherit saved observer preferences while explicit flags win. `--speed` is the absolute
RealTime tick rate (1–4000, default 60) and out-of-range values are rejected even with headless
`run`. `--tui` and `--no-tui` apply only to the observer path; `run`/`docs` reject them. `--no-tui`
remains a legacy alias for a short headless report; prefer `--tui` and `run`. See
[`docs/alcagent/ALCAGENT.md`](docs/alcagent/ALCAGENT.md) for the full reference.

The ratatui observer is the keyboard-driven debug view: `1`–`9`/`0`/`A` switch views, `Space`
pauses, `+`/`-` changes speed, arrows/`hjkl` move the cursor, `/` searches, `?` opens its help overlay,
`S` saves and `q`/`Q`/`Esc`/`Ctrl-C` quit. Those keybinds apply **only** to `--tui` mode; the GUI uses
visible buttons and menus instead.

---

## What you can do with it

- **Observe** a living prison in an 11-view graphical observer (default) or the ratatui debug
  observer (`--tui`).
- **Simulate** long-running worlds headlessly at extreme speed; the same seed always
  reproduces the same institution.
- **Embed** the engine as the Rust crate `sumo` and drive it through a validated API —
  propose agent decisions, inspect read-only observations and map frames, manage
  policy/parole/rehab, save/load and check invariants. See [`docs/sumo/SUMO_API.md`](docs/sumo/SUMO_API.md).
- **Extend** the simulation through clearly separated crates (needs, social, economy,
  warden, guards, AI orchestration, persistence, UI).

---

## Repository layout

```
.
├── agents.md                  Master development directive
├── Cargo.toml / Cargo.lock    Rust workspace (edition 2024, 15 member crates)
├── CHANGELOG.md               Version history
├── crates/
│   ├── alc-core               Time, RNG, IDs, capabilities, config
│   ├── alc-world              World state and the main tick loop
│   ├── alc-prisoner           Prisoner entities: needs, schedules, intake
│   ├── alc-social             Relationships, gangs, conflict, escape, parole, rehab
│   ├── alc-economy            Money/ledger, funding, expenses, health
│   ├── alc-warden             Warden AI, expansion catalog, finance planning
│   ├── alc-guard              Guard agents: posts, ranks, duty loop, reports
│   ├── alc-events             EventBus, WorldEvent, EventKind taxonomy
│   ├── alc-agents             Decisions, validation, memory, orchestration
│   ├── alc-persistence        Versioned bincode saves, migration, autosave, save discovery
│   ├── alc-ui                 Ratatui debug observer adapter (input/rendering only)
│   ├── alc-testing            Invariants and soak helpers
│   ├── sumo                   THE ENGINE: public API + shared graphics models
│   ├── alcagent               THE binary: graphical observer / --tui / headless run
│   └── alc-gui                Egui graphical observer adapter (library, no separate binary)
├── docs/                      In-repo documentation (canonical)
│   ├── README.md              Documentation index
│   ├── sumo/                  Engine product/API/roadmap docs
│   └── alcagent/              Gameplay, systems, project and communication docs
```

`sumo` is the supported integration surface. Application code should go through `sumo::Sim`
rather than mutating `alc_world::World` directly; agents propose decisions and the engine
validates and applies them. Shared display/graphics models — palette, view catalog, world-map
layout and zones, camera math and the construction-kind catalog — live in `sumo::graphics`.
`alc-gui` and `alc-ui` are rendering/input adapters over `sumo::Sim`; neither mutates arbitrary
world state.

World saves remain v5 directories containing `world.bin`. Observer preferences are separate JSON
at `saves/alc-gui/settings.json` and do not alter world saves.

The `sumo` engine and its `alc-*` dependencies may also exist as an out-of-tree snapshot at
`../sumo`, but the docs under this repository's `docs/` are canonical and are not synced with that
snapshot. This repository owns the single `alcagent` binary and its two observer adapters.

---

## Documentation

| Audience | Document |
|---|---|
| New player / operator | [Gameplay & systems](docs/alcagent/ALCAGENT_GAMEPLAY.md), [launch & observer reference](docs/alcagent/ALCAGENT.md), [quick start](#quick-start) |
| Rust developer embedding the engine | [sumo engine design](docs/sumo/SUMO_ENGINE.md), [sumo API guide](docs/sumo/SUMO_API.md) |
| Contributor | [CONTRIBUTING.md](CONTRIBUTING.md) |
| Architect / reviewer | [Docs index](docs/README.md), [engine research](docs/sumo/sumo-engine-research.md), [project status](docs/alcagent/Project/PROJECT.md) |
| Directive | [agents.md](agents.md) — the full autonomous-development specification |

Detailed system contracts (guard loop, warden cycle, parole, rehab, capabilities) live under
[`docs/alcagent/Systems/`](docs/alcagent/Systems/), design intent under
[`docs/alcagent/Design/Core/`](docs/alcagent/Design/Core/), and the event taxonomy is published in
[`docs/sumo/SUMO_API.md`](docs/sumo/SUMO_API.md) with source of truth
`crates/alc-events/src/lib.rs`.

---

## Architecture in one paragraph

`sumo::Sim` owns an `alc_world::World` and advances it in ticks. Each tick drifts every
prisoner's 25 needs, runs a throttled hourly guard-duty sweep, resolves the daily schedule,
lets prisoners choose need-driven actions (trait-biased; AI deliberation is gated by a token
budget with a deterministic fallback), resolves co-presence encounters, and — at day boundaries —
accrues finances, progresses sentences, resolves escapes and parole, runs warden arbitration,
and admits new prisoners subject to capacity and policy. Everything observable is published on a
bounded event bus with structured causes; read-only snapshots (`Observation`, `MapFrame`,
`Stats`, summary types) feed the UI through the shared `sumo::graphics` display models. Worlds are
saved as versioned bincode with a JSON debug sidecar (currently format v5); seeded RNG streams make
replays reproducible. See the [project status document](docs/alcagent/Project/PROJECT.md) for the
implemented scope.

---

## Deferred / future work (beyond 0.05)

The 0.05 release landed council/shadow policy modes, structured incident chains, the full
rehab/contraband lifecycle and both spatial frontends. Deferred depth includes a real navigation
graph, a full `KnowledgeStore`, provider-backed narrative adapters and further GUI fidelity.
See [version milestones](docs/alcagent/Project/version-milestones.md) for the next planned scope.

## License

MIT — see [LICENSE](LICENSE).
