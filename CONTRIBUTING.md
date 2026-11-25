# Contributing to Project Alcagent

Thanks for helping build a living prison society. This document covers the workflow, code
conventions, testing and documentation expectations. The full product specification is
[`agents.md`](agents.md) (the master directive).

## Ground rules

- **No unsafe code.** `unsafe_code = "forbid"` is set at the workspace level.
- **Determinism is a feature.** All randomness must flow through `alc_core::RngStream`,
  seeded from the world seed and an entity/time-derived id. Never use wall-clock time or
  `HashMap` iteration order for simulation decisions — sort keys first.
- **The engine owns mutations.** Application/UI code goes through `sumo::Sim`. Agents emit
  decisions; `sumo` validates and applies them. Do not mutate `alc_world::World` directly from
  UI or agent code.
- **Events carry causes.** Every `WorldEvent` must have a `summary` and, where meaningful,
  `causes` explaining why it happened (AGENTS.md §17).
- **Agents never see everything.** Preserve the information model (direct knowledge, rumors,
  suspicion, belief) and fog-of-war: the warden reads reports through its inbox, not global state.

## Development workflow (AGENTS.md §24)

1. **Investigate** — read the relevant design doc under `docs/Design/` and system
   contract under `docs/Systems/`, plus the current code. Use subagents for large audits.
2. **Design** — add/update a design or planning doc before large changes; note dependencies.
3. **Implement** — small, incremental changes with the crate's file ownership respected. Coordinate
   so two workers never edit the same file simultaneously.
4. **Test** — add or update tests under `tests/` or `#[cfg(test)]` modules; run the suite.
5. **Review** — self-review, then independent review via subagents for significant features.
6. **Document** — update user docs (`README.md`, `docs/`) and internal docs
   (`docs/Project/PROJECT.md`, `design-status.md`, `version-milestones.md`,
   `CHANGELOG.md`) in the same change. Stale docs are treated as defects.

## Build and test

```powershell
cargo build --workspace
cargo test --workspace
cargo run -q -- run 365            # headless smoke / soak
cargo test -p sumo --test chaos_soak -- --nocapture   # 730-day, 3 seeds
cargo test -p sumo --test integration_wave4           # end-to-end engine flow
```

Current baseline (2026-11-25): 439 passed, 0 failed, 1 ignored (the 10k-entry log stress test).

## Crate map (14 workspace members)

| Crate | Owns |
|---|---|
| `alc-core` | `SimTime`, `TimeSpeed`, `RngStream`, IDs, capabilities, config |
| `alc-world` | `World` state and the main `tick()` loop (needs drift, schedules, encounters, day boundary) |
| `alc-prisoner` | Prisoner model, 29 needs, schedules, intake |
| `alc-social` | Relationships, gangs, conflict, escape, parole board, rehab, contraband |
| `alc-economy` | `Money`, ledger, funding, expenses, health |
| `alc-warden` | Warden traits/arbitration, expansion catalog, finance planning |
| `alc-guard` | Guard agents, posts, ranks, authority, duty loop, reports |
| `alc-events` | EventBus, WorldEvent, EventKind taxonomy |
| `alc-agents` | Decision catalog, validation, memory, orchestration |
| `alc-persistence` | Versioned bincode saves, migration, autosave |
| `alc-testing` | Invariants, soak helpers |
| `alc-ui` | Ratatui observer (read-only) |
| `sumo` | **Public engine API** — the integration surface |
| `alcagent` | CLI binary |

`crates/alc-gui` is an incomplete experiment and **not** a workspace member. Do not build on it
without first completing its missing modules and adding it to `Cargo.toml`.

## Code style

- Rustfmt + clippy with workspace lints (`clippy::pedantic`, `clippy::nursery` are warnings).
- Public items need doc comments — `sumo` denies `missing_docs` and `unused_crate_dependencies`.
- Prefer small, typed interfaces; keep serialization stable (`#[serde(default)]` on new fields,
  never reorder `EventKind` variants — append instead, as with `Dialogue`/`Interaction`).
- Use `RngStream::new(world.seed, id ^ ... ^ SALT)` for entity-scoped rolls.

## Adding a feature

1. Where does it live? Pure simulation logic → an `alc-*` crate. Public control surface → `sumo`.
2. Does it need new state? Add it to `World` or `EngineState` with `#[serde(default)]`, bump
   `SAVE_FORMAT_VERSION` only if the binary layout changes incompatibly, and add migration.
3. Does it need an event? Add an `EventKind` at the end and document emit points in
   [`docs/Systems/event-contract.md`](docs/Systems/event-contract.md).
4. Does it need an agent action? Add a `Decision` variant plus validation rules and engine
   application, then document it in [`docs/SUMO_API.md`](docs/SUMO_API.md) and
   [`docs/Systems/engine-api.md`](docs/Systems/engine-api.md).
5. Add tests: unit, integration, and an invariant/soak check for long-run stability.

## Reporting issues

Include: seed, command, simulation day, relevant event summaries from `world.debug.json` or the
headless report, and whether the issue reproduces with a fresh world. Determinism makes most
bugs reproducible from a seed alone.

## License

By contributing you agree that your contributions are licensed under the MIT license
(see [`LICENSE`](LICENSE)).
