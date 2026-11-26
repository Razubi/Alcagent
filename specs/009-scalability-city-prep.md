# Specification: Scalability and City Simulation Prep

## Feature: Prepare for Thousands of Agents and Procedural City

### Overview
The simulation must eventually support thousands of agents extending beyond the prison into a procedurally generated city. This spec prepares the architecture: spatial indexing, chunked world updates, LOD simulation, and city generation hooks. The prison becomes one district in a larger simulated world.

### User Stories
- As a developer, I want the engine to handle 1000+ agents without stalling.
- As an observer, I want the prison to exist in a larger simulated city.
- As a developer, I want spatial indexing for efficient neighbor queries.

---

## Functional Requirements

### FR-1: Spatial indexing
Add a spatial grid for efficient neighbor queries. Replace O(n²) encounter scanning with grid-based lookups.

**Acceptance Criteria:**
- [ ] `SpatialGrid` struct in `alc-world` or `sumo`
- [ ] O(1) neighbor lookup per cell
- [ ] Co-presence encounters use spatial grid
- [ ] Deterministic iteration order maintained

### FR-2: Chunked world updates
Divide the map into chunks. Only simulate chunks with active agents. Distant chunks use simplified updates.

**Acceptance Criteria:**
- [ ] Map divided into chunks
- [ ] Only active chunks fully simulated
- [ ] Distant chunks use LOD updates
- [ ] Chunk boundaries handled correctly

### FR-3: LOD simulation
Agents far from the prison (in the city) use simplified simulation: needs drift only, no encounters, no decisions. Agents near the prison use full simulation.

**Acceptance Criteria:**
- [ ] LOD levels defined (Full, Reduced, Minimal)
- [ ] LOD assignment based on distance from prison
- [ ] Full simulation for prison interior
- [ ] Reduced simulation for prison exterior
- [ ] Minimal simulation for distant city

### FR-4: City generation hooks
Add hooks for procedural city generation around the prison. City districts, roads, buildings, and population.

**Acceptance Criteria:**
- [ ] `CityGenerator` trait/hook in `alc-world`
- [ ] District types: Residential, Commercial, Industrial, Government
- [ ] Road network generation
- [ ] Building placement
- [ ] Population seeding

### FR-5: Agent streaming
Agents can be streamed in/out of the simulation based on relevance. Distant city agents are not in memory.

**Acceptance Criteria:**
- [ ] Agent streaming based on distance
- [ ] Agents outside range are serialized to disk
- [ ] Agents re-enter simulation when relevant
- [ ] Memory bounded regardless of city size

### FR-6: Performance profiling
Add profiling hooks for tick time, memory usage, and agent count.

**Acceptance Criteria:**
- [ ] Tick time reported per step
- [ ] Memory usage reported
- [ ] Agent count by LOD level
- [ ] Profiling data available via `Sim::profiling()`

---

## Success Criteria

- 1000-agent simulation runs at stable tick rate
- Spatial grid reduces encounter scanning from O(n²) to O(n)
- City generation hooks are defined and testable
- All existing tests pass

---

## Dependencies
- `alc-world` (World, World::tick, encounters)
- `sumo` (Sim, observation DTOs)
- `alc-core` (Config, SimTime)

## Assumptions
- City simulation is future work; this spec prepares hooks only
- LOD transitions are smooth (no visible pop-in)

---

## Completion Signal

### Implementation Checklist
- [ ] SpatialGrid implementation
- [ ] Chunked world updates
- [ ] LOD simulation levels
- [ ] City generation hooks
- [ ] Agent streaming
- [ ] Performance profiling
- [ ] Tests for all new systems
- [ ] All existing tests pass

<!-- NR_OF_TRIES: 0 -->