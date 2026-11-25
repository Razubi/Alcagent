//! Map Frame API (R1) integration tests.

use alc_events::EventKind;
use sumo::incidents::Incident;
use sumo::{Policy, Sim};

#[test]
fn map_frame_captures_every_pawn_with_coordinates() {
    let sim = Sim::new(42);
    let frame = sim.map_frame();

    assert_eq!(frame.prisoners.len(), sim.world().prisoners.len());
    assert_eq!(frame.guards.len(), sim.world().guards.len());
    assert_eq!(frame.at, sim.world().time);

    for pawn in frame.prisoners.iter().chain(frame.guards.iter()) {
        assert!(pawn.x > 0.0, "pawn {} has non-positive x", pawn.id);
        assert!(pawn.y > 0.0, "pawn {} has non-positive y", pawn.id);
    }
}

#[test]
fn map_frame_is_deterministic_for_same_state() {
    let sim = Sim::new(99);
    let a = sim.map_frame();
    let b = sim.map_frame();
    assert_eq!(a, b);
}

#[test]
fn map_frame_has_edges_and_locked_nodes() {
    let mut sim = Sim::new(7);
    {
        let frame = sim.map_frame();
        assert!(
            frame.edges.len() >= sim.world().facilities.len(),
            "expected at least one edge per facility"
        );
        assert!(
            frame.locked_nodes.is_empty(),
            "nodes should be unlocked under normal policy"
        );
    }

    sim.set_policy(Policy::Lockdown);
    let frame = sim.map_frame();
    assert!(
        !frame.locked_nodes.is_empty(),
        "all facilities should be locked during lockdown"
    );
    assert_eq!(frame.locked_nodes.len(), sim.world().facilities.len());
}

#[test]
fn map_frame_reflects_movement() {
    let mut sim = Sim::new(11);
    let pid = match sim.world().prisoners.first() {
        Some(p) => p.id,
        None => return, // no prisoners in this world; nothing to assert
    };

    let before = sim.map_frame();
    let old_pawn = before.prisoners.iter().find(|p| p.id == pid.raw()).unwrap();
    let old_node = old_pawn.node_id;
    let old_x = old_pawn.x;
    let old_y = old_pawn.y;

    // Directly move the prisoner to facility 2 (world_mut is pub).
    sim.world_mut()
        .prisoner_location
        .insert(pid, alc_core::FacilityId(2));

    let after = sim.map_frame();
    let new_pawn = after.prisoners.iter().find(|p| p.id == pid.raw()).unwrap();
    assert_eq!(new_pawn.node_id, 2);
    assert_ne!(
        new_pawn.node_id, old_node,
        "node_id should change after move"
    );
    assert_ne!(
        (new_pawn.x, new_pawn.y),
        (old_x, old_y),
        "coordinates should change after move"
    );
}

#[test]
fn map_frame_includes_incidents() {
    let mut sim = Sim::new(3);
    let incident = Incident::new(
        EventKind::Fight,
        sim.world().time,
        "Yard".to_string(),
        Vec::new(),
    );
    sim.record_sidecar_incident(incident.clone());

    let frame = sim.map_frame();
    assert!(frame.incidents.contains(&incident));
}
