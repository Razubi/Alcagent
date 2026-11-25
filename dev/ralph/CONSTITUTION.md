# Alcagent 0.05 Constitution

All autonomous development sessions must obey this document.

## Goal

Ship **Alcagent 0.05 — The Living Institution**: a persistent, autonomous prison society with a real spatial map, buildable egui GUI, polished ratatui TUI, validated engine controls, and deterministic long-run stability.

## Non-negotiable priorities

1. **Performance and speed first.** Prefer O(n) frame work, cached snapshots, bounded logs, and no per-frame heavy allocation.
2. **Clean code second.** Small modules, documented interfaces, no dead dependencies, no compiler warnings.
3. **Correctness third.** Deterministic simulation, validated state transitions, save/load fidelity, no raw world mutation from UI.
4. **Features fourth.** Add systemic, emergent features only when their foundations exist.

## Authority boundary

- `sumo::Sim` is the single authoritative engine.
- `alc-ui` and `alc-gui` render read-only DTOs/summaries and call validated `Sim`/`SimDriver` controls.
- No frontend may mutate `World` directly.
- No new egui/eframe dependency in `sumo`; the GUI lives in `alc-gui`.

## Validation gate

A spec is complete only when all of the following pass:

```powershell
cargo build --workspace --all-targets
cargo test --workspace
```

Plus every spec-specific acceptance check. Long-run specs must include a soak of at least 365 simulated days.

## Commit policy

- Use the configured repo identity: `Razubi <matthewmanning278@gmail.com>`.
- Commit each completed spec with a conventional commit message.
- Never push, force-push, reset, or rewrite history.
- Never create temporary validation crates or repoint workspace dependencies to shims.
- Append a short entry to `dev/ralph/history.txt` before finishing.

## Completion signal

Output exactly one of:

- `<promise>DONE</promise>` — spec fully complete, validated, status set to `done`, changes committed.
- `<promise>BLOCKED</promise>` — spec cannot be completed; explain why in `dev/ralph/history.txt`.

Never output `DONE` unless every acceptance criterion is verified.
