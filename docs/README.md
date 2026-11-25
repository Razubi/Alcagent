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
    ├── ALCAGENT.md           CLI/TUI quick reference
    ├── Design/               Game system designs
    ├── Systems/              Game-domain contracts
    ├── Reports/              Game/workspace reports
    ├── Project/              Product status & milestones
    └── Communication/        AI-to-AI handoffs
```

## sumo (engine)

| Document | Purpose |
|---|---|
| [`sumo/SUMO_ENGINE.md`](sumo/SUMO_ENGINE.md) | Engine product & system design |
| [`sumo/SUMO_API.md`](sumo/SUMO_API.md) | Complete API reference with examples and gotchas |
| [`sumo/sumo-engine-research.md`](sumo/sumo-engine-research.md) | Deep research: architecture, GUI, determinism, backlog, questions |
| [`sumo/world-expansion-plan.md`](sumo/world-expansion-plan.md) | **v0.5 plan: living town + prison coupling, GUI rewrite, roadmap** |
| [`sumo/sumo-gui-and-support-plan.md`](sumo/sumo-gui-and-support-plan.md) | sumo GUI shell & long-term support annex |

The canonical, maintained engine docs (including engine-owned internal design/Reports) also
live in the standalone repository at `../sumo/docs/`.

## Alcagent (game)

| Document | Purpose |
|---|---|
| [`alcagent/ALCAGENT_GAMEPLAY.md`](alcagent/ALCAGENT_GAMEPLAY.md) | Gameplay, systems and all controls |
| [`alcagent/ALCAGENT.md`](alcagent/ALCAGENT.md) | CLI/TUI operator quick reference |
| [`alcagent/Design/`](alcagent/Design/) | Game system designs |
| [`alcagent/Systems/`](alcagent/Systems/) | Game-domain contracts |
| [`alcagent/Reports/`](alcagent/Reports/) | Product/workspace reports |
| [`alcagent/Project/`](alcagent/Project/) | Status, design cross-reference, milestones |
| [`alcagent/Communication/`](alcagent/Communication/) | AI handoffs |

The master directive is [`../agents.md`](../agents.md).
