# Specification: UI and Map Overhaul

## Feature: CK3/RimWorld-Style Map and Clean UI

### Overview
The current UI is cluttered with disjointed text and buttons. This spec overhauls the interface: a scrollable, zoomable map with distinct buildings, clickable pawns, clear causal links, and a clean information hierarchy inspired by CK3, RimWorld, Prison Architect, Dwarf Fortress, and Songs of Syx. Remove clutter, pointless text, and lazy design.

### User Stories
- As an observer, I want to scroll and zoom the map to see the whole prison.
- As an observer, I want buildings to look distinct so I can identify them at a glance.
- As an observer, I want to click on prisoners and buildings to see details.
- As an observer, I want a clean UI with clear information hierarchy, not cluttered text.

---

## Functional Requirements

### FR-1: Scrollable, zoomable map
The map supports smooth scrolling and zooming. Mouse wheel zooms, drag pans. Buildings and pawns scale appropriately.

**Acceptance Criteria:**
- [ ] Mouse wheel zooms in/out
- [ ] Click-drag pans the map
- [ ] Zoom levels: 0.5x to 4x
- [ ] Pawns and buildings scale with zoom
- [ ] Minimap shows full prison

### FR-2: Distinct buildings
Each facility type has a unique visual appearance (color, shape, icon). Cell blocks, yards, kitchens, medical, admin, workshops, education, visitor, recreation all look different.

**Acceptance Criteria:**
- [ ] 10+ distinct building visuals
- [ ] Buildings show occupancy/capacity
- [ ] Buildings show condition/maintenance
- [ ] Building labels visible at appropriate zoom

### FR-3: Clickable pawns
Clicking a pawn selects it and shows its detail panel. Hovering shows name and status tooltip. Selected pawn is highlighted on map.

**Acceptance Criteria:**
- [ ] Click selects pawn
- [ ] Hover shows tooltip (name, status, gang, needs)
- [ ] Selected pawn highlighted
- [ ] Double-click centers on pawn

### FR-4: Clickable buildings
Clicking a building selects it and shows its detail panel (occupancy, condition, staff, activities).

**Acceptance Criteria:**
- [ ] Click selects building
- [ ] Building detail panel shows all relevant info
- [ ] Construction progress visible

### FR-5: Clean information hierarchy
Information is organized into clear panels: overview stats, selected entity details, event feed, map. No redundant text. Tooltips explain controls.

**Acceptance Criteria:**
- [ ] No duplicate information across panels
- [ ] Controls have tooltips
- [ ] Key information is visually prominent
- [ ] Secondary information is collapsible

### FR-6: Remove clutter
Remove pointless text, redundant labels, and lazy UI elements. Every UI element must serve a purpose.

**Acceptance Criteria:**
- [ ] No placeholder/lorem text
- [ ] No redundant labels
- [ ] Every button has a clear purpose
- [ ] Layout is clean and professional

### FR-7: Research-backed design
UI design informed by RimWorld (schedule/needs panels), CK3 (map/character panels), HOI4 (production graphs), Dwarf Fortress (information density), Prison Architect (facility management), Songs of Syx (large-scale readability).

**Acceptance Criteria:**
- [ ] Design document references specific games
- [ ] Key UI patterns adopted from research
- [ ] Consistent visual language throughout

---

## Success Criteria

- Map is scrollable, zoomable, and shows distinct buildings
- Pawns and buildings are clickable with detail panels
- UI is clean, professional, and information-dense
- All existing tests pass

---

## Dependencies
- `sumo` (MapFrame, graphics, observation DTOs)
- `alc-gui` (egui rendering)
- `alc-ui` (ratatui rendering)
- `alc-world` (facility kinds, pawn positions)

## Assumptions
- Map uses 2D top-down view (not 3D)
- Building visuals are colored shapes with icons, not sprites

---

## Completion Signal

### Implementation Checklist
- [ ] Scrollable zoomable map
- [ ] Distinct building visuals
- [ ] Clickable pawns with detail
- [ ] Clickable buildings with detail
- [ ] Clean information hierarchy
- [ ] Clutter removed
- [ ] Design document with game references
- [ ] Tests for map rendering and interaction
- [ ] All existing tests pass

<!-- NR_OF_TRIES: 0 -->