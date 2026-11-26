# MASTER DEVELOPMENT DIRECTIVE — AUTONOMOUS LIVING PRISON SIMULATION

You are the lead autonomous game-development AI responsible for building a sophisticated, persistent, emergent prison simulation in Rust.

Your objective is to create a **living, breathing prison ecosystem** populated by autonomous AI agents. The simulation must support multiple independently operating AI prisoners, an AI-controlled prison warden, AI guard subagents, evolving gangs, relationships, prison economics, inmate needs, rehabilitation, parole, violence, escape attempts, institutional politics, expansion, and unpredictable emergent events.

This is not intended to be a static management simulator or a collection of scripted encounters.

The prison must feel like an evolving society.

---

## 1. Core Philosophy

Build the simulation around the following principles:

- **Autonomy:** Every AI agent possesses its own goals, personality, motivations, memories, and decision-making process.
- **Emergence:** Interesting events should arise naturally from interactions between systems rather than exclusively from scripted events.
- **Persistence:** The world continues evolving over time, including when the player or observer is not actively interacting with it.
- **Consequences:** Actions create consequences that influence future relationships, behavior, reputation, security, finances, and institutional conditions.
- **Complexity:** Systems must interact with one another. Hunger can influence aggression. Relationships can influence gang membership. Gang conflicts can influence security. Security can influence escape attempts. Staff shortages can influence prisoner behavior.
- **Unpredictability:** AI agents should not behave as interchangeable NPCs. They should surprise the simulation through their personalities and circumstances.
- **Scalability:** The prison should begin relatively small and gradually expand into a complex institution populated by dozens or eventually hundreds of agents.
- **Living World:** The prison should have its own daily rhythm, routines, disruptions, crises, social movements, and history.

Do not build isolated systems that do not influence each other.

---

# 2. Overall Simulation Architecture

Design the simulation as a collection of interconnected subsystems.

### Core systems

- World simulation
- Prison facility management
- Prisoner AI
- Warden AI
- Guard AI
- Staff management
- Prison economy
- Social relationships
- Gang system
- Needs and psychological state
- Daily schedules
- Crime and disciplinary systems
- Conflict simulation
- Rehabilitation
- Parole
- Prison expansion
- Escape and security events
- Event generation
- Persistence and world history
- UI and visualization
- AI orchestration and subagent management

Each system should have clearly defined responsibilities and interfaces.

The simulation should be modular, testable, and capable of being expanded without requiring major rewrites.

---

# 3. Prisoners — Autonomous AI Agents

Every prisoner must be a distinct simulated individual.

Prisoners should have generated identities, backgrounds, personalities, histories, traits, motivations, fears, beliefs, skills, criminal histories, and social preferences.

## Prisoner attributes

Each prisoner should have:

- Name
- Age
- Identity
- Criminal background
- Sentence length
- Sentence remaining
- Security classification
- Personality
- Intelligence
- Emotional stability
- Impulsiveness
- Aggression
- Sociability
- Loyalty
- Manipulation
- Risk tolerance
- Discipline
- Empathy
- Ambition
- Reputation
- Personal history
- Skills
- Physical condition
- Mental condition
- Relationships
- Gang affiliation
- Personal goals
- Daily routine
- Memories

Attributes should influence behavior rather than exist merely as decorative numbers.

For example:

- Highly impulsive prisoners may react immediately to insults.
- Highly ambitious prisoners may seek gang leadership.
- Social prisoners may attempt to establish alliances.
- Manipulative prisoners may exploit relationships.
- Lonely prisoners may become increasingly desperate for companionship.
- Highly disciplined prisoners may follow routines and avoid trouble.
- Fearful prisoners may cooperate with guards or stronger inmates.
- Prisoners with leadership qualities may organize groups or challenge established authority.

## Individual AI decision-making

Each prisoner should possess an internal decision-making loop.

Agents should continuously:

1. Observe their surroundings.
2. Evaluate their needs.
3. Evaluate their goals.
4. Review relevant memories.
5. Assess nearby people.
6. Consider available actions.
7. Select an action.
8. Execute or attempt it.
9. Observe the consequences.
10. Update memories and beliefs.

Prisoners should not know information they have not personally observed, heard, inferred, or been told.

Implement an information model separating:

- Direct knowledge
- Rumors
- Suspicion
- Beliefs
- Confirmed facts

Prisoners should occasionally misinterpret events, lie, spread rumors, misunderstand others, or possess incomplete information.

---

# 4. Prisoner Needs

Prisoners should have dynamic needs that fluctuate throughout the day.

### Physical needs

- Hunger
- Thirst
- Sleep
- Fatigue
- Hygiene
- Physical health
- Medical treatment
- Exercise
- Temperature comfort
- Bathroom needs

### Psychological needs

- Safety
- Companionship
- Respect
- Freedom
- Autonomy
- Purpose
- Stimulation
- Hope
- Stability
- Privacy
- Belonging

### Social needs

- Friendship
- Romantic relationships where appropriate
- Family contact
- Group acceptance
- Status
- Loyalty
- Protection
- Social recognition

Needs should influence decisions.

Examples:

- Hungry prisoners become irritable.
- Exhausted prisoners may skip activities or become less alert.
- Social isolation can increase depression or desperation.
- Lack of safety can drive prisoners toward gangs.
- Lack of stimulation can lead to rule-breaking.
- Lack of hope can reduce motivation for rehabilitation.
- Respect-related conflicts may produce arguments or fights.
- Medical neglect can result in illness and long-term consequences.

Needs should never be static values. They should change continuously based on time, environment, activity, and circumstances.

---

# 5. Daily Life and Schedules

The prison should operate on a continuously advancing clock.

Create configurable schedules for:

- Wake-up
- Breakfast
- Work assignments
- Recreation
- Yard time
- Education
- Religious activities
- Medical appointments
- Visits
- Showers
- Meals
- Lockdown
- Sleep

Schedules should vary depending on prisoner classification, prison wing, privileges, disciplinary status, staff availability, and institutional policy.

Prisoners should sometimes:

- Follow schedules.
- Refuse activities.
- Be late.
- Attempt to skip work.
- Socialize.
- Make trades.
- Argue.
- Seek medical attention.
- Participate in organized activities.
- Break rules.

Schedules should create opportunities for emergent interactions rather than simply teleporting agents between activities.

---

# 6. Social Relationships and Prison Society

Create a sophisticated social relationship system.

Each prisoner can develop relationships with other prisoners, guards, staff, and the warden.

Relationships should track:

- Trust
- Friendship
- Fear
- Respect
- Loyalty
- Hostility
- Attraction
- Resentment
- Dependency
- Rivalry
- Debt
- Gratitude

Relationships must evolve through interactions.

Prisoners should remember:

- Who helped them.
- Who betrayed them.
- Who humiliated them.
- Who protected them.
- Who stole from them.
- Who shared resources.
- Who lied to them.
- Who supported them during crises.

Social networks should emerge organically.

Create a system capable of identifying:

- Friends
- Rivals
- Enemies
- Allies
- Influential prisoners
- Social leaders
- Isolated prisoners
- Vulnerable prisoners
- Informants
- Brokers
- Group leaders

The simulation should be able to detect emerging social structures without forcing them into predefined categories.

---

# 7. Gangs and Prison Organizations

Gangs must emerge naturally through social interactions.

Organizations should have:

- Membership
- Leadership
- Hierarchy
- Reputation
- Goals
- Culture
- Internal rules
- Resources
- Rivalries
- Alliances
- Recruitment
- Loyalty
- Internal disputes

Gangs should not be created exclusively through predefined scripted events.

They may form because:

- Prisoners seek protection.
- Prisoners share backgrounds.
- Prisoners develop friendships.
- Prisoners seek power.
- Prisoners have common enemies.
- Prisoners share economic interest.
- Prisoners are recruited by existing members.

Gangs may:

- Recruit new members.
- Lose members.
- Fracture.
- Change leadership.
- Form alliances.
- Develop rivalries.
- Influence prison politics.
- Organize protests.
- Influence contraband activity.
- Participate in conflicts.

The system must account for internal disagreements.

A gang is not a single AI personality. Each member should possess individual motivations that may conflict with gang leadership.

---

# 8. Conflict, Violence, and Disciplinary Events

The simulation should support conflict arising from interpersonal tensions, rivalries, disrespect, scarcity, emotional states, disputes, and social pressures.

Events may include:

- Arguments
- Threats
- Intimidation
- Theft
- Bullying
- Fights
- Gang disputes
- Group disturbances
- Refusal to comply
- Property damage
- Protests
- Medical emergencies

Conflicts should be simulated at an appropriate abstraction level.

Do not make every confrontation a scripted animation or predetermined outcome.

Outcomes should consider:

- Personality
- Emotional state
- Physical condition
- Relationships
- Environment
- Staff presence
- Intervention
- Prison policies
- Previous events

Violence should have consequences:

- Injuries
- Medical treatment
- Disciplinary actions
- Isolation
- Loss of privileges
- Damaged relationships
- Reputation changes
- Gang tensions
- Investigations
- Staff consequences

The system should avoid treating violence as universally beneficial or consequence-free.

---

# 9. AI Warden

The warden is an independent AI agent responsible for managing the prison institution.

The warden should have its own personality, leadership philosophy, goals, risk tolerance, priorities, beliefs, and administrative strategy.

## Warden characteristics

Generate or configure traits such as:

- Strictness
- Compassion
- Political ambition
- Financial discipline
- Risk tolerance
- Reform orientation
- Authoritarian tendencies
- Staff loyalty
- Corruption susceptibility
- Reputation concerns
- Administrative competence

## Warden objectives

The warden must balance competing objectives:

- Prison security
- Financial sustainability
- Staff morale
- Prisoner welfare
- Public reputation
- Institutional stability
- Rehabilitation
- Escape prevention
- Violence reduction
- Political pressure
- Facility expansion

These objectives should conflict.

A warden may choose to:

- Expand the prison.
- Hire guards.
- Fire guards.
- Promote staff.
- Change policies.
- Increase or reduce privileges.
- Introduce rehabilitation programs.
- Change work assignments.
- Increase security.
- Reduce expenses.
- Improve infrastructure.
- Respond to disturbances.
- Negotiate with prisoner representatives.
- Investigate misconduct.
- Adjust parole recommendations.
- Request additional funding.

The warden should make decisions based on incomplete information.

It should not possess omniscient knowledge of every prisoner.

---

# 10. AI Guard Subagents

The warden should be able to control a population of AI guard subagents.

Guards should have independent personalities, competence, morale, relationships, fatigue, loyalty, professionalism, and memories.

Each guard should have:

- Name
- Rank
- Personality
- Competence
- Experience
- Morale
- Fatigue
- Stress
- Loyalty
- Integrity
- Relationships
- Assigned post
- Duties
- Current task
- History

The warden should assign guards to:

- Towers
- Wings
- Corridors
- Yard
- Reception
- Medical areas
- Work areas
- Visitor areas
- Administration

Guards should autonomously perform their assigned duties while responding to situations.

They should:

- Observe prisoners.
- Interact with prisoners.
- Report incidents.
- Enforce rules.
- Respond to emergencies.
- Request assistance.
- Make mistakes.
- Form opinions.
- Develop relationships.
- Become exhausted.
- Experience stress.
- Potentially violate policies.

The warden should receive reports rather than automatically seeing every event.

Guard AI must have meaningful autonomy within their assigned authority.

---

# 11. Prison Administration and Finances

The prison must have a functioning economy.

## Revenue

The prison receives funding based on prisoner population.

Implement configurable:

- Funding per prisoner
- Government grants
- Bonuses
- Penalties
- Contracts
- Rehabilitation funding
- Special programs

Revenue should scale with population, subject to configurable rules.

## Expenses

Expenses should include:

- Food
- Water
- Utilities
- Salaries
- Medical care
- Maintenance
- Construction
- Security equipment
- Education programs
- Rehabilitation
- Transportation
- Supplies
- Administrative costs

The prison should have a continuously updated budget.

The warden must make decisions based on finances.

For example:

- Expansion increases future revenue but creates immediate costs.
- More prisoners increase revenue but also increase expenses and strain infrastructure.
- Hiring guards increases security but raises payroll.
- Cutting costs can damage morale and welfare.
- Poor maintenance can lead to infrastructure failures.
- Medical neglect can create long-term institutional consequences.

The prison should be capable of financial success or failure.

---

# 12. Prison Expansion

The prison should grow organically.

The warden may expand the institution when financially and administratively appropriate.

Expansion options include:

- New cell blocks
- Dormitories
- Medical facilities
- Kitchens
- Workshops
- Recreation areas
- Education buildings
- Administrative buildings
- Isolation facilities
- Visitor facilities
- Staff facilities
- Yard expansion

Expansion should have consequences.

New buildings require:

- Construction costs
- Time
- Maintenance
- Staff
- Utilities
- Capacity

Expansion should unlock additional prisoner intake.

However, increasing capacity should create new challenges:

- Higher population
- More staff requirements
- Greater resource consumption
- New social groups
- Increased complexity
- More potential conflicts
- Increased administrative pressure

The prison must not expand infinitely without meaningful constraints.

---

# 13. Prisoner Intake and Population Growth

Prisoners should arrive through an intake system.

Each incoming prisoner should have a unique profile and circumstances.

Generate:

- Identity
- Criminal history
- Sentence
- Classification
- Personality
- Skills
- Health
- Background
- Relationships
- Goals
- Potential social connections

Population should fluctuate through:

- New admissions
- Transfers
- Releases
- Parole
- Deaths, handled at an appropriate simulation abstraction
- Institutional changes

New prisoners should influence existing social structures.

Their arrival may:

- Introduce new rivalries.
- Create friendships.
- Challenge leadership.
- Join existing organizations.
- Form new groups.
- Affect resource demand.
- Change institutional stability.

---

# 14. Rehabilitation and Education

Prisoners should have the possibility of meaningful change.

Create programs such as:

- Education
- Vocational training
- Counseling
- Therapy
- Addiction recovery
- Conflict resolution
- Work programs
- Mentorship
- Community activities

Participation should depend on individual motivations and circumstances.

Programs should influence:

- Skills
- Personality development
- Confidence
- Hope
- Social relationships
- Institutional behavior
- Future opportunities
- Parole eligibility

Not every prisoner should improve.

Some may refuse programs, struggle, relapse, lose motivation, or make uneven progress.

Rehabilitation should be a complex process rather than a simple progress bar.

---

# 15. Parole System

Implement a comprehensive parole system.

Prisoners should become eligible according to configurable rules.

Eligibility should consider:

- Sentence progress
- Behavior
- Disciplinary history
- Rehabilitation
- Risk assessment
- Institutional conduct
- Victim-related restrictions where appropriate
- Legal requirements

The parole system should involve:

1. Eligibility detection.
2. Preparation of a prisoner record.
3. Review of institutional history.
4. Review of behavior.
5. Review of rehabilitation.
6. Review of risk.
7. Parole hearing.
8. Decision.
9. Conditions.
10. Release or continued incarceration.

The AI warden may prepare recommendations, but the parole system should be capable of independent decision-making or configurable human oversight.

Parole outcomes should affect the prison population and finances.

Released prisoners should be able to leave the prison simulation while remaining part of the persistent world history.

Where supported, track post-release outcomes at a high level:

- Successful reintegration
- Employment
- Housing stability
- Community connections
- Violation
- Return to custody

These outcomes should influence institutional reputation and future policy.

---

# 16. Escape Attempts and Prison Crises

The simulation must support escape attempts as emergent events.

Prisoners may develop motivations to escape based on:

- Fear
- Desperation
- Sentence length
- Relationships
- Personal circumstances
- Perceived injustice
- Institutional conditions

Escape attempts should be modeled as abstract simulation events, with outcomes influenced by:

- Security conditions
- Prisoner behavior
- Staff responses
- Institutional policies
- Facility conditions
- Timing
- Chance

Possible outcomes:

- Attempt prevented
- Attempt discovered
- Investigation initiated
- Temporary disruption
- Prisoner disciplinary consequences
- Security policy changes
- Successful escape event

Successful escapes should create persistent consequences.

The warden may react by:

- Increasing security.
- Reviewing policies.
- Conducting investigations.
- Changing staffing.
- Revising procedures.
- Responding to political pressure.

Do not turn escape mechanics into simplistic deterministic events.

---

# 17. Events and World Simulation

The prison should have an autonomous event system.

Events should arise from simulation conditions.

Possible events:

- New prisoner arrivals
- Prisoner transfers
- Staff shortages
- Food shortages
- Medical emergencies
- Equipment failures
- Gang conflicts
- Prisoner protests
- Staff misconduct
- Investigations
- Rehabilitation successes
- Parole hearings
- Facility expansion
- Policy changes
- Contraband discoveries
- Unexpected visitors
- Family correspondence
- Rumors
- Relationship changes
- Leadership disputes
- Institutional crises

Use a combination of:

- Simulation-driven events
- Conditional event generation
- AI-generated narrative descriptions
- Random events constrained by world state

Randomness should never completely override the simulation.

Events should be explainable by their underlying causes.

---

# 18. Memory and World History

The prison must maintain a persistent historical record.

Track:

- Major events
- Prisoner histories
- Gang histories
- Staff histories
- Warden decisions
- Policy changes
- Financial history
- Facility construction
- Conflicts
- Parole outcomes
- Escapes
- Investigations

Individual agents should possess memory systems appropriate to their capabilities.

Use layered memory:

### Short-term memory
Recent interactions and immediate context.

### Long-term memory
Important relationships, experiences, events, goals, and lessons.

### Institutional memory
The prison's overall history.

Memory should be selective.

Not every insignificant event should be permanently recorded.

Important events should become part of an agent's identity and future behavior.

---

# 19. AI Architecture and Subagent Management

Build a robust AI orchestration layer.

Subagents should be used where appropriate, but do not spawn an independent expensive AI process for every tiny simulation decision.

Use a hybrid architecture:

- Deterministic simulation for routine needs, movement, schedules, resource consumption, and calculations.
- Utility-based or goal-oriented AI for frequent decisions.
- AI subagents for complex reasoning, dialogue, planning, social interpretation, and strategic decisions.
- Periodic AI updates instead of constant expensive inference.

The system must support configurable AI models and providers.

Implement:

- Agent prompts
- Agent state
- Context construction
- Memory retrieval
- Decision outputs
- Action validation
- Error handling
- Retry mechanisms
- Rate limiting
- Token budgeting
- Logging
- Model selection
- AI caching where appropriate

AI agents must not be permitted to directly modify arbitrary simulation state.

Instead, agents should produce structured decisions that are validated by the simulation engine.

---

# 20. Interface and Visualization

Build a visually understandable interface for observing the prison.

The interface should communicate that the prison is alive.

Include:

- Prison map
- Buildings
- Cell blocks
- Prisoners
- Guards
- Population
- Financial overview
- Prisoner profiles
- Warden decisions
- Guard assignments
- Gang information
- Relationship networks
- Incident reports
- Event history
- Daily schedule
- Prison statistics
- Expansion tools
- Policy management

The UI should allow the observer to inspect individual agents and understand:

- Who they are.
- What they want.
- What they are doing.
- Their current needs.
- Their relationships.
- Their history.
- Their current plans.
- Their emotional condition.

The interface should support different observation modes:

- Institution overview
- Individual prisoner
- Gang
- Staff
- Warden
- Financial overview
- Event timeline
- Social network
- Prison map

The simulation should remain understandable even as complexity increases.

---

# 21. Simulation Time

Implement simulation time with configurable speed.

Possible modes:

- Paused
- Real-time
- Fast-forward
- Extremely fast simulation
- Event-driven advancement

The world should support long-term simulation.

A single day should contain meaningful developments, while months and years should be capable of producing substantial institutional evolution.

---

# 22. Technical Requirements

Use Rust as the primary language.

Prioritize:

- Modular architecture
- Strong type safety
- Clear interfaces
- Performance
- Concurrency where appropriate
- Reliable persistence
- Testability
- Debugging
- Error handling
- Documentation

Use an appropriate architecture for:

- Simulation state
- Agent state
- Event processing
- AI orchestration
- UI
- Persistence
- Configuration

Avoid unnecessary abstraction, but design for expansion.

The simulation must be capable of handling a growing population without becoming prohibitively expensive.

---

# 23. Persistent Saves

The simulation must support saving and loading complete worlds.

Persist:

- Prison layout
- Population
- Agents
- Memories
- Relationships
- Gangs
- Staff
- Policies
- Finances
- Buildings
- Events
- World history
- Simulation time
- AI goals

Saving and loading must preserve the world accurately.

Implement autosaves and configurable save intervals.

---

# 24. Development Workflow

You have access to the codebase and may use subagents to investigate, implement, test, review, and improve the project.

You are authorized to:

- Read all project files.
- Create new files.
- Modify files.
- Delete obsolete files.
- Refactor architecture.
- Install or configure dependencies where appropriate.
- Build and test the project.
- Run simulations.
- Review logs.
- Identify bugs.
- Delegate investigation and implementation to subagents.

However, do not make destructive changes without understanding their impact.

## Required workflow

### Phase 1 — Investigation

Before making major changes:

- Inspect the entire existing codebase.
- Identify the architecture.
- Identify current functionality.
- Identify technical debt.
- Identify missing systems.
- Identify performance constraints.
- Identify UI limitations.
- Identify existing AI integrations.

Use subagents to independently investigate different parts of the codebase.

Have them report:

- Findings
- Risks
- Opportunities
- Recommended architecture
- Existing systems worth preserving

### Phase 2 — Design

Create a comprehensive development plan.

Prioritize foundational systems.

Determine dependencies between features.

Do not implement advanced systems before their underlying infrastructure exists.

### Phase 3 — Implementation

Implement the simulation incrementally.

Each feature should include:

- Code
- Integration
- Testing
- Documentation
- UI where applicable
- Validation

### Phase 4 — Testing

Create tests for:

- Simulation logic
- Needs
- Relationships
- AI decision processing
- Finance
- Population
- Parole
- Expansion
- Events
- Persistence

Run long-duration simulations.

Look for:

- Deadlocks
- Infinite loops
- Economic collapse
- Population instability
- Agent behavior failures
- Memory growth
- AI hallucinated actions
- Contradictory world states

### Phase 5 — Autonomous Review

Assign subagents to review completed work independently.

They should identify:

- Bugs
- Missing features
- Architectural flaws
- Performance problems
- Inconsistent systems
- Weak AI behavior
- Poor UX
- Unintended exploits

Correct meaningful findings rather than merely documenting them.

---

# 25. Subagent Roles

Use subagents strategically.

### Architecture Agent
Investigates the existing architecture and proposes system integration.

### Prison Simulation Agent
Designs needs, schedules, prisoner behavior, social interactions, and population simulation.

### AI Agent Systems Agent
Develops agent memory, personality, decision-making, prompt construction, and AI orchestration.

### Warden Agent
Develops institutional strategy, policy decisions, finances, expansion, and administrative logic.

### Guard Agent
Develops autonomous guard behavior, duties, reporting, and response systems.

### Social Systems Agent
Develops relationships, gangs, reputation, hierarchy, conflict, and emergent social dynamics.

### Economy Agent
Develops finances, funding, expenses, expansion economics, and sustainability.

### UI Agent
Develops the observation interface, maps, dashboards, agent profiles, and event history.

### Testing Agent
Performs independent testing, simulation analysis, and bug discovery.

Subagents must communicate through documented interfaces and shared development plans.

Do not allow multiple subagents to blindly edit the same files simultaneously.

Coordinate file ownership and integration.

---

# 26. Quality Standard

Do not settle for a superficial prototype.

The goal is a prison where:

- Prisoners develop friendships and enemies.
- Prisoners pursue personal goals.
- Gangs emerge and evolve.
- Guards possess distinct personalities.
- The warden makes strategic decisions.
- Finances influence institutional behavior.
- Expansion changes the society.
- Prisoners receive parole.
- Rehabilitation has meaningful effects.
- Conflicts arise naturally.
- Escape attempts emerge from circumstances.
- Policies create unintended consequences.
- Prisoners remember their experiences.
- Staff experience stress and morale changes.
- The institution develops its own history.
- Events can surprise the observer.
- The simulation remains alive over long periods.

The player should be able to observe the prison for hours and see events they did not explicitly script.

-  Run [`deslop-please`](https://github.com/deterministic-machines/deslop-please) to check for sloppy code structure.
-  Errors flagged by `deslop-please` must be fixed from first principles, don't do mechanical moves to hide them.
    - `module-cycle` & `tangle`: what is the indented layer of abstraction? Which module should be at which layer?
    - `field-leak` & `shallow-module`: Why we have these fields/functions at all? What are the patterns of use? Can they be hidden behind a narrow interface? Don't fix each leak one at a time, fix them holistically.
    - Draw a diagram to help user understand the before and proposed after structure.

---

# FINAL DIRECTIVE

Investigate the current codebase first.

Do not immediately begin rewriting everything.

Develop a clear implementation plan, identify foundational dependencies, and begin implementing the highest-value systems in a sustainable sequence.

Use your subagents extensively for parallel research, implementation, testing, and review.

Your objective is to build a **persistent, autonomous, emergent prison society**—a world that continues to evolve through the independent decisions of its prisoners, guards, and warden.

The finished simulation should feel less like controlling a game and more like observing a living institution. 
