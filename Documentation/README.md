# Documentation Index — Project Alcagent

Single home for all project documentation. Anything about **intent, delivery, interfaces or
communication** lives under `Documentation/`. The master directive (`agents.md`) stays at the
repository root so the harness can load it — do not move it.

**Current state (verified 2026-11-23):** version 0.05, `cargo build --workspace` and
`cargo test --workspace` green (184 passed, 0 failed, 1 ignored). See `Project/PROJECT.md`.

## Repository hierarchy

```
/                                  REPOSITORY ROOT
├── agents.md                      MASTER DIRECTIVE (kept at root on purpose)
├── Cargo.toml / Cargo.lock        Workspace manifest / lockfile
├── CHANGELOG.md                   Release / change notes
├── README.md                      PUBLIC README (start here)
├── CONTRIBUTING.md                Contribution workflow and conventions
├── LICENSE                        MIT license
├── docs/                          PUBLIC user docs: SUMO_API.md, ALCAGENT.md
├── .gitignore                     Ignores /target, *.log, build_output.txt, saves/
├── crates/                        Implementation (Rust workspace)
│   ├── alc-core/, alc-world/, alc-agents/, alc-prisoner/, alc-social/
│   ├── alc-economy/, alc-warden/, alc-guard/, alc-events/
│   ├── alc-persistence/, alc-ui/, alc-testing/
│   ├── sumo/                      Simulation engine (public API)
│   ├── alcagent/                  Binary: run / observe / docs
│   └── alc-gui/                   Incomplete experimental crate — NOT a workspace member
└── Documentation/                 ALL project documentation
    ├── README.md                  ← this index
    ├── Project/                   Status, cross-reference, milestones (delivered state)
    │   ├── PROJECT.md
    │   ├── design-status.md
    │   └── version-milestones.md
    ├── Design/                    Intent / plans (not necessarily implemented)
    │   ├── Core/
    │   │   ├── prison-simulation.md
    │   │   ├── social-systems.md
    │   │   ├── warden-system.md
    │   │   └── ai-agent-systems.md
    │   ├── Engine/
    │   │   ├── sumo-engine.md
    │   │   ├── sumo-expansion.md
    │   │   └── sumo-next.md
    │   └── Planning/
    │       └── beta-0.05.md
    ├── Systems/                   AI-facing contracts / interfaces (current APIs)
    │   ├── capabilities.md
    │   ├── determinism.md
    │   ├── engine-api.md
    │   ├── event-contract.md
    │   ├── guard-loop.md
    │   ├── parole-pipeline.md
    │   ├── rehab.md
    │   └── warden-cycle.md
    ├── Reports/                   Audit + test findings (dated snapshots)
    │   ├── audit-report.md
    │   ├── beta-0.05-integration.md
    │   ├── sumo-audit.md
    │   └── testing.md
    └── Communication/             AI-to-AI handoffs
        ├── README.md
        ├── Sumo-AI-Team/           Requests to the engine team
        │   ├── README.md
        │   └── NEXT-UPDATE-SUPPORT.md
        └── Note's/                 Informal notes left by AIs
            ├── README.md
            ├── _TEMPLATE.md
            ├── persistence-lib-rs-changes-needed.md
            ├── wave4-implementation-status.md
            └── wave4-support-needed.md
```

## Quick links

| I want… | Read |
|---|---|
| What the project is and its current status | `Project/PROJECT.md` |
| Design-to-code map / what is implemented vs deferred | `Project/design-status.md` |
| Current version and next milestone (0.1 beta) | `Project/version-milestones.md` |
| Wave 4 engine support requests (R1–R7) | `Communication/Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md` |
| Wave 4 cross-team note | `Communication/Note's/wave4-support-needed.md` |
| Wave 4 implementation status / Wave 5 items | `Communication/Note's/wave4-implementation-status.md` |
| Prisoner needs / schedules / intake | `Design/Core/prison-simulation.md` |
| Gangs, relationships, conflict, escapes | `Design/Core/social-systems.md` |
| Warden, guards, economy, expansion | `Design/Core/warden-system.md` |
| Decision loops, memory, orchestration | `Design/Core/ai-agent-systems.md` |
| Engine design and public API | `Design/Engine/sumo-engine.md`, `Systems/engine-api.md` |
| Event taxonomy | `Systems/event-contract.md` |
| How to run tests / soaks | `Reports/testing.md` |
| Release / change history | `../CHANGELOG.md` |

## Conventions

- `Design/` describes **intent**. `Systems/` describes **interfaces**. `Reports/` describes
  **findings** (dated snapshots). `Project/` describes **delivered state** (kept current).
- Paths in these documents are **repository-root relative** (e.g. `Documentation/Systems/engine-api.md`).
- Plans in `Design/` are plan-only until promoted into `Project/PROJECT.md`.
- AIs leaving information for other AIs must use `Communication/Note's/` and the `_TEMPLATE.md`
  format. Requests to the sumo engine team go in `Communication/Sumo-AI-Team/`.
- Transient build/test artifacts (`*.log`, `build_output.txt`) are gitignored and should not be
  committed or kept in the tree.

## Cleanup log

- 2026-11-23: rebuilt hierarchy index to match the actual tree; documented 184-test green
  baseline; removed obsolete root-level duplicate summaries, stale `design/` session artifacts,
  random `.log` files and `build_output.txt`; gitignored transient artifacts.
- The former `architecture-report.md` was removed earlier; it was superseded by
  `Reports/audit-report.md` and `Design/Engine/sumo-engine.md`.
- 2026-11-20: beta-0.05 update list (`Design/Planning/beta-0.05.md`) and next-version plan
  (`Design/Engine/sumo-next.md`) added as deep-planning deliverables.
