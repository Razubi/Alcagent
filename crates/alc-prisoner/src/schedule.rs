//! Daily life & schedules per Directive §5 (design doc §4).
//!
//! A [`ScheduleTemplate`] is configurable data (the default mirrors the
//! medium-security weekday in design doc §4.6). [`resolve_schedule`] resolves
//! it against a prisoner and the current institutional policy into concrete
//! [`ResolvedBlock`]s, honoring classification, wing, privilege, disciplinary,
//! education-enrollment, staffing, weather, lockdown and medical-hold gates.
//! [`attendance_roll`] / [`attendance_roll_with`] produce the full range of
//! compliance outcomes the Directive requires (follow, late, refuse, skip,
//! socialize, trade, argue, seek-medical, break-rule — plus program
//! participation when enrolled).

use crate::{Classification, Needs, Prisoner};
use alc_core::RngStream;
use serde::{Deserialize, Serialize};

pub type BlockId = u32;
pub type ScheduleId = u32;

/// Activities in a prison day (Directive §5, design doc §4.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Activity {
    Wake,
    Breakfast,
    Work,
    Recreation,
    Yard,
    Education,
    Religious,
    Medical,
    Visits,
    Showers,
    Lunch,
    Dinner,
    Lockdown,
    Sleep,
    Count,
    FreeTime,
    #[default]
    SickCall,
    Program,
}

/// Who may attend a block. Empty `classifications` / `wings` mean "all".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Eligibility {
    pub classifications: Vec<Classification>,
    pub wings: Vec<u32>,
    pub min_privilege: f32,
    pub allow_disciplinary: bool,
    pub require_education: bool,
}

impl Eligibility {
    pub fn all() -> Self {
        Self {
            classifications: Vec::new(),
            wings: Vec::new(),
            min_privilege: 0.0,
            allow_disciplinary: true,
            require_education: false,
        }
    }
}

/// Discipline consequences are expressed as strings here so this crate does
/// not depend on the Crime system (Directive §2: typed contracts only).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AttendancePolicy {
    pub mandatory: bool,
    pub refusal_allowed: bool,
    pub late_tolerance_minutes: u8,
    pub on_refusal: String,
    pub on_unexcused_absence: String,
}

/// One window in a schedule template.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScheduleBlock {
    pub id: BlockId,
    pub label: String,
    pub activity: Activity,
    pub start_minute: u16,
    pub end_minute: u16,
    pub eligibility: Eligibility,
    /// Guards/teachers needed to run the block. Zero = self-directed.
    pub staff_requirement: u8,
    pub attendance: AttendancePolicy,
    /// If true, agents travel to `location` (encounters happen en route).
    pub requires_travel: bool,
}

impl ScheduleBlock {
    fn new(
        id: BlockId,
        activity: Activity,
        start_minute: u16,
        end_minute: u16,
        staff_requirement: u8,
    ) -> Self {
        let label = format!("{activity:?}");
        let mandatory = !matches!(
            activity,
            Activity::Recreation | Activity::FreeTime | Activity::Program | Activity::Religious
        );
        let require_education = matches!(activity, Activity::Education | Activity::Program);
        let min_privilege = match activity {
            Activity::Recreation => 0.20,
            Activity::Visits => 0.40,
            _ => 0.0,
        };
        Self {
            id,
            label,
            activity,
            start_minute,
            end_minute,
            eligibility: Eligibility {
                min_privilege,
                allow_disciplinary: !matches!(activity, Activity::Work | Activity::Visits),
                require_education,
                ..Eligibility::all()
            },
            staff_requirement,
            attendance: AttendancePolicy {
                mandatory,
                refusal_allowed: true,
                late_tolerance_minutes: if mandatory { 10 } else { 20 },
                on_refusal: if mandatory { "refused-mandatory".into() } else { "refused-optional".into() },
                on_unexcused_absence: "unexcused-absence".into(),
            },
            requires_travel: !matches!(activity, Activity::Sleep | Activity::Lockdown | Activity::Count),
        }
    }
}

/// A reusable, configurable daily template.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScheduleTemplate {
    pub id: ScheduleId,
    pub name: String,
    /// Higher priority templates win overlapping windows (design doc §4.2).
    pub priority: u8,
    pub blocks: Vec<ScheduleBlock>,
}

impl ScheduleTemplate {
    /// Default medium-security weekday from design doc §4.6.
    pub fn default_medium() -> Self {
        use Activity as A;
        let specs = [
            (A::Wake, 360, 390, 1),        // 06:00–06:30
            (A::Breakfast, 390, 435, 2),   // 06:30–07:15
            (A::Work, 450, 690, 3),        // 07:30–11:30
            (A::Lunch, 705, 750, 2),       // 11:45–12:30
            (A::Education, 765, 825, 1),   // 12:45–13:45
            (A::Yard, 840, 930, 4),        // 14:00–15:30
            (A::Religious, 945, 990, 1),   // 15:45–16:30
            (A::Showers, 1005, 1050, 2),   // 16:45–17:30
            (A::Dinner, 1050, 1095, 2),    // 17:30–18:15
            (A::Recreation, 1110, 1200, 2),// 18:30–20:00
            (A::Visits, 1200, 1230, 2),    // 20:00–20:30
            (A::Lockdown, 1260, 1440, 2),  // 21:00–24:00
            (A::Sleep, 0, 360, 2),         // 00:00–06:00
        ];
        let blocks = specs
            .into_iter()
            .enumerate()
            .map(|(i, (a, s, e, staff))| ScheduleBlock::new(i as BlockId + 1, a, s, e, staff))
            .collect();
        Self {
            id: 1,
            name: "medium-security weekday".into(),
            priority: 10,
            blocks,
        }
    }

    /// Whether the template contains a given activity (handy for UI/tests).
    pub fn has_activity(&self, activity: Activity) -> bool {
        self.blocks.iter().any(|b| b.activity == activity)
    }
}

/// Why a block did not run for a prisoner on a given day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CancelReason {
    Classification,
    Wing,
    Privileges,
    Disciplinary,
    StaffShortage,
    Lockdown,
    Weather,
    NotEnrolled,
    MedicalHold,
}

/// A concrete, prisoner-specific window. `cancelled` blocks remain in the
/// vector so the UI/history can explain *why* an activity did not happen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedBlock {
    pub block: ScheduleBlock,
    pub start_minute: u16,
    pub end_minute: u16,
    pub cancelled: bool,
    pub reason: Option<CancelReason>,
}

pub type ResolvedSchedule = Vec<ResolvedBlock>;

/// Institutional policy context consumed by [`resolve_schedule`]. This is a
/// local view (Directive §2), not a handle into the warden's internals.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SchedulePolicy {
    pub lockdown: bool,
    /// Global privilege multiplier (warden policy changes shift daily life).
    pub privileges_level: f32,
    pub security_level: f32,
    /// 0..1 : fraction of required posts actually staffed today.
    pub staff_coverage: f32,
    pub weather_ok: bool,
    pub rehab_funding_multiplier: f32,
}

impl Default for SchedulePolicy {
    fn default() -> Self {
        Self {
            lockdown: false,
            privileges_level: 1.0,
            security_level: 0.5,
            staff_coverage: 1.0,
            weather_ok: true,
            rehab_funding_multiplier: 1.0,
        }
    }
}

/// Resolve a template for one prisoner under today's policy (Directive §5
/// variation rules). Pure: no mutation of the prisoner or world.
pub fn resolve_schedule(
    prisoner: &Prisoner,
    template: &ScheduleTemplate,
    policies: &SchedulePolicy,
) -> ResolvedSchedule {
    let effective_privileges = prisoner.schedule.privileges * policies.privileges_level;
    let max_security = prisoner.classification >= Classification::Maximum;
    let medical_hold = prisoner.needs.medical_need > 0.6 || prisoner.condition.health < 0.4;
    let mut out: ResolvedSchedule = Vec::with_capacity(template.blocks.len() + 2);

    for block in &template.blocks {
        let mut resolved = ResolvedBlock {
            block: block.clone(),
            start_minute: block.start_minute,
            end_minute: block.end_minute,
            cancelled: false,
            reason: None,
        };

        // 1. Emergency lockdown replaces nearly everything.
        if policies.lockdown
            && !matches!(
                block.activity,
                Activity::Lockdown
                    | Activity::Sleep
                    | Activity::Count
                    | Activity::Medical
                    | Activity::SickCall
            )
        {
            cancel(&mut resolved, CancelReason::Lockdown);
        }

        // 2. Static eligibility gates.
        if !block.eligibility.classifications.is_empty()
            && !block.eligibility.classifications.contains(&prisoner.classification)
        {
            cancel(&mut resolved, CancelReason::Classification);
        }
        if !block.eligibility.wings.is_empty()
            && !block.eligibility.wings.contains(&prisoner.schedule.wing)
        {
            cancel(&mut resolved, CancelReason::Wing);
        }
        if effective_privileges < block.eligibility.min_privilege {
            cancel(&mut resolved, CancelReason::Privileges);
        }
        if prisoner.schedule.disciplinary_status && !block.eligibility.allow_disciplinary {
            cancel(&mut resolved, CancelReason::Disciplinary);
        }
        if block.eligibility.require_education && !prisoner.schedule.education {
            cancel(&mut resolved, CancelReason::NotEnrolled);
        }

        // 3. Dynamic world-state gates.
        if block.activity == Activity::Yard && !policies.weather_ok {
            cancel(&mut resolved, CancelReason::Weather);
        }
        if block.staff_requirement > 0 {
            // Four guards is "fully staffed" for the heaviest block (Yard).
            let required = (block.staff_requirement as f32 / 4.0).min(1.0);
            if policies.staff_coverage + f32::EPSILON < required {
                cancel(&mut resolved, CancelReason::StaffShortage);
            } else if policies.staff_coverage < required + 0.15
                && matches!(
                    block.activity,
                    Activity::Education | Activity::Recreation | Activity::Work
                )
            {
                // Partial coverage: shorten rather than cancel (§4.2).
                let span = resolved.end_minute.saturating_sub(resolved.start_minute);
                resolved.end_minute = resolved.start_minute.saturating_add((span as f32 * 0.75) as u16);
            }
        }

        // 4. Classification / medical overlays.
        if max_security && matches!(block.activity, Activity::Work | Activity::Yard) {
            cancel(&mut resolved, CancelReason::Classification);
        }
        if medical_hold
            && matches!(
                block.activity,
                Activity::Work | Activity::Yard | Activity::Recreation
            )
        {
            cancel(&mut resolved, CancelReason::MedicalHold);
        }

        out.push(resolved);
    }

    // Substitutions are appended so history can explain the change.
    if max_security {
        let mut controlled = ScheduleBlock::new(900, Activity::Recreation, 840, 900, 4);
        controlled.label = "controlled recreation (maximum security)".into();
        out.push(ResolvedBlock {
            block: controlled,
            start_minute: 840,
            end_minute: 900,
            cancelled: policies.lockdown,
            reason: if policies.lockdown { Some(CancelReason::Lockdown) } else { None },
        });
    }
    if medical_hold {
        let mut sick = ScheduleBlock::new(901, Activity::SickCall, 540, 570, 1);
        sick.label = "sick call (medical hold)".into();
        out.push(ResolvedBlock {
            block: sick,
            start_minute: 540,
            end_minute: 570,
            cancelled: false,
            reason: None,
        });
    }

    out.sort_by_key(|b| b.start_minute);
    out
}

fn cancel(block: &mut ResolvedBlock, reason: CancelReason) {
    block.cancelled = true;
    if block.reason.is_none() {
        block.reason = Some(reason);
    }
}

/// The active, non-cancelled block at a minute-of-day (handles midnight wrap).
pub fn current_block<'a>(schedule: &'a ResolvedSchedule, minute_of_day: u16) -> Option<&'a ResolvedBlock> {
    schedule.iter().find(|rb| {
        if rb.cancelled {
            return false;
        }
        if rb.start_minute <= rb.end_minute {
            minute_of_day >= rb.start_minute && minute_of_day < rb.end_minute
        } else {
            minute_of_day >= rb.start_minute || minute_of_day < rb.end_minute
        }
    })
}

/// The next non-cancelled block after `minute_of_day`, if any.
pub fn next_block<'a>(schedule: &'a ResolvedSchedule, minute_of_day: u16) -> Option<&'a ResolvedBlock> {
    schedule
        .iter()
        .filter(|rb| !rb.cancelled && rb.start_minute > minute_of_day)
        .min_by_key(|rb| rb.start_minute)
}

/// All compliance outcomes required by Directive §5. `Participate` is only
/// emitted when a program is enrolled, so a plain `attendance_roll(rng)` yields
/// exactly the nine baseline outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AttendanceOutcome {
    Follow,
    Late,
    Refuse,
    Skip,
    Socialize,
    Trade,
    Argue,
    SeekMedical,
    BreakRule,
    Participate,
}

impl AttendanceOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            AttendanceOutcome::Follow => "follow",
            AttendanceOutcome::Late => "late",
            AttendanceOutcome::Refuse => "refuse",
            AttendanceOutcome::Skip => "skip",
            AttendanceOutcome::Socialize => "socialize",
            AttendanceOutcome::Trade => "trade",
            AttendanceOutcome::Argue => "argue",
            AttendanceOutcome::SeekMedical => "seek-medical",
            AttendanceOutcome::BreakRule => "break-rule",
            AttendanceOutcome::Participate => "participate",
        }
    }

    /// Whether this outcome is disciplinary (feeds Crime system §8).
    pub fn is_disciplinary(self) -> bool {
        matches!(
            self,
            AttendanceOutcome::Refuse
                | AttendanceOutcome::Skip
                | AttendanceOutcome::Argue
                | AttendanceOutcome::BreakRule
        )
    }
}

/// Local decision context for an attendance roll (design doc §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AttendanceContext {
    pub discipline: f32,
    /// Total unmet need pressure, normalized 0..1.
    pub unmet_pressure: f32,
    pub fatigue: f32,
    pub alertness: f32,
    pub routine_mismatch: f32,
    pub privilege_incentive: f32,
    pub guard_presence: f32,
    /// Peers skipping raises skipping (emergent group norms).
    pub group_skip_norm: f32,
    pub medical_pressure: f32,
    pub respect_pressure: f32,
    pub stimulation: f32,
    pub risk_tolerance: f32,
    pub sociability: f32,
    pub refusal_allowed: bool,
    pub late_tolerance_minutes: u8,
    pub program_enrolled: bool,
}

impl Default for AttendanceContext {
    fn default() -> Self {
        Self {
            discipline: 0.5,
            unmet_pressure: 0.3,
            fatigue: 0.2,
            alertness: 0.8,
            routine_mismatch: 0.1,
            privilege_incentive: 0.3,
            guard_presence: 0.5,
            group_skip_norm: 0.1,
            medical_pressure: 0.1,
            respect_pressure: 0.2,
            stimulation: 0.3,
            risk_tolerance: 0.4,
            sociability: 0.5,
            refusal_allowed: true,
            late_tolerance_minutes: 10,
            program_enrolled: false,
        }
    }
}

impl AttendanceContext {
    /// Build a context from what a prisoner actually knows about themselves.
    pub fn from_prisoner(p: &Prisoner) -> Self {
        let n: &Needs = &p.needs;
        Self {
            discipline: p.traits.discipline,
            unmet_pressure: n.total_pressure(),
            fatigue: n.fatigue,
            alertness: n.alertness(),
            routine_mismatch: (1.0 - p.traits.discipline) * 0.5,
            privilege_incentive: p.schedule.privileges,
            guard_presence: 0.5,
            group_skip_norm: 0.1,
            medical_pressure: n.medical_neglect(),
            respect_pressure: n.respect,
            stimulation: n.stimulation,
            risk_tolerance: p.traits.risk_tolerance,
            sociability: p.traits.sociability,
            refusal_allowed: true,
            late_tolerance_minutes: 10,
            program_enrolled: p.schedule.education,
        }
    }
}

/// Design doc §4.4 attendance probability. Higher = more likely to comply.
pub fn attendance_probability(c: &AttendanceContext) -> f32 {
    let p = 0.55
        + c.discipline * 0.35
        - c.unmet_pressure * 0.20
        - (1.0 - c.alertness) * 0.25
        - c.routine_mismatch * 0.15
        + c.privilege_incentive * 0.10
        + c.guard_presence * 0.10
        - c.group_skip_norm * 0.20;
    p.clamp(0.05, 0.98)
}

/// Baseline roll (neutral context). Produces all nine baseline outcomes.
pub fn attendance_roll(rng: &mut RngStream) -> AttendanceOutcome {
    attendance_roll_with(rng, &AttendanceContext::default())
}

/// Contextual roll: a single RNG draw over explicitly computed weights, so
/// every required outcome is reachable and the math is auditable.
pub fn attendance_roll_with(rng: &mut RngStream, c: &AttendanceContext) -> AttendanceOutcome {
    let p = attendance_probability(c);
    let mut weights: Vec<(AttendanceOutcome, f32)> = vec![
        (AttendanceOutcome::Follow, p * if c.program_enrolled { 0.6 } else { 1.0 }),
        (
            AttendanceOutcome::Late,
            if c.late_tolerance_minutes > 0 {
                0.08 + (1.0 - c.alertness) * 0.12 + c.routine_mismatch * 0.10
            } else {
                0.0
            },
        ),
        (
            AttendanceOutcome::Refuse,
            if c.refusal_allowed {
                0.05 + (1.0 - c.discipline) * 0.25 + c.unmet_pressure * 0.15 + c.group_skip_norm * 0.15
            } else {
                0.0
            },
        ),
        (
            AttendanceOutcome::Skip,
            0.04 + c.unmet_pressure * 0.12 + (1.0 - c.discipline) * 0.15 + c.group_skip_norm * 0.10,
        ),
        (AttendanceOutcome::Socialize, 0.06 + c.sociability * 0.12),
        (AttendanceOutcome::Trade, 0.03 + c.sociability * 0.05),
        (
            AttendanceOutcome::Argue,
            0.02 + c.respect_pressure * 0.18 + c.unmet_pressure * 0.05,
        ),
        (AttendanceOutcome::SeekMedical, c.medical_pressure * 0.70),
        (
            AttendanceOutcome::BreakRule,
            c.stimulation
                * (1.0 - c.discipline * 0.6)
                * c.risk_tolerance
                * (1.0 - c.guard_presence * 0.7),
        ),
    ];
    if c.program_enrolled {
        weights.push((AttendanceOutcome::Participate, 0.40));
    }

    let total: f32 = weights.iter().map(|(_, w)| w.max(0.0)).sum();
    let mut roll = (rng.next_f64() as f32) * total;
    for (outcome, w) in weights {
        roll -= w.max(0.0);
        if roll <= 0.0 {
            return outcome;
        }
    }
    AttendanceOutcome::Follow
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PrisonerId;
    use alc_core::RngStream;

    fn sample() -> Prisoner {
        let mut rng = RngStream::new(7, 1);
        Prisoner::generate(PrisonerId(1), &mut rng)
    }

    #[test]
    fn default_template_covers_all_required_activities() {
        let t = ScheduleTemplate::default_medium();
        for activity in [
            Activity::Wake, Activity::Breakfast, Activity::Work, Activity::Recreation,
            Activity::Yard, Activity::Education, Activity::Visits, Activity::Showers,
            Activity::Lockdown, Activity::Sleep,
        ] {
            assert!(t.has_activity(activity), "missing {activity:?}");
        }
    }

    #[test]
    fn lockdown_cancels_normal_blocks_but_keeps_sleep() {
        let p = sample();
        let mut policy = SchedulePolicy::default();
        policy.lockdown = true;
        let schedule = resolve_schedule(&p, &ScheduleTemplate::default_medium(), &policy);
        let work = schedule.iter().find(|b| b.block.activity == Activity::Work).unwrap();
        assert!(work.cancelled);
        assert_eq!(work.reason, Some(CancelReason::Lockdown));
        let sleep = schedule.iter().find(|b| b.block.activity == Activity::Sleep).unwrap();
        assert!(!sleep.cancelled);
    }

    #[test]
    fn staff_shortage_cancels_yard_and_low_privilege_cancels_visits() {
        let mut p = sample();
        p.schedule.privileges = 0.1;
        let mut policy = SchedulePolicy::default();
        policy.staff_coverage = 0.4;
        let schedule = resolve_schedule(&p, &ScheduleTemplate::default_medium(), &policy);
        let yard = schedule.iter().find(|b| b.block.activity == Activity::Yard).unwrap();
        assert_eq!(yard.reason, Some(CancelReason::StaffShortage));
        let visits = schedule.iter().find(|b| b.block.activity == Activity::Visits).unwrap();
        assert_eq!(visits.reason, Some(CancelReason::Privileges));
    }

    #[test]
    fn attendance_roll_produces_all_baseline_outcomes() {
        let mut rng = RngStream::new(11, 0);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..4000 {
            seen.insert(attendance_roll(&mut rng));
        }
        for outcome in [
            AttendanceOutcome::Follow, AttendanceOutcome::Late, AttendanceOutcome::Refuse,
            AttendanceOutcome::Skip, AttendanceOutcome::Socialize, AttendanceOutcome::Trade,
            AttendanceOutcome::Argue, AttendanceOutcome::SeekMedical,
            AttendanceOutcome::BreakRule,
        ] {
            assert!(seen.contains(&outcome), "never produced {outcome:?}");
        }
    }

    #[test]
    fn discipline_and_medical_pressure_shift_outcomes() {
        let disciplined = AttendanceContext { discipline: 0.95, ..Default::default() };
        let loose = AttendanceContext { discipline: 0.05, ..Default::default() };
        assert!(attendance_probability(&disciplined) > attendance_probability(&loose));

        let sick = AttendanceContext { medical_pressure: 1.0, ..Default::default() };
        let mut rng = RngStream::new(3, 9);
        let mut sought = 0;
        for _ in 0..500 {
            if attendance_roll_with(&mut rng, &sick) == AttendanceOutcome::SeekMedical {
                sought += 1;
            }
        }
        assert!(sought > 50);
    }

    #[test]
    fn high_discipline_prisoner_skips_fewer_activities_than_low_discipline() {
        // Identical prisoners except for the discipline trait, driven by the
        // same deterministic stream so the difference is purely trait-driven.
        let mut disciplined = sample();
        let mut loose = sample();
        disciplined.traits.discipline = 0.95;
        loose.traits.discipline = 0.05;
        let ctx_hi = AttendanceContext::from_prisoner(&disciplined);
        let ctx_lo = AttendanceContext::from_prisoner(&loose);
        assert!(attendance_probability(&ctx_hi) > attendance_probability(&ctx_lo));

        let mut rng_hi = RngStream::new(31, 1);
        let mut rng_lo = RngStream::new(31, 1);
        let mut skipped_hi = 0;
        let mut skipped_lo = 0;
        for _ in 0..2_000 {
            if attendance_roll_with(&mut rng_hi, &ctx_hi).is_disciplinary() {
                skipped_hi += 1;
            }
            if attendance_roll_with(&mut rng_lo, &ctx_lo).is_disciplinary() {
                skipped_lo += 1;
            }
        }
        assert!(skipped_lo > skipped_hi + 100,
            "discipline had no measurable effect: {skipped_hi} vs {skipped_lo} disciplinary rolls");
    }
}
