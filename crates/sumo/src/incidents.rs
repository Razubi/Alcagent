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
    /// Simulation time when this incident occurred.
    pub timestamp: SimTime,
    /// Location description (wing, node, or free-form).
    pub location: String,
    /// Prisoners involved in this incident.
    pub involved_prisoners: Vec<PrisonerId>,
    /// Severity of the incident on a scale of 0.0 to 1.0.
    pub severity: f32,
    /// Whether the incident has been resolved/investigated.
    pub resolved: bool,
}

impl Default for Incident {
    fn default() -> Self {
        Incident {
            id: 0,
            kind: EventKind::Argument,
            cause: None,
            consequence: None,
            timestamp: SimTime::default(),
            location: String::new(),
            involved_prisoners: Vec::new(),
            severity: 0.0,
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
            timestamp,
            location,
            involved_prisoners,
            severity: 0.5,
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
    }

    #[test]
    fn incident_causality_chain() {
        let cause = Incident {
            id: 1,
            kind: EventKind::Argument,
            cause: None,
            consequence: None,
            timestamp: SimTime { secs: 50 },
            location: "Cell block".to_string(),
            involved_prisoners: Vec::new(),
            severity: 0.3,
            resolved: false,
        };
        let effect = Incident::new(
            EventKind::Fight,
            SimTime { secs: 100 },
            "Yard A".to_string(),
            Vec::new(),
        )
        .with_cause(cause.id)
        .with_consequence(u64::MAX);

        assert_eq!(effect.cause, Some(1));
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
            timestamp: SimTime { secs: 50 },
            location: "Yard".to_string(),
            involved_prisoners: Vec::new(),
            severity: 0.3,
            resolved: false,
        };
        let inc2 = Incident {
            id: 2,
            kind: EventKind::Fight,
            cause: None,
            consequence: None,
            timestamp: SimTime { secs: 150 },
            location: "Yard".to_string(),
            involved_prisoners: Vec::new(),
            severity: 0.7,
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
                timestamp: SimTime { secs: 50 },
                location: "Yard A".to_string(),
                involved_prisoners: vec![PrisonerId(1)],
                severity: 0.3,
                resolved: false,
            },
            Incident {
                id: 2,
                kind: EventKind::Fight,
                cause: Some(1),
                consequence: None,
                timestamp: SimTime { secs: 150 },
                location: "Cell Block B".to_string(),
                involved_prisoners: vec![PrisonerId(1), PrisonerId(2)],
                severity: 0.8,
                resolved: true,
            },
            Incident {
                id: 3,
                kind: EventKind::Theft,
                cause: None,
                consequence: None,
                timestamp: SimTime { secs: 250 },
                location: "Yard A".to_string(),
                involved_prisoners: vec![PrisonerId(2)],
                severity: 0.5,
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