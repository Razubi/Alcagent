# Alcagent Constitution

> Autonomous living prison simulation in Rust. A persistent, emergent prison society where autonomous AI agents (prisoners, guards, warden) make independent decisions, form gangs, develop relationships, pursue goals, and create unpredictable emergent events. The observer watches the institution evolve over hours, days, months, and years. It must support hundreds of agents, persist complete worlds, and remain deterministic and testable.

---

## Context Detection

**Ralph Loop Mode** (started by ralph-loop*.sh):
- Pick highest priority incomplete spec from `specs/`
- Implement, test, commit, push
- Output `<promise>DONE</promise>` only when 100% complete
- Output `<promise>ALL_DONE</promise>` when no work remains

**Interactive Mode** (normal conversation):
- Be helpful, guide decisions, create specs

---

## Core Principles

1. **Emergence over scripting** — Interesting events arise naturally from system interactions, not scripted encounters.
2. **Autonomy over control** — Every agent has its own goals, personality, memories, and decision-making loop; the observer influences, never controls.
3. **Determinism and persistence** — Same seed reproduces the same world; complete save/load fidelity; institutional history survives.
4. **Consequences over convenience** — Every action creates ripple effects across needs, relationships, gangs, security, finances, and reputation.
5. **Modularity and testability** — Each system has clearly defined responsibilities and interfaces; all features include tests.
6. **Performance and scalability** — The simulation must handle growing populations without becoming prohibitively expensive.

---

## Technical Stack

- **Language:** Rust (edition 2024, rust-version 1.85)
- **Architecture:** 15-crate workspace (alc-core, alc-world, alc-agents, alc-prisoner, alc-social, alc-economy, alc-warden, alc-guard, alc-events, alc-persistence, sumo, alc-gui, alc-ui, alc-testing, alcagent)
- **Serialization:** serde + bincode (v5 save format)
- **GUI:** egui 0.37 (alc-gui)
- **TUI:** ratatui 0.29 (alc-ui)
- **Binary:** alcagent (single binary, GUI default, --tui for ratatui, run <days> headless)
- **Engine:** sumo (facade over all subsystems)
- **No LLM dependency** in core crates — hybrid deterministic/AI orchestration

---

## Autonomy

YOLO Mode: ENABLED
Git Autonomy: ENABLED

Commit and push after each spec using Razubi (matthewmanning278@gmail.com).

---

## Specs

Specs live in `specs/` as markdown files. Pick the highest priority incomplete spec (lower number = higher priority). A spec is incomplete if it lacks `## Status: COMPLETE`.

Spec template: https://raw.githubusercontent.com/github/spec-kit/refs/heads/main/templates/spec-template.md

When all specs are complete, re-verify a random one before signaling done.

---

## NR_OF_TRIES

Track attempts per spec via `<!-- NR_OF_TRIES: N -->` at the bottom of the spec file. Increment each attempt. At 10+, the spec is too hard — split it into smaller specs.

---

## History

Append a 1-line summary to `history.md` after each spec completion. For details, create `history/YYYY-MM-DD--spec-name.md` with lessons learned, decisions made, and issues encountered. Check history before starting work on any spec.

---

## Completion Signal

All acceptance criteria verified, tests pass, changes committed and pushed — output `<promise>DONE</promise>`. Never output this until truly complete.

---

## GitHub Issues

Work on issues from `Razubi/Alcagent` in addition to specs. Use `gh` CLI:
  gh issue list --repo Razubi/Alcagent --state open
  gh issue close <number> --repo Razubi/Alcagent

Only work on issues approved by the project owner (Razubi).

---

## Completion Logs

After each spec, create `completion_log/YYYY-MM-DD--HH-MM-SS--spec-name.md` with a brief summary.

---

**Created:** 2026-11-26
**Ralph Wiggum version:** 3f15f0fb83b8c2e0ac8d11abdae0e83ab8204981