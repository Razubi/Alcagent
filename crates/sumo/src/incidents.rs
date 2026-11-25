//! Structured incidents with causality for the SUMO prison simulation.
//!
//! Incidents are a sidecar storage parallel to the world event bus (EventKind enum).
//! They add causal chain tracking (cause → incident → consequence) for traceability
//! while keeping EventKind unchanged for bincode stability.

use serde::{Serialize, Deserialize};
use alc_core::{PrisonerId, SimTime};
use alc_events::EventKind;

/// Unique identifier for an incident.
pub type IncidentId = u64;

/// Disposition of an incident after investigation/response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IncidentDisposition {
    /// No disposition assigned yet.
    #[default]
    None,
    /// Investigated by staff.
    Investigated,
    /// Disciplinary action taken.
    Disciplinary,
    /// Medical response required.
    Medical,
    /// Incident fully resolved.
    Resolved,
}

/// A single response action taken in response to an incident.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseAction {
    /// Type of response.
    pub action: ResponseActionKind,
    /// Who performed the response (guard, warden, system).
    pub responder: Responder,
    /// Time the response was executed.
    pub at: SimTime,
    /// Brief description.
    pub description: String,
    /// Whether the action succeeded.
    pub success: bool,
}

/// Kind of response action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseActionKind {
    /// Guard intervention response.
    GuardIntervention,
    /// Medical response.
    MedicalResponse,
    /// Disciplinary hearing opened.
    DisciplinaryHearing,
    /// Segregation assigned.
    Segregation,
    /// Privilege revocation.
    PrivilegeRevocation,
    /// Investigation opened.
    InvestigationOpened,
    /// Lockdown triggered.
    Lockdown,
    /// Transfer initiated.
    Transfer,
    /// Counseling referral.
    CounselingReferral,
    /// Contraband search conducted.
    ContrabandSearch,
    /// Verbal warning issued.
    VerbalWarning,
    /// Written warning issued.
    WrittenWarning,
}

/// Who performed a response action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Responder {
    /// Guard who responded.
    Guard(u64),
    /// Warden who responded.
    Warden,
    /// System automated response.
    System,
    /// Medical staff response.
    MedicalStaff,
    /// Administrative staff response.
    AdminStaff,
}

/// Kind of incident, referencing the EventKind taxonomy without modifying it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Incident {
    /// Unique identifier for this incident.
    pub id: IncidentId,
    /// The type of event (argument, theft, fight, etc.) — references EventKind.
    pub kind: EventKind,
    /// Optional incident id that caused this one (for causality chain).
    pub cause: Option<IncidentId>,
    /// Optional incident id that this one causes (for causality chain).
    pub consequence: Option<IncidentId>,
    /// Full causal chain: list of incident/event IDs leading to this incident.
    pub causal_chain: Vec<CausalLink>,
    /// Simulation time when this incident occurred.
    pub timestamp: SimTime,
    /// Location description (wing, node, or free-form).
    pub location: String,
    /// Prisoners involved in this incident.
    pub involved_prisoners: Vec<PrisonerId>,
    /// Witnesses to this incident (prisoner IDs who observed it).
    pub witnesses: Vec<PrisonerId>,
    /// Severity of the incident on a scale of 0.0 to 1.0.
    pub severity: f32,
    /// Current disposition of the incident.
    #[serde(default)]
    pub disposition: IncidentDisposition,
    /// Response actions taken in reaction to this incident.
    #[serde(default)]
    pub response_actions: Vec<ResponseAction>,
    /// Whether the incident has been resolved/investigated.
    pub resolved: bool,
}

/// A link in the causal chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalLink {
    /// The incident or event ID that contributed to this incident.
    pub source_id: u64,
    /// Whether this is an incident (true) or world event (false).
    pub is_incident: bool,
    /// Human-readable description of the causal relationship.
    pub description: String,
}

impl Default for Incident {
    fn default() -> Self {
        Incident {
            id: 0,
            kind: EventKind::Argument,
            cause: None,
            consequence: None,
            causal_chain: Vec::new(),
            timestamp: SimTime::default(),
            location: String::new(),
            involved_prisoners: Vec::new(),
            witnesses: Vec::new(),
            severity: 0.0,
            disposition: IncidentDisposition::None,
            response_actions: Vec::new(),
            resolved: false,
        }
    }
}

impl Incident {
    /// Create a new incident with the given kind and basic info.
    pub fn new(kind: EventKind, timestamp: SimTime, location: String, involved_prisoners: Vec<PrisonerId>) -> Self {
        // We'll assign the id later by the engine
        Incident {
            id: 0,
            kind,
            cause: None,
            consequence: None,
            causal_chain: Vec::new(),
            timestamp,
            location,
            involved_prisoners,
            witnesses: Vec::new(),
            severity: 0.5,
            disposition: IncidentDisposition::None,
            response_actions: Vec::new(),
            resolved: false,
        }
    }

    /// Mark this incident as caused by another incident.
    pub fn with_cause(mut self, cause: IncidentId) -> Self {
        self.cause = Some(cause);
        self
    }

    /// Mark this incident as causing another incident.
    pub fn with_consequence(mut self, consequence: IncidentId) -> Self {
        self.consequence = Some(consequence);
        self
    }

    /// Add a causal link to the causal chain.
    pub fn with_causal_link(mut self, link: CausalLink) -> Self {
        self.causal_chain.push(link);
        self
    }

    /// Add multiple causal links to the causal chain.
    pub fn with_causal_chain(mut self, chain: Vec<CausalLink>) -> Self {
        self.causal_chain = chain;
        self
    }

    /// Add witnesses to this incident.
    pub fn with_witnesses(mut self, witnesses: Vec<PrisonerId>) -> Self {
        self.witnesses = witnesses;
        self
    }

    /// Add a response action taken for this incident.
    pub fn with_response_action(mut self, action: ResponseAction) -> Self {
        self.response_actions.push(action);
        self
    }

    /// Set the incident disposition.
    pub fn with_disposition(mut self, disposition: IncidentDisposition) -> Self {
        self.disposition = disposition;
        self
    }

    /// Mark as resolved.
    pub fn resolved(mut self) -> Self {
        self.resolved = true;
        self
    }

    /// Set severity.
    pub fn severity(mut self, s: f32) -> Self {
        self.severity = s.clamp(0.0, 1.0);
        self
    }
}

/// Returns incidents since a given simulation time.
///
/// This is the sidecar counterpart to `world.events.since()`.
/// It reads from the incident sidecar storage in `Sim`, not from the
/// EventKind-enriched world event bus.
pub fn incidents_since(incidents: &[Incident], at: SimTime) -> Vec<Incident> {
    incidents.iter().filter(|e| e.timestamp >= at).cloned().collect()
}

/// Declarative filter for the observer Incident Reports view (0.05).
///
/// Every field is optional: an all-`None`/all-empty filter matches every
/// incident. The filter is applied by [`filter_incidents`] and through
/// `Sim::incident_records_filtered` / `Sim::incidents_filtered`, so the UI
/// never has to re-implement incident query semantics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IncidentFilter {
    /// Only incidents at or after this time.
    pub since: Option<SimTime>,
    /// Only incidents strictly before this time.
    pub until: Option<SimTime>,
    /// Only these kinds (empty = any kind).
    pub kinds: Vec<EventKind>,
    /// Minimum severity (inclusive), 0.0..1.0.
    pub min_severity: Option<f32>,
    /// Only incidents involving this prisoner.
    pub prisoner: Option<PrisonerId>,
    /// Case-insensitive substring match on the location field.
    pub location_contains: Option<String>,
    /// Only incidents with `resolved == resolved_wanted`.
    ///
    /// `None` accepts both resolved and unresolved incidents.
    pub resolved: Option<bool>,
    /// Only incidents with this disposition.
    pub disposition: Option<IncidentDisposition>,
    /// Only incidents with at least one witness.
    pub has_witnesses: Option<bool>,
    /// Only incidents with response actions.
    pub has_response: Option<bool>,
    /// Keep only the most recent `limit` matches (applied after filtering).
    pub limit: Option<usize>,
}

impl IncidentFilter {
    /// A filter that matches every incident.
    pub fn all() -> Self {
        Self::default()
    }

    /// Builder helper: restrict to a set of event kinds.
    pub fn with_kinds(mut self, kinds: Vec<EventKind>) -> Self {
        self.kinds = kinds;
        self
    }

    /// Builder helper: restrict to a minimum severity.
    pub fn with_min_severity(mut self, min: f32) -> Self {
        self.min_severity = Some(min.clamp(0.0, 1.0));
        self
    }

    /// Builder helper: restrict to incidents involving one prisoner.
    pub fn with_prisoner(mut self, prisoner: PrisonerId) -> Self {
        self.prisoner = Some(prisoner);
        self
    }

    /// Builder helper: restrict to incidents with a specific disposition.
    pub fn with_disposition(mut self, disposition: IncidentDisposition) -> Self {
        self.disposition = Some(disposition);
        self
    }

    /// Builder helper: restrict to incidents with/without witnesses.
    pub fn with_witnesses(mut self, has_witnesses: bool) -> Self {
        self.has_witnesses = Some(has_witnesses);
        self
    }

    /// Builder helper: restrict to incidents with/without response actions.
    pub fn with_response(mut self, has_response: bool) -> Self {
        self.has_response = Some(has_response);
        self
    }

    /// Builder helper: restrict to the last `n` matches.
    pub fn with_limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Whether `incident` satisfies this filter.
    pub fn matches(&self, incident: &Incident) -> bool {
        if let Some(since) = self.since {
            if incident.timestamp < since {
                return false;
            }
        }
        if let Some(until) = self.until {
            if incident.timestamp >= until {
                return false;
            }
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&incident.kind) {
            return false;
        }
        if let Some(min) = self.min_severity {
            if incident.severity < min {
                return false;
            }
        }
        if let Some(prisoner) = self.prisoner {
            if !incident.involved_prisoners.contains(&prisoner) {
                return false;
            }
        }
        if let Some(needle) = &self.location_contains {
            let needle = needle.to_lowercase();
            if !incident.location.to_lowercase().contains(&needle) {
                return false;
            }
        }
        if let Some(want) = self.resolved {
            if incident.resolved != want {
                return false;
            }
        }
        if let Some(want) = self.disposition {
            if incident.disposition != want {
                return false;
            }
        }
        if let Some(want) = self.has_witnesses {
            if incident.witnesses.is_empty() == want {
                return false;
            }
        }
        if let Some(want) = self.has_response {
            if incident.response_actions.is_empty() == want {
                return false;
            }
        }
        true
    }
}

/// Apply an [`IncidentFilter`] to a slice of incidents.
///
/// Results keep their original (chronological) order; `limit` keeps the most
/// recent matches, which is what the observer timeline wants.
pub fn filter_incidents(incidents: &[Incident], filter: &IncidentFilter) -> Vec<Incident> {
    let mut out: Vec<Incident> = incidents
        .iter()
        .filter(|inc| filter.matches(inc))
        .cloned()
        .collect();
    if let Some(limit) = filter.limit {
        if out.len() > limit {
            out.drain(..out.len() - limit);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::SimTime;

    #[test]
    fn incident_basic_creation() {
        let inc = Incident::new(
            EventKind::Fight,
            SimTime { secs: 100 },
            "Yard A".to_string(),
            vec![],
        );
        assert_eq!(inc.kind, EventKind::Fight);
        assert_eq!(inc.timestamp, SimTime { secs: 100 });
        assert!(!inc.resolved);
        assert_eq!(inc.severity, 0.5);
        assert_eq!(inc.disposition, IncidentDisposition::None);
        assert!(inc.witnesses.is_empty());
        assert!(inc.response_actions.is_empty());
        assert!(inc.causal_chain.is_empty());
    }

    #[test]
    fn incident_causality_chain() {
        let cause = Incident {
            id: 1,
            kind: EventKind::Argument,
            cause: None,
            consequence: None,
            causal_chain: Vec::new(),
            timestamp: SimTime { secs: 50 },
            location: "Cell block".to_string(),
            involved_prisoners: Vec::new(),
            witnesses: Vec::new(),
            severity: 0.3,
            disposition: IncidentDisposition::None,
            response_actions: Vec::new(),
            resolved: false,
        };
        let effect = Incident::new(
            EventKind::Fight,
            SimTime { secs: 100 },
            "Yard A".to_string(),
            Vec::new(),
        )
        .with_cause(cause.id)
        .with_consequence(u64::MAX)
        .with_causal_link(CausalLink {
            source_id: cause.id,
            is_incident: true,
            description: "Escalated from argument".to_string(),
        });

        assert_eq!(effect.cause, Some(1));
        assert_eq!(effect.causal_chain.len(), 1);
        assert_eq!(effect.causal_chain[0].source_id, 1);
        // Note: consequence set to max() as placeholder; actual linking
        // happens when incidents are registered in the sidecar Vec.
    }

    #[test]
    fn incidents_since_filter() {
        let inc1 = Incident {
            id: 1,
            kind: EventKind::Argument,
            cause: None,
            consequence: None,
            causal_chain: Vec::new(),
            timestamp: SimTime { secs: 50 },
            location: "Yard".to_string(),
            involved_prisoners: Vec::new(),
            witnesses: Vec::new(),
            severity: 0.3,
            disposition: IncidentDisposition::None,
            response_actions: Vec::new(),
            resolved: false,
        };
        let inc2 = Incident {
            id: 2,
            kind: EventKind::Fight,
            cause: None,
            consequence: None,
            causal_chain: Vec::new(),
            timestamp: SimTime { secs: 150 },
            location: "Yard".to_string(),
            involved_prisoners: Vec::new(),
            witnesses: Vec::new(),
            severity: 0.7,
            disposition: IncidentDisposition::Investigated,
            response_actions: Vec::new(),
            resolved: true,
        };

        let results = incidents_since(&[inc1, inc2], SimTime { secs: 100 });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, 2);
    }

    fn fixture() -> Vec<Incident> {
        vec![
            Incident {
                id: 1,
                kind: EventKind::Argument,
                cause: None,
                consequence: None,
                causal_chain: Vec::new(),
                timestamp: SimTime { secs: 50 },
                location: "Yard A".to_string(),
                involved_prisoners: vec![PrisonerId(1)],
                witnesses: Vec::new(),
                severity: 0.3,
                disposition: IncidentDisposition::None,
                response_actions: Vec::new(),
                resolved: false,
            },
            Incident {
                id: 2,
                kind: EventKind::Fight,
                cause: Some(1),
                consequence: None,
                causal_chain: vec![CausalLink {
                    source_id: 1,
                    is_incident: true,
                    description: "Escalated from argument".to_string(),
                }],
                timestamp: SimTime { secs: 150 },
                location: "Cell Block B".to_string(),
                involved_prisoners: vec![PrisonerId(1), PrisonerId(2)],
                witnesses: Vec::new(),
                severity: 0.8,
                disposition: IncidentDisposition::Investigated,
                response_actions: Vec::new(),
                resolved: true,
            },
            Incident {
                id: 3,
                kind: EventKind::Theft,
                cause: None,
                consequence: None,
                causal_chain: Vec::new(),
                timestamp: SimTime { secs: 250 },
                location: "Yard A".to_string(),
                involved_prisoners: vec![PrisonerId(2)],
                witnesses: Vec::new(),
                severity: 0.5,
                disposition: IncidentDisposition::None,
                response_actions: Vec::new(),
                resolved: false,
            },
        ]
    }

    #[test]
    fn filter_by_kind_and_severity() {
        let all = fixture();
        let filter = IncidentFilter::all()
            .with_kinds(vec![EventKind::Fight, EventKind::Theft])
            .with_min_severity(0.6);
        let hits = filter_incidents(&all, &filter);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 2);
    }

    #[test]
    fn filter_by_prisoner_and_unresolved() {
        let all = fixture();
        let filter = IncidentFilter {
            prisoner: Some(PrisonerId(2)),
            resolved: Some(false),
            ..IncidentFilter::all()
        };
        let hits = filter_incidents(&all, &filter);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 3);
    }

    #[test]
    fn filter_time_window_location_and_limit() {
        let all = fixture();
        let filter = IncidentFilter {
            since: Some(SimTime { secs: 100 }),
            location_contains: Some("yard a".to_string()),
            ..IncidentFilter::all()
        };
        let hits = filter_incidents(&all, &filter);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 3);

        let limited = filter_incidents(&all, &IncidentFilter::all().with_limit(2));
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].id, 2);
        assert_eq!(limited[1].id, 3);
    }
}