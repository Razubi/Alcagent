//! Map Frame API (R1): Read-only snapshot of pawn positions for UI rendering.
//!
//! Provides a [`MapFrame`] containing every pawn's current location
//! (node/edge position) plus richer per-pawn metadata so the UI can draw a
//! live dot-map without needing `world_mut()` access.

use alc_core::{FacilityId, SimTime};
use alc_world::World;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A pawn's position in the prison navigation graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PawnPosition {
    /// The pawn's unique identifier.
    pub id: u64,
    /// The pawn's display name.
    pub name: String,
    /// Whether this pawn is a prisoner (true) or guard (false).
    pub is_prisoner: bool,
    /// Current node (facility/wing) ID where the pawn is located.
    pub node_id: u64,
    /// Progress along the edge to the next node (0.0 = at node, 1.0 = arrived at next).
    /// None if stationary at a node.
    pub edge_progress: Option<f32>,
    /// Target node if moving along an edge.
    pub target_node: Option<u64>,
    /// Current activity/state description.
    pub activity: String,
    /// Pixel X coordinate of the pawn, derived from `facility_positions`.
    pub x: f32,
    /// Pixel Y coordinate of the pawn, derived from `facility_positions`.
    pub y: f32,
    /// Gang affiliation (prisoners only). None if unknown.
    pub gang: Option<u64>,
    /// Security classification (prisoners only). None if unknown.
    pub classification: Option<String>,
    /// Mood estimate in 0..1 derived from condition/needs (prisoners) or
    /// morale minus stress (guards). None if unknown.
    pub mood: Option<f32>,
    /// Current intention, from the world's structured decision map (prisoners)
    /// or the guard's `current_task`. None if unknown.
    pub intention: Option<String>,
    /// Threat estimate in 0..1 derived from the aggression trait (prisoners).
    pub threat: Option<f32>,
    /// Guard post description (guards only). None for prisoners.
    pub post: Option<String>,
}

/// A navigation-graph edge between two facilities.
///
/// Placeholder graph: until `alc-world` owns a real nav graph, edges are
/// derived from facility insertion order as a ring/mesh (i -> i+1 and
/// last -> first) with `sightline: true, restricted: false`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NavEdge {
    /// Source facility/node ID.
    pub from: u64,
    /// Target facility/node ID.
    pub to: u64,
    /// Whether there is a clear line of sight along this edge.
    pub sightline: bool,
    /// Whether passage along this edge is restricted.
    pub restricted: bool,
}

/// A point-in-time snapshot of all pawn positions in the prison.
///
/// This is the primary API for the UI to render a live map without
/// requiring mutable world access.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapFrame {
    /// Simulation time at which this frame was captured.
    pub at: SimTime,
    /// All prisoner positions.
    pub prisoners: Vec<PawnPosition>,
    /// All guard positions.
    pub guards: Vec<PawnPosition>,
    /// Facility/node positions for rendering the map background.
    /// Maps facility ID to (x, y) coordinates.
    pub facility_positions: HashMap<u64, (f32, f32)>,
    /// Placeholder navigation graph edges derived from facility insertion
    /// order. Documented as a placeholder until alc-world owns a real graph.
    pub edges: Vec<NavEdge>,
    /// Facility IDs currently locked (e.g. all nodes during a lockdown).
    pub locked_nodes: Vec<u64>,
    /// Structured incidents from the engine sidecar log (R1/R4 tie-in).
    ///
    /// Populated from `sim.engine_state.incident_sidecar`. When a node filter
    /// is active only incidents whose `location` parses as a facility ID are
    /// included; currently all incidents are included because the free-form
    /// location string is not guaranteed to be a numeric node id.
    pub incidents: Vec<crate::incidents::Incident>,
}

impl MapFrame {
    /// Capture a map frame from the current world state.
    ///
    /// This variant has no access to engine-side state (e.g. incidents), so
    /// it produces an empty incident list; prefer [`crate::map_frame::map_frame`]
    /// when a `Sim` is available.
    pub fn capture(world: &World) -> Self {
        Self::capture_with_incidents(world, &[])
    }

    /// Capture a map frame, including the supplied engine sidecar incidents.
    pub fn capture_with_incidents(world: &World, incidents: &[crate::incidents::Incident]) -> Self {
        let mut prisoners = Vec::new();
        let mut guards = Vec::new();
        let mut facility_positions = HashMap::new();

        // Facility positions use a synthetic grid placeholder: 4 columns,
        // spaced 200x150 units apart. Replace with real layout data when
        // alc-world exports facility coordinates.
        for (idx, facility) in world.facilities.iter().enumerate() {
            let col = (idx % 4) as f32;
            let row = (idx / 4) as f32;
            facility_positions.insert(facility.id.0, (col * 200.0 + 100.0, row * 150.0 + 100.0));
        }

        // Placeholder nav graph: a ring connecting facilities in insertion
        // order so the UI has something meaningful to render today.
        let facility_ids: Vec<u64> = world.facilities.iter().map(|f| f.id.0).collect();
        let mut edges = Vec::new();
        if facility_ids.len() == 2 {
            edges.push(NavEdge {
                from: facility_ids[0],
                to: facility_ids[1],
                sightline: true,
                restricted: false,
            });
            edges.push(NavEdge {
                from: facility_ids[1],
                to: facility_ids[0],
                sightline: true,
                restricted: false,
            });
        } else if facility_ids.len() > 2 {
            for i in 0..facility_ids.len() {
                let next = (i + 1) % facility_ids.len();
                edges.push(NavEdge {
                    from: facility_ids[i],
                    to: facility_ids[next],
                    sightline: true,
                    restricted: false,
                });
            }
        }

        // Lockdown locks every node; otherwise none.
        let locked_nodes = if world.policies.lockdown {
            facility_ids.clone()
        } else {
            Vec::new()
        };

        // Capture prisoner positions.
        for prisoner in &world.prisoners {
            let facility_id = world
                .prisoner_location
                .get(&prisoner.id)
                .copied()
                .unwrap_or(FacilityId(0))
                .raw();
            let (x, y) = facility_positions
                .get(&facility_id)
                .copied()
                .unwrap_or((100.0, 100.0));

            let activity = Self::determine_activity(prisoner, world);
            let intention = world
                .current_decisions
                .get(&prisoner.id)
                .map(|d| format!("{:?}", d));
            // Mood heuristic: blend mental health and hope; low safety need
            // drags the mood down. Kept simple and deterministic for UI use.
            let mood = Some(
                (prisoner.condition.mental_health * 0.5
                    + prisoner.condition.hope * 0.3
                    + prisoner.needs.safety * 0.2)
                    .clamp(0.0, 1.0),
            );

            prisoners.push(PawnPosition {
                id: prisoner.id.raw(),
                name: prisoner.name.clone(),
                is_prisoner: true,
                node_id: facility_id,
                edge_progress: None, // No pathfinding integration yet
                target_node: None,
                activity,
                x,
                y,
                gang: prisoner.gang_affiliation,
                classification: Some(format!("{:?}", prisoner.classification)),
                mood,
                intention,
                threat: Some(prisoner.traits.aggression.clamp(0.0, 1.0)),
                post: None,
            });
        }

        // Capture guard positions.
        for guard in &world.guards {
            let facility_id = world
                .guard_location
                .get(&guard.id)
                .copied()
                .unwrap_or(FacilityId(0))
                .raw();
            let (x, y) = facility_positions
                .get(&facility_id)
                .copied()
                .unwrap_or((100.0, 100.0));

            let activity = Self::determine_guard_activity(guard);
            let mood = Some((guard.morale - guard.stress * 0.5).clamp(0.0, 1.0));

            guards.push(PawnPosition {
                id: guard.id.raw(),
                name: guard.name.clone(),
                is_prisoner: false,
                node_id: facility_id,
                edge_progress: None,
                target_node: None,
                activity,
                x,
                y,
                gang: None,
                classification: None,
                mood,
                intention: Some(guard.current_task.clone()),
                threat: None,
                post: Some(format!("{:?}", guard.post)),
            });
        }

        MapFrame {
            at: world.time,
            prisoners,
            guards,
            facility_positions,
            edges,
            locked_nodes,
            incidents: incidents.to_vec(),
        }
    }

    fn determine_activity(prisoner: &alc_prisoner::Prisoner, world: &World) -> String {
        // Check World's current_activity map (set by resolve_schedules_step)
        if let Some(activity) = world.current_activity.get(&prisoner.id) {
            return format!("{:?}", activity);
        }
        // Fall back to needs-based activity
        let needs = &prisoner.needs;
        if needs.hunger > 0.7 {
            "Eating".to_string()
        } else if needs.thirst > 0.7 {
            "Drinking".to_string()
        } else if needs.sleep > 0.7 {
            "Sleeping".to_string()
        } else if needs.safety < 0.3 {
            "Seeking Safety".to_string()
        } else if needs.companionship < 0.3 {
            "Socializing".to_string()
        } else {
            "Idle".to_string()
        }
    }

    fn determine_guard_activity(guard: &alc_guard::Guard) -> String {
        format!("Guarding {:?}", guard.post)
    }
}

/// Get a map frame from the simulation.
/// This is the main R1 API entry point.
pub fn map_frame(sim: &crate::Sim) -> MapFrame {
    MapFrame::capture_with_incidents(&sim.world, &sim.engine_state.incident_sidecar)
}
