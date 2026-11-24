//! Guard subagents per Directive §10.

use alc_core::{GuardId, RngStream, SimTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GuardRank { Officer, Sergeant, Lieutenant, Captain }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum PostType {
    Tower, Wing, Corridor, Yard, Reception, Medical, Work, Visitor, Admin,
}

impl PostType {
    /// Whether this post is typically staffed around the clock.
    pub fn is_24h(self) -> bool {
        matches!(
            self,
            PostType::Tower
                | PostType::Wing
                | PostType::Corridor
                | PostType::Yard
                | PostType::Reception
                | PostType::Medical
                | PostType::Admin
        )
    }

    /// Baseline minimum staffing recommendation for a post instance.
    pub fn min_staffing(self) -> u32 {
        match self {
            PostType::Tower => 1,
            PostType::Wing => 2,
            PostType::Corridor => 1,
            PostType::Yard => 2,
            PostType::Reception => 1,
            PostType::Medical => 1,
            PostType::Work => 1,
            PostType::Visitor => 2,
            PostType::Admin => 1,
        }
    }

    /// Relative incident/activity intensity used for fatigue and observation load.
    pub fn risk_profile(self) -> f32 {
        match self {
            PostType::Yard => 0.9,
            PostType::Wing => 0.7,
            PostType::Reception => 0.6,
            PostType::Work => 0.5,
            PostType::Visitor => 0.5,
            PostType::Corridor => 0.4,
            PostType::Medical => 0.4,
            PostType::Tower => 0.3,
            PostType::Admin => 0.2,
        }
    }

    pub fn coverage_hours(self) -> &'static str {
        match self {
            PostType::Work => "program-hours",
            PostType::Visitor => "visitation-hours",
            _ if self.is_24h() => "24/7",
            _ => "scheduled",
        }
    }
}

/// Authority levels L0–L5 as used in guard duty and escalation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AuthorityLevel {
    L0, // Observe, log
    L1, // Verbal directive, warning
    L2, // Escort, seize contraband, deny entry
    L3, // Physical intervention, restraint
    L4, // Emergency authorization (area lockdown, medical override)
    L5, // Institution-wide command in warden's absence
}

impl AuthorityLevel {
    pub fn as_u8(self) -> u8 {
        match self {
            AuthorityLevel::L0 => 0,
            AuthorityLevel::L1 => 1,
            AuthorityLevel::L2 => 2,
            AuthorityLevel::L3 => 3,
            AuthorityLevel::L4 => 4,
            AuthorityLevel::L5 => 5,
        }
    }
}

impl GuardRank {
    /// Baseline authority cap for the rank (can be contextually elevated by policy/emergency).
    pub fn authority_cap(self) -> AuthorityLevel {
        match self {
            GuardRank::Officer => AuthorityLevel::L2,
            GuardRank::Sergeant => AuthorityLevel::L3,
            GuardRank::Lieutenant => AuthorityLevel::L4,
            GuardRank::Captain => AuthorityLevel::L5,
        }
    }
}

/// Simple 3-shift rotation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftRotation {
    /// Current shift index (0 = day, 1 = evening, 2 = night) by convention.
    pub current_shift: u8,
    /// Ticks/hours spent on current shift (abstract counter for stub rotation).
    pub ticks_on_shift: u32,
    /// Rotation period in duty ticks before advancing shift.
    pub rotation_period_ticks: u32,
    /// Consecutive shifts worked without extended rest.
    pub consecutive_shifts: u32,
}

impl Default for ShiftRotation {
    fn default() -> Self {
        Self {
            current_shift: 0,
            ticks_on_shift: 0,
            rotation_period_ticks: 480, // abstract: e.g. 8h * 60m
            consecutive_shifts: 0,
        }
    }
}

impl ShiftRotation {
    /// Advance rotation counters; returns true if shift boundary crossed.
    pub fn advance(&mut self) -> bool {
        self.ticks_on_shift = self.ticks_on_shift.saturating_add(1);
        if self.ticks_on_shift >= self.rotation_period_ticks {
            self.ticks_on_shift = 0;
            self.current_shift = (self.current_shift + 1) % 3;
            self.consecutive_shifts = self.consecutive_shifts.saturating_add(1);
            true
        } else {
            false
        }
    }

    pub fn rest(&mut self) {
        self.ticks_on_shift = 0;
        self.consecutive_shifts = 0;
    }
}

/// Credibility-scored report produced by a guard duty action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardReport {
    pub at: SimTime,
    pub post: PostType,
    pub summary: String,
    pub severity: u8, // 0=info .. 3=critical
    pub credibility: f32,
    pub witnessed_directly: bool,
    pub requests_assistance: bool,
}

/// Severity band for guard incidents and reports (0=info .. 3=critical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SeverityLevel {
    Info,
    Minor,
    Serious,
    Critical,
}

impl SeverityLevel {
    /// Convert a raw `0..=3` severity into a level; values above 3 clamp to Critical.
    pub fn from_u8(severity: u8) -> Self {
        match severity {
            0 => SeverityLevel::Info,
            1 => SeverityLevel::Minor,
            2 => SeverityLevel::Serious,
            _ => SeverityLevel::Critical,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            SeverityLevel::Info => 0,
            SeverityLevel::Minor => 1,
            SeverityLevel::Serious => 2,
            SeverityLevel::Critical => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SeverityLevel::Info => "Info",
            SeverityLevel::Minor => "Minor",
            SeverityLevel::Serious => "Serious",
            SeverityLevel::Critical => "Critical",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SeverityLevel::Info => "Routine observation; no action required.",
            SeverityLevel::Minor => "Low-impact incident; note and monitor.",
            SeverityLevel::Serious => "Significant incident; enforcement or follow-up warranted.",
            SeverityLevel::Critical => "Critical incident; immediate response and escalation.",
        }
    }

    pub fn is_critical(self) -> bool {
        matches!(self, SeverityLevel::Critical)
    }
}

/// An enriched guard incident: a [`GuardReport`] plus the spatial and actor
/// context that makes it inspectable and auditable. [`GuardReport`] itself is
/// intentionally unchanged so existing duty/report behavior and callers stay
/// compatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardIncident {
    /// The underlying credibility-scored report.
    pub report: GuardReport,
    /// Location where the incident occurred (post name, wing, facility, etc.).
    pub location: String,
    /// Prisoner/guard ids involved, if known.
    pub involved_actors: Vec<u64>,
    /// Incident kind, e.g. "fight", "contraband", "medical", "disturbance".
    pub incident_kind: String,
}

impl GuardIncident {
    pub fn from_report(report: GuardReport, location: impl Into<String>, involved_actors: Vec<u64>, incident_kind: impl Into<String>) -> Self {
        Self {
            report,
            location: location.into(),
            involved_actors,
            incident_kind: incident_kind.into(),
        }
    }

    pub fn severity_level(&self) -> SeverityLevel {
        SeverityLevel::from_u8(self.report.severity)
    }

    pub fn one_line_summary(&self) -> String {
        format!(
            "[{}] {} at {}: {} (actors: {})",
            self.severity_level().label(),
            self.incident_kind,
            self.location,
            self.report.summary,
            if self.involved_actors.is_empty() {
                "none".to_string()
            } else {
                self.involved_actors.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(",")
            },
        )
    }
}

/// Minimal policy/context snapshot for guard duty decisions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DutyContext {
    pub at: SimTime,
    pub lockdown: bool,
    pub security_level: f32,
}

/// Outcome of one duty-loop pass.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DutyLoopOutcome {
    pub observation: Option<String>,
    pub report: Option<GuardReport>,
    pub enforcement_action: Option<String>,
    pub assistance_requested: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guard {
    pub id: GuardId,
    pub name: String,
    pub rank: GuardRank,
    pub personality: f32,   // 0 = rigid by-the-book, 1 = lenient
    pub competence: f32,
    pub experience: f32,
    pub morale: f32,
    pub fatigue: f32,
    pub stress: f32,
    pub loyalty: f32,
    pub integrity: f32,
    pub post: PostType,
    pub current_task: String,
    pub shift: u8,
    pub history: Vec<String>,
    /// Current authority level in effect (normally rank cap, may be adjusted by context).
    #[serde(default = "default_authority")]
    pub authority: AuthorityLevel,
    /// Shift rotation state.
    #[serde(default)]
    pub rotation: ShiftRotation,
    /// Rolling credibility signal for recent reporting quality.
    #[serde(default = "default_credibility")]
    pub report_credibility: f32,
    /// Sim-hour of the last assistance escalation (cooldown gate).
    #[serde(default)]
    pub last_escalation_hour: u64,
}

fn default_authority() -> AuthorityLevel {
    AuthorityLevel::L2
}

fn default_credibility() -> f32 {
    0.5
}

impl Guard {
    pub fn new(id: GuardId, name: impl Into<String>, rank: GuardRank, post: PostType, rng: &mut RngStream) -> Self {
        let competence = rng.range(0.2, 0.95) as f32;
        let experience = rng.range(0.0, 1.0) as f32;
        let fatigue: f32 = 0.0;
        let credibility = (competence * 0.5 + experience * 0.3 + (1.0 - fatigue) * 0.2) as f32;
        Self {
            id,
            name: name.into(),
            rank,
            personality: rng.range(0.1, 0.9) as f32,
            competence,
            experience,
            morale: rng.range(0.5, 0.9) as f32,
            fatigue,
            stress: rng.range(0.0, 0.3) as f32,
            loyalty: rng.range(0.4, 0.9) as f32,
            integrity: rng.range(0.3, 0.95) as f32,
            post,
            current_task: "patrol".into(),
            shift: 0,
            history: Vec::new(),
            authority: rank.authority_cap(),
            rotation: ShiftRotation::default(),
            report_credibility: credibility.clamp(0.05, 0.99),
            last_escalation_hour: 0,
        }
    }

    /// Duty tick: fatigue rises, stress from incidents decays slowly.
    pub fn tick(&mut self, _at: SimTime, incident_load: f32) {
        let rotated = self.rotation.advance();
        self.shift = self.rotation.current_shift;
        let fatigue_rate = 0.008 + self.post.risk_profile() * 0.004;
        self.fatigue = (self.fatigue + fatigue_rate).min(1.0);
        // Stress equilibrates toward the current incident load: busy posts
        // keep guards on edge, quiet ones let them recover. A ratchet-only
        // model saturated every guard at 1.0 and made them escalate forever.
        self.stress = (self.stress + (incident_load - self.stress) * 0.05).clamp(0.0, 1.0);
        if self.fatigue > 0.8 { self.morale = (self.morale - 0.001).max(0.0); }
        if rotated {
            self.experience = (self.experience + 0.0005).min(1.0);
            self.current_task = "patrol".into();
            // Abstract off-shift recovery marker in history (stub).
            self.history.push(format!("shift rotation -> {}", self.shift));
            if self.rotation.consecutive_shifts >= 6 {
                self.stress = (self.stress + 0.02).min(1.0);
                self.morale = (self.morale - 0.01).max(0.0);
            }
        }
        self.report_credibility = self.compute_credibility(true);
    }

    /// Credibility of this guard's report (used by warden inbox).
    pub fn credibility(&self) -> f32 {
        self.compute_credibility(true)
    }

    fn compute_credibility(&self, witnessed_directly: bool) -> f32 {
        let witness = if witnessed_directly { 1.0 } else { 0.5 };
        let stress_impairment = (self.stress * 0.25).min(0.25);
        let fatigue_penalty = self.fatigue * 0.10;
        let base = 0.45 * self.competence
            + 0.20 * self.integrity
            + 0.15 * self.experience
            + 0.10 * witness
            + 0.10 * self.morale;
        (base - stress_impairment - fatigue_penalty).clamp(0.05, 0.99)
    }

    /// Duty-loop step: observe surroundings/post state (stub).
    pub fn observe(&mut self, at: SimTime, incident_load: f32) -> Option<String> {
        self.current_task = "observe".into();
        let salience = incident_load + self.post.risk_profile() * 0.1 + self.stress * 0.05;
        if salience > 0.55 {
            let note = format!(
                "t={} post={:?}: notable activity observed (salience {:.2})",
                at.as_secs(),
                self.post,
                salience.min(1.0)
            );
            self.history.push(format!("observe@{}", at.as_secs()));
            Some(note)
        } else {
            None
        }
    }

    /// Duty-loop step: file a credibility-scored report (stub).
    pub fn report(&mut self, at: SimTime, summary: impl Into<String>, severity: u8, witnessed_directly: bool) -> GuardReport {
        self.current_task = "report".into();
        let credibility = self.compute_credibility(witnessed_directly);
        self.report_credibility = credibility;
        let report = GuardReport {
            at,
            post: self.post,
            summary: summary.into(),
            severity: severity.min(3),
            credibility,
            witnessed_directly,
            requests_assistance: false,
        };
        self.history.push(format!(
            "report@{} sev={} cred={:.2}",
            at.as_secs(),
            report.severity,
            report.credibility
        ));
        report
    }

    /// Duty-loop step: enforce rules within authority (stub).
    pub fn enforce(&mut self, at: SimTime, situation: impl Into<String>) -> Option<String> {
        self.current_task = "enforce".into();
        let required = if self.post.risk_profile() > 0.75 { AuthorityLevel::L3 } else { AuthorityLevel::L2 };
        if self.authority >= required {
            let action = format!(
                "t={} {:?} enforced {:?} action: {}",
                at.as_secs(),
                self.rank,
                required,
                situation.into()
            );
            self.history.push(format!("enforce@{}", at.as_secs()));
            self.stress = (self.stress + 0.01).min(1.0);
            Some(action)
        } else {
            self.history.push(format!("enforce_denied@{}", at.as_secs()));
            None
        }
    }

    /// Duty-loop step: request assistance/escalation (stub).
    pub fn request_assistance(&mut self, at: SimTime, reason: impl Into<String>) -> GuardReport {
        self.current_task = "request_assistance".into();
        let reason = reason.into();
        let mut report = self.report(
            at,
            format!("Assistance requested: {reason}"),
            3,
            true,
        );
        report.requests_assistance = true;
        self.stress = (self.stress + 0.03).min(1.0);
        report
    }

    /// File a credibility-scored report and wrap it in an enriched [`GuardIncident`]
    /// with location and involved actors. Does not alter duty/report behavior;
    /// use this when a report needs to be auditable by place and participants.
    pub fn report_incident(
        &mut self,
        at: SimTime,
        summary: impl Into<String>,
        severity: u8,
        location: impl Into<String>,
        involved_actors: Vec<u64>,
        witnessed_directly: bool,
    ) -> GuardIncident {
        let report = self.report(at, summary, severity, witnessed_directly);
        GuardIncident::from_report(report, location, involved_actors, "incident")
    }

    /// Build an enriched [`GuardIncident`] from an existing report without
    /// writing to guard history or credibility (read-only enrichment).
    pub fn incident_from_report(
        &self,
        report: &GuardReport,
        location: impl Into<String>,
        involved_actors: Vec<u64>,
        incident_kind: impl Into<String>,
    ) -> GuardIncident {
        GuardIncident::from_report(report.clone(), location, involved_actors, incident_kind)
    }

    /// Duty-loop stub: observe → report → enforce → request_assistance.
    pub fn duty_loop(&mut self, at: SimTime, incident_load: f32) -> DutyLoopOutcome {
        let mut outcome = DutyLoopOutcome::default();
        outcome.observation = self.observe(at, incident_load);

        if let Some(obs) = &outcome.observation {
            let severity = if incident_load > 0.8 { 3 } else if incident_load > 0.5 { 2 } else { 1 };
            let mut report = self.report(at, obs.clone(), severity, true);
            if incident_load > 0.85 {
                outcome.enforcement_action = self.enforce(at, "stabilize incident");
                if outcome.enforcement_action.is_none() || incident_load > 0.95 {
                    let assist = self.request_assistance(at, "incident exceeds safe solo response");
                    report.requests_assistance = true;
                    outcome.assistance_requested = true;
                    outcome.report = Some(assist);
                } else {
                    outcome.report = Some(report);
                }
            } else if incident_load > 0.6 {
                outcome.enforcement_action = self.enforce(at, "issue directive / de-escalate");
                outcome.report = Some(report);
            } else {
                outcome.report = Some(report);
            }
        }

        outcome
    }

    /// Rotate to the next shift explicitly (e.g. schedule boundary handoff).
    pub fn rotate_shift(&mut self) {
        self.rotation.ticks_on_shift = self.rotation.rotation_period_ticks;
        let _ = self.rotation.advance();
        self.shift = self.rotation.current_shift;
    }

    /// Observe nearby prisoners, report incidents with credibility, and request
    /// assistance when stress is high.
    ///
    /// `world_prisoners` contains per-prisoner observed tension/pressure values
    /// (0..1, one entry per nearby prisoner). `world_policies` provides the
    /// current simulation time and relevant security posture.
    pub fn duty_tick(
        &mut self,
        world_prisoners: &[f32],
        world_policies: DutyContext,
        rng: &mut RngStream,
    ) -> Vec<GuardReport> {
        let at = world_policies.at;
        let mut reports = Vec::new();

        let nearby = world_prisoners.len() as f32;
        let avg_tension = if world_prisoners.is_empty() {
            0.0
        } else {
            (world_prisoners.iter().copied().sum::<f32>() / nearby).clamp(0.0, 1.0)
        };
        let peak_tension = world_prisoners
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
            .clamp(0.0, 1.0);

        let lockdown_pressure = if world_policies.lockdown { 0.08 } else { 0.0 };
        let incident_load = (avg_tension * 0.6
            + peak_tension * 0.4
            + self.post.risk_profile() * 0.15
            + world_policies.security_level.clamp(0.0, 1.0) * 0.1
            + lockdown_pressure)
            .clamp(0.0, 1.0);

        // Base fatigue/stress/rotation progression for this duty tick.
        self.tick(at, incident_load);

        // Observe: always maintain situational awareness; notable observations
        // become reports when something stands out.
        let observation = self.observe(at, incident_load);
        let notable = observation.is_some() || peak_tension > 0.7 || incident_load > 0.6;
        if notable {
            let severity = if peak_tension > 0.95 || incident_load > 0.95 {
                3
            } else if peak_tension > 0.8 || incident_load > 0.8 {
                2
            } else {
                1
            };
            let summary = observation.unwrap_or_else(|| {
                format!(
                    "Routine observation at {:?}: {} nearby prisoner(s), tension {:.2}",
                    self.post,
                    world_prisoners.len(),
                    peak_tension
                )
            });

            let witnessed_directly = rng.chance((0.55 + self.competence as f64 * 0.35).min(0.98));
            let mut report = self.report(at, summary, severity, witnessed_directly);

            // Attempt enforcement when the situation warrants it and authority allows.
            if incident_load > 0.6 || peak_tension > 0.7 {
                let _ = self.enforce(at, "contain escalating incident");
            }

            // Request assistance when stress is high or the situation exceeds
            // safe response. A per-guard cooldown keeps a stressed guard from
            // re-escalating on every hourly duty tick.
            let hour = at.as_secs() / 3600;
            let can_escalate = hour.saturating_sub(self.last_escalation_hour) >= 12;
            let stress_high = self.stress > 0.8 && incident_load > 0.6;
            let overwhelmed =
                peak_tension > 0.9 || (incident_load > 0.85 && self.authority < AuthorityLevel::L3);
            if (stress_high || overwhelmed) && can_escalate {
                self.last_escalation_hour = hour;
                let reason = if stress_high {
                    "high stress during incident response"
                } else {
                    "incident exceeds safe solo response"
                };
                let mut assist = self.request_assistance(at, reason);
                assist.severity = assist.severity.max(severity).max(if overwhelmed { 3 } else { 2 });
                assist.credibility = self.compute_credibility(true);
                report.requests_assistance = true;
                reports.push(assist);
            }

            reports.push(report);
        } else if self.stress > 0.85
            && (at.as_secs() / 3600).saturating_sub(self.last_escalation_hour) >= 12
        {
            // Even without a visible incident, an over-stressed guard asks for relief.
            self.last_escalation_hour = at.as_secs() / 3600;
            let assist = self.request_assistance(at, "guard stress critical; relief requested");
            reports.push(assist);
        }

        // Keep credibility signal current for consumers (warden inbox, etc.).
        self.report_credibility = self.compute_credibility(true);
        reports
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alc_core::{GuardId, RngStream, SimTime};

    #[test]
    fn duty_tick_sustained_tension_escalates_at_most_once_per_12h_and_stress_bounded() {
        let mut guard = Guard::new(
            GuardId(1),
            "Test Guard",
            GuardRank::Officer,
            PostType::Yard,
            &mut RngStream::new(42, 1),
        );
        let mut rng = RngStream::new(42, 2);
        let mut assistance_reports = 0u32;
        for hour in 0..(7 * 24u64) {
            let ctx = DutyContext {
                at: SimTime::from_secs(hour * 3600),
                lockdown: false,
                security_level: 1.0,
            };
            let reports = guard.duty_tick(&[1.0], ctx, &mut rng);
            for r in &reports {
                // Count actual escalation requests (the routine observation may also be flagged).
                if r.requests_assistance && r.summary.starts_with("Assistance requested:") {
                    assistance_reports += 1;
                }
                assert!(r.credibility.is_finite(), "credibility not finite at hour {hour}");
                assert!(
                    (0.0..=1.0).contains(&r.credibility),
                    "credibility out of range at hour {hour}: {}",
                    r.credibility
                );
            }
            assert!(guard.stress.is_finite(), "stress NaN/inf at hour {hour}");
            assert!(
                (0.0..=1.0).contains(&guard.stress),
                "stress out of range at hour {hour}: {}",
                guard.stress
            );
            assert!(guard.fatigue.is_finite(), "fatigue NaN/inf at hour {hour}");
            assert!(
                (0.0..=1.0).contains(&guard.fatigue),
                "fatigue out of range at hour {hour}: {}",
                guard.fatigue
            );
            assert!(guard.morale.is_finite(), "morale NaN/inf at hour {hour}");
            assert!(
                (0.0..=1.0).contains(&guard.morale),
                "morale out of range at hour {hour}: {}",
                guard.morale
            );
        }
        assert!(
            assistance_reports <= 14,
            "too many assistance escalations over 7 days: {assistance_reports}"
        );
    }
}