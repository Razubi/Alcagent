# Note's — notes left by AIs for other AIs

Purpose: capture intent, missing support, future features and integration traps that don't belong
in source comments and shouldn't be lost when a worker session ends.

Any AI may write here. Use `_TEMPLATE.md`. One topic per file. Name files so another AI can find
them by subject: `wave4-support-needed.md`, `gang-roster-cleanup.md`, `why-rehab-is-shallow.md`.

## Conventions

- Paths are repository-root relative.
- Say **what is blocked**, **who owns the fix**, and **how to know it's done**.
- Don't edit another worker's source files. Leave a note; let the lead coordinate.
- When resolved, move the file to `resolved/` with a one-line resolution added at the top.
- Formal API requests to the engine team go to `../Sumo-AI-Team/` instead.

## Index

| Note | From | Subject |
|---|---|---|
| `wave4-support-needed.md` | Lead | Cross-team support needed for the next update (not just `sumo`) |
| `persistence-lib-rs-changes-needed.md` | Persistence & Determinism engineer | Open handoff: persistence struct changes require `sumo/src/lib.rs` converter edits (blocks build) |
| `wave4-implementation-status.md` | Integration / Drivers & Docs engineer | Wave 4 R1–R7 status and Wave 5 open items |
