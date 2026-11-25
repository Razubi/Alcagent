# Beta 0.05 — Final Integration Report (2026-11-20)

## 1. Build / test status

| Command | Exit code | Result |
|---|---|---|
| `cargo build --workspace` | 0 | Finished dev profile; warnings only (unused deps). |
| `cargo test --workspace` | 0 | 136 tests passed, 0 failed, 0 ignored (across alc-agents, alc-core, alc-events, alc-guard, alc-social, alc-ui, alc-warden, alc-world, sumo units + integration tests). |

Notes:
- `alc-gui` is **not** a workspace member, so it is excluded from both commands. It does not compile standalone (missing modules + missing `theme`).

## 2. What was implemented per subagent

- **GUI data layer** (`crates/alc-gui/src/data.rs`): pure extraction for incident buckets, live feed (capped ring), policy knobs, guard roster, daily schedule, stats dashboard, leaderboard, needs rows. Tests: 10+ (inside data.rs, not compiled by workspace).
- **Capabilities** (`crates/alc-core/src/capability.rs`): 23-kind catalog + trait delta table + tests.
- **Contraband** (`crates/alc-social/src/contraband.rs`): market + discovery + tests.
- **Rehab** (`crates/alc-social/src/rehab.rs`): program state machine + tests.
- **Schedule/placement** (`crates/alc-world`): `resolve_schedules_step`, `prisoner_location`, `schedule_policy`.
- **Engine rehab** (`crates/sumo`): `apply_rehab_progress`, `rehab_score`.

## 3. Integration issues and disposition

1. `alc-gui` not in workspace + missing modules (`app`, `panels`, `screens`, `settings`, `theme`) and `data.rs` depends on `crate::theme`. **Disposition:** report; not fixed (docs-only/integration pass scope; requires GUI owner).
2. `alc-gui::data::policy_change_action` emits `ChangePolicy { policy: "lockdown" }`, but `sumo::drivers::apply_warden_action` has no `"lockdown"` arm → no-op at engine level. **Disposition:** report; engine owner to add arm or GUI to set flag via `world_mut`.
3. Capabilities/contraband/rehab libraries are not wired into `Prisoner`/`World`/`Sim` (no fields, no steps, no events). **Disposition:** deferred; contracts documented as planned.
4. `World::history` is never appended (dead field); `EventBus` is the live log. **Disposition:** documented; no change.
5. `Sim::load` resets parole ledger + population history (persistence gap). **Disposition:** documented in parole-pipeline.md.
6. `engine_rules` catch-all `_ => ok()` accepts unhandled decisions. **Disposition:** documented; engine owner to tighten.
7. Guard duty double-driving (`World::tick` vs `Sim::guard_duty_cycle`). **Disposition:** documented.

## 4. Docs changed

- `../Project/design-status.md` — beta-0.05 status section + test count.
- `../Project/version-milestones.md` — current version/test count note.
- `../Systems/capabilities.md`, `rehab.md`, `engine-api.md` — status notes.
- `CHANGELOG.md` — created with beta-0.05 entry.
- `../Reports/beta-0.05-integration.md` — this report.

## 5. Remaining gaps (severity-ranked)

**High**
- GUI crate non-buildable / not in workspace (blocks headline feature).
- Engine `ChangePolicy` lacks lockdown arm (policy panel ineffective).
- R1 (incident log), R2 (decision audit), R4 (post-release), R5 (decision batching) not implemented.

**Medium**
- H1 policy mgmt partially (no set_policy/set_lockdown APIs).
- H2 expansion lifecycle (`under_construction`) not implemented.
- H3 stats gaps (deaths/transfers/guard injuries/contraband discoveries/policy changes).
- H4 contraband not wired to World.
- H5 KnowledgeStore still stub.
- H8 finance dashboard not wired to World.
- N1 `--ui` flag, N2 `World::summary`, N3 guard duty unification, N5 decision-log viewer not implemented.

**Low**
- N6 superseded-doc cleanup.
- Capabilities not applied to `Prisoner` traits.
- Rehab Sim API simplified (does not use `alc_social::rehab`).

## 6. Recommended verification commands

```
cargo build --workspace
cargo test -p sumo
cargo test --workspace
cargo run -p alcagent -- --no-tui run 730
```

Soak: `cargo test -p sumo --test soak` (730-day run) and `cargo test -p sumo --test chaos_soak`.