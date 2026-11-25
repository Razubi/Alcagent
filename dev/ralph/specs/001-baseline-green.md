# Spec 001 — Baseline green and warning-free

Status: done

## Requirements
- Workspace builds with no compiler or manifest warnings.
- Targeted suites for the newly integrated 0.05 work pass.

## Acceptance criteria
- [x] `cargo build --workspace --all-targets` succeeds with zero warnings.
- [x] `cargo test -p alc-ui -p alc-gui -p alc-persistence` passes.
- [x] Integration commits use `Razubi <matthewmanning278@gmail.com>`.

Completion signal: `<promise>DONE</promise>`
