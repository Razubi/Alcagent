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
}