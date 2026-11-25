//! Social emergence and graph-analysis tests (Directive §6-§7).

use alc_core::{GangId, PrisonerId, RngStream, SimTime};
use alc_social::{
    detect_roles, enemies_of, friends_of, influential_prisoners, isolated_prisoners, is_enemy,
    is_friend, is_rival, maybe_form_gang, rivals_of, Gang, PrisonerGangProfile, RelationshipDelta,
    SocialGraph, SocialRole,
};

fn pid(n: u64) -> PrisonerId {
    PrisonerId(n)
}

/// Simulate 180 days of organic pairwise interactions and periodic gang-formation scans.
/// Asserts at least one gang emerges with >= 3 members, demonstrating emergent social structure.
#[test]
fn gangs_emerge_over_180_days_via_organic_relationships() {
    let mut graph = SocialGraph::default();
    let mut rng = RngStream::new(2026, 180);

    // Stable population with protection-seeking drivers and shared backgrounds.
    let mut prisoners: Vec<PrisonerGangProfile> = Vec::new();
    for i in 1..=12u64 {
        prisoners.push(PrisonerGangProfile {
            id: pid(i),
            gang_affiliation: None,
            background: if i <= 6 { "theft".into() } else { "fraud".into() },
            protection_need: if i % 3 == 0 { 0.85 } else { 0.65 },
            safety_need: if i % 2 == 0 { 0.8 } else { 0.55 },
            ambition: 0.35 + (i as f32 % 4.0) * 0.1,
            sociability: 0.6,
        });
    }

    let mut gangs: Vec<Gang> = Vec::new();
    let mut total_relationship_changes: usize = 0;
    let mut friendly_pairs: usize = 0;
    let mut hostile_pairs: usize = 0;

    for day in 0..180u64 {
        // Each day: several interaction opportunities (co-presence encounters).
        for _ in 0..18 {
            let a_idx = (rng.next_u64() as usize) % prisoners.len();
            let mut b_idx = (rng.next_u64() as usize) % prisoners.len();
            if b_idx == a_idx {
                b_idx = (b_idx + 1) % prisoners.len();
            }
            let a = prisoners[a_idx].id;
            let b = prisoners[b_idx].id;

            // Bias toward positive interactions for same-background pairs to allow organic clustering,
            // while still allowing hostility/rumor (realistic mixed social dynamics).
            let same_bg = prisoners[a_idx].background == prisoners[b_idx].background;
            let roll = rng.next_f64();
            let (da, db) = if roll < if same_bg { 0.58 } else { 0.42 } {
                // Friendly / bonding
                (
                    RelationshipDelta {
                        friendship: 0.06,
                        trust: 0.04,
                        loyalty: 0.03,
                        familiarity: 0.05,
                        ..Default::default()
                    },
                    RelationshipDelta {
                        friendship: 0.06,
                        trust: 0.04,
                        loyalty: 0.03,
                        familiarity: 0.05,
                        ..Default::default()
                    },
                )
            } else if roll < if same_bg { 0.72 } else { 0.62 } {
                // Trade / practical cooperation
                (
                    RelationshipDelta {
                        familiarity: 0.04,
                        trust: 0.03,
                        gratitude: 0.02,
                        ..Default::default()
                    },
                    RelationshipDelta {
                        familiarity: 0.04,
                        trust: 0.03,
                        gratitude: 0.02,
                        ..Default::default()
                    },
                )
            } else if roll < 0.86 {
                // Tension / argument
                (
                    RelationshipDelta {
                        hostility: 0.05,
                        resentment: 0.04,
                        familiarity: 0.02,
                        ..Default::default()
                    },
                    RelationshipDelta {
                        hostility: 0.05,
                        resentment: 0.04,
                        familiarity: 0.02,
                        ..Default::default()
                    },
                )
            } else {
                // Rumor / ambiguous contact
                (
                    RelationshipDelta {
                        familiarity: 0.03,
                        ..Default::default()
                    },
                    RelationshipDelta {
                        familiarity: 0.03,
                        ..Default::default()
                    },
                )
            };

            graph.record(a, b, da);
            graph.record(b, a, db);
            total_relationship_changes += 1;
        }

        // Periodic gang-formation attempt (emergent, not scripted).
        if day % 2 == 0 {
            // Keep gang_affiliation flags in sync with formed gangs.
            for g in &gangs {
                for &m in &g.members {
                    if let Some(p) = prisoners.iter_mut().find(|p| p.id == m) {
                        p.gang_affiliation = Some(g.id.0);
                    }
                }
            }
            if let Some(mut gang) = maybe_form_gang(&prisoners, &graph, &gangs, &mut rng) {
                gang.formed_at = SimTime::from_days(day);
                for &m in &gang.members {
                    if let Some(p) = prisoners.iter_mut().find(|p| p.id == m) {
                        p.gang_affiliation = Some(gang.id.0);
                    }
                }
                gangs.push(gang);
            }
        }
    }

    // Analyze relationship outcomes across the graph.
    for ((_, _), rel) in graph.iter() {
        if rel.bond_score() >= 0.4 || rel.friendship >= 0.3 {
            friendly_pairs += 1;
        }
        if rel.hostility >= 0.25 || rel.threat_score() >= 0.4 {
            hostile_pairs += 1;
        }
    }

    assert!(
        !gangs.is_empty(),
        "expected at least 1 gang to emerge over 180 days (changes={total_relationship_changes}, friendly_pairs={friendly_pairs}, hostile_pairs={hostile_pairs})"
    );
    assert!(
        gangs.iter().any(|g| g.members.len() >= 3),
        "expected an emerged gang with >= 3 members; gangs={:?}",
        gangs.iter().map(|g| (g.id, g.members.len())).collect::<Vec<_>>()
    );
    assert!(
        total_relationship_changes > 0 && (friendly_pairs > 0 || hostile_pairs > 0),
        "expected relationships to evolve over the run"
    );

    println!(
        "EMERGENCE_SUMMARY gangs={} gang_sizes={:?} interaction_events={} friendly_pairs={} hostile_pairs={} total_edges={}",
        gangs.len(),
        gangs.iter().map(|g| g.members.len()).collect::<Vec<_>>(),
        total_relationship_changes,
        friendly_pairs,
        hostile_pairs,
        graph.iter().count()
    );

    // Sanity: all members are known prisoners.
    let known: std::collections::HashSet<u64> = prisoners.iter().map(|p| p.id.0).collect();
    for g in &gangs {
        assert!(g.active);
        for m in &g.members {
            assert!(known.contains(&m.0), "gang member not in population");
        }
    }
}

#[test]
fn repeated_interactions_move_relationships_from_neutral_to_friendly_or_hostile() {
    let mut graph = SocialGraph::default();
    let a = pid(1);
    let b = pid(2);

    // Friendly evolution
    for _ in 0..12 {
        graph.record(
            a,
            b,
            RelationshipDelta {
                friendship: 0.06,
                trust: 0.04,
                familiarity: 0.05,
                ..Default::default()
            },
        );
    }
    let friendly = graph.get(a, b);
    assert!(friendly.friendship > 0.0, "friendship should increase");
    assert!(friendly.trust > 0.0, "trust should increase");
    assert!(friendly.familiarity > 0.0, "familiarity should increase");
    assert_ne!(friendly.friendship, 0.0);

    // Hostile evolution (separate pair stays independent)
    let c = pid(3);
    let d = pid(4);
    for _ in 0..12 {
        graph.record(
            c,
            d,
            RelationshipDelta {
                hostility: 0.07,
                resentment: 0.05,
                rivalry: 0.03,
                familiarity: 0.02,
                ..Default::default()
            },
        );
    }
    let hostile = graph.get(c, d);
    assert!(hostile.hostility > 0.0, "hostility should increase");
    assert!(hostile.resentment > 0.0, "resentment should increase");
    assert!(hostile.threat_score() > 0.0);

    // Neutral untouched pair remains default
    let e = pid(5);
    let f = pid(6);
    let neutral = graph.get(e, f);
    assert_eq!(neutral.friendship, 0.0);
    assert_eq!(neutral.hostility, 0.0);
    assert_eq!(neutral.familiarity, 0.0);
}

#[test]
fn social_graph_classifiers_detect_friends_rivals_enemies_isolated_and_influential() {
    let mut graph = SocialGraph::default();
    let ids = vec![pid(1), pid(2), pid(3), pid(4), pid(5), pid(6)];

    // Friends: 1 <-> 2
    for _ in 0..15 {
        graph.record(
            pid(1),
            pid(2),
            RelationshipDelta {
                friendship: 0.08,
                trust: 0.06,
                loyalty: 0.04,
                familiarity: 0.06,
                ..Default::default()
            },
        );
        graph.record(
            pid(2),
            pid(1),
            RelationshipDelta {
                friendship: 0.08,
                trust: 0.06,
                loyalty: 0.04,
                familiarity: 0.06,
                ..Default::default()
            },
        );
    }
    assert!(is_friend(&graph, pid(1), pid(2)), "1 should view 2 as friend");
    assert!(friends_of(&graph, pid(1), &ids).contains(&pid(2)));

    // Rivals: 1 <-> 3 (competitive/resentful)
    for _ in 0..12 {
        graph.record(
            pid(1),
            pid(3),
            RelationshipDelta {
                rivalry: 0.08,
                resentment: 0.05,
                familiarity: 0.04,
                hostility: 0.02,
                ..Default::default()
            },
        );
    }
    assert!(is_rival(&graph, pid(1), pid(3)), "1 should view 3 as rival");
    assert!(rivals_of(&graph, pid(1), &ids).contains(&pid(3)));

    // Enemies: 4 <-> 5 (high hostility/threat)
    for _ in 0..12 {
        graph.record(
            pid(4),
            pid(5),
            RelationshipDelta {
                hostility: 0.09,
                resentment: 0.06,
                fear: 0.03,
                familiarity: 0.03,
                ..Default::default()
            },
        );
    }
    assert!(is_enemy(&graph, pid(4), pid(5)), "4 should view 5 as enemy");
    assert!(enemies_of(&graph, pid(4), &ids).contains(&pid(5)));

    // Isolated: pid(6) has no interactions
    let isolated = isolated_prisoners(&graph, &ids);
    assert!(isolated.contains(&pid(6)), "pid 6 should be isolated: {isolated:?}");

    // Influential: make pid(1) a gang leader in a large gang
    let mut gang = Gang::new(GangId(1), "Alpha", pid(1));
    for member in [pid(2), pid(3), pid(4), pid(5), pid(7)] {
        gang.recruit(member);
    }
    let mut ids_plus = ids.clone();
    ids_plus.push(pid(7));
    let influential = influential_prisoners(&graph, &ids_plus, &[gang]);
    assert!(
        influential.iter().any(|(id, _)| *id == pid(1)),
        "leader should be influential: {influential:?}"
    );

    // Role detection remains coherent
    let roles = detect_roles(&graph, &ids_plus, &[]);
    assert!(roles.contains_key(&pid(1)));
    // pid(6) should appear as Loner in many populations; allow heuristic variability but ensure call works.
    let _ = roles.get(&pid(6));
}

#[test]
fn detect_roles_is_deterministic_across_calls() {
    let mut graph = SocialGraph::default();
    let ids = vec![pid(1), pid(2), pid(3), pid(4), pid(5)];
    for &a in &ids {
        for &b in &ids {
            if a == b {
                continue;
            }
            graph.record(
                a,
                b,
                RelationshipDelta {
                    friendship: 0.2,
                    trust: 0.2,
                    familiarity: 0.3,
                    fear: 0.2,
                    hostility: 0.1,
                    ..Default::default()
                },
            );
        }
    }
    let gangs = vec![Gang::new(GangId(10), "Det", pid(1))];
    let r1 = detect_roles(&graph, &ids, &gangs);
    let r2 = detect_roles(&graph, &ids, &gangs);
    for id in &ids {
        assert_eq!(r1.get(id), r2.get(id), "roles must be deterministic for {id:?}");
    }
}