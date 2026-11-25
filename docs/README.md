# Project Alcagent — Documentation

Two products are documented here, kept deliberately separate:

## The game: Alcagent

| Document | Purpose |
|---|---|
| [`ALCAGENT_GAMEPLAY.md`](ALCAGENT_GAMEPLAY.md) | **Complete gameplay & systems guide** — what happens in the prison and why (needs, schedules, social/gangs, conflict, escape, economy, warden, rehab/contraband/construction, parole, events), plus controls. |
| [`ALCAGENT.md`](ALCAGENT.md) | CLI/TUI operator reference — every flag, keybinding and view. |

## The engine: sumo

| Document | Purpose |
|---|---|
| [`SUMO_ENGINE.md`](SUMO_ENGINE.md) | **Engine product & system design** — architecture, subsystems, vision, current domain-specific vs generalizable parts. |
| [`SUMO_API.md`](SUMO_API.md) | Complete engine API reference with examples and gotchas. |

## Internal documentation

[`../Documentation/README.md`](../Documentation/README.md) holds design intent, system
contracts, reports and AI handoffs. The master directive is [`../agents.md`](../agents.md).

The engine is also published as a standalone repository at `../sumo`, which carries copies of
the engine docs (`SUMO_ENGINE.md`, `SUMO_API.md`) plus a copy of the gameplay guide for context.
