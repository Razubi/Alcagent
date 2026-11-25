# Project Alcagent — Autonomous Living Prison Simulation

A persistent, autonomous prison society in Rust. Dozens of independently simulated prisoners,
guards and an AI warden form an evolving institution: needs drift, gangs emerge from
relationships, conflicts have consequences, finances constrain the warden's choices, parole
and escapes change the population, and the world keeps developing whether or not anyone is
watching. No LLM is required to run it — the simulation is deterministic, seed-driven and
fully local.

> **Status (v0.05):** last verified green baseline 2026-11-23 — `cargo build --workspace` and
> `cargo test --workspace` passed (**184 passed, 0 failed, 1 ignored**), including a
> 730-day three-seed chaos soak and save/load replay determinism. A new gameplay wave is
> mid-edit as of 2026-11-24 (see docs); the tree may not compile until it lands.

---

## Quick start

Requirements: **Rust 1.85+** (edition 2024) — the project has been built with Rust 1.100.

```powershell
cargo build --workspace
cargo test --workspace

# Watch the prison in the interactive terminal observer
cargo run -- observe            # (or just `cargo run`)

# Run a headless 1-year simulation and print a report
cargo run -- run 365

# CLI help (lists all flags and keybindings)
cargo run -- --help
```

Running `cargo run -- run N` writes a save to `saves/default` by default. See
[`docs/ALCAGENT.md`](docs/ALCAGENT.md) for the complete CLI/TUI reference.

---

## What you can do with it

- **Observe** a living prison in an 11-view ratatui TUI (9 core views plus incident reports and
  decision audit).
- **Simulate** long-running worlds headlessly at extreme speed; the same seed always
  reproduces the same institution.
- **Embed** the engine as the Rust crate `sumo` and drive it through a validated API —
  propose agent decisions, inspect read-only observations and map frames, manage
  policy/parole/rehab, save/load and check invariants. See [`docs/SUMO_API.md`](docs/SUMO_API.md).
- **Extend** the simulation through clearly separated crates (needs, social, economy,
  warden, guards, AI orchestration, persistence, UI).

---

## Repository layout

```
.
├── agents.md                  Master development directive
├── Cargo.toml / Cargo.lock    Rust workspace (edition 2024, 14 member crates)
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
│   ├── alc-persistence        Versioned bincode saves, migration, autosave
│   ├── alc-ui                 Ratatui observer UI (read-only over the engine)
│   ├── alc-testing            Invariants and soak helpers
│   ├── sumo                   THE ENGINE: public API over the alc-* stack
│   ├── alcagent               Binary: run / observe / docs
│   └── alc-gui                (experimental, NOT a workspace member, non-buildable)
├── docs/                      User-facing docs (this file's directory)
│   ├── SUMO_API.md            Complete sumo engine API guide
│   ├── ALCAGENT.md            CLI + TUI user guide
│   └── README.md              Documentation index
└── Documentation/             Internal design, systems, reports, AI handoffs
    └── README.md              Internal documentation index
```

`sumo` is the supported integration surface. Application code should go through `sumo::Sim`
rather than mutating `alc_world::World` directly; agents propose decisions and the engine
validates and applies them.

The engine and its `alc-*` dependencies are also published as a standalone repository at
`../sumo` (`docs/SUMO_API.md` is shared between both). This repository additionally owns the
`alcagent` CLI, the `alc-ui` TUI and the `alc-gui` experiment.

---

## Documentation

| Audience | Document |
|---|---|
| New player / operator | [Gameplay & systems](docs/ALCAGENT_GAMEPLAY.md), [CLI & TUI reference](docs/ALCAGENT.md), [quick start](#quick-start) |
| Rust developer embedding the engine | [sumo engine design](docs/SUMO_ENGINE.md), [sumo API guide](docs/SUMO_API.md) |
| Contributor | [CONTRIBUTING.md](CONTRIBUTING.md) |
| Architect / reviewer | [Docs index](docs/README.md), [sumo engine research](Documentation/Reports/sumo-engine-research.md), [internal docs](Documentation/README.md), [project status](Documentation/Project/PROJECT.md) |
| Directive | [agents.md](agents.md) — the full autonomous-development specification |

Detailed system contracts (event taxonomy, guard loop, warden cycle, parole, rehab,
determinism) live under [`Documentation/Systems/`](Documentation/Systems/), and design
intent under [`Documentation/Design/`](Documentation/Design/).

---

## Architecture in one paragraph

`sumo::Sim` owns an `alc_world::World` and advances it in ticks. Each tick drifts every
prisoner's 25 needs, runs a throttled hourly guard-duty sweep, resolves the daily schedule,
lets prisoners choose need-driven actions (trait-biased; AI deliberation is gated by a token
budget with a deterministic fallback), resolves co-presence encounters, and — at day boundaries —
accrues finances, progresses sentences, resolves escapes and parole, runs warden arbitration,
and admits new prisoners subject to capacity and policy. Everything observable is published on a
bounded event bus with structured causes; read-only snapshots (`Observation`, `MapFrame`,
`Stats`, summary types) feed the UI. Worlds are saved as versioned bincode with a JSON debug
sidecar; seeded RNG streams make replays reproducible. See the
[internal status document](Documentation/Project/PROJECT.md) for the implemented scope and
known gaps.

---

## Roadmap / known gaps

Active work continues on Wave 4/5 features (navigation graph, council/shadow behavior,
structured incident chains, full knowledge store, rehab/contraband lifecycle wiring, GUI).
See [version milestones](Documentation/Project/version-milestones.md) and the
[Wave 4 status note](Documentation/Communication/Note's/wave4-implementation-status.md).

## License

MIT — see [LICENSE](LICENSE).
