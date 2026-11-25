# Spec 014 — Release documentation and changelog

Status: done

## Requirements
Final 0.05 release state must be discoverable and accurate.

## Acceptance criteria
- [x] Version reconciled to `0.5.0-beta.1` or documented exception approved in history.
- [x] Root CHANGELOG.md updated with all 0.05 features and fixes.
- [x] Project status/milestone/design docs no longer call UI/GUI/map blockers.
- [x] API docs match actual save format, speed, autosave, and GUI wiring.
- [x] Final `cargo build --workspace --all-targets` and `cargo test --workspace` results recorded.
- [x] Every spec in dev/ralph/specs is `done` or `blocked` with justification.

Validation (2026-11-25): all 14 specs are `done`; `cargo fmt --all -- --check`,
`cargo build --workspace --all-targets` and `cargo test --workspace` are green (439 passed,
1 intentionally ignored, 0 failed). Save format remains v5.

Completion signal: `<promise>DONE</promise>`
