# Alcagent 0.05 — Overnight Implementation Plan

This plan is consumed by the Ralph loop. Each spec in `specs/` has one `Status:` line and testable acceptance criteria.

## Dependency order

1. Baseline green + warning-free build.
2. Verify and accept R0 (TUI/GUI real map, clipped-panel fixes).
3. Harden already-implemented 0.05 systems (persistence, audit, rehab, contraband, post-release, construction, parole).
4. Add remaining systemic depth (emergent events, gang politics, guard autonomy, warden strategy, AI orchestration, social-network analysis).
5. Long-run soak, performance, and release docs.

## Spec index

| Spec | Title | Depends on |
|---|---|---|
| 001 | Baseline green | — |
| 002 | R0 map + GUI/TUI acceptance | 001 |
| 003 | Persistence durability | 001 |
| 004 | Incident + decision audit depth | 001 |
| 005 | Rehab / contraband / post-release lifecycles | 003 |
| 006 | Policy / construction / parole controls | 003 |
| 007 | Emergent event system | 002 |
| 008 | Gang politics and social structure | 007 |
| 009 | Guard autonomy and reporting | 007 |
| 010 | Warden institutional strategy | 006 |
| 011 | AI orchestration and decision batching | 004 |
| 012 | Social-network analysis | 008 |
| 013 | Long-run soak and performance | 011, 012 |
| 014 | Release documentation and changelog | 013 |

## Definition of 0.05

0.05 is reached when every spec is `done`, the workspace builds warning-free, all tests pass, a 365+ day soak is stable, and both TUI and GUI render the real spatial map from `sumo::MapFrame`.
