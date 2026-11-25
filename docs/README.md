# Project Alcagent — Documentation

All **Alcagent (game)** documentation lives in this single `docs/` root. The `sumo` engine is a
separate product with its own repository at `../sumo/docs/`; the two cross-reference each other.

## Product guides (Alcagent)

| Document | Purpose |
|---|---|
| [`ALCAGENT_GAMEPLAY.md`](ALCAGENT_GAMEPLAY.md) | **Gameplay & systems** — what happens in the prison, why, and how to observe/control it. |
| [`ALCAGENT.md`](ALCAGENT.md) | CLI/TUI operator quick reference (flags, keys, views). |

Engine guides live in the sumo repo:
[`SUMO_ENGINE.md`](../sumo/docs/SUMO_ENGINE.md) (product & system design) and
[`SUMO_API.md`](../sumo/docs/SUMO_API.md) (API reference).

## Alcagent-owned internal docs

| Folder | Contents |
|---|---|
| [`Design/Core/`](Design/Core/) | Prisoner, social, warden and AI-agent system designs |
| [`Design/Planning/`](Design/Planning/) | Product release planning |
| [`Systems/`](Systems/) | Game-domain contracts (capabilities, parole, guard loop, rehab, warden cycle) |
| [`Reports/`](Reports/) | Product/workspace audits, integration, testing and game gap report |
| [`Project/`](Project/) | Product status, design cross-reference, version milestones |
| [`Communication/`](Communication/) | AI-to-AI handoffs |

Engine-owned internal docs (engine design, interface contracts and the
[sumo engine research report](../sumo/docs/Reports/sumo-engine-research.md)) live in the
[sumo repo](../sumo/docs/README.md).

The master directive is [`../agents.md`](../agents.md).
