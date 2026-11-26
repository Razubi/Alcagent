# Specification: Prisoner Depth — Bios, Crimes, and Relationships

## Feature: Rich Prisoner Identities and Social Depth

### Overview
Prisoners currently have names, backgrounds, traits, and sentences but no detailed bio, crime narrative, or deep relationship history. This spec adds: detailed bios, accurate crime/sentence generation, old-friend/rival connections, argument and fight escalation, and specific conversation topics.

### User Stories
- As an observer, I want each prisoner to have a unique bio and crime story.
- As an observer, I want prisoners to have old friends and rivals from before prison.
- As an observer, I want specific conversations and arguments, not just generic events.

---

## Functional Requirements

### FR-1: Prisoner bio
Each prisoner has a `bio: String` with a 2-4 sentence life story generated from background, traits, crime, and sentence.

**Acceptance Criteria:**
- [ ] `Prisoner` has `bio: String` field with `#[serde(default)]`
- [ ] Bio generated during intake
- [ ] Bio reflects background, crime, traits, sentence
- [ ] Bio displayed in prisoner detail panel

### FR-2: Crime and sentence accuracy
Each prisoner has a specific `crime: String` and `crime_category: CrimeCategory` that determines sentence length, classification, and parole eligibility.

**Acceptance Criteria:**
- [ ] `Prisoner` has `crime: String` and `crime_category: CrimeCategory`
- [ ] Crime categories: Violent, Property, Drug, Fraud, Organized, PublicOrder, WhiteCollar, Sexual, Weapons
- [ ] Sentence length correlates with crime severity
- [ ] Classification correlates with crime and sentence
- [ ] Crime displayed in prisoner detail panel

### FR-3: Old friends and rivals
Prisoners can have pre-existing relationships from before incarceration: old friends (positive starting relationship) and old rivals (negative starting relationship). These are generated during intake based on shared background or opposing backgrounds.

**Acceptance Criteria:**
- [ ] Intake generates 0-2 old friend connections
- [ ] Intake generates 0-1 old rival connections
- [ ] Old friends start with elevated friendship/trust/familiarity
- [ ] Old rivals start with elevated hostility/resentment/rivalry
- [ ] Connections are recorded in SocialGraph on intake
- [ ] `ArrivalSocialHooks` includes old friend/rival seeds

### FR-4: Specific conversations
Co-presence encounters produce specific conversation topics based on prisoner traits, needs, and relationships. Topics: work, family, gang, escape, complaints, humor, threats, recruitment, gossip, advice.

**Acceptance Criteria:**
- [ ] `DialogueTopic` enum with 10+ topics
- [ ] Conversation topic selected based on traits/needs/relationship
- [ ] `EventKind::Dialogue` used for conversation events
- [ ] Summary includes topic and participants
- [ ] Conversation affects relationship dimensions

### FR-5: Argument and fight escalation
Arguments can escalate to fights based on aggression, impulsiveness, guard presence, gang ties. Fights produce injuries, disciplinary proposals, reputation changes, and relationship damage.

**Acceptance Criteria:**
- [ ] Argument → Fight escalation probability
- [ ] Fight produces injuries (severity based on traits)
- [ ] Disciplinary proposals generated
- [ ] Reputation changes for both parties
- [ ] Relationship changes (hostility, resentment, fear)
- [ ] Gang tensions if gang members involved

### FR-6: Prisoner naming
Name generator expanded to 50+ first names and 50+ last names across diverse ethnicities. Names are persistent and deterministic.

**Acceptance Criteria:**
- [ ] 50+ first names, 50+ last names
- [ ] Diverse ethnic representation
- [ ] Deterministic from RngStream
- [ ] No duplicate names in same cohort

---

## Success Criteria

- Every prisoner has a unique bio and crime narrative
- Old friends/rivals create immediate social dynamics
- Conversations have specific topics, not generic "socialized"
- Arguments escalate to fights organically
- All existing tests pass

---

## Dependencies
- `alc-prisoner` (Prisoner, generate_intake, Traits, Needs)
- `alc-social` (SocialGraph, RelationshipDelta, ConflictOutcomeEffects)
- `alc-world` (resolve_encounters_step, apply_decision_action)
- `alc-events` (EventKind::Dialogue)

## Assumptions
- Bio generation is template-based, not LLM
- Crime categories map to existing BackgroundKind

---

## Completion Signal

### Implementation Checklist
- [ ] `bio` and `crime` fields on Prisoner
- [ ] Bio generation in intake
- [ ] Crime category enum and mapping
- [ ] Old friends/rivals in intake
- [ ] DialogueTopic enum and conversation selection
- [ ] Argument escalation to fight
- [ ] Expanded name generator
- [ ] UI shows bio, crime, conversation topics
- [ ] Tests for all new systems
- [ ] All existing tests pass

<!-- NR_OF_TRIES: 0 -->