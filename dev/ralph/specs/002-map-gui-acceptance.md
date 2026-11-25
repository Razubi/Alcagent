# Spec 002 â€” R0 map + GUI/TUI acceptance

Status: pending

## Requirements
- Both frontends render a real spatial map from `sumo::MapFrame`, not a facility table.
- The egui GUI is a buildable workspace member with all 11 observation views.
- Critical panels, help overlay, and tab bar are legible at target sizes.

## Acceptance criteria
- [ ] TUI map shows facilities, prisoner/guard pawns, nav edges, locked nodes, incident overlays, occupancy, and selected pawn/facility detail.
- [ ] GUI map has pan/zoom, hover/click detail, cell/wing rosters, capacity/utilization bars, legend, fit/reset.
- [ ] Both frontends expose 11 views and route control mutations through validated `Sim`/`SimDriver` APIs.
- [ ] Launch the GUI with the game-development skill and inspect at least one screenshot; no clipped critical panels.
- [ ] `cargo build --workspace --all-targets` and `cargo test --workspace` pass.
- [ ] Docs reflect map/GUI as implemented.

Completion signal: `<promise>DONE</promise>`

