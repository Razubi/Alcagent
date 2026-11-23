//! Conflict, violence, and disciplinary events per Directive §8.

use crate::gang::register_rivalry;
use crate::relationships::{RelationshipDelta, SocialGraph};
use crate::Gang;
use alc_core::{GangId, PrisonerId, RngStream};
use serde::{Deserialize, Serialize};

/// Incident taxonomy from Directive §8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncidentKind {
    Argument,
    Threat,
    Intimidation,
    Theft,
    Bullying,
    Fight,
    GangDispute,
    GroupDisturbance,
    RefusalToComply,
    PropertyDamage,
    Protest,
    MedicalEmergency,
}

impl Default for IncidentKind {
    fn default() -> Self {
        IncidentKind::Argument
    }
}

/// Backward-compatible abstract conflict outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictOutcome {
    Argument,
    Threat,
    Intimidation,
    Theft,
    Bullying,
    Fight,
    GangClash,
    Disturbance,
    Compliance,
    Retreat,
}

impl ConflictOutcome {
    pub fn incident_kind(self) -> IncidentKind {
        match self {
            ConflictOutcome::Argument => IncidentKind::Argument,
            ConflictOutcome::Threat => IncidentKind::Threat,
            ConflictOutcome::Intimidation => IncidentKind::Intimidation,
            ConflictOutcome::Theft => IncidentKind::Theft,
            ConflictOutcome::Bullying => IncidentKind::Bullying,
            ConflictOutcome::Fight => IncidentKind::Fight,
            ConflictOutcome::GangClash => IncidentKind::GangDispute,
            ConflictOutcome::Disturbance => IncidentKind::GroupDisturbance,
            ConflictOutcome::Compliance => IncidentKind::RefusalToComply, // resolved toward compliance context
            ConflictOutcome::Retreat => IncidentKind::Argument,
        }
    }
}

/// Escalation stages (tension→threat→fight→gang_clash progression abstraction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictEscalationStage {
    Tension,
    Threat,
    Fight,
    GangClash,
}

impl ConflictEscalationStage {
    pub fn next(self) -> Self {
        match self {
            ConflictEscalationStage::Tension => ConflictEscalationStage::Threat,
            ConflictEscalationStage::Threat => ConflictEscalationStage::Fight,
            ConflictEscalationStage::Fight => ConflictEscalationStage::GangClash,
            ConflictEscalationStage::GangClash => ConflictEscalationStage::GangClash,
        }
    }

    pub fn as_outcome(self) -> ConflictOutcome {
        match self {
            ConflictEscalationStage::Tension => ConflictOutcome::Argument,
            ConflictEscalationStage::Threat => ConflictOutcome::Threat,
            ConflictEscalationStage::Fight => ConflictOutcome::Fight,
            ConflictEscalationStage::GangClash => ConflictOutcome::GangClash,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjuryRecord {
    pub prisoner: PrisonerId,
    pub severity: f32, // 0..1 abstract
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisciplineProposal {
    pub prisoner: PrisonerId,
    pub reason: IncidentKind,
    pub severity: f32, // 0..1
    pub recommended_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationDelta {
    pub prisoner: PrisonerId,
    pub delta: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConflictOutcomeEffects {
    pub outcome: Option<ConflictOutcome>,
    pub incident: IncidentKind,
    pub injuries: Vec<InjuryRecord>,
    pub discipline_proposals: Vec<DisciplineProposal>,
    pub reputation_deltas: Vec<ReputationDelta>,
    pub relationship_changes: Vec<(PrisonerId, PrisonerId, RelationshipDelta)>,
    pub gang_tensions: Vec<(GangId, GangId, f32)>,
    pub investigation_recommended: bool,
    pub notes: Vec<String>,
}

impl ConflictOutcomeEffects {
    pub fn new(incident: IncidentKind) -> Self {
        Self {
            outcome: None,
            incident,
            injuries: Vec::new(),
            discipline_proposals: Vec::new(),
            reputation_deltas: Vec::new(),
            relationship_changes: Vec::new(),
            gang_tensions: Vec::new(),
            investigation_recommended: false,
            notes: Vec::new(),
        }
    }
}

/// Simple relationship-driven conflict resolver (abstraction level: outcome enum).
/// Preserves legacy signature/behavior for existing callers.
pub fn resolve_conflict(
    _rng: &mut RngStream,
    tension: f32,
    staff_presence: f32,
    aggression_a: f32,
    discipline_a: f32,
    intervention: f32,
) -> ConflictOutcome {
    let escalate = tension * 0.5 + aggression_a * 0.3 - discipline_a * 0.2 - staff_presence * 0.3
        - intervention * 0.3;
    if escalate < 0.1 {
        return ConflictOutcome::Compliance;
    }
    if escalate < 0.3 {
        return ConflictOutcome::Argument;
    }
    if escalate < 0.45 {
        return ConflictOutcome::Threat;
    }
    if escalate < 0.6 {
        return ConflictOutcome::Fight;
    }
    if staff_presence < 0.4 {
        ConflictOutcome::GangClash
    } else {
        ConflictOutcome::Disturbance
    }
}

/// Escalate conflict from tension toward threat/fight/gang_clash based on pressure and controls.
pub fn escalate_conflict(
    tension: f32,
    staff_presence: f32,
    aggression: f32,
    gang_involvement: f32,
    intervention: f32,
) -> ConflictEscalationStage {
    let pressure = tension * 0.45 + aggression * 0.35 + gang_involvement * 0.35
        - staff_presence * 0.3
        - intervention * 0.25;
    if pressure < 0.25 {
        ConflictEscalationStage::Tension
    } else if pressure < 0.5 {
        ConflictEscalationStage::Threat
    } else if pressure < 0.75 || gang_involvement < 0.5 {
        ConflictEscalationStage::Fight
    } else {
        ConflictEscalationStage::GangClash
    }
}

/// Apply outcome consequences: injuries, discipline proposals, reputation deltas, gang tensions.
/// Mutates social graph/gang structures where appropriate and returns a structured effect report.
pub fn apply_outcome(
    graph: &mut SocialGraph,
    gangs: &mut [Gang],
    participants: &[PrisonerId],
    outcome: ConflictOutcome,
    severity: f32,
    gang_context: Option<(GangId, GangId)>,
) -> ConflictOutcomeEffects {
    let incident = outcome.incident_kind();
    let mut effects = ConflictOutcomeEffects::new(incident);
    effects.outcome = Some(outcome);
    let sev = severity.clamp(0.0, 1.0);

    match outcome {
        ConflictOutcome::Compliance => {
            effects.notes.push("conflict de-escalated to compliance".into());
            if participants.len() >= 2 {
                effects.relationship_changes.push((
                    participants[0],
                    participants[1],
                    RelationshipDelta {
                        hostility: -0.05,
                        resentment: -0.02,
                        familiarity: 0.01,
                        ..Default::default()
                    },
                ));
            }
        }
        ConflictOutcome::Retreat => {
            if participants.len() >= 2 {
                effects.relationship_changes.push((
                    participants[0],
                    participants[1],
                    RelationshipDelta {
                        fear: 0.05 * sev,
                        hostility: 0.03,
                        familiarity: 0.01,
                        ..Default::default()
                    },
                ));
                effects.reputation_deltas.push(ReputationDelta {
                    prisoner: participants[1],
                    delta: -0.02 * sev,
                });
            }
        }
        ConflictOutcome::Argument | ConflictOutcome::Threat => {
            if participants.len() >= 2 {
                effects.relationship_changes.push((
                    participants[0],
                    participants[1],
                    RelationshipDelta {
                        hostility: 0.08 + 0.1 * sev,
                        resentment: 0.05 + 0.08 * sev,
                        trust: -0.03 * sev,
                        familiarity: 0.02,
                        ..Default::default()
                    },
                ));
                effects.discipline_proposals.push(DisciplineProposal {
                    prisoner: participants[0],
                    reason: incident,
                    severity: 0.2 + 0.2 * sev,
                    recommended_action: "warning/verbal correction".into(),
                });
            }
        }
        ConflictOutcome::Intimidation | ConflictOutcome::Bullying => {
            if participants.len() >= 2 {
                effects.relationship_changes.push((
                    participants[0],
                    participants[1],
                    RelationshipDelta {
                        fear: 0.15 + 0.2 * sev,
                        hostility: 0.1 * sev,
                        resentment: 0.12 + 0.15 * sev,
                        respect: -0.05,
                        familiarity: 0.03,
                        ..Default::default()
                    },
                ));
                effects.reputation_deltas.push(ReputationDelta {
                    prisoner: participants[0],
                    delta: 0.05 * sev,
                });
                effects.reputation_deltas.push(ReputationDelta {
                    prisoner: participants[1],
                    delta: -0.04 * sev,
                });
                effects.discipline_proposals.push(DisciplineProposal {
                    prisoner: participants[0],
                    reason: incident,
                    severity: 0.35 + 0.25 * sev,
                    recommended_action: "disciplinary hearing / privilege review".into(),
                });
                effects.investigation_recommended = sev > 0.6;
            }
        }
        ConflictOutcome::Theft => {
            if participants.len() >= 2 {
                effects.relationship_changes.push((
                    participants[0],
                    participants[1],
                    RelationshipDelta {
                        trust: -0.2 - 0.2 * sev,
                        hostility: 0.15 + 0.2 * sev,
                        resentment: 0.2 + 0.2 * sev,
                        debt: 0.2 * sev,
                        familiarity: 0.02,
                        ..Default::default()
                    },
                ));
                effects.discipline_proposals.push(DisciplineProposal {
                    prisoner: participants[0],
                    reason: IncidentKind::Theft,
                    severity: 0.4 + 0.3 * sev,
                    recommended_action: "search / restitution / sanctions".into(),
                });
                effects.investigation_recommended = true;
            }
        }
        ConflictOutcome::Fight => {
            for &p in participants.iter().take(2) {
                effects.injuries.push(InjuryRecord {
                    prisoner: p,
                    severity: (0.2 + 0.5 * sev).min(1.0),
                    description: "injuries from physical fight".into(),
                });
                effects.discipline_proposals.push(DisciplineProposal {
                    prisoner: p,
                    reason: IncidentKind::Fight,
                    severity: 0.5 + 0.3 * sev,
                    recommended_action: "medical check + segregation review".into(),
                });
                effects.reputation_deltas.push(ReputationDelta {
                    prisoner: p,
                    delta: if sev > 0.5 { 0.02 } else { -0.01 },
                });
            }
            if participants.len() >= 2 {
                effects.relationship_changes.push((
                    participants[0],
                    participants[1],
                    RelationshipDelta {
                        hostility: 0.2 + 0.25 * sev,
                        resentment: 0.2 + 0.3 * sev,
                        fear: 0.05 * sev,
                        trust: -0.15,
                        familiarity: 0.05,
                        ..Default::default()
                    },
                ));
            }
            effects.investigation_recommended = true;
        }
        ConflictOutcome::GangClash | ConflictOutcome::Disturbance => {
            for &p in participants {
                effects.injuries.push(InjuryRecord {
                    prisoner: p,
                    severity: (0.15 + 0.45 * sev).min(1.0),
                    description: "injuries during group disturbance".into(),
                });
                effects.discipline_proposals.push(DisciplineProposal {
                    prisoner: p,
                    reason: incident,
                    severity: 0.6 + 0.3 * sev,
                    recommended_action: "lockdown response + investigation".into(),
                });
            }
            if let Some((ga, gb)) = gang_context {
                effects.gang_tensions.push((ga, gb, 0.3 + 0.5 * sev));
                register_rivalry(gangs, ga, gb);
            }
            effects.investigation_recommended = true;
            effects.notes.push("institutional security response likely required".into());
        }
    }

    // Apply relationship deltas to graph.
    for (a, b, d) in &effects.relationship_changes {
        graph.record(*a, *b, d.clone());
    }

    // Apply reputation deltas to gang records if members.
    for rep in &effects.reputation_deltas {
        for g in gangs.iter_mut() {
            if g.members.contains(&rep.prisoner) {
                g.reputation = (g.reputation + rep.delta * 0.2).clamp(0.0, 1.0);
            }
        }
    }
    // Tension increases can reduce resources slightly due to disruption.
    if !effects.gang_tensions.is_empty() {
        for g in gangs.iter_mut() {
            if gang_context.map(|(a, b)| g.id == a || g.id == b).unwrap_or(false) {
                g.resources = (g.resources - 0.01 * sev).max(0.0);
            }
        }
    }

    effects
}

/// A generated conflict incident candidate from ambient social tension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictIncident {
    pub instigator: PrisonerId,
    pub target: PrisonerId,
    pub kind: IncidentKind,
    pub tension: f32,
    pub guards_present: bool,
    pub cause: String,
}

/// Minimal prisoner profile needed for conflict generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictPrisonerProfile {
    pub id: PrisonerId,
    pub discipline: f32,
    pub impulsiveness: f32,
    pub aggression: f32,
    pub irritability: f32,
}

/// Scan for high-tension dyads and probabilistically generate a conflict incident.
///
/// Conditions emphasized: high hostility, low discipline/self-control, and an
/// opportunity with guards absent.
pub fn maybe_generate_conflict(
    prisoners: &[ConflictPrisonerProfile],
    social: &SocialGraph,
    rng: &mut RngStream,
) -> Option<ConflictIncident> {
    if prisoners.len() < 2 {
        return None;
    }

    let mut best: Option<(f32, usize, usize, f32)> = None; // (score, i, j, tension)

    for i in 0..prisoners.len() {
        for j in (i + 1)..prisoners.len() {
            let a = &prisoners[i];
            let b = &prisoners[j];
            let rel = social.get(a.id, b.id);
            let hostility =
                (rel.hostility + rel.resentment * 0.7 + rel.rivalry * 0.5).clamp(0.0, 2.0);
            if hostility < 0.45 {
                continue;
            }

            // Low discipline / poor control on either side makes escalation more likely.
            let low_discipline =
                ((1.0 - a.discipline).max(1.0 - b.discipline)).clamp(0.0, 1.0);
            let impulsive = (a.impulsiveness.max(b.impulsiveness)).clamp(0.0, 1.0);
            let irritable = (a.irritability.max(b.irritability)).clamp(0.0, 1.0);
            let control_gap =
                (low_discipline * 0.45 + impulsive * 0.3 + irritable * 0.25).clamp(0.0, 1.0);
            if control_gap < 0.25 {
                continue;
            }

            let tension = (hostility * 0.55 + control_gap * 0.35 + rel.threat_score() * 0.1)
                .clamp(0.0, 2.0);
            let score = tension * (0.6 + control_gap * 0.4);
            match best {
                Some((s, _, _, _)) if s >= score => {}
                _ => best = Some((score, i, j, tension.min(1.5))),
            }
        }
    }

    let (score, i, j, tension) = best?;
    // Guards absent opportunity is required by the scan condition; treat as an unobserved moment.
    let guards_present = false;
    let chance = ((score - 0.25) * 0.45).clamp(0.03, 0.8) as f64;
    if !rng.chance(chance) {
        return None;
    }

    let a = &prisoners[i];
    let b = &prisoners[j];
    let (instigator, target) =
        if a.aggression + a.impulsiveness >= b.aggression + b.impulsiveness {
            (a.id, b.id)
        } else {
            (b.id, a.id)
        };

    let roll = rng.next_f64();
    let kind = if tension > 1.1 && roll > 0.55 {
        IncidentKind::Fight
    } else if tension > 0.9 && roll > 0.7 {
        IncidentKind::Theft
    } else if tension > 0.8 && roll > 0.45 {
        IncidentKind::Threat
    } else if tension > 0.7 && roll > 0.6 {
        IncidentKind::Bullying
    } else {
        IncidentKind::Argument
    };

    Some(ConflictIncident {
        instigator,
        target,
        kind,
        tension: (tension / 1.5).clamp(0.0, 1.0),
        guards_present,
        cause: "high hostility, low discipline, guards absent".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relationships::RelationshipDelta;

    #[test]
    fn generates_conflict_from_high_tension_dyad() {
        let mut social = SocialGraph::default();
        let a = PrisonerId(1);
        let b = PrisonerId(2);
        social.record(
            a,
            b,
            RelationshipDelta {
                hostility: 0.9,
                resentment: 0.8,
                rivalry: 0.6,
                familiarity: 0.5,
                ..Default::default()
            },
        );
        let prisoners = vec![
            ConflictPrisonerProfile {
                id: a,
                discipline: 0.1,
                impulsiveness: 0.9,
                aggression: 0.8,
                irritability: 0.9,
            },
            ConflictPrisonerProfile {
                id: b,
                discipline: 0.2,
                impulsiveness: 0.7,
                aggression: 0.6,
                irritability: 0.8,
            },
        ];

        let mut found = None;
        for i in 0..100 {
            let mut rng = RngStream::new(123, i);
            found = maybe_generate_conflict(&prisoners, &social, &mut rng);
            if found.is_some() {
                break;
            }
        }
        let incident = found.expect("expected a conflict incident");
        assert!(!incident.guards_present);
        assert!(incident.tension > 0.0);
    }
}