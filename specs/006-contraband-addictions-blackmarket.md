# Specification: Contraband Addictions and Black Market

## Feature: Item-Specific Effects, Addictions, and Organic Black Market

### Overview
Contraband currently has 5 item types with basic discovery. This spec adds: item-specific effects (drugs cause addiction, weapons increase violence, phones enable coordination, money enables bribery), addiction progression, withdrawal, black market dynamics, and organic smuggling where certain prisoners become smugglers based on traits and opportunities.

### User Stories
- As an observer, I want drugs to cause addiction that worsens over time.
- As an observer, I want a black market to emerge organically from prisoner traits and needs.
- As an observer, I want smugglers to be distinct prisoners with specific motivations.

---

## Functional Requirements

### FR-1: Expand contraband item types
Add: Alcohol, Cigarettes, Medicine, Blueprints, Radio, Tools (already exists), plus existing 5.

**Acceptance Criteria:**
- [ ] 10+ contraband item types
- [ ] Each item has distinct effects, severity, and value

### FR-2: Item-specific effects
Each item type has unique effects on prisoner needs, traits, behavior.

**Acceptance Criteria:**
- [ ] Drugs: +stimulation, -discipline, +addiction risk, +aggression withdrawal
- [ ] Weapons: +safety, +aggression, +violence risk
- [ ] Phones: +freedom, +coordination, +gang communication
- [ ] Money: +status, +bribery capability
- [ ] Alcohol: +social, -discipline, +addiction risk
- [ ] Cigarettes: +calm, -health, +addiction risk
- [ ] Medicine: +health, -medical_need (but misuse risk)
- [ ] Blueprints: +escape motivation, +planning
- [ ] Radio: +information, +coordination
- [ ] Tools: +escape capability, +work skill

### FR-3: Addiction system
Prisoners can develop addictions to drugs, alcohol, cigarettes. Addiction progresses through stages: none → mild → moderate → severe → critical. Withdrawal causes need impulses and behavioral changes.

**Acceptance Criteria:**
- [ ] `AddictionState` on `Prisoner` or `ContrabandState` with item + stage + duration
- [ ] Addiction progresses based on frequency of use
- [ ] Withdrawal causes irritability, aggression, medical need
- [ ] Rehab programs can treat addiction (existing AddictionRecovery program)
- [ ] Addiction survives save/load

### FR-4: Black market dynamics
Contraband market prices fluctuate based on supply, demand, gang control, and discovery risk. Prices rise when supply is low or risk is high.

**Acceptance Criteria:**
- [ ] Market prices fluctuate deterministically
- [ ] Gang control affects prices
- [ ] Discovery risk affects prices
- [ ] Trade volume tracked

### FR-5: Smuggler emergence
Certain prisoners become smugglers based on traits (manipulation, risk tolerance, gang affiliation) and opportunities (visits, work assignments, guard corruption). Smuggling is organic, not scripted.

**Acceptance Criteria:**
- [ ] Smuggler role emerges from trait + opportunity
- [ ] Smugglers have higher contraband inventory
- [ ] Guard corruption increases smuggling success
- [ ] Smuggling creates detectable events

### FR-6: Contraband UI
GUI/TUI show contraband inventory, addiction status, market prices, smuggler indicators.

**Acceptance Criteria:**
- [ ] Prisoner panel shows contraband items and addiction
- [ ] Warden panel shows market overview
- [ ] Contraband events in incident timeline

---

## Success Criteria

- Drugs cause measurable addiction progression
- Black market prices fluctuate realistically
- Smugglers emerge organically from traits
- All existing tests pass

---

## Dependencies
- `alc-social` (ContrabandItem, contraband_market, contraband_discovery)
- `alc-prisoner` (ContrabandState, Condition)
- `alc-world` (contraband_state, contraband_cases, emergent events)
- `alc-guard` (Guard integrity, discovery)

## Assumptions
- Addiction is abstract (stage-based, not biochemical simulation)
- Black market is statistical, not item-by-item tracked

---

## Completion Signal

### Implementation Checklist
- [ ] Expanded ContrabandItem enum
- [ ] Item-specific effects
- [ ] Addiction system with stages and withdrawal
- [ ] Black market price fluctuation
- [ ] Organic smuggler emergence
- [ ] Contraband UI
- [ ] Tests for all new systems
- [ ] All existing tests pass

<!-- NR_OF_TRIES: 0 -->