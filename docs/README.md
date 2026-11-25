# Project Alcagent — Documentation

All documentation lives in this single `docs/` root. The two products are documented separately
but cross-reference each other; copies of the engine guides are kept here for convenience (the
canonical, maintained versions live in the standalone `../sumo/docs/` repository).

## Alcagent (game) product guides

| Document | Purpose |
|---|---|
| [`ALCAGENT_GAMEPLAY.md`](ALCAGENT_GAMEPLAY.md) | **Gameplay & systems** — what happens in the prison, why, and how to observe/control it. |
| [`ALCAGENT.md`](ALCAGENT.md) | CLI/TUI operator quick reference. |

## sumo (engine) product guides

| Document | Purpose |
|---|---|
| [`SUMO_ENGINE.md`](SUMO_ENGINE.md) | **Engine product & system design** — architecture, subsystems, vision. |
| [`SUMO_API.md`](SUMO_API.md) | Complete engine API reference with examples and gotchas. |
| [`Reports/sumo-engine-research.md`](Reports/sumo-engine-research.md) | Deep research on what sumo should become (architecture, GUI, determinism, backlog, questions). |

Canonical engine docs (and engine-owned internal docs): `../sumo/docs/README.md`.

## Alcagent-owned internal docs

| Folder | Contents |
|---|---|
| [`Design/Core/`](Design/Core/) | Prisoner, social, warden and AI-agent system designs |
| [`Design/Planning/`](Design/Planning/) | Product release planning |
| [`Systems/`](Systems/) | Game-domain contracts (capabilities, parole, guard loop, rehab, warden cycle) |
| [`Reports/`](Reports/) | Product/workspace reports and the sumo research copy |
| [`Project/`](Project/) | Product status, design cross-reference, version milestones |
| [`Communication/`](Communication/) | AI-to-AI handoffs |

The master directive is [`../agents.md`](../agents.md).
