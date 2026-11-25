//! Spec 008: pure gang-politics helpers and contraband gang-control effects.

use alc_core::{GangId, PrisonerId, RngStream};
use alc_social::contraband::{ContrabandPrisonerProfile, contraband_market};
use alc_social::relationships::{RelationshipDelta, SocialGraph};
use alc_social::{
    DisputeAction, Gang, GangPoliticsProfile, GroupDirective, InternalDispute, gang_pressure,
    gang_relation_scores, leadership_challenger, split_candidates,
};
use alc_social::{detect_internal_disputes, gang_event_cause};

fn politics_profile(
    prisoner: u64,
    loyalty: f32,
    autonomy: f32,
    discipline: f32,
    leadership: f32,
    ambition: f32,
    goal_conflict: f32,
) -> GangPoliticsProfile {
    GangPoliticsProfile {
        prisoner: PrisonerId(prisoner),
        loyalty,
        autonomy,
        discipline,
        leadership,
        ambition,
        goal_conflict,
    }
}

fn dispute(prisoner: u64, action: DisputeAction, reason: &str) -> InternalDispute {
    InternalDispute {
        prisoner: PrisonerId(prisoner),
        action,
        grievance_delta: 0.2,
        reason: reason.to_owned(),
    }
}

#[test]
fn fracture_uses_supplied_id_and_registers_mutual_rivalry() {
    let mut gang = Gang::new(GangId(1), "A", PrisonerId(1));
    for id in 2..=5 {
        gang.recruit(PrisonerId(id));
    }
    gang.resources = 1.0;

    let splinter = gang
        .fracture(&[PrisonerId(4), PrisonerId(3)], GangId(77))
        .expect("fracture should succeed");

    assert_eq!(splinter.id, GangId(77));
    assert_eq!(splinter.members, vec![PrisonerId(3), PrisonerId(4)]);
    assert_eq!(splinter.leader, Some(PrisonerId(3)));
    assert!(!gang.members.contains(&PrisonerId(3)));
    assert!(!gang.members.contains(&PrisonerId(4)));
    assert!(gang.rivalries.contains(&GangId(77)));
    assert!(splinter.rivalries.contains(&GangId(1)));
    assert!((gang.resources - 0.7).abs() < 0.001);

    // A caller-supplied id equal to the parent is rejected without mutation.
    assert!(gang.fracture(&[PrisonerId(5)], GangId(1)).is_none());
    assert!(gang.members.contains(&PrisonerId(5)));
    assert_eq!(gang_event_cause(GangId(77)), "gang:77");
}

#[test]
fn merge_from_moves_members_resources_and_relations() {
    let mut left = Gang::new(GangId(1), "Left", PrisonerId(1));
    left.recruit(PrisonerId(2));
    left.resources = 1.0;
    left.rivalries = vec![GangId(2)];

    let mut right = Gang::new(GangId(2), "Right", PrisonerId(3));
    right.recruit(PrisonerId(4));
    right.resources = 2.0;
    right.rivalries = vec![GangId(9)];
    right.alliances = vec![GangId(8)];

    let moved = left.merge_from(&mut right);

    assert_eq!(moved, vec![PrisonerId(3), PrisonerId(4)]);
    assert!(left.members.contains(&PrisonerId(3)));
    assert!(left.members.contains(&PrisonerId(4)));
    assert!((left.resources - 3.0).abs() < 0.001);
    assert!(left.rivalries.contains(&GangId(9)));
    assert!(left.alliances.contains(&GangId(8)));
    assert!(!left.rivalries.contains(&GangId(2)));
    assert!(!left.alliances.contains(&GangId(1)));
    assert!(right.members.is_empty());
    assert!(right.hierarchy.is_empty());
    assert!(!right.active);
    assert_eq!(right.resources, 0.0);
}

#[test]
fn relation_scores_distinguish_allies_from_rivals() {
    let mut a = Gang::new(GangId(1), "A", PrisonerId(1));
    a.goals = vec!["protection".into(), "respect".into()];
    let mut b = Gang::new(GangId(2), "B", PrisonerId(3));
    b.goals = vec!["PROTECTION".into(), "respect".into()];

    let mut graph = SocialGraph::default();
    for &x in &[PrisonerId(1), PrisonerId(2)] {
        for &y in &[PrisonerId(3), PrisonerId(4)] {
            graph.record(
                x,
                y,
                RelationshipDelta {
                    friendship: 0.9,
                    trust: 0.8,
                    loyalty: 0.6,
                    familiarity: 0.7,
                    ..Default::default()
                },
            );
        }
    }

    let (friendly, hostile) = gang_relation_scores(&a, &b, &graph);
    assert!(friendly > hostile);
    assert!(friendly > 0.6);
    assert!(friendly <= 1.0 && hostile <= 1.0);

    let mut hostile_graph = SocialGraph::default();
    for &x in &[PrisonerId(1), PrisonerId(2)] {
        for &y in &[PrisonerId(3), PrisonerId(4)] {
            hostile_graph.record(
                x,
                y,
                RelationshipDelta {
                    hostility: 0.9,
                    rivalry: 0.8,
                    fear: 0.6,
                    resentment: 0.5,
                    familiarity: 0.7,
                    ..Default::default()
                },
            );
        }
    }
    a.form_rivalry(&mut b);
    let (low_affinity, high_hostility) = gang_relation_scores(&a, &b, &hostile_graph);
    assert!(high_hostility > low_affinity);
    assert!(high_hostility > 0.7);

    let (self_affinity, self_hostility) = gang_relation_scores(&a, &a, &graph);
    assert_eq!(self_affinity, 1.0);
    assert_eq!(self_hostility, 0.0);
}

#[test]
fn disputes_ignore_the_leader_and_are_sorted() {
    let mut gang = Gang::new(GangId(1), "A", PrisonerId(1));
    for id in 2..=3 {
        gang.recruit(PrisonerId(id));
    }
    let profiles = vec![
        politics_profile(1, 0.0, 0.9, 0.1, 0.9, 0.9, 1.0),
        politics_profile(3, 0.2, 0.2, 0.9, 0.1, 0.1, 0.6),
        politics_profile(2, 0.1, 0.2, 0.6, 0.8, 0.8, 0.9),
    ];
    let mut graph = SocialGraph::default();
    graph.record(
        PrisonerId(2),
        PrisonerId(1),
        RelationshipDelta {
            hostility: 0.8,
            rivalry: 0.5,
            fear: 0.5,
            resentment: 0.5,
            ..Default::default()
        },
    );
    let directive = GroupDirective {
        description: "run the stall".into(),
        pressure: 0.5,
        expected_gain: 0.1,
        personal_risk: 0.2,
    };

    let disputes = detect_internal_disputes(&gang, &graph, &profiles, &directive);
    assert_eq!(
        disputes.iter().map(|d| d.prisoner).collect::<Vec<_>>(),
        vec![PrisonerId(2), PrisonerId(3)]
    );
    assert_eq!(disputes[0].action, DisputeAction::ChallengeLeader);
    assert!(disputes.iter().all(|d| d.grievance_delta > 0.0));
    assert!(gang.internal_disputes.is_empty());
    assert_eq!(gang.leader, Some(PrisonerId(1)));
}

#[test]
fn split_candidates_require_a_shared_grievance() {
    let mut gang = Gang::new(GangId(1), "A", PrisonerId(1));
    for id in 2..=5 {
        gang.recruit(PrisonerId(id));
    }
    let coherent = vec![
        dispute(4, DisputeAction::ChallengeLeader, "goal conflict"),
        dispute(2, DisputeAction::Refuse, "goal conflict"),
    ];
    assert_eq!(
        split_candidates(&gang, &coherent),
        Some(vec![PrisonerId(2), PrisonerId(4)])
    );

    let mut incoherent = coherent.clone();
    incoherent.push(dispute(3, DisputeAction::Covert, "leader distrust"));
    incoherent.push(dispute(5, DisputeAction::Refuse, "low loyalty"));
    assert_eq!(split_candidates(&gang, &incoherent), None);
    assert_eq!(split_candidates(&gang, &coherent[..1]), None);
}

#[test]
fn leadership_challenger_prefers_leadership_and_grievance() {
    let mut gang = Gang::new(GangId(1), "A", PrisonerId(1));
    for id in 2..=3 {
        gang.recruit(PrisonerId(id));
    }
    let strong = politics_profile(3, 0.05, 0.8, 0.2, 0.9, 0.9, 0.9);
    let weak = politics_profile(2, 0.9, 0.1, 0.9, 0.1, 0.1, 0.1);
    let profiles = vec![weak, strong];

    for seed in 0..32u64 {
        let mut rng = RngStream::new(17, seed);
        assert_eq!(
            leadership_challenger(&gang, &profiles, &mut rng),
            Some(PrisonerId(3))
        );
    }
}

#[test]
fn gang_pressure_is_bounded_and_deterministic() {
    let empty = gang_pressure(&[], &SocialGraph::default());
    assert_eq!(empty.total(), 0.0);

    let mut a = Gang::new(GangId(1), "A", PrisonerId(1));
    a.recruit(PrisonerId(2));
    let mut b = Gang::new(GangId(2), "B", PrisonerId(3));
    b.recruit(PrisonerId(4));
    a.form_rivalry(&mut b);
    for _ in 0..20 {
        a.record_dispute("pressure");
    }
    let mut graph = SocialGraph::default();
    graph.record(
        PrisonerId(1),
        PrisonerId(3),
        RelationshipDelta {
            hostility: 0.9,
            rivalry: 0.9,
            fear: 0.8,
            familiarity: 0.9,
            ..Default::default()
        },
    );
    let gangs = vec![a, b];
    let first = gang_pressure(&gangs, &graph);
    let second = gang_pressure(&gangs, &graph);
    assert_eq!(first, second);
    assert!(first.rivalry <= 1.0 && first.disputes <= 1.0 && first.contraband <= 1.0);
    assert!(first.total() > 0.0 && first.total() <= 1.0);
}

fn market_profile(id: u64, gang_control: f32) -> ContrabandPrisonerProfile {
    ContrabandPrisonerProfile {
        id: PrisonerId(id),
        discipline: 0.8,
        risk_tolerance: 0.4,
        manipulation: 0.5,
        aggression: 0.5,
        stimulation: 0.6,
        freedom: 0.2,
        safety: 0.5,
        desperation: 0.6,
        gang_control,
    }
}

fn count_market_offers(gang_control: f32) -> usize {
    let prisoners = vec![
        market_profile(1, gang_control),
        market_profile(2, gang_control),
    ];
    (0..200usize)
        .map(|seed| {
            let mut rng = RngStream::new(11, seed as u64);
            contraband_market(&prisoners, &mut rng).len()
        })
        .sum()
}

#[test]
fn contraband_gang_control_raises_market_activity_but_is_bounded() {
    let without_control = count_market_offers(0.0);
    let with_control = count_market_offers(1.0);
    let with_absurd_control = count_market_offers(f32::MAX);
    assert!(with_control > without_control);
    assert_eq!(with_control, with_absurd_control);

    let prisoners = vec![market_profile(1, 1.0), market_profile(2, 1.0)];
    let mut rng = RngStream::new(11, 3);
    for offer in contraband_market(&prisoners, &mut rng) {
        assert!(offer.price <= 216);
    }
}
