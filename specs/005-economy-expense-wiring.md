# Specification: Economy Expense Wiring

## Feature: Full Expense Category Integration

### Overview
The economy has 20 accounts but only food, water, utilities, salaries, maintenance, and construction are accrued automatically. Medical, education, rehabilitation, security equipment, transportation, supplies, and administrative costs exist as accounts but aren't wired into daily accrual or world events. This spec connects every expense category to actual simulation events.

### User Stories
- As an observer, I want the budget to reflect all real costs.
- As a warden, I want financial decisions to account for all expenses.

---

## Functional Requirements

### FR-1: Medical expenses
Medical needs, injuries, illnesses, and medical emergencies create `Account::Medical` expenses.

**Acceptance Criteria:**
- [ ] `resolve_medical_consequences_step` records medical expenses
- [ ] `RequestMedical` decision creates a medical expense
- [ ] Medical staff salaries tracked

### FR-2: Education expenses
Education blocks and programs create `Account::Education` expenses.

**Acceptance Criteria:**
- [ ] Education activities accrue education expenses
- [ ] Education program enrollment costs tracked

### FR-3: Rehabilitation expenses
Rehab programs create `Account::Rehabilitation` expenses based on `rehab_funding_multiplier`.

**Acceptance Criteria:**
- [ ] Rehab enrollment/progress creates rehab expenses
- [ ] Funding multiplier affects expense amount

### FR-4: Security equipment expenses
Security level changes, guard equipment, and surveillance create `Account::SecurityEquipment` expenses.

**Acceptance Criteria:**
- [ ] Security increases create equipment expenses
- [ ] Guard hiring includes equipment cost

### FR-5: Transportation expenses
Transfers, intake, and visitor transport create `Account::Transportation` expenses.

**Acceptance Criteria:**
- [ ] Transfer-out creates transportation expense
- [ ] Intake creates transportation expense

### FR-6: Supplies and administrative expenses
Daily supplies and admin overhead create `Account::Supplies` and `Account::Administration` expenses.

**Acceptance Criteria:**
- [ ] Daily supplies expense based on population
- [ ] Administrative overhead based on staff count
- [ ] All 20 accounts have at least one accrual path

### FR-7: Configurable expense rates
All expense rates are configurable via `EconomyConfig` or `Policies`.

**Acceptance Criteria:**
- [ ] Expense rates in config, not hardcoded
- [ ] Warden can adjust rates via ChangePolicy

---

## Success Criteria

- All 20 accounts have accrual paths
- Budget reflects all real simulation costs
- All existing tests pass

---

## Dependencies
- `alc-economy` (Economy, Account, Money)
- `alc-world` (World::tick, all step functions)
- `alc-warden` (arbitrate, ChangePolicy)

## Assumptions
- Expense rates are approximate; precision is secondary to completeness

---

## Completion Signal

### Implementation Checklist
- [ ] Medical expenses wired
- [ ] Education expenses wired
- [ ] Rehabilitation expenses wired
- [ ] Security equipment expenses wired
- [ ] Transportation expenses wired
- [ ] Supplies and admin expenses wired
- [ ] Configurable rates
- [ ] Tests for all expense categories
- [ ] All existing tests pass

<!-- NR_OF_TRIES: 0 -->