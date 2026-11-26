# Project Alcagent — Documentation

All documentation lives in this single `docs/` root, split by product:

```
docs/
├── README.md            ← this index
├── sumo/                ALL engine (sumo) docs
│   ├── SUMO_ENGINE.md   Engine product & system design
│   ├── SUMO_API.md      Engine API reference
│   └── sumo-engine-research.md  Deep research on sumo's future
└── alcagent/            ALL game (Alcagent) docs
    ├── ALCAGENT_GAMEPLAY.md  Gameplay, systems & controls
    ├── ALCAGENT.md           Launch / CLI & observer quick reference
    ├── Design/               Game system designs
    ├── Systems/              Game-domain contracts
    ├── Reports/              Game/workspace reports
    ├── Project/              Product status & milestones
    └── Communication/        AI-to-AI handoffs
```

## sumo (engine)

For current API/behavior, use [`sumo/SUMO_API.md`](sumo/SUMO_API.md) and
[`alcagent/Project/PROJECT.md`](alcagent/Project/PROJECT.md). Research and roadmap docs below are
dated planning snapshots; historical notes are retained but not treated as active status.

| Document | Purpose |
|---|---|
| [`sumo/SUMO_ENGINE.md`](sumo/SUMO_ENGINE.md) | Engine product & system design (incl. shared `sumo::graphics` models) |
| [`sumo/SUMO_API.md`](sumo/SUMO_API.md) | Complete API reference with examples and gotchas |
| [`sumo/sumo-engine-research.md`](sumo/sumo-engine-research.md) | Deep research: architecture, GUI, determinism, backlog, questions |
| [`sumo/SUMO_ROADMAP.md`](sumo/SUMO_ROADMAP.md) | **0.05 → 0.6+ engine roadmap + mass bug-fixing plan** |
| [`sumo/world-expansion-plan.md`](sumo/world-expansion-plan.md) | **v0.5 plan: living town + prison coupling, GUI rewrite, roadmap** |
| [`sumo/sumo-gui-and-support-plan.md`](sumo/sumo-gui-and-support-plan.md) | sumo GUI shell & long-term support annex |

Engine design/reference copies also exist under `alcagent/Design/` and `alcagent/Reports/` in
this repository. The docs under this repository's `docs/` are canonical; they are not synced with
any out-of-tree `../sumo` snapshot.

## Alcagent (game)

| Document | Purpose |
|---|---|
| [`alcagent/ALCAGENT_GAMEPLAY.md`](alcagent/ALCAGENT_GAMEPLAY.md) | Gameplay, systems and controls |
| [`alcagent/ALCAGENT.md`](alcagent/ALCAGENT.md) | Launch, CLI and observer quick reference |
| [`alcagent/Design/`](alcagent/Design/) | Game system designs |
| [`alcagent/Systems/`](alcagent/Systems/) | Game-domain contracts |
| [`alcagent/Reports/`](alcagent/Reports/) | Product/workspace reports |
| [`alcagent/Project/`](alcagent/Project/) | Status, design cross-reference, milestones |
| [`alcagent/Communication/`](alcagent/Communication/) | AI handoffs |

### Launching (current)

There is one binary, `alcagent`: `cargo run` (workspace `default-members`) opens the graphical
observer at its Main Menu — seed field + **Start** (new world), **Load Existing Save**, **Settings**,
**Quit**. Use `cargo run -- --tui` for the ratatui debug observer and `cargo run -- run <days>` for
headless runs.
The GUI is button/menu-only; the TUI key map lives in its `?` help overlay. World saves are v5
directories with `world.bin`; GUI preferences are separate JSON in `saves/alc-gui/settings.json`.
Both frontends are rendering/input adapters over `sumo::Sim` using shared `sumo::graphics` models.

The master directive is [`../agents.md`](../agents.md).
