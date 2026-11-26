//! Guard subagents per Directive §10.

use alc_core::{GuardId, RngStream, SimTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GuardRank {
    Officer,
    Sergeant,
    Lieutenant,
    Captain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum PostType {
    Tower,
    Wing,
    Corridor,
    Yard,
    Reception,
    Medical,
    Work,
    Visitor,
    Admin,
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
            rotation_period_ticks: 8, // eight hourly duty ticks before a rest block
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
    pub fn from_report(
        report: GuardReport,
        location: impl Into<String>,
        involved_actors: Vec<u64>,
        incident_kind: impl Into<String>,
    ) -> Self {
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
                self.involved_actors
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
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
    /// Warden-level tolerance for corner-cutting, 0..1. Biases guard
    /// violations and makes corruption a visible environmental pressure.
    #[serde(default)]
    pub corruption_pressure: f32,
}

/// Outcome of one duty-loop pass.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DutyLoopOutcome {
    pub observation: Option<String>,
    pub report: Option<GuardReport>,
    pub enforcement_action: Option<String>,
    pub assistance_requested: bool,
}

/// One co-located prisoner observed from a guard's assigned post.
///
/// The world supplies only prisoners sharing the guard's current facility, so
/// duty perception is area-scoped rather than omniscient.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GuardObservation {
    pub prisoner_id: u64,
    pub facility_id: u64,
    /// Local friction signal in `0.0..=1.0`.
    pub tension: f32,
}

/// Durable impression of one prisoner.
///
/// Impressions are encoded in [`Guard::history`] instead of a new persisted
/// field: this keeps the version-5 bincode layout loadable while still making
/// opinions survive save/load.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GuardOpinion {
    pub suspicion: f32,
    pub trust: f32,
    pub fear: f32,
    pub last_seen_hour: u64,
}

impl Default for GuardOpinion {
    fn default() -> Self {
        Self {
            suspicion: 0.0,
            trust: 0.35,
            fear: 0.0,
            last_seen_hour: 0,
        }
    }
}

/// Guard needs (0..1, higher = more unmet). These are simpler than prisoner
/// needs but still affect performance and reporting quality.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GuardNeeds {
    pub rest: f32,
    pub social: f32,
    pub safety: f32,
    pub purpose: f32,
    pub hygiene: f32,
    pub stimulation: f32,
}

impl Default for GuardNeeds {
    fn default() -> Self {
        Self {
            rest: 0.2,
            social: 0.4,
            safety: 0.2,
            purpose: 0.3,
            hygiene: 0.2,
            stimulation: 0.4,
        }
    }
}

impl GuardNeeds {
    /// Drift needs during a duty tick. `load` and `tension` are 0..1.
    pub fn drift_duty(&mut self, load: f32, tension: f32) {
        self.rest = (self.rest + 0.015 + load * 0.02).clamp(0.0, 1.0);
        self.safety = (self.safety + tension * 0.03).clamp(0.0, 1.0);
        self.hygiene = (self.hygiene + 0.005 + load * 0.01).clamp(0.0, 1.0);
        self.stimulation = (self.stimulation + 0.01 + tension * 0.01).clamp(0.0, 1.0);
        self.social = (self.social + 0.005).clamp(0.0, 1.0);
        self.purpose = (self.purpose - 0.005).clamp(0.0, 1.0);
    }

    /// Recover needs during off-duty rest.
    pub fn recover_off_duty(&mut self) {
        self.recover_with(OffDutyActivity::Rest);
    }

    /// Recover one off-duty block with the chosen activity. Rest restores the
    /// most physical needs, socialising restores belonging, and training trades
    /// some rest for purpose and stimulation.
    pub fn recover_with(&mut self, activity: OffDutyActivity) {
        let (rest, social, purpose, stimulation, hygiene) = match activity {
            OffDutyActivity::Rest => (0.09, 0.02, 0.01, 0.01, 0.05),
            OffDutyActivity::Socialize => (0.04, 0.10, 0.02, 0.03, 0.03),
            OffDutyActivity::Train => (0.03, 0.02, 0.08, 0.06, 0.04),
            OffDutyActivity::OffHoursPatrol => (0.01, 0.01, 0.06, 0.02, 0.01),
        };
        self.rest = (self.rest - rest).clamp(0.0, 1.0);
        self.social = (self.social - social).clamp(0.0, 1.0);
        self.safety = (self.safety - 0.02).clamp(0.0, 1.0);
        self.hygiene = (self.hygiene - hygiene).clamp(0.0, 1.0);
        self.stimulation = (self.stimulation - stimulation).clamp(0.0, 1.0);
        self.purpose = (self.purpose - purpose).clamp(0.0, 1.0);
    }

    /// Average unmet need across every tracked need.
    #[must_use]
    pub fn pressure(&self) -> f32 {
        (self.rest + self.social + self.safety + self.purpose + self.hygiene + self.stimulation)
            / 6.0
    }

    /// Performance penalty from unmet needs, weighted toward rest and safety.
    #[must_use]
    pub fn performance_penalty(&self) -> f32 {
        (self.rest * 0.35
            + self.safety * 0.20
            + self.hygiene * 0.15
            + self.purpose * 0.12
            + self.stimulation * 0.10
            + self.social * 0.08)
            .clamp(0.0, 1.0)
    }

    /// Most urgent unmet need and its value.
    #[must_use]
    pub fn dominant(&self) -> (GuardNeedKind, f32) {
        let mut best = (GuardNeedKind::Rest, self.rest);
        for (kind, value) in [
            (GuardNeedKind::Social, self.social),
            (GuardNeedKind::Safety, self.safety),
            (GuardNeedKind::Purpose, self.purpose),
            (GuardNeedKind::Hygiene, self.hygiene),
            (GuardNeedKind::Stimulation, self.stimulation),
        ] {
            if value > best.1 {
                best = (kind, value);
            }
        }
        best
    }
}

/// Guard need labels for UI and narrative text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardNeedKind {
    Rest,
    Social,
    Safety,
    Purpose,
    Hygiene,
    Stimulation,
}

impl GuardNeedKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rest => "rest",
            Self::Social => "social",
            Self::Safety => "safety",
            Self::Purpose => "purpose",
            Self::Hygiene => "hygiene",
            Self::Stimulation => "stimulation",
        }
    }
}

/// How a guard spends an off-duty block. Off-duty behaviour is deliberately
/// simple but has real next-shift consequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OffDutyActivity {
    #[default]
    Rest,
    Socialize,
    Train,
    OffHoursPatrol,
}

impl OffDutyActivity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rest => "rest",
            Self::Socialize => "socialize",
            Self::Train => "train",
            Self::OffHoursPatrol => "off-hours patrol",
        }
    }

    /// Deterministic activity selection from a guard's id and shift.
    #[must_use]
    pub fn for_guard(id: GuardId, shift: u8) -> Self {
        match (id.raw().wrapping_add(shift as u64)) % 4 {
            0 => Self::Rest,
            1 => Self::Socialize,
            2 => Self::Train,
            _ => Self::OffHoursPatrol,
        }
    }
}

/// A durable off-duty social bond between two guards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardBond {
    pub guard: GuardId,
    pub strength: f32,
    pub last_day: u64,
}

/// Kind of direct guard → prisoner interaction produced by a duty tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardInteractionKind {
    Observe,
    Directive,
    Restrain,
    Assist,
    PolicyViolation,
}

impl GuardInteractionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Directive => "directive",
            Self::Restrain => "restraint",
            Self::Assist => "assist",
            Self::PolicyViolation => "policy_violation",
        }
    }
}

/// A structured interaction the world can apply to prisoner state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardInteraction {
    pub at: SimTime,
    pub guard: GuardId,
    pub prisoner_id: u64,
    pub kind: GuardInteractionKind,
    /// Bounded intensity/strength in `0.0..=1.0`.
    pub intensity: f32,
}

/// Full result of one area-scoped duty tick.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuardDutyResult {
    pub reports: Vec<GuardReport>,
    pub interactions: Vec<GuardInteraction>,
    /// Mistakes made during this tick (cumulative totals live in history).
    pub mistakes: u32,
    /// Policy violations made during this tick (cumulative totals in history).
    pub violations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guard {
    pub id: GuardId,
    pub name: String,
    pub rank: GuardRank,
    pub personality: f32, // 0 = rigid by-the-book, 1 = lenient
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
    /// Cumulative mistakes; survives bounded history trimming.
    #[serde(default)]
    pub cumulative_mistakes: u32,
    /// Cumulative policy violations; survives bounded history trimming.
    #[serde(default)]
    pub cumulative_violations: u32,
    /// Guard needs (rest, social, safety, purpose, hygiene, stimulation).
    #[serde(default)]
    pub needs: GuardNeeds,
    /// Whether the guard is currently in an off-duty recovery block.
    #[serde(default)]
    pub off_duty: bool,
    /// Activity chosen for the current/last off-duty block.
    #[serde(default)]
    pub off_duty_activity: OffDutyActivity,
    /// Remaining hourly ticks in the current off-duty block.
    #[serde(default)]
    pub off_duty_ticks: u8,
    /// Bounded off-duty social bonds with other guards.
    #[serde(default)]
    pub bonds: Vec<GuardBond>,
}

fn default_authority() -> AuthorityLevel {
    AuthorityLevel::L2
}

fn default_credibility() -> f32 {
    0.5
}

impl Guard {
    pub fn new(
        id: GuardId,
        name: impl Into<String>,
        rank: GuardRank,
        post: PostType,
        rng: &mut RngStream,
    ) -> Self {
        let competence = rng.range(0.2, 0.95) as f32;
        let experience = rng.range(0.0, 1.0) as f32;
        let fatigue: f32 = 0.0;
        let credibility = (competence * 0.5 + experience * 0.3 + (1.0 - fatigue) * 0.2) as f32;
        let mut rotation = ShiftRotation::default();
        // Stagger rest blocks so the whole shift never goes off duty at once.
        rotation.ticks_on_shift = (id.raw() % rotation.rotation_period_ticks.max(1) as u64) as u32;
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
            rotation,
            report_credibility: credibility.clamp(0.05, 0.99),
            last_escalation_hour: 0,
            cumulative_mistakes: 0,
            cumulative_violations: 0,
            needs: GuardNeeds::default(),
            off_duty: false,
            off_duty_activity: OffDutyActivity::Rest,
            off_duty_ticks: 0,
            bonds: Vec::new(),
        }
    }

    /// Duty tick: fatigue rises, stress from incidents decays slowly, and every
    /// eight hourly duty ticks the guard enters a short off-duty recovery block.
    pub fn tick(&mut self, at: SimTime, incident_load: f32) {
        if self.off_duty_ticks > 0 {
            self.off_duty_recovery_tick(at);
            return;
        }
        let rotated = self.rotation.advance();
        self.shift = self.rotation.current_shift;
        let fatigue_rate = 0.008 + self.post.risk_profile() * 0.004;
        self.fatigue = (self.fatigue + fatigue_rate).min(1.0);
        // Stress equilibrates toward the current incident load: busy posts
        // keep guards on edge, quiet ones let them recover. A ratchet-only
        // model saturated every guard at 1.0 and made them escalate forever.
        self.stress = (self.stress + (incident_load - self.stress) * 0.05).clamp(0.0, 1.0);
        if self.fatigue > 0.8 {
            self.morale = (self.morale - 0.001).max(0.0);
        }
        if rotated {
            self.experience = (self.experience + 0.0005).min(1.0);
            self.begin_off_duty(at);
            if self.rotation.consecutive_shifts >= 6 {
                self.stress = (self.stress + 0.02).min(1.0);
                self.morale = (self.morale - 0.01).max(0.0);
            }
        }
        self.report_credibility = self.compute_credibility(true);
    }

    /// Begin a two-hour off-duty recovery block with a deterministic activity.
    pub fn begin_off_duty(&mut self, at: SimTime) {
        self.off_duty_activity = OffDutyActivity::for_guard(self.id, self.shift);
        self.off_duty_ticks = 2;
        self.off_duty = true;
        self.needs.recover_with(self.off_duty_activity);
        self.apply_off_duty_effects();
        self.current_task = format!("off-duty: {}", self.off_duty_activity.label());
        self.push_history(format!(
            "off_duty@{}:{}",
            at.as_secs() / 3600,
            self.off_duty_activity.label()
        ));
    }

    /// One hourly off-duty tick. Needs recover and the block eventually ends.
    pub fn off_duty_recovery_tick(&mut self, _at: SimTime) {
        self.off_duty = true;
        self.needs.recover_with(self.off_duty_activity);
        self.apply_off_duty_effects();
        self.off_duty_ticks = self.off_duty_ticks.saturating_sub(1);
        if self.off_duty_ticks == 0 {
            self.off_duty = false;
            self.current_task = "patrol".into();
        }
        self.report_credibility = self.compute_credibility(true);
    }

    fn apply_off_duty_effects(&mut self) {
        match self.off_duty_activity {
            OffDutyActivity::Rest => {
                self.fatigue = (self.fatigue - 0.12).max(0.0);
            }
            OffDutyActivity::Socialize => {
                self.morale = (self.morale + 0.03).min(1.0);
                self.stress = (self.stress - 0.04).max(0.0);
            }
            OffDutyActivity::Train => {
                self.experience = (self.experience + 0.01).min(1.0);
                self.fatigue = (self.fatigue - 0.04).max(0.0);
            }
            OffDutyActivity::OffHoursPatrol => {
                self.fatigue = (self.fatigue - 0.02).max(0.0);
                self.stress = (self.stress - 0.01).max(0.0);
            }
        }
    }

    /// Strengthen (or create) a bounded bond with another guard.
    pub fn form_bond(&mut self, peer: GuardId, day: u64) {
        if self.id == peer {
            return;
        }
        if let Some(bond) = self.bonds.iter_mut().find(|b| b.guard == peer) {
            bond.strength = (bond.strength + 0.12).min(1.0);
            bond.last_day = day;
        } else {
            self.bonds.push(GuardBond {
                guard: peer,
                strength: 0.15,
                last_day: day,
            });
            if self.bonds.len() > 12 {
                self.bonds.sort_by(|a, b| {
                    a.strength
                        .total_cmp(&b.strength)
                        .then_with(|| a.guard.raw().cmp(&b.guard.raw()))
                });
                self.bonds.remove(0);
            }
        }
        self.needs.social = (self.needs.social - 0.02).clamp(0.0, 1.0);
    }

    /// Composite performance score used to rank promotions and reductions.
    #[must_use]
    pub fn performance_score(&self) -> f32 {
        let base = self.competence * 0.35
            + self.experience * 0.20
            + self.integrity * 0.15
            + self.morale * 0.15
            + self.report_credibility * 0.15;
        let penalties = self.cumulative_mistakes as f32 * 0.01
            + self.cumulative_violations as f32 * 0.04
            + self.fatigue * 0.10
            + self.stress * 0.10;
        (base - penalties).clamp(0.0, 1.0)
    }

    /// Apply the result of a misconduct investigation. Returns true when
    /// corruption or a policy violation was substantiated.
    pub fn apply_investigation(&mut self, found_misconduct: bool, day: u64, detail: &str) -> bool {
        if found_misconduct {
            self.integrity = (self.integrity - 0.15).max(0.0);
            self.morale = (self.morale - 0.10).max(0.0);
            self.loyalty = (self.loyalty - 0.08).max(0.0);
            self.cumulative_violations = self.cumulative_violations.saturating_add(1);
            self.push_history(format!("investigation@{day}:substantiated:{detail}"));
            true
        } else {
            self.morale = (self.morale + 0.02).min(1.0);
            self.push_history(format!("investigation@{day}:cleared"));
            false
        }
    }

    /// Credibility of this guard's report (used by warden inbox).
    pub fn credibility(&self) -> f32 {
        self.compute_credibility(true)
    }

    fn compute_credibility(&self, witnessed_directly: bool) -> f32 {
        let witness = if witnessed_directly { 1.0 } else { 0.5 };
        let stress_impairment = (self.stress * 0.25).min(0.25);
        let fatigue_penalty = self.fatigue * 0.10;
        let needs_penalty = self.needs.performance_penalty() * 0.25;
        let base = 0.45 * self.competence
            + 0.20 * self.integrity
            + 0.15 * self.experience
            + 0.10 * witness
            + 0.10 * self.morale;
        (base - stress_impairment - fatigue_penalty - needs_penalty).clamp(0.05, 0.99)
    }

    /// Guard history is a bounded audit log. Bounding it keeps long runs fast
    /// while retaining the structured opinion/mistake/violation markers that
    /// the observer surfaces derive from.
    const HISTORY_CAP: usize = 256;

    fn push_history(&mut self, entry: String) {
        self.history.push(entry);
        if self.history.len() > Self::HISTORY_CAP {
            let drop = self.history.len() - Self::HISTORY_CAP;
            self.history.drain(0..drop);
        }
    }

    fn history_f32(entry: &str) -> f32 {
        entry.parse::<f32>().unwrap_or(0.0).clamp(0.0, 1.0)
    }

    /// Latest impression formed about a prisoner, derived from durable history.
    pub fn opinion_of(&self, prisoner_id: u64) -> GuardOpinion {
        let prefix = format!("opinion@");
        for entry in self.history.iter().rev() {
            let Some(rest) = entry.strip_prefix(&prefix) else {
                continue;
            };
            let mut parts = rest.split(':');
            let hour = parts
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let id = parts
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(u64::MAX);
            if id != prisoner_id {
                continue;
            }
            let suspicion = parts.next().map(Self::history_f32).unwrap_or(0.0);
            let trust = parts.next().map(Self::history_f32).unwrap_or(0.35);
            let fear = parts.next().map(Self::history_f32).unwrap_or(0.0);
            return GuardOpinion {
                suspicion,
                trust,
                fear,
                last_seen_hour: hour,
            };
        }
        GuardOpinion::default()
    }

    /// Number of distinct prisoners this guard has formed an opinion about.
    pub fn opinion_count(&self) -> usize {
        let mut ids = Vec::new();
        for entry in &self.history {
            let Some(rest) = entry.strip_prefix("opinion@") else {
                continue;
            };
            let mut parts = rest.split(':');
            let _ = parts.next();
            if let Some(id) = parts.next().and_then(|s| s.parse::<u64>().ok()) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        ids.len()
    }

    /// Cumulative number of recorded perceptual/enforcement mistakes.
    /// History is bounded, so the durable counter is authoritative.
    pub fn mistake_count(&self) -> u32 {
        self.cumulative_mistakes
    }

    /// Cumulative number of recorded policy violations.
    /// History is bounded, so the durable counter is authoritative.
    pub fn violation_count(&self) -> u32 {
        self.cumulative_violations
    }

    /// Rebuild cumulative audit counters from legacy history on load.
    ///
    /// Older saves predate the durable counters, so their totals live only in
    /// the bounded history log. New saves keep nonzero counters authoritative;
    /// this only fills a genuine zero baseline and never double-counts.
    pub fn restore_legacy_counters_from_history(&mut self) {
        if self.cumulative_mistakes == 0 {
            let seen = self
                .history
                .iter()
                .filter(|entry| entry.starts_with("mistake@"))
                .count();
            self.cumulative_mistakes = u32::try_from(seen).unwrap_or(u32::MAX);
        }
        if self.cumulative_violations == 0 {
            let seen = self
                .history
                .iter()
                .filter(|entry| entry.starts_with("violation@"))
                .count();
            self.cumulative_violations = u32::try_from(seen).unwrap_or(u32::MAX);
        }
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
            self.push_history(format!("observe@{}", at.as_secs()));
            Some(note)
        } else {
            None
        }
    }

    /// Duty-loop step: file a credibility-scored report (stub).
    pub fn report(
        &mut self,
        at: SimTime,
        summary: impl Into<String>,
        severity: u8,
        witnessed_directly: bool,
    ) -> GuardReport {
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
        self.push_history(format!(
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
        let required = if self.post.risk_profile() > 0.75 {
            AuthorityLevel::L3
        } else {
            AuthorityLevel::L2
        };
        if self.authority >= required {
            let action = format!(
                "t={} {:?} enforced {:?} action: {}",
                at.as_secs(),
                self.rank,
                required,
                situation.into()
            );
            self.push_history(format!("enforce@{}", at.as_secs()));
            self.stress = (self.stress + 0.01).min(1.0);
            Some(action)
        } else {
            self.push_history(format!("enforce_denied@{}", at.as_secs()));
            None
        }
    }

    /// Duty-loop step: request assistance/escalation (stub).
    pub fn request_assistance(&mut self, at: SimTime, reason: impl Into<String>) -> GuardReport {
        self.current_task = "request_assistance".into();
        let reason = reason.into();
        let mut report = self.report(at, format!("Assistance requested: {reason}"), 3, true);
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
            let severity = if incident_load > 0.8 {
                3
            } else if incident_load > 0.5 {
                2
            } else {
                1
            };
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
        let rotated = self.rotation.advance();
        self.shift = self.rotation.current_shift;
        if rotated {
            self.needs.recover_off_duty();
        }
    }

    /// Area-scoped duty tick: observe only the supplied co-located prisoners,
    /// form opinions, interact directly, file imperfect reports, and escalate.
    ///
    /// Returns structured interactions so the world can apply their prisoner
    /// consequences without the guard reaching into arbitrary simulation state.
    pub fn duty_tick_area(
        &mut self,
        observations: &[GuardObservation],
        policies: DutyContext,
        rng: &mut RngStream,
    ) -> GuardDutyResult {
        let at = policies.at;
        let hour = at.as_secs() / 3600;
        let mut result = GuardDutyResult::default();

        // Stable ordering prevents HashMap-style iteration from changing traces.
        let mut seen: Vec<GuardObservation> = observations.to_vec();
        seen.sort_by_key(|o| o.prisoner_id);
        let nearby = seen.len() as f32;
        let avg_tension = if seen.is_empty() {
            0.0
        } else {
            (seen.iter().map(|o| o.tension.clamp(0.0, 1.0)).sum::<f32>() / nearby).clamp(0.0, 1.0)
        };
        let peak_tension = seen
            .iter()
            .map(|o| o.tension.clamp(0.0, 1.0))
            .fold(0.0_f32, f32::max);
        let peak_obs = seen
            .iter()
            .max_by(|a, b| a.tension.total_cmp(&b.tension))
            .copied();

        let lockdown_pressure = if policies.lockdown { 0.08 } else { 0.0 };
        let incident_load = (avg_tension * 0.6
            + peak_tension * 0.4
            + self.post.risk_profile() * 0.15
            + policies.security_level.clamp(0.0, 1.0) * 0.1
            + lockdown_pressure)
            .clamp(0.0, 1.0);
        self.needs
            .drift_duty((nearby / 10.0).clamp(0.0, 1.0), avg_tension);

        self.tick(at, incident_load);

        // Opinions are updated from direct observation only. A small threshold
        // avoids rewriting identical entries every hour.
        for obs in &seen {
            let tension = obs.tension.clamp(0.0, 1.0);
            let mut opinion = self.opinion_of(obs.prisoner_id);
            let suspicion = (opinion.suspicion * 0.85 + tension * 0.15).clamp(0.0, 1.0);
            let trust = (opinion.trust * 0.85 + (1.0 - tension) * 0.15).clamp(0.0, 1.0);
            let fear = (opinion.fear * 0.9 + tension * 0.1).clamp(0.0, 1.0);
            let delta = (suspicion - opinion.suspicion).abs()
                + (trust - opinion.trust).abs()
                + (fear - opinion.fear).abs();
            if delta > 0.02 {
                self.push_history(format!(
                    "opinion@{hour}:{}:{suspicion:.3}:{trust:.3}:{fear:.3}",
                    obs.prisoner_id
                ));
                opinion = GuardOpinion {
                    suspicion,
                    trust,
                    fear,
                    last_seen_hour: hour,
                };
            }
            result.interactions.push(GuardInteraction {
                at,
                guard: self.id,
                prisoner_id: obs.prisoner_id,
                kind: GuardInteractionKind::Observe,
                intensity: tension,
            });
            let _ = opinion;
        }

        let observation = self.observe(at, incident_load);
        let notable = observation.is_some() || peak_tension > 0.7 || incident_load > 0.6;
        if notable {
            let mut severity: u8 = if peak_tension > 0.95 || incident_load > 0.95 {
                3
            } else if peak_tension > 0.8 || incident_load > 0.8 {
                2
            } else {
                1
            };
            let mut summary = observation.unwrap_or_else(|| {
                format!(
                    "Routine observation at {:?}: {} co-located prisoner(s), tension {:.2}",
                    self.post, nearby, peak_tension
                )
            });

            let mut witnessed_directly = rng.chance(
                (0.55 + self.competence as f64 * 0.35
                    - self.needs.performance_penalty() as f64 * 0.25)
                    .clamp(0.2, 0.98),
            );

            // Imperfect perception: fatigued/stressed or incompetent guards can
            // misremember severity or witness status. The degraded report still
            // carries a cause string, so this remains explainable to observers.
            let mistake_chance = ((1.0 - self.competence) * 0.40
                + self.fatigue * 0.25
                + self.stress * 0.20
                + self.needs.performance_penalty() * 0.20
                - self.integrity * 0.10)
                .clamp(0.01, 0.70) as f64;
            if rng.chance(mistake_chance) {
                witnessed_directly = false;
                severity = severity.saturating_sub(1);
                summary = format!("Unverified/misremembered report: {summary}");
                self.push_history(format!("mistake@{hour}:perception"));
                self.cumulative_mistakes = self.cumulative_mistakes.saturating_add(1);
                result.mistakes += 1;
            }

            let mut report = self.report(at, summary, severity, witnessed_directly);
            if !witnessed_directly {
                report.credibility = (report.credibility * 0.65).clamp(0.05, 0.99);
                self.report_credibility = report.credibility;
            }

            // Direct intervention creates a structured prisoner effect.
            if (incident_load > 0.6 || peak_tension > 0.7) && result.mistakes == 0 {
                if let Some(enforcement) = self.enforce(at, "contain escalating incident") {
                    if let Some(target) = peak_obs {
                        let kind = if incident_load > 0.85 || peak_tension > 0.9 {
                            GuardInteractionKind::Restrain
                        } else {
                            GuardInteractionKind::Directive
                        };
                        result.interactions.push(GuardInteraction {
                            at,
                            guard: self.id,
                            prisoner_id: target.prisoner_id,
                            kind,
                            intensity: incident_load.clamp(0.0, 1.0),
                        });
                    }
                    let _ = enforcement;
                }
            }

            // Occasional policy violation: leniency under stress, or excessive
            // force from a rigid, low-integrity guard. Bounded and rate-limited
            // by the same incident pressure that drives enforcement.
            let violation_chance = ((1.0 - self.integrity) * 0.35
                + self.stress * 0.20
                + self.personality * 0.10
                + policies.corruption_pressure.clamp(0.0, 1.0) * 0.15
                - self.morale * 0.10)
                .clamp(0.0, 0.50) as f64;
            if (incident_load > 0.55 || self.stress > 0.7) && rng.chance(violation_chance) {
                let kind = if self.personality > 0.6 {
                    "leniency"
                } else {
                    "excessive_force"
                };
                self.push_history(format!("violation@{hour}:{kind}"));
                self.cumulative_violations = self.cumulative_violations.saturating_add(1);
                result.violations += 1;
                if let Some(target) = peak_obs {
                    result.interactions.push(GuardInteraction {
                        at,
                        guard: self.id,
                        prisoner_id: target.prisoner_id,
                        kind: GuardInteractionKind::PolicyViolation,
                        intensity: violation_chance.clamp(0.0, 1.0) as f32,
                    });
                }
            }

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
                assist.severity =
                    assist
                        .severity
                        .max(severity)
                        .max(if overwhelmed { 3 } else { 2 });
                assist.credibility = self.compute_credibility(true);
                report.requests_assistance = true;
                if let Some(target) = peak_obs {
                    result.interactions.push(GuardInteraction {
                        at,
                        guard: self.id,
                        prisoner_id: target.prisoner_id,
                        kind: GuardInteractionKind::Assist,
                        intensity: incident_load,
                    });
                }
                result.reports.push(assist);
            }

            result.reports.push(report);
        } else if self.stress > 0.85 && hour.saturating_sub(self.last_escalation_hour) >= 12 {
            self.last_escalation_hour = hour;
            let assist = self.request_assistance(at, "guard stress critical; relief requested");
            result.reports.push(assist);
        }

        self.report_credibility = self.compute_credibility(true);
        result
    }

    /// Backward-compatible duty tick over anonymous tension values.
    ///
    /// The world uses [`Guard::duty_tick_area`] so observations are area-scoped;
    /// this wrapper keeps existing AI/driver callers working.
    pub fn duty_tick(
        &mut self,
        world_prisoners: &[f32],
        world_policies: DutyContext,
        rng: &mut RngStream,
    ) -> Vec<GuardReport> {
        let observations: Vec<GuardObservation> = world_prisoners
            .iter()
            .enumerate()
            .map(|(i, tension)| GuardObservation {
                prisoner_id: i as u64,
                facility_id: 0,
                tension: *tension,
            })
            .collect();
        self.duty_tick_area(&observations, world_policies, rng)
            .reports
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
                corruption_pressure: 0.0,
            };
            let reports = guard.duty_tick(&[1.0], ctx, &mut rng);
            for r in &reports {
                // Count actual escalation requests (the routine observation may also be flagged).
                if r.requests_assistance && r.summary.starts_with("Assistance requested:") {
                    assistance_reports += 1;
                }
                assert!(
                    r.credibility.is_finite(),
                    "credibility not finite at hour {hour}"
                );
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

    fn test_guard(id: u64, post: PostType) -> Guard {
        Guard::new(
            GuardId(id),
            format!("Guard {id}"),
            GuardRank::Officer,
            post,
            &mut RngStream::new(7, id),
        )
    }

    fn ctx(hour: u64) -> DutyContext {
        DutyContext {
            at: SimTime::from_secs(hour * 3600),
            lockdown: false,
            security_level: 1.0,
            corruption_pressure: 0.0,
        }
    }

    fn observation(prisoner_id: u64, tension: f32) -> GuardObservation {
        GuardObservation {
            prisoner_id,
            facility_id: 4,
            tension,
        }
    }

    fn best_credibility(result: &GuardDutyResult) -> f32 {
        result
            .reports
            .iter()
            .map(|r| r.credibility)
            .fold(0.0_f32, f32::max)
    }

    #[test]
    fn fatigue_and_stress_reduce_report_credibility_deterministically() {
        let mut fresh = test_guard(1, PostType::Yard);
        let mut tired = test_guard(2, PostType::Yard);
        tired.fatigue = 0.95;
        tired.stress = 0.95;
        tired.morale = 0.05;

        let obs = [observation(101, 1.0)];
        let fresh_result = fresh.duty_tick_area(&obs, ctx(0), &mut RngStream::new(11, 7));
        let tired_result = tired.duty_tick_area(&obs, ctx(0), &mut RngStream::new(11, 7));
        assert!(best_credibility(&fresh_result) > 0.0);
        assert!(
            best_credibility(&tired_result) < best_credibility(&fresh_result),
            "fatigue/stress must degrade credibility: fresh={} tired={}",
            best_credibility(&fresh_result),
            best_credibility(&tired_result)
        );
    }

    #[test]
    fn repeated_area_observations_form_durable_prisoner_opinions() {
        let mut guard = test_guard(3, PostType::Wing);
        let obs = [observation(201, 0.8)];
        for hour in 0..6 {
            guard.duty_tick_area(&obs, ctx(hour), &mut RngStream::new(19, hour));
        }
        let opinion = guard.opinion_of(201);
        assert!(opinion.suspicion > 0.3, "suspicion: {opinion:?}");
        assert!(opinion.fear > 0.0);
        assert_eq!(opinion.last_seen_hour, 5);
        assert_eq!(guard.opinion_count(), 1);
    }

    #[test]
    fn low_integrity_guards_record_mistakes_and_policy_violations() {
        let mut guard = test_guard(4, PostType::Yard);
        guard.competence = 0.05;
        guard.integrity = 0.02;
        guard.stress = 0.98;
        guard.fatigue = 0.98;
        guard.morale = 0.02;
        guard.personality = 0.9;

        let obs = [observation(301, 1.0)];
        for hour in 0..48 {
            guard.duty_tick_area(&obs, ctx(hour), &mut RngStream::new(23, hour));
        }
        assert!(guard.mistake_count() > 0, "no mistakes recorded");
        assert!(guard.violation_count() > 0, "no violations recorded");
        assert!(guard.history.len() <= Guard::HISTORY_CAP);
    }

    #[test]
    fn cumulative_counts_survive_bounded_history_trimming() {
        let mut guard = test_guard(5, PostType::Yard);
        guard.competence = 0.05;
        guard.integrity = 0.02;
        guard.stress = 0.98;
        guard.fatigue = 0.98;
        guard.morale = 0.02;
        guard.personality = 0.9;

        let obs = [observation(401, 1.0)];
        for hour in 0..(Guard::HISTORY_CAP * 2) {
            guard.duty_tick_area(&obs, ctx(hour as u64), &mut RngStream::new(29, hour as u64));
        }
        let mistakes_in_history = guard
            .history
            .iter()
            .filter(|e| e.starts_with("mistake@"))
            .count();
        let violations_in_history = guard
            .history
            .iter()
            .filter(|e| e.starts_with("violation@"))
            .count();
        assert!(guard.mistake_count() > 0);
        assert!(guard.violation_count() > 0);
        assert!(guard.mistake_count() as usize >= mistakes_in_history);
        assert!(guard.violation_count() as usize >= violations_in_history);
        assert!(guard.history.len() <= Guard::HISTORY_CAP);

        // Even a fully trimmed history must not erase the audit totals.
        guard.history.clear();
        assert!(guard.mistake_count() > 0);
        assert!(guard.violation_count() > 0);
    }

    #[test]
    fn legacy_counters_are_restored_once_from_history() {
        let mut guard = test_guard(6, PostType::Yard);
        guard.push_history("mistake@1:perception".to_string());
        guard.push_history("mistake@2:perception".to_string());
        guard.push_history("violation@3:bribery".to_string());
        guard.restore_legacy_counters_from_history();
        assert_eq!(guard.mistake_count(), 2);
        assert_eq!(guard.violation_count(), 1);

        // New counters are authoritative; a second restore must not double-count.
        guard.push_history("mistake@4:perception".to_string());
        guard.restore_legacy_counters_from_history();
        assert_eq!(guard.mistake_count(), 2);
    }

    #[test]
    fn off_duty_blocks_recover_needs_and_form_bonds() {
        let mut a = test_guard(31, PostType::Wing);
        let mut b = test_guard(32, PostType::Wing);
        a.needs.rest = 0.9;
        a.needs.social = 0.9;
        a.fatigue = 0.9;
        b.needs.social = 0.9;
        let before = a.needs.performance_penalty();
        a.begin_off_duty(SimTime::from_secs(3600));
        b.begin_off_duty(SimTime::from_secs(3600));
        assert!(a.off_duty && b.off_duty);
        assert!(a.needs.rest < 0.9, "rest need should recover off-duty");
        assert!(a.fatigue < 0.9, "fatigue should recover off-duty");
        assert!(a.performance_score() > 0.0);
        a.form_bond(b.id, 1);
        b.form_bond(a.id, 1);
        assert_eq!(a.bonds.len(), 1);
        assert_eq!(a.bonds[0].guard, b.id);
        assert!(a.bonds[0].strength > 0.0);
        assert!(a.needs.performance_penalty() < before);
        // Two more recovery ticks end the block and return the guard to duty.
        for _ in 0..2 {
            a.off_duty_recovery_tick(SimTime::from_secs(7200));
        }
        assert!(!a.off_duty);
        assert_eq!(a.current_task, "patrol");
    }

    #[test]
    fn unmet_needs_degrade_report_quality() {
        let rested = test_guard(41, PostType::Corridor);
        let mut exhausted = rested.clone();
        exhausted.id = GuardId(42);
        exhausted.needs.rest = 1.0;
        exhausted.needs.safety = 1.0;
        exhausted.needs.hygiene = 1.0;
        exhausted.needs.social = 1.0;
        exhausted.needs.purpose = 1.0;
        exhausted.needs.stimulation = 1.0;
        assert!(rested.credibility() > exhausted.credibility());
        // Credibility and performance are distinct signals: need penalties act
        // on reporting quality first, while mistakes/violations dominate the
        // performance score. Verify the need-aware scoring path directly.
        assert!(rested.needs.performance_penalty() < exhausted.needs.performance_penalty());
        assert!((rested.needs.performance_penalty() - 0.0).abs() < 0.6);
    }

    #[test]
    fn investigation_consequences_are_durable() {
        let mut guard = test_guard(51, PostType::Corridor);
        let integrity = guard.integrity;
        let violations = guard.violation_count();
        assert!(guard.apply_investigation(true, 3, "bribery"));
        assert!(guard.integrity < integrity);
        assert!(guard.violation_count() > violations);
        assert!(guard.history.iter().any(|h| h.contains("substantiated")));
        assert!(!guard.apply_investigation(false, 4, "cleared"));
    }
}
