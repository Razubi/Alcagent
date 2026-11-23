# Testing harness and long-duration soak results

Owner: Testing/Review agent. Editable paths: `crates/sumo/tests/**`,
`crates/alc-testing/**`, this file. Library crates are owned by other agents;
defects found there are reported, not patched.

## Running the suite

```powershell
# Fast scope (does not depend on the GUI crate, which other agents may be editing)
cargo test -p sumo -p alc-testing

# Full workspace (only green when every crate is mid-edit free)
cargo test

# Soak evidence with per-seed output
cargo test -p sumo --test chaos_soak -- --nocapture

# Binary smoke at the same horizon
cargo run -q -- --no-tui run 730
```

## Tests added by the Testing/Review agent

### `crates/sumo/tests/chaos_soak.rs`

| Test | Assertions |
|---|---|
| `chaos_soak_730_days_three_seeds` | 730 days at `TimeSpeed::Extreme` from 3 seeds; `days_simulated >= 729`; `Sim::check_invariants()` clean; population in `[0, capacity]`; every stored need finite and in `[0,1]` (plus `total_pressure()`); `economy.cash.as_major()` and `economy.health()` finite; `events().len() <= 50_000`. All three seeds are summarised before any assertion fires. |
| `same_seed_reproduces_event_count_population_and_trace` | Two identical 10-day runs from the same seed produce identical population, event count and FNV `trace_hash`. |
| `different_seeds_produce_different_event_counts` | Seeds 1 and 2 produce different 10-day event counts and trace hashes (10 days is before bus saturation, so the count is a meaningful signal). |
| `save_load_after_year_continues_invariant_clean_and_deterministic` | 365-day run → save → load → `set_speed(Extreme)` on the loaded copy → 30 more days on both; both invariant-clean; clock, day counter, population, cash, event count and trace hash identical (loaded worlds replay rather than merely survive). |

### `crates/alc-testing/src/lib.rs`

| Test | Assertions |
|---|---|
| `deadlock_invariant_flags_non_finite_prisoner_need` | Injects `f32::NAN` into a prisoner's hunger and verifies both `invariant_no_deadlock` and `check_world_invariants` report `no_deadlock` — validates the NaN detector the soak tests rely on. |

## 2-year soak results (recorded run)

`cargo test -p sumo --test chaos_soak -- --nocapture` at the time of writing:

| Seed | Days | Population | Cash (cents) | Security | Lockdown | Events | Invariants |
|---|---:|---:|---:|---:|:---:|---:|---|
| `0xA1C_0A_6E_17_0` | 730 | 4 / 80 | -9,965,200 | 1.00 | true | 50,000 | **FAIL**: `economy.cash is negative` |
| `1` | 730 | 4 / 80 | -11,022,400 | 1.00 | true | 50,000 | **FAIL**: `economy.cash is negative` |
| `2026` | 730 | 5 / 80 | -7,762,000 | 1.00 | true | 50,000 | **FAIL**: `economy.cash is negative` |

All other assertions (needs, budget finiteness, bus bound, population bound,
day count) pass for all three seeds. The determinism and save/load tests pass.

## Defects found in crates owned by other agents (NOT fixed here)

1. **Permanent lockdown + security ratchet → population collapse and insolvency.**
   Repro: seed `0xA1C_0A_6E_17_0`, Extreme speed. Security climbs `0.60 → 1.00`
   by day 5 (warden returns `IncreaseSecurity` whenever weekly incidents > 5);
   lockdown flips true by day 10 after an escape consequence and is **never**
   reset anywhere in the codebase (`grep 'lockdown = false'` → no matches).
   `accept_intake_step` then refuses all admissions (lockdown or security >= 0.9),
   population falls 25 → 4, while four guards' `$140/day` payroll continues.
   Cash crosses zero on day 554 (+374 → -156 cents) and is `-9,965,200` cents
   by day 730. `Sim::check_invariants()` then reports `economy.cash is negative`.
   Relevant sites: `alc-warden/src/lib.rs:252-255`, `alc-world/src/lib.rs:768-770`,
   `alc-world/src/lib.rs:863-865`, `alc-economy/src/lib.rs:114-121`,
   `sumo/src/lib.rs:199-201`.

2. **Event bus flooded by guard security reports.** By day ~365 the 50,000-entry
   bus is saturated and ~70% of the history is `SecurityResponse` ("notable
   activity observed (salience 1.00)") emitted hourly by every guard. Meaningful
   social/parole events are evicted within ~1.5 simulated years. Contributing
   sites: `alc-guard/src/lib.rs:428-433` (incident load floored by security +
   lockdown + risk profile) and `alc-world/src/lib.rs:260-324` (every severity
   >= 2 report becomes a bus event). The 12-hour escalation cooldown helps
   assistance spam but routine severity-2 reports still flood the bus.

3. **`SimTime` display emits malformed clock strings.** `SimTime::Display`
   prints `minute_of_day()` (range 0..1439) as the minute field, producing e.g.
   `Day 0364 23:1380` for 23:00. Repro: any `Sim::report()` or binary output
   line at a non-zero minute-of-day. Site: `alc-core/src/simtime.rs:41`.

4. **Harness note (this crate, low priority):** `invariant_money_integral(cents)`
   is `cents % 1 == 0`, which is always true for `i64`. Integrality is a
   type-level guarantee; the check is a no-op. Kept as-is for API compatibility,
   documented here rather than changed.

## Full-suite status at time of writing

`cargo test --workspace --exclude alc-gui --no-fail-fast`: **134 tests run,
133 passed, 1 failed** (only `chaos_soak_730_days_three_seeds`, defect 1).
A full `cargo test` / `cargo run` from the workspace root was blocked because
the UI agent was mid-edit on `alc-gui` (missing modules `app`, `panels`,
`screens`, `settings`, `theme` plus two type errors). Retried the build five
times over ~4 minutes; scoped the binary smoke to
`cargo run -q -p alcagent -- --no-tui run 730`, which completed in ~2 s with
no panic and reproduced the soak end state (population 4, cash -99,652.00,
50,000 events).

## Notes on flakiness

All tests use fixed seeds and time/id-derived RNG streams. The save/load test
uses a unique temp dir per process id and removes it after the assertions. The
soak test deliberately asserts healthy behaviour and therefore fails while
defect 1 is unfixed; that failure is deterministic and is the regression signal,
not flakiness.
