# Determinism (`Documentation/Systems/determinism.md`)

## Seeds and RNG (`alc_core::RngStream`)

All stochastic behavior flows through `RngStream` (a per-agent, serialized stream; each agent owns one stream and it is advanced only by that agent's own rolls). Examples of construction in the codebase:

- `RngStream::new(world.seed, self.time.as_secs() ^ (g.id.raw() << 8) ^ 0xA11CE)` — guard duty (`World::tick`).
- `RngStream::new(world.seed, world.time.secs)` — engine fight/escape resolutions.
- `RngStream::new(seed, id ^ days_simulated << 24 ^ 0x35C4B)` — health/guard/conflict rolls.
- `RngStream::new(world.seed, id)` — intake generation; `RngStream::fork(label)` derives child streams.

API: `new(root_seed: u64, id: u64)`, `fork(label)`, `next_u64() -> u64`, `next_f64() -> f64` (53-bit), `range(lo: f64, hi: f64) -> f64`, `chance(p: f64) -> bool`.

## Speed modes

`TimeSpeed`: `Paused`, `RealTime` (1 min/tick), `FastForward` (5), `Extreme` (30), `EventDriven` (1). `World::tick` advances `time` by the speed's `dt_mins`; a day boundary fires when `as_days()` increases; `Sim::run_until(target)` runs while `world.time < target`.

## Trace hash

`Sim::trace_hash()` — FNV-1a over `(time secs, population, recent event summaries)` — a replay fingerprint AI determinism/replay checks can compare across runs (Directive A22).

## Bootstrap determinism

`World::new(seed)` → the same world every time: bootstrap facilities and the 24 intake prisoners derive from the seed; guards are constructed with `RngStream::new(seed, id)`.

## Tests

`cargo test -p sumo` — engine_api, decisions, determinism, observation (world state, stats window, population), persistence, review_agent, soak. Determinism test: `cargo test -p sumo --test determinism`.

## Notes for AI agents

- Use `RngStream::new(world.seed, id ^ days_simulated << N ^ salt)` for entity-scoped rolls.
- Never use wall-clock time or iteration-order-dependent randomness in simulation logic.
- `Sim::trace_hash()` is a cheap equality check for "same sim state": same time, population, and recent event stream.
