//! Spec 008 integration tests: world-owned gang membership, durable political
//! history and the weekly gang-politics pass.

use alc_core::{GangId, SimTime, TimeSpeed};
use alc_social::{GangHistoryKind, RelationshipDelta};
use alc_world::{GangHistoryEntry, World};

fn add_gang(world: &mut World, id: u64, name: &str, founder: alc_core::PrisonerId) {
    assert!(world.create_gang(GangId(id), name, founder));
}

fn set_mutinous(world: &mut World, prisoner: alc_core::PrisonerId, leader: alc_core::PrisonerId) {
    if let Some(p) = world.prisoners.iter_mut().find(|p| p.id == prisoner) {
        p.traits.loyalty = 0.0;
        p.traits.discipline = 0.0;
        p.traits.ambition = 1.0;
        p.traits.leadership = 1.0;
        p.needs.stability = 0.0;
        p.needs.freedom = 1.0;
        p.needs.autonomy = 1.0;
        p.condition.hope = 0.0;
    }
    let mut hostile = RelationshipDelta::default();
    hostile.hostility = 1.0;
    hostile.resentment = 1.0;
    hostile.rivalry = 1.0;
    world.social.record(prisoner, leader, hostile);
}

fn politics_world(seed: u64) -> World {
    let mut world = World::new(seed);
    let ids: Vec<_> = world.prisoners.iter().take(5).map(|p| p.id).collect();
    let (leader, a, b, rival_leader) = (ids[0], ids[1], ids[2], ids[3]);

    add_gang(&mut world, 1, "Iron Choir", leader);
    world.join_gang(a, GangId(1));
    world.join_gang(b, GangId(1));
    set_mutinous(&mut world, a, leader);
    set_mutinous(&mut world, b, leader);
    for record in &mut world.gangs[0].hierarchy {
        if record.prisoner != leader {
            record.grievances = 0.85;
        }
    }

    add_gang(&mut world, 2, "Ash Gang", rival_leader);
    world.join_gang(ids[4], GangId(2));
    {
        let (left, right) = world.gangs.split_at_mut(1);
        left[0].form_rivalry(&mut right[0]);
    }
    world.days_simulated = 7;
    world
}

#[test]
fn membership_api_keeps_rosters_and_affiliations_in_sync() {
    let mut world = World::new(100);
    let leader = world.prisoners[0].id;
    let recruit = world.prisoners[1].id;
    add_gang(&mut world, 1, "Test Crew", leader);

    assert!(world.join_gang(recruit, GangId(1)));
    assert_eq!(world.prisoner_gang(recruit), Some(GangId(1)));
    assert!(world.gangs[0].members.contains(&recruit));

    assert!(world.leave_gang(leader));
    assert_eq!(world.prisoner_gang(leader), None);
    assert_eq!(world.gangs[0].leader, Some(recruit));

    let kinds: Vec<_> = world
        .gang_history()
        .iter()
        .map(|entry| entry.kind)
        .collect();
    assert!(kinds.contains(&GangHistoryKind::Recruit));
    assert!(kinds.contains(&GangHistoryKind::Loss));
    assert!(kinds.contains(&GangHistoryKind::LeadershipChange));
}

#[test]
fn reconciliation_repairs_legacy_drift() {
    let mut world = World::new(101);
    let leader = world.prisoners[0].id;
    let member = world.prisoners[1].id;
    let stray = world.prisoners[2].id;
    add_gang(&mut world, 1, "Legacy Crew", leader);
    world.gangs[0].members.push(member);

    // Simulate drift from an older save / repaired decision: one roster member
    // has no affiliation, one prisoner points at a missing gang, and one is
    // affiliated without being on the roster.
    world.prisoners[1].gang_affiliation = None;
    world.prisoners[2].gang_affiliation = Some(999);
    world.prisoners[3].gang_affiliation = Some(1);

    assert!(world.reconcile_gang_membership() >= 3);
    assert_eq!(world.prisoner_gang(member), Some(GangId(1)));
    assert_eq!(world.prisoner_gang(stray), None);
    assert!(world.gangs[0].members.contains(&world.prisoners[3].id));
    for prisoner in &world.prisoners {
        if let Some(raw) = prisoner.gang_affiliation {
            assert!(
                world
                    .gangs
                    .iter()
                    .any(|g| g.id.raw() == raw && g.members.contains(&prisoner.id))
            );
        }
    }
}

#[test]
fn gang_pressure_changes_contraband_channel_strength() {
    let mut world = World::new(102);
    let leader = world.prisoners[0].id;
    let member = world.prisoners[1].id;
    add_gang(&mut world, 1, "Trade Crew", leader);
    world.join_gang(member, GangId(1));

    assert_eq!(world.gang_control_for(world.prisoners[2].id), 0.0);
    let baseline = world.gang_control_for(member);
    assert!(baseline > 0.0, "affiliation must create a gang channel");

    world.gangs[0].resources = 1.0;
    assert!(
        world.gang_control_for(member) > baseline,
        "resources must strengthen the gang channel"
    );
    assert!(world.gang_pressure().total() >= 0.0);
}

#[test]
fn weekly_politics_writes_disputes_and_replays_deterministically() {
    let first = politics_world(0x008);
    let mut second = politics_world(0x008);
    assert_eq!(first.gang_history(), second.gang_history());

    second.gang_politics_step();
    assert!(
        second
            .gang_history()
            .iter()
            .any(|entry| entry.kind == GangHistoryKind::Dispute),
        "weekly review must record member disputes"
    );
    assert!(
        second
            .gangs
            .iter()
            .any(|gang| !gang.internal_disputes.is_empty()),
        "disputes must be visible on the organization"
    );
    assert!(second.gang_pressure().total() > 0.0);
    assert_eq!(
        first.gang_history(),
        &second.gang_history()[..first.gang_history().len()],
        "pre-step history must be unchanged"
    );
}

#[test]
fn politics_can_fracture_or_change_leadership_under_grievance() {
    let mut saw_split = false;
    let mut saw_leadership = false;
    for seed in 0..64u64 {
        let mut world = politics_world(seed);
        world.gang_politics_step();
        let kinds: Vec<GangHistoryKind> = world
            .gang_history()
            .iter()
            .map(|entry: &GangHistoryEntry| entry.kind)
            .collect();
        saw_split |= kinds.contains(&GangHistoryKind::Split);
        saw_leadership |= kinds.contains(&GangHistoryKind::LeadershipChange);
        if saw_split && saw_leadership {
            break;
        }
    }
    assert!(
        saw_split,
        "at least one deterministic seed must fracture a gang"
    );
    assert!(
        saw_leadership,
        "at least one deterministic seed must record a leadership change"
    );
}

#[test]
fn merger_moves_members_and_records_both_sides() {
    let mut world = World::new(103);
    let ids: Vec<_> = world.prisoners.iter().take(4).map(|p| p.id).collect();
    add_gang(&mut world, 1, "North Crew", ids[0]);
    world.join_gang(ids[1], GangId(1));
    add_gang(&mut world, 2, "South Crew", ids[2]);
    world.join_gang(ids[3], GangId(2));
    {
        let (left, right) = world.gangs.split_at_mut(1);
        left[0].form_alliance(&mut right[0]);
    }

    assert!(world.merge_gangs(GangId(2), GangId(1)));
    assert_eq!(world.gangs.len(), 1);
    for id in ids.iter().skip(1) {
        assert_eq!(world.prisoner_gang(*id), Some(GangId(1)));
    }
    let kinds: Vec<_> = world
        .gang_history()
        .iter()
        .map(|entry| entry.kind)
        .collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == GangHistoryKind::Merge)
            .count(),
        2
    );
}

#[test]
fn rival_encounters_can_produce_gang_clash_events() {
    fn clash_world(seed: u64) -> World {
        let mut world = politics_world(seed);
        let member = world.gangs[0].members[1];
        let rival_leader = world.gangs[1].leader.unwrap();
        for p in &mut world.prisoners {
            p.traits.sociability = 1.0;
            p.traits.aggression = 1.0;
            p.needs.respect = 1.0;
        }
        let yard = world.facilities[0].id;
        let other = world.facilities[1].id;
        for (pid, location) in world.prisoner_location.iter_mut() {
            *location = if *pid == member || *pid == rival_leader {
                yard
            } else {
                other
            };
        }
        world.guard_location.clear();
        world.social.record(
            member,
            rival_leader,
            RelationshipDelta {
                hostility: 1.0,
                rivalry: 1.0,
                ..RelationshipDelta::default()
            },
        );
        world.speed = TimeSpeed::EventDriven;
        world.time = SimTime::from_mins(59);
        world.tick();
        world
    }

    let mut saw_clash = false;
    for seed in 0..256u64 {
        let world = clash_world(seed);
        if world
            .events
            .recent()
            .any(|event| event.kind == alc_events::EventKind::GangClash)
        {
            saw_clash = true;
            break;
        }
    }
    assert!(
        saw_clash,
        "at least one deterministic seed must surface a rival encounter as a gang clash"
    );
}
