# Alcagent — Gameplay, Systems & Controls

This is the product-side manual for **Project Alcagent**: a deterministic, autonomous prison
simulation you can watch evolve or run headlessly. It documents *what happens in the world* and
*how to observe/control it*. The engine that powers all of this is `sumo` — see
[`SUMO_ENGINE.md`](../sumo/SUMO_ENGINE.md) for its design and [`SUMO_API.md`](../sumo/SUMO_API.md) to embed or
extend the simulation in Rust. Engine docs are maintained under `docs/sumo/` in this repository.

> **Status (2026-11-25):** the 0.05 gameplay wave is stable and green: policy controls, queued
> construction, incident/decision audit views, rehab/contraband/post-release lifecycles, report-only
> warden strategy, autonomous guards, social roles and both spatial frontends are implemented.
> Launch is consolidated to one `alcagent` binary: the graphical observer is the default and
> `--tui` opens the keyboard-driven debug observer.

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

cargo run --                 # graphical observer at the Main Menu (default)
cargo run -- observe         # same, explicitly
cargo run -- --tui           # ratatui debug observer
cargo run -- run 365         # headless 1-year simulation + report + save
                             # (`run` with no number defaults to 30 days)
cargo run -- --help          # full flags (TUI keys are in the in-app ? overlay)
cargo run -- docs            # one-line documentation pointer
```

| Flag | Default | Effect |
|---|---|---|
| `--tui` | off | Open the ratatui debug observer instead of the graphical observer; key map is in its `?` help overlay. Observer path only; rejected by `run`/`docs`. |
| `--no-tui` | off | Legacy alias for a short headless 30-day report; not the main UI switch. Prefer `--tui` and `run`. Observer path only; rejected by `run`/`docs`. |
| `--seed <u64>` | saved preference / `0xA1C_0A_6E_17_0` | World-generation seed; same seed ⇒ same institution. Ignored when loading a save. GUI uses saved preference unless this flag is supplied. |
| `--speed <u32>` | saved preference / `60` | Absolute RealTime tick rate; CLI rejects values outside `1..=4000`, even with headless `run`. Used only by observers; GUI uses saved preference unless supplied. |
| `--load <DIR>` | none | Resume from a save directory; overrides `--seed`. In GUI mode the save opens directly; otherwise the Main Menu offers a save browser. |
| `--save-dir <DIR>` | saved preference / `saves/default` | Where saves are written. GUI uses saved preference unless this flag is supplied. |
| `--autosave-days <N>` | saved preference / `7` | Autosave cadence in simulated days; `0` disables. GUI uses saved preference unless this flag is supplied. |
| `RUST_LOG=sumo=debug` | unset | Enables engine trace logging (PowerShell: `$env:RUST_LOG='sumo=debug'`). |

Headless `run N` adds `N` days to the world's current day (when loading) and **always saves** on
completion. The run prints real elapsed time, tick count, requested/progressed days and the
engine report; there is no `Ticks/sec` label in the current output.

### 2.2 TUI keybindings (complete)

These keys apply only to the ratatui debug observer launched with `--tui`; they are also listed in
its in-app `?` help overlay. The default graphical observer is button/menu-only and has no hidden
keyboard shortcuts (see §2.4).

| Key | Action |
|---|---|
| `q`, `Q`, `Esc`, `Ctrl-C` | Quit (the terminal is always restored; `Esc` closes modal help/search first). |
| `Space` | Pause / resume the observer tick loop (not policy mode). |
| `+` / `=`, `-` / `_` | Speed up / slow down (max 4000, min 1). |
| `1`–`9`, `0`, `A` | Switch observation view (see §2.3); `0` = Incident Reports, `A` = Decision Audit. |
| `↑ ↓ ← →` / `h j k l` | Move the selection cursor (held keys repeat). |
| `/` | Free-text search; `Backspace` edits, `Enter` finishes, `Ctrl-W` clears the query. |
| `?` | Open modal help; `Esc` or `?` closes it. |
| `Tab` | Pin/unpin the selected prisoner, gang or guard. |
| `n` | Toggle the observer notices/pins feed. |
| `V` | Toggle auto-pause on newly seen critical incidents. |
| `Ctrl-R` | Resolve a critical incident via validated control. |
| `S` | Manual save to `--save-dir`. |
| `Ctrl-L` | Quick load from `--save-dir`. |
| `b`, `B` | Toggle manual intake pause. |
| `N`, `E`, `K` | Policy band: Normal / Enhanced / Lockdown. |
| `p` | Cycle policy mode: Deterministic → Council → Shadow. |
| `L`, `U` | Lock / unlock; `U` unlocks outside the Warden view. |
| `x`, `X` | Outside the Warden view, toggle lockdown. |
| `[` / `]` | Security level −/+. |
| `,` / `.` | Privileges −/+. |
| `;` / `'` | Rehab funding −/+. |
| Warden `c`, `C` | Cycle construction kind / queue the selected kind. |
| Warden `u`, `U` | Cycle construction project selection. |
| Warden `v`, `x`, `X`, `>` | Approve / cancel / fail / advance the selected project one day. |
| Prisoner `H`, `G`, `J`, `Y` | Schedule hearing / grant / deny / release on parole. |
| `t`, `r`, `R` | Rehab program cycle / enroll / drop. |
| `f`, `F` | Contraband case cycle / resolve (Incident Reports). |
| `m`, `o` | Incident severity / resolved-status filters (Incident Reports). |
| `d`, `D` | Decision validation filter (Decision Audit). |

The TUI help bar shows common controls; `?` opens the modal help and the policy panel lives in the
Warden view. A red `LOCKDOWN` badge appears in the status bar when lockdown is active. These
controls landed in the 0.05 release.

### 2.3 Observation views (9 core + 2 audit)

| Key | View | What it reveals |
|---|---|---|
| 1 | Institution Overview | Time, population/capacity, guards, facilities, gangs, event count, security/privileges/economy bars, lockdown, recent events, facilities table. |
| 2 | Prisoner | Selectable list + profile (sentence, class, gang, reputation), 12 personality traits, 25 need-deficit bars, goals, schedule, skills, memory summary. |
| 3 | Gang | Leader, members, hierarchy, reputation/resources, culture, rivalries, alliances, disputes, goals. |
| 4 | Staff | Guard roster, rank/post/authority/shift, duty condition (competence, morale, fatigue), disposition and duty log. |
| 5 | Warden | Philosophy, 11 traits, attention, inbox/report count, pending actions, latest strategy rationale and policy controls. |
| 6 | Financial | Treasury, health index, per-prisoner funding, crowding threshold, paged daily ledger and warden affordances. |
| 7 | Event Timeline | Newest-first events with causes (`a → b`) and confidence. |
| 8 | Social Network | Relationship dyads and derived social roles ranked by bond strength, with trust/fear/hostility/rivalry. |
| 9 | World Map | Layered campus-style map: zones, schematic roads, facilities, cell/wing occupancy, pawn positions, incident overlays and condition bars from `sumo::MapFrame` / `sumo::graphics`. |
| 0 / A | Incident Reports / Decision Audit | Structured incidents (severity, OPEN/RES, cause/consequence) and audited decisions (actor, source, validation, reason, rationale). |

Views are read-only over engine snapshots and remain stable on sparse or 2,000-prisoner worlds.

### 2.4 Graphical observer (default)

The egui/eframe observer opens at a **Main Menu**: seed field + **Start** (new world), Load Existing Save, Settings and Quit.
All navigation and controls are visible buttons/menus — there are no hidden GUI keyboard shortcuts.
The save screen uses `alc_persistence::list_saves`/`probe_save_dir` and shows each save's version,
compatibility status, population and simulation time. `--load <dir>` skips the browser and opens a
save at startup. The world map offers explicit on-screen camera controls (Fit/Reset and zoom) over
the shared campus layout. GUI preferences live in `saves/alc-gui/settings.json`, separate from v5
world saves containing `world.bin`; they are used on later launches unless explicitly overridden by
the matching CLI flag.

`alc-gui` and `alc-ui` are rendering/input adapters over `sumo::Sim`. Shared display models —
palette, view catalog, world-map layout/zones, camera math and construction-kind catalog — live in
`sumo::graphics`; frontends never mutate arbitrary world state.

---

## 3. Gameplay systems (designs & observable effects)

### 3.1 Time, schedule and daily rhythm

- A tick advances 1/5/30 simulated minutes depending on speed (`RealTime`/`FastForward`/`Extreme`).
- Every tick: needs drift → hourly guard sweep → schedule resolution → prisoner decisions →
  co-presence encounters → (on day boundary) economy, sentences, escape/parole, warden, intake,
  gangs, rehab/contraband/construction steps and report-driven warden strategy.
- Schedules assign activities (wake, meals, work, yard, education, recreation, medical, showers,
  visits, lockdown, sleep) gated by classification, wing, privileges, discipline, staffing,
  weather and lockdown. Prisoners can be late, skip work, socialize or break rules.
- **Watch it in:** views 1, 2 and 7 in either observer.

### 3.2 Needs and psychology

25 stored need-deficit pressures: physical (hunger, thirst, sleep, fatigue, hygiene, medical,
exercise, temperature, bathroom), psychological (safety, companionship, respect, freedom,
autonomy, purpose, stimulation, stability, privacy, belonging) and social (friendship, family
contact, group acceptance, status, protection, recognition). High values mean *unmet* need.

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
resource → legality → safety → clamp/repair). Batches share one canonical context, repaired
decisions are revalidated, and rejected proposals never mutate the world. Provider/model,
token budgets, rate limits, retries, cache bounds and tier telemetry are configurable through
`AiConfig` and observable in both frontends; the bundled provider is offline and deterministic.

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
pause. These are directly controllable from both frontends (§2.2–2.4).

### 3.9 Expansion and construction

Warden `Expand` actions no longer add a facility instantly: they **queue a construction project**
(7–14 days depending on facility type, with a cost charged to the economy ledger when queued and
staffing/material/prerequisite flags recorded). The queue advances daily; completion adds the
facility and emits `ConstructionComplete`. Progress appears in the world map view.

### 3.10 Rehabilitation, contraband and post-release

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
the world a persistent past. Structured `IncidentRecord`s and `DecisionAuditRecord`s expose
causality and actor/source/validation for debugging.

---

## 4. Saves, replay and reproducibility

- `saves/<dir>/world.bin` — versioned bincode (world + engine state); `world.debug.json` —
  human-readable sidecar. Format version is currently **5**; v1/v3/v4 saves are migrated on load.
  GUI preferences are separate JSON at `saves/alc-gui/settings.json` and do not alter world saves.
- Autosave cadence is configurable; `S` saves manually; headless runs always save.
- Load with `--load <dir>`; the save's seed wins. Loaded worlds can be continued and replayed
  deterministically; `trace_hash()` / `trace_hash_v2()` fingerprints verify equality.
- The same `--seed` reproduces the same institution from a fresh world — share seeds to share
  scenarios.

---

## 5. Support matrix and troubleshooting

| Symptom | Fix |
|---|---|
| Garbled TUI / arrow keys print `^[[A` | Use Windows Terminal/modern console; CI should use `cargo run -- run N`. |
| Looking for the GUI binary | There is only `alcagent`: `cargo run` opens the GUI; `alc-gui` is a library adapter. |
| GUI shortcuts not working | The GUI is button/menu-only; use `--tui` for the keyboard-driven observer. |
| "No save found" on `--load` | Point at a *directory* containing `world.bin`, or use the GUI save browser. |
| Different results with same seed | You may have loaded a save (its seed overrides the CLI value). |
| Terminal stuck in raw mode after crash | Open a fresh shell; normal shutdown always restores the terminal. |
| Old save won't load | Save format is v5; regenerate or migrate older saves. |

---

## 6. See also

- [`ALCAGENT.md`](ALCAGENT.md) — launch/CLI and observer reference (flags, keys, views).
- [`SUMO_ENGINE.md`](../sumo/SUMO_ENGINE.md) — engine product & system design.
- [`SUMO_API.md`](../sumo/SUMO_API.md) — engine API guide.
- [`Design/`](Design/) — game system design documents (in this repo).
- [`Systems/`](Systems/) — game-domain interface contracts (in this repo).
- [`agents.md`](../../agents.md) — the master development directive.
