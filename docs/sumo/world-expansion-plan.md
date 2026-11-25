# v0.5 World Expansion Plan — A Living Town Around the Prison

> **Historical planning snapshot:** this vision is not implemented. References below to save
> format v3, 47 `EventKind` variants and a non-buildable GUI reflect the 2026-11-24 baseline;
> current code uses save format v5, 51 event kinds and a buildable `alc-gui` observer. Current
> status is in [`../alcagent/Project/PROJECT.md`](../alcagent/Project/PROJECT.md).

**Status:** deep research & design plan · **Date:** 2026-11-24 · **Target:** v0.5 (polish, not to be
fully implemented now) · **No code changed.**

## 0. Owner directives

1. The prison must work in **correspondence with the world/town around it**.
2. Build a **full live city** with pawns living normal lives — kids, schools, jobs, families.
3. The prison is the destination for **all criminals in the world**.
4. **UI and GUI rewrites** will be required.
5. **sumo** keeps updating for future support needs/capabilities while **Alcagent** grows in
   gameplay/features; sumo owns the GUI/editor shell.
6. Deep research and planning only — implementation/polish is a v0.5 goal.

---

## 1. Vision

Today the prison is a self-contained society. v0.5 makes it **one institution inside a living
town**: citizens are born, go to school, work, form families and households, commit crimes, are
arrested and sentenced into the prison, and — after release — attempt to reintegrate, sometimes
successfully, sometimes re-offending. Everything remains deterministic and replayable, and the
player observes it through a GUI shell owned by sumo with game-specific panels supplied by
Alcagent.

**Core loop of the expanded world:**
`town life → crime → arrest → court → prison → rehabilitation → release → reintegration → recidivism or recovery`

---

## 2. Ground truth: what exists today

- `sumo::Sim` is a deterministic façade over `alc_world::World`: time control, validated decision
  pipeline, observation snapshots, event bus, persistence, invariants, replay digests
  (`docs/sumo/SUMO_ENGINE.md`, `crates/sumo/src/lib.rs`).
- `World` is prison-shaped: `Facility`, `Prisoner`, `Guard`, `Warden`, `Gang`, `Economy`,
  `SocialGraph`, `prisoner_location`, `post_release_records`, `EventBus`, `history`.
- Typed IDs (`PrisonerId`, `GuardId`, `FacilityId`, …); identity ends when a prisoner leaves —
  there is **no stable cross-boundary person identity**.
- Determinism: seeded `RngStream` keyed by `(seed, entity/time id)`, sorted keys, no wall clock;
  730-day 3-seed chaos soaks, `trace_hash`/`trace_hash_v2`, save→load replay tests.
- Persistence: versioned bincode (`world.bin`, v3 in flight) + JSON sidecar; `#[serde(default)]`
  on new fields.
- Scaling tactics already proven: 50k bounded event bus, hourly guard sweeps, 10k log caps.
- UI: ratatui `alc-ui` with 11 views over snapshots; sumo GUI is a stub (`SumoApp` unit struct,
  `launch_gui() -> Ok(())`); `MapFrame` geometry is a placeholder grid/ring.

**Implication:** v0.5 should add a **TownDomain alongside the prison domain**, not rewrite sumo.
The engine services already generalize; the world model is the thing to extend.

---

## 3. Research findings (with sources)

### 3.1 Urban/city simulation
- **Agent-based modeling (ABM)** is the established paradigm: autonomous, boundedly-rational
  agents with interaction topology and environment producing emergent macro patterns (Schelling,
  Sugarscape; urban planning and pedestrian flow applications). **[Proven]**
  <https://en.wikipedia.org/wiki/Agent-based_model>
- **MATSim** is a production activity-based transport simulator: each agent carries a *plan set*
  (daily activity chains), replans by mutation/scoring, and runs a mobility simulation over a
  network; learned choice sets are bounded (~4 plans). This is a proven template for "daily
  routine + spatial graph + scoring". **[Proven]** <https://matsim.org/docs/userguide/terminology>
- **Bevy ECS** validates entities-as-ids + component queries + mostly-parallel systems for many
  agents. **[Proven]** <https://bevy.org/learn/quick-start/getting-started/ecs/>
- **The Sims** used Maslow-like needs and life stages/weekday schedules — a proven game precedent
  for need-driven agents with school/work routines. **[Proven]** <https://en.wikipedia.org/wiki/The_Sims>

### 3.2 Population lifecycle
- Life-stage models (child → student → worker → parent → retired → deceased), wants/fears,
  careers, education, businesses and death are proven game mechanics (The Sims 1–4). **[Proven]**
- Synthetic populations and network/culture co-evolution are standard in large ABMs. **[Proven]**

### 3.3 Justice pipeline
- The U.S. justice process is conventionally: crime → arrest → charge/initial appearance →
  trial/plea → sentencing → incarceration → parole/release → reintegration; BJS publishes
  statistical coverage of the full process. A usable abstraction set for the game. **[Proven]**
  <https://bjs.ojp.gov/bjs-statistical-coverage-crime-and-justice-process>
- The current `post_release_records` (`PostReleaseRecord`) is a natural reintegration hook.

### 3.4 Pitfalls in life sims
- **Needs spiral** — endlessly chasing unmet needs; mitigate with satiation, habituation and goal
  commitment. **[Speculation, informed by The Sims]**
- **Capacity deadlocks** — no jobs/schools/housing → idle masses; guarantee baseline capacity,
  queues and fallback assignments; invariant on demand vs capacity.
- **Runaway population** — bound births by housing/jobs, abstract deaths, migration controls.
- **Reproducibility drift** — sorted iteration, seeded streams, no float non-associativity in
  decisions, domain digests.
- **UI overwhelm** — layered observation (world → district → household → person), filters,
  follow-focus.
- **Performance cliffs** — tiered simulation, fixed budgets, no per-pawn LLM calls.

---

## 4. Target architecture

### 4.1 World model (in Alcagent, on top of sumo)

```
sumo::Sim  (unchanged façade: time, decisions, events, observation, persistence, invariants)
 └─ alc_world::World
     ├─ PrisonDomain (existing): facilities, prisoners, guards, warden, gangs, economy
     └─ TownDomain (new)
         ├─ Town { districts, pois: Vec<PointOfInterest>, nav: NavGraph }
         ├─ citizens: Vec<Citizen>            // stable PersonId, lifecycle, household
         ├─ households: Vec<Household>
         ├─ jobs / schools: capacity + assignment queues
         ├─ justice: JusticeLedger            // cases, arrests, hearings, sentences
         └─ sentiment: TownSentiment          // public opinion, crime fear, budget mood
```

### 4.2 Identity continuity (the critical seam)

- Introduce a single stable **`PersonId`** as the identity root; `PrisonerId` becomes a *role
  handle* referencing the same person. Never identify by name.
- A `Role` component: `Citizen | Inmate | Guard | Staff | Warden | Official`; a person can hold
  different roles over time. Prison-specific state stays in its own struct.
- **Intake:** citizen record marked `Incarcerated`; traits, memory, relationships, skills,
  criminal record migrate into the prisoner; emit an audited `RoleChange`/`Intake` event.
- **Release:** reverse migration with prison deltas (rehab progress, trauma, injuries, reputation,
  sentence served) into the citizen; `PostReleaseRecord` records outcome and seed.

### 4.3 Spatial & activity model

- Town is a **POI graph**: nodes = home, school, workplace, shop, clinic, park, transit stop,
  courthouse, police station, prison; edges carry travel time/cost/visibility.
- Citizens hold **MATSim-style daily plans**: ordered activity slots with time windows targeting
  a POI class. Plans are selected/mutated by need/utility scoring with bounded plan sets.
- Movement is explicit (route + edge progress), not teleportation. Co-presence at POIs generates
  encounters, queues, trades, learning and crime opportunities. POIs have capacity; overflow
  produces queues, lateness and friction.
- Replace the current placeholder `MapFrame` grid/ring with real town spatial data.

### 4.4 Justice ↔ prison coupling

1. Crime event in town (victim, witnesses, severity, evidence quality).
2. Policing: patrol presence → detection/arrest probability (wrongful arrests possible).
3. Adjudication: charge → hearing/trial → verdict → sentence length + classification.
4. Intake → prison sim runs unchanged.
5. Parole/release → citizen re-enters with altered traits and a record.
6. Reintegration success/failure (job/housing/family/reputation) → recidivism probability.
7. Town public opinion (crime fear, scandals, prison cost) → warden funding, strictness, parole
   pressure and expansion approvals — delivered as **reports**, not omniscient state.

### 4.5 Scale & determinism

- **Tiered LOD:**
  - `Active`: full needs, plans, movement, encounters, AI deliberation (prisoners and focused/
    incident-involved townsfolk).
  - `Scheduled`: needs drift + plan resolution hourly; no pathfinding.
  - `Background`: daily aggregates only.
- Spatial hash/grid over the POI graph for radius/POI queries; precomputed travel times.
- Single-threaded deterministic core; parallelize only pure read-only analysis, apply results in
  sorted entity-id order. All randomness from seeded streams. Add per-domain digests to
  `trace_hash_v2()` and replay soaks at 730+ days.

### 4.6 Emergence & narrative

No scripted stories: needs → goals → memories → relationships → plans → encounters → events with
causes. Extend the existing information model (direct knowledge / rumor / suspicion / belief) to
town: information spreads along relationship edges with distortion by source credibility.
Reputation is per-observer belief, aggregated only for UI/institutions.

### 4.7 Persistence

- Bump save format to v4 with explicit v3 migration; `#[serde(default)]` on all new fields;
  persist town in the world blob plus a new `TownState` sidecar; domain digests for replay parity.
- Consider full snapshot + append-only event delta log for long horizons (speculative).

---

## 5. GUI & frontend rewrite (sumo-owned shell, Alcagent-owned panels)

### 5.1 Principle: two release tracks

```
sumo (engine)         — stable API, general, owns GUI/editor shell, forward-compatible
   ▲ depends-on
alcagent (game)       — prison/town gameplay, families, schools, jobs, crime; game-specific panels
```

sumo changes = infrastructure (APIs, abstractions, event/persistence contracts, replay,
observability, GUI shell). Alcagent changes = content and rules. The engine must never import
game code or a game UI crate, which is what lets sumo serve future projects.

### 5.2 Resolve sumo's GUI concerns

- Replace `crates/sumo/src/gui.rs` stub with a real `sumo-gui` crate (egui/eframe native+wasm,
  `egui_dock` panels) that depends **only** on snapshot/command types, never on gameplay.
- Provide a `SimDriver`-style trait: GUI reads `Observation`/`MapFrame`/generic summaries and
  sends frame-tagged `Command`s consumed at tick boundaries — UI input joins the replay stream.
- Generic panels: playback bar (pause/step day/speed/seek), entity inspector, event/incident
  timeline with cause chains, map viewport (pan/zoom/pick/overlays), stats/invariant/debug
  overlays, settings/project loader.
- **Game panel plugin trait:** Alcagent registers prison/town/family/school/justice panels into
  the shell; the engine never learns what a prisoner is.
- Keep the ratatui `alc-ui` as a headless/CI observer over the same snapshots (feature flags), so
  soaks and tests remain cheap.

### 5.3 New observer modes for v0.5

World/district view, citizen/household inspector, family tree, school/workplace rosters, justice
pipeline board, crime heatmap, person-lifecycle timeline (follow one citizen from school → crime →
prison → release), and prison views retained. Progressive disclosure and filters to avoid
information overload.

### 5.4 UI/GUI research findings (2026 upstream + codebase)

- **Current GUI does not build:** `alc-gui` declares modules (`app`, `panels`, `screens`, `settings`,
  `theme`) that don't exist and is not a workspace member; `sumo` GUI is a 13-line stub
  **[Codebase]**. egui/eframe deps are declared but unused in sumo.
- **egui 0.37 requires Rust 1.95+**, while the workspace declares MSRV 1.85 (installed toolchain
  1.100) — a decision is needed (bump MSRV or pin older egui) **[Proven]**.
- **Rendering pattern:** batch pawn shapes with `Painter::extend()` (not one widget per pawn),
  camera via `egui::Scene` (pan/zoom, persisted rect), viewport culling + uniform-grid spatial index
  and zoom-based LOD; expected ceiling ~1–3k visible pawns in egui; tens of thousands would justify
  Bevy instancing **[Proven mechanisms / Speculation on scale]**.
- **Inspector/dashboard template:** Rerun's viewer (Blueprint / Selection / Timeline / Viewport,
  Ctrl+K palette, play/pause/step/loop, scrubber, event markers) is a near-exact model for an
  emergent-society observer **[Proven]**. Progressive disclosure is the canonical anti-overload
  pattern **[Proven]** <https://en.wikipedia.org/wiki/Progressive_disclosure>.
- **Do not adopt Bevy for v0.5:** a second ECS world risks diverging from the deterministic `Sim`,
  inspector can mutate arbitrary state (violating validated commands), and ~3-month breaking cadence
  **[Speculation on fit; Proven cadence]**. Keep the ratatui TUI as headless/CI frontend.
- **Suggested crate split:** `alc-view` (pure serde DTOs / `ViewModel`), `alc-frontend-api`
  (`EngineFrontend` trait, `UiCommand`, `Selection`), `alc-ui` (ratatui), `alc-gui` (eframe),
  `sumo` (engine, remove GUI stub + egui deps), `alcagent` (binary selects frontend). GUI never
  holds `&mut World`; all writes are validated commands; UI input joins the replay stream.

### 5.5 Codebase gap audit (what is actually missing today)

- **No town exists at all:** no `TownId`, `CitizenId`, `FamilyId`, `SchoolId`, `JobId` in
  `alc-core/src/ids.rs`; no town/citizen/family/job structs; `World::population()` is prisoners
  only. `Visitor`/`FamilyCorrespondence` event kinds are declared but never emitted **[Codebase]**.
- **No `Pawn` entity:** "pawn" is only a render label (`PawnPosition` = prisoner or guard);
  locations are `HashMap<PrisonerId/GuardId, FacilityId>` with no coordinates.
- **Identity is lost on exit:** `post_release_records` keeps id/name/reason/day/sentence info and an
  `outcome_seed`; traits, needs, skills, health, relationships, memory and rehab completions are
  discarded. Three incompatible `PostReleaseRecord` types exist; engine ledger is one `bool`.
- **MapFrame geometry is placeholder:** synthetic 4-column grid + insertion-order ring,
  `edge_progress=None`; the TUI map does not even consume `MapFrame`.
- **Event taxonomy has 51 kinds** (not 47) and is append-only for bincode stability — good for
  town additions; mirrored incident causality is still self-referential placeholders.
- **SocialGraph keys are `PrisonerId` only** — must generalize to cross-domain actors.
- **Co-presence is quadratic per facility** — a town square with 1,000 citizens would be ~500k
  pair checks per sim-hour; needs spatial bucketing and staggered cadence.
- **Duplicated tick/driver authority** (`World::tick` vs `sumo::drivers`) must be unified before a
  third town authority is added.
- **Save format v3** with container-level `#[serde(default)]`; town state needs v4 migration,
  converters both ways, and extended digests/invariants or replay tests silently pass while town
  state diverges.

**Gap-closing backlog (design [D] / implementation [I], P0–P2):**
1. [D→I] Unify tick/driver authority; decide applier signature — blocks the town driver.
2. [D] Town domain model & interfaces (new ids, citizen aggregate, lifecycle).
3. [I] `alc-town` crate: entities, households, jobs, schools, businesses, coarse daily cadence.
4. [D→I] Real nav graph & coordinates; generalize `MoveTo`; prison↔town links.
5. [I] Generalize `SocialGraph` to generic actor ids.
6. [I] Durable post-release snapshot → citizen + rich outcome simulation (replaces bool).
7. [I] Town economy (income, business ledger, taxes, town budget).
8. [I] Append town `EventKind`s + typed scope; emit from town steps; fix causality placeholders.
9. [I] Town decisions/validation (`go_to`, `apply_for_job`, `enroll_school`, `check_in`, …).
10. [I] Extend DTOs/Observation/Stats/MapFrame; town/citizen/family views; rewire map.
11. [I] Persistence v4 + migrations + roundtrip/soak tests.
12. [D→I] Determinism/perf hardening (spatial buckets, staggered cadence, log budgets, digests).
13. [I] Wire dead hooks (`arrival_social_hooks`, Visitor/FamilyCorrespondence, KnowledgeStore).
14. [D→I] Docs/tests alignment; avoid the two-code-tree drift seen with `../sumo`.

**Sequence:** 1 → 2 → 3 → (4 ∥ 5 ∥ 6) → 7 → 8 → 9 → 10 → 11 → 12 → 13 → 14.
Highest implementation risk: 3, 6, 9, 11.

---

## 6. Phased roadmap to v0.5 (build order)

Foundation first — each phase has a verification gate before the next:

1. **Identity & role migration** — `PersonId`, citizen↔prisoner transitions, audit events, save
   migration stub. *Unlocks all coupling; highest leverage.*
2. **Town skeleton + POI graph + capacities** — a place for citizens to live.
3. **Citizen lifecycle & households** — birth, aging, school, jobs, death; capacity invariants.
4. **Daily plans & need-driven routines** — MATSim-style plans, LOD tiers, deterministic RNG.
5. **Justice pipeline** — crime → arrest → charge → hearing → sentence → existing intake.
6. **Parole/reintegration + recidivism** — reuse `post_release_records`.
7. **Public opinion ↔ warden** coupling.
8. **Persistence v4 + domain digests + long-horizon soaks** (730+ days, thousands of pawns).
9. **GUI shell in sumo + game panels in Alcagent**; UI town views; then rumor/reputation depth.

**Explicitly out of scope for v0.5:** real-time per-pawn pathfinding, LLM dialogue for all
citizens, deep legal nuance, full graphical editor.

---

## 7. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Needs spiral / idle masses | Satiation, habituation, baseline capacity, fallback assignments, invariants |
| Population explosion | Housing/job-bounded births, abstract deaths, migration controls |
| Determinism drift at scale | Sorted iteration, seeded streams, domain digests, replay soaks |
| Save breakage | v4 migration, serde defaults, fixture-based migration tests |
| UI overwhelm / perf cliffs | LOD, layered observation, filters, culling, fixed simulation budgets |
| GUI recoupling to gameplay | Snapshot/command boundary; game panels as plugins; engine never imports game crates |
| Engine/game release skew | Semver engine API, documented compatibility window, game releases between engine minors |

---

## 8. Owner decisions required before design locks

1. **Identity:** unified `PersonId` with role handles (recommended) or keep `PrisonerId` separate?
2. **Town scale for v0.5:** 1k / 5k / 20k citizens? (drives LOD aggressiveness)
3. **LOD boundary:** focus-based, incident-involved, or prison-first?
4. **Justice depth:** one deterministic case-resolution step or multi-stage arrest/charge/hearing/trial?
5. **Sentencing/parole authority:** warden, independent court, or hybrid? Who can override?
6. **Reintegration:** how strongly do rehab/trauma carry into released citizens; who sees the record?
7. **Public opinion:** direct event metric or mediated by officials/news agents?
8. **Crime scope:** crimes against citizens (with victims) or property/institutional only?
9. **Family model:** marriage/parentage with aging children, or simplified household slots?
10. **Save strategy:** full snapshot vs append-only delta log; acceptable save size/time?
11. **Determinism strictness:** single-threaded core with parallel read-only analysis, or
    partitioned deterministic systems with merge order?
12. **v0.5 success criterion:** breadth of town views, or one traceable "school → crime → prison →
    release" narrative?
13. **GUI confirmation:** should the sumo GUI shell be the primary v0.5 frontend, with ratatui
    retained only for CI/soaks? (Recommended.)
14. **MSRV:** bump `rust-version` to 1.95+ for egui 0.37, or pin an older egui for 1.85?
15. **Town placement:** living town in the same `Sim`/`World` as the prison, or a separate sim
    overlaid by the GUI? (Recommendation: same world, one `PersonId` namespace.)
16. **Map fidelity:** schematic POI/node map (extend `MapFrame`) or real tile/city grid with
    buildings/roads/rooms?
17. **Replay:** true deterministic seek to arbitrary tick, or bounded snapshot ring
    (1 keyframe/sim-day) acceptable?
18. **GUI write authority:** expose all controls (policy, warden, rehab, construction, incidents),
    or gate an "admin mode"?
19. **Fog of war:** omniscient snapshots (today) or per-agent knowledge views (new DTO layer)?
20. **Repo drift:** the standalone `../sumo` repo is a **pinned, stripped snapshot** of the engine
    (lacks 0.05 APIs); should v0.5 work treat the game repo as canonical and re-sync sumo later,
    or maintain both trees in lockstep? (Recommendation: game repo canonical; periodic sync.)

---

## 9. Sources

- Agent-based modeling: <https://en.wikipedia.org/wiki/Agent-based_model>
- MATSim (plans, replanning, mobsim): <https://matsim.org/docs/userguide/terminology>
- Bevy ECS: <https://bevy.org/learn/quick-start/getting-started/ecs/>
- The Sims (needs, life stages, careers): <https://en.wikipedia.org/wiki/The_Sims>
- BJS justice process & corrections: <https://bjs.ojp.gov/bjs-statistical-coverage-crime-and-justice-process>
- Local codebase & docs: `crates/alc-world/src/lib.rs`, `crates/sumo/src/lib.rs`,
  `docs/sumo/SUMO_ENGINE.md`, `docs/sumo/sumo-gui-and-support-plan.md`
