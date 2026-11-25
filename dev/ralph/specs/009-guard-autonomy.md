# Spec 009 — Guard autonomy and reporting

Status: done

## Requirements
Guards are independent subagents with duties, fatigue, stress, opinions, and imperfect reporting.

## Acceptance criteria
- [x] Guards observe assigned areas, interact with prisoners, report incidents, and request assistance.
- [x] Guards can make mistakes, become exhausted/stressed, form opinions, and occasionally violate policy.
- [x] Warden receives reports rather than omniscient event access.
- [x] Guard duty changes alter prisoner behavior and incident likelihood.
- [x] Add deterministic tests for reporting/fatigue consequences; full workspace passes.

Completion signal: `<promise>DONE</promise>`
