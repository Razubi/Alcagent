# Spec 008 — Gang politics and social structure

Status: done

## Requirements
Gangs must be dynamic organizations with internal disagreement, not monolithic blocks.

## Acceptance criteria
- [x] Gangs can form, recruit, lose members, fracture, change leadership, and form/rival alliances based on relationships and needs.
- [x] Internal member motivations can disagree with leadership and produce detectable disputes.
- [x] Gang history records leadership changes, mergers/splits, and major conflicts.
- [x] Gang changes influence security, contraband, and incident generation.
- [x] Expose meaningful gang detail in both frontends.
- [x] Add deterministic tests; full workspace passes.

Validation (2026-11-25): `cargo test -p alc-social --test gang_politics_008` 8/8,
`cargo test -p alc-world --test gang_politics_008` 7/7, `cargo test -p sumo --test gang_politics_008`
5/5, both frontend suites green, and the full workspace gate green after integration.

Completion signal: `<promise>DONE</promise>`
