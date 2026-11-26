# Agent Instructions

**Read:** `.specify/memory/constitution.md`

That file is your source of truth for this project.

## Project Overview
- **Name:** Alcagent (Autonomous Living Prison Simulation)
- **Language:** Rust (edition 2024)
- **Workspace:** 15 crates (alc-core, alc-world, alc-agents, alc-prisoner, alc-social, alc-economy, alc-warden, alc-guard, alc-events, alc-persistence, sumo, alc-gui, alc-ui, alc-testing, alcagent)
- **Engine:** sumo (headless, owns all simulation state)
- **Frontends:** alcagent (binary), alc-gui (egui), alc-ui (ratatui)
- **Save format:** v5 (bincode + serde)
- **Tests:** 500+ across all crates

## Key Directories
- `specs/` — Feature specifications with acceptance criteria
- `history/` — Per-spec completion notes
- `completion_log/` — Completion summaries
- `.specify/memory/constitution.md` — Project constitution (read this first)

## Commands
- `cargo build` — Build all crates
- `cargo test` — Run all tests
- `cargo test --workspace` — Full workspace test suite
- `cargo fmt --all` — Format code
- `cargo clippy` — Lint

## Git
- Remote: https://github.com/Razubi/Alcagent.git
- Author: Razubi (matthewmanning278@gmail.com)
- Always commit and push after each spec completion