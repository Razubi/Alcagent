# Specification: Guard and Warden Depth

## Feature: Guard Needs, Lives, and Warden Strategic Depth

### Overview
Guards currently have personality, fatigue, stress, and morale but no real needs or life outside duty. The warden has traits and arbitration but several actions are never selected and corruption is unused. This spec adds depth: guard needs (rest, social, safety, purpose), off-duty behavior, warden corruption integration, promotion/firing/investigation logic, and negotiation outcomes.

### User Stories
- As an observer, I want guards to have distinct needs and off-duty lives.
- As an observer, I want the warden to use all available actions and adapt to outcomes.
- As an observer, I want corruption to be a real risk with consequences.

---

## Functional Requirements

### FR-1: Guard needs system
Guards have needs: rest, social, safety, purpose, hygiene, stimulation. Needs drift during duty and off-duty.

**Acceptance Criteria:**
- [ ] Guard has `needs: GuardNeeds` struct with 6+ needs
- [ ] Needs drift during duty (fatigue, stress)
- [ ] Needs recover off-duty
- [ ] Low needs affect performance (credibility, vigilance, mistake rate)

### FR-2: Guard off-duty behavior
Guards have off-duty time with activities: rest, socialize, train, patrol off-hours. Off-duty behavior affects next shift.

**Acceptance Criteria:**
- [ ] Guards assigned off-duty blocks
- [ ] Off-duty recovery improves next-shift stats
- [ ] Guard social relationships form during off-duty

### FR-3: Warden corruption integration
`corruption_susceptibility` trait influences decisions: higher susceptibility → more likely to accept bribes, cover up misconduct, favor certain prisoners/gangs.

**Acceptance Criteria:**
- [ ] Corruption trait factors into warden arbitration
- [ ] Corruption creates detectable misconduct events
- [ ] High corruption increases guard misconduct probability
- [ ] Investigations can uncover corruption

### FR-4: Warden selects all actions
Arbitration logic for: AdjustParoleRecommendation, InvestigateMisconduct, PromoteStaff, FireGuards, ImproveInfrastructure, NegotiateWithRepresentatives.

**Acceptance Criteria:**
- [ ] Each action has at least one trigger condition
- [ ] PromoteStaff selects best guard by performance
- [ ] FireGuards removes worst guards when overstaffed/insolvent
- [ ] InvestigateMisconduct triggers on incident patterns
- [ ] AdjustParoleRecommendation influences parole board
- [ ] ImproveInfrastructure upgrades worst facility
- [ ] NegotiateWithRepresentatives changes policy based on prisoner demands

### FR-5: Warden adaptation
Warden traits and strategy adapt based on outcomes (success/failure of past decisions).

**Acceptance Criteria:**
- [ ] Warden tracks decision outcomes
- [ ] Traits shift slightly based on outcomes (e.g., failed expansion → lower risk tolerance)
- [ ] Philosophy can shift after major events

### FR-6: Guard promotion/firing UI
GUI/TUI expose guard promotion and firing controls.

**Acceptance Criteria:**
- [ ] Guard roster shows promotion/firing buttons
- [ ] Actions emit auditable events
- [ ] Validation prevents invalid operations

---

## Success Criteria

- Guards feel like distinct individuals with needs and off-duty lives
- Warden uses all 14 actions meaningfully
- Corruption is a real, detectable risk
- All existing tests pass

---

## Dependencies
- `alc-guard` (Guard, PostType, duty_tick)
- `alc-warden` (Warden, arbitrate, decide_strategy)
- `alc-world` (World, apply_warden_action)
- `alc-social` (relationships, gang influence)

## Assumptions
- Guard needs are simpler than prisoner needs (6 vs 20+)
- Warden adaptation is gradual, not dramatic

---

## Completion Signal

### Implementation Checklist
- [ ] GuardNeeds struct and drift
- [ ] Off-duty behavior system
- [ ] Corruption integration in warden arbitration
- [ ] All warden actions have trigger conditions
- [ ] Warden adaptation from outcomes
- [ ] Guard promotion/firing UI
- [ ] Tests for all new systems
- [ ] All existing tests pass

<!-- NR_OF_TRIES: 0 -->