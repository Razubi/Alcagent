# Alcagent — Gameplay, Systems & Controls

This is the product-side manual for **Project Alcagent**: a deterministic, autonomous prison
simulation you can watch evolve or run headlessly. It documents *what happens in the world* and
*how to observe/control it*. The engine that powers all of this is `sumo` — see
[`SUMO_ENGINE.md`](../sumo/SUMO_ENGINE.md) for its design and [`SUMO_API.md`](../sumo/SUMO_API.md) to embed or
extend the simulation in Rust. Engine docs are maintained under `docs/sumo/` in this repository.

> **Status (2026-11-24):** the base simulation is stable (last green baseline: 184 tests passed,
> 0 failed, 1 ignored). A new gameplay wave is landing: policy controls, queued construction,
> incident/decision audit views, and daily rehab/contraband steps. Features marked **in-flight**
> exist in the working tree but may change before release.

---

## 1. The game in one paragraph

You are observing a living prison. There is no direct "play" loop where you move a character;
instead you run a deterministic world and watch prisoners, guards and a warden make their own
choices. Every prisoner has needs, traits, a criminal history, goals and relationships. Time
advances in ticks; each tick drifts needs, assigns schedules, lets agents choose actions,
resolves encounters, and (at day boundaries) runs the institution's economy, disciplinary and
parole processes. The same seed always produces the same institution, so a world can be shared,
replayed and debugged like a replay file in a strategy game.

**Core fantasy:** *watch a society emerge and evolve under institutional pressure.*

---

## 2. Getting started / controls

### 2.1 Command line

```powershell
cargo build --workspace

cargo run --                 # interactive TUI (default)
cargo run -- observe         # same, explicitly
cargo run -- observe --no-tui # 30-day headless report instead of TUI
cargo run -- run 365         # headless 1-year simulation + report + save
                             # (`run` with no number defaults to 30 days)
cargo run -- --help          # full help (flags and keybindings)
cargo run -- docs            # one-line documentation pointer
```

| Flag | Default | Effect |
|---|---|---|
| `--seed <u64>` | `0xA1C_0A_6E_17_0` | World-generation seed; same seed ⇒ same institution. Ignored when loading a save. |
| `--speed <u32>` | `60` | TUI tick rate (ticks/second, clamped 1–4000). Ignored by headless `run`. |
| `--no-tui` | off | `observe` becomes a 30-day headless report (CI-friendly). |
| `--load <DIR>` | none | Resume from a save directory; overrides `--seed`. |
| `--save-dir <DIR>` | `saves/default` | Where saves are written. |
| `--autosave-days <N>` | `7` | Autosave cadence in simulated days; `0` disables. |
| `RUST_LOG=sumo=debug` | unset | Enables engine trace logging (PowerShell: `$env:RUST_LOG='sumo=debug'`). |

Headless `run N` adds `N` days to the world's current day (when loading) and **always saves** on
completion. The printed `Ticks/sec` is actually days/second (legacy label).

### 2.2 TUI keybindings (complete)

| Key | Action |
|---|---|
| `q`, `Q`, `Esc`, `Ctrl-C` | Quit (terminal is always restored). |
| `Space` | Pause / resume the observer tick loop (not policy mode). |
| `+` / `=` | Double tick speed (max 4000). |
| `-` / `_` | Halve tick speed (min 1). |
| `1`–`9` | Switch observation view (see §2.3). |
| `0`, `i` / `I` | Incident Reports view. |
| `a` / `A` | Decision Audit view. |
| `↑ ↓ ← →` / `k j h l` | Move the selection cursor (held keys repeat). |
| `s` / `S` | Manual save to `--save-dir`. |
| `Ctrl-L` | Quick load from `--save-dir`. |
| `x` / `X`, `L` / `U` | Toggle / enable / lift lockdown. |
| `p` / `P` | Cycle policy mode: Deterministic → Council → Shadow. |
| `N` / `E` / `K` | Policy band: Normal / Enhanced / Lockdown. |
| `[` / `]` | Security level −/+ 0.05. |
| `,` / `.` | Privileges −/+ 0.05. |
| `;` / `'` | Rehab funding −/+ 0.1. |
| `b` / `B` | Toggle manual intake pause. |
| `t` / `T` | Cycle selected rehab program (Prisoner/Warden views). |
| `r` / `R` | Enroll / drop selected prisoner from rehab program. |
| `f` / `F` | Cycle / resolve selected contraband case (Incident Reports). |
| `m` / `M` | Cycle incident severity filter (all → high → critical). |
| `o` / `O` | Cycle incident resolved filter (all → open → resolved). |
| `d` / `D` | Cycle decision validation filter (all → accepted → repaired → rejected). |

The in-app help bar shows common controls; the full policy panel lives in the Warden view.
A red `LOCKDOWN` badge appears in the status bar when lockdown is active. (A `UiState.search`
field exists in code but has no bound key, so search is not a usable control.)

> **In-flight (2026-11-24):** the policy keys, `0`/`A` views, rehab/contraband controls and
> construction table are in the working tree but the gameplay wave has not landed as a stable
> release — the tree may not compile while mid-edit.

### 2.3 Observation views (9 core + 2 audit)

| Key | View | What it reveals |
|---|---|---|
| 1 | Institution Overview | Time, population/capacity, guards, facilities, gangs, event count, security/privileges/economy bars, lockdown, recent events, facilities table. |
| 2 | Prisoner | Selectable list + profile (sentence, class, gang, reputation), 12 personality traits, 25 need-deficit bars, goals, schedule, skills, memory summary. |
| 3 | Gang | Leader, members, hierarchy, reputation/resources, culture, rivalries, alliances, disputes, goals. |
| 4 | Staff | Guard roster, rank/post/authority/shift, duty condition (competence, morale, fatigue), disposition and duty log. |
| 5 | Warden | Philosophy, 11 traits, attention, inbox/report count, pending actions; **in-flight** policy-control panel. |
| 6 | Financial | Treasury, health index, per-prisoner funding, crowding threshold, paged daily ledger. |
| 7 | Event Timeline | Newest-first events with causes (`a → b`) and confidence. |
| 8 | Social Network | Relationship dyads ranked by bond strength, with trust/fear/hostility/rivalry. |
| 9 | Prison Map | ASCII schematic of facilities by kind and condition, guard posts; **in-flight** construction queue table. |
| 0 / A | Incident Reports / Decision Audit (**in-flight**) | Structured incidents (severity, OPEN/RES, cause/consequence) and audited decisions (actor, source, validation, reason). |

Views are read-only over engine snapshots and remain stable on sparse or 2,000-prisoner worlds.

---

## 3. Gameplay systems (designs & observable effects)

### 3.1 Time, schedule and daily rhythm

- A tick advances 1/5/30 simulated minutes depending on speed (`RealTime`/`FastForward`/`Extreme`).
- Every tick: needs drift → hourly guard sweep → schedule resolution → prisoner decisions →
  co-presence encounters → (on day boundary) economy, sentences, escape/parole, warden, intake,
  gangs, and in-flight rehab/contraband/construction steps.
- Schedules assign activities (wake, meals, work, yard, education, recreation, medical, showers,
  visits, lockdown, sleep) gated by classification, wing, privileges, discipline, staffing,
  weather and lockdown. Prisoners can be late, skip work, socialize or break rules.
- **Watch it in:** TUI views 1, 2 and 7.

### 3.2 Needs and psychology

29 need-deficit pressures: physical (hunger, thirst, sleep, fatigue, hygiene, medical, exercise,
temperature, bathroom), psychological (safety, companionship, respect, freedom, autonomy,
purpose, stimulation, stability, privacy, belonging) and social (friendship, family contact,
group acceptance, status, protection, recognition). High values mean *unmet* need.

Derived behaviors drive decisions: hunger → irritability, exhaustion → low alertness, isolation →
desperation, safety deficit → gang-seeking, stimulation deficit → rule-breaking, low hope →
reduced rehab motivation, medical neglect → illness risk.

### 3.3 Prisoner AI and decisions

Each prisoner observes their facility, evaluates needs/goals/memories and ranks 13 possible
actions: `MoveTo`, `Socialize`, `Trade`, `Fight`, `Comply`, `Refuse`, `EscapeAttempt`,
`RequestMedical`, `JoinGang`, `LeaveGang`, `LeadGang`, `SpreadRumor`, `None`. Personality traits
bias the ranking (`Disposition`). AI deliberation is token/rate-gated; a deterministic fallback
keeps the simulation moving when the budget is exhausted.

Decisions pass an 8-stage validation pipeline (format → semantic → authority → world-state →
resource → legality → safety → clamp/repair). The engine — never the agent — applies them.

### 3.4 Social relationships, gangs and roles

- Relationships track 12 dimensions (trust, friendship, fear, respect, loyalty, hostility,
  attraction, resentment, dependency, rivalry, debt, gratitude) and evolve through co-presence
  encounters. Prisoners remember who helped, betrayed, humiliated or protected them.
- Social-network detection identifies friends, rivals, leaders, influencers, isolates, brokers,
  informants and victims.
- Gangs **emerge** weekly from clusters of unaffiliated prisoners with protection needs, shared
  backgrounds and ambition. They have leaders, hierarchies, reputation, resources, rivalries,
  alliances and internal disputes; members can challenge leaders and gangs fracture when members
  leave.
- **Watch it in:** views 2, 3 and 8.

### 3.5 Conflict, discipline and security

Arguments, threats, bullying, theft, fights and gang clashes arise from tension, disrespect,
scarcity and rivalries. Outcomes consider personality, emotional state, physical condition,
staff presence and policy; consequences include injuries, disciplinary status, reputation loss,
security policy changes and investigations. Guards have credibility-scored reports, authority
levels, shift rotation, fatigue and stress, and request assistance when overwhelmed.

### 3.6 Escape attempts

Prisoners with high escape motivation (fear, desperation, long sentence, perceived injustice)
can roll a daily abstract attempt. Security level, staff coverage, determination and chance
decide the outcome: prevented, discovered, disruption, discipline, policy change, or a successful
escape (the prisoner leaves the world permanently, with institutional consequences).

### 3.7 Economy and finance

The prison receives per-prisoner funding plus grants; pays food, utilities, salaries, medical,
maintenance and programs. `Money` is integer minor units. Economic health drives warden decisions
(cuts when poor). Sustainability guards include a base grant, security de-escalation after quiet
weeks, lockdown expiry, staff right-sizing and emergency funding with a privilege cost.

### 3.8 Warden AI and policy

The warden balances security, finances, morale, welfare, reputation, stability, rehabilitation,
escape/violence reduction, political pressure and expansion, acting under fog of war (an inbox of
guard reports). Modes: **Deterministic** (historical arbitration), **Council** (deterministic
drive vote with rationales) and **Shadow** (runs both, applies baseline, logs divergence).
Policies: Normal/Enhanced/Lockdown bands, security/privileges/rehab-funding levels and intake
pause. **In-flight:** these are directly controllable from the TUI (§2.2).

### 3.9 Expansion and construction (**in-flight**)

Warden `Expand` actions no longer add a facility instantly: they **queue a construction project**
(7–14 days, with a cost charged to the economy ledger). The queue advances daily; completion
adds the facility and emits `ConstructionComplete`. Progress appears in the map view.

### 3.10 Rehabilitation, contraband and post-release (**in-flight**)

A daily rehab step auto-enrolls motivated, non-disciplinary prisoners in Education, Vocational,
Counseling, AddictionRecovery or AngerManagement; progress changes skills, hope, mental/physical
health, aggression and needs, emitting `ProgramEnrolled`/`RehabilitationSuccess`. A contraband
step runs a market and discovery process; discovered holders gain disciplinary status and lose
reputation while security tightens. Post-release tracking records reintegration outcomes.

### 3.11 Parole and population

Prisoners become eligible at 60% of sentence with clean discipline. The world's daily parole
process scores behavior, rehab progress and residual risk and grants or denies release. Releases
update gangs and history. Intake admits new correlated prisoners every three days when there is
spare capacity and policy permits (blocked by lockdown, high security or manual pause).

### 3.12 Events, memory and history

Every meaningful change is an event with a summary, causes, actors and confidence, stored in a
bounded (50k) bus plus engine incident/decision audit logs. Institutional history and per-agent
memories (short-term, long-term, institutional layers with importance/decay/consolidation) give
the world a persistent past. **In-flight:** structured `IncidentRecord`s and `DecisionAuditRecord`s
expose causality and actor/source/validation for debugging.

---

## 4. Saves, replay and reproducibility

- `saves/<dir>/world.bin` — versioned bincode (world + engine state); `world.debug.json` —
  human-readable sidecar. Format version is currently **3**.
- Autosave cadence is configurable; `s` saves manually; headless runs always save.
- Load with `--load <dir>`; the save's seed wins. Loaded worlds can be continued and replayed
  deterministically; `trace_hash()` / `trace_hash_v2()` fingerprints verify equality.
- The same `--seed` reproduces the same institution from a fresh world — share seeds to share
  scenarios.

---

## 5. Support matrix and troubleshooting

| Symptom | Fix |
|---|---|
| Garbled TUI / arrow keys print `^[[A` | Use Windows Terminal/modern console; CI should use `--no-tui`. |
| Wrong binary built | Use `cargo run -p alcagent -- <args>`. |
| "No save found" on `--load` | Point at a *directory* containing `world.bin`. |
| Different results with same seed | You may have loaded a save (its seed overrides the CLI value). |
| Terminal stuck in raw mode after crash | Open a fresh shell; normal shutdown always restores the terminal. |
| Old save won't load | Save format is v3; regenerate or migrate older saves. |

---

## 6. See also

- [`ALCAGENT.md`](ALCAGENT.md) — CLI/TUI operator reference (flags, keys, views).
- [`SUMO_ENGINE.md`](../sumo/SUMO_ENGINE.md) — engine product & system design.
- [`SUMO_API.md`](../sumo/SUMO_API.md) — engine API guide.
- [`Design/`](Design/) — game system design documents (in this repo).
- [`Systems/`](Systems/) — game-domain interface contracts (in this repo).
- [`agents.md`](../../agents.md) — the master development directive.
