# Communication

This folder exists so autonomous workers can leave information for each other without
editing each other's code or scattering context across chat sessions.

## Where things go

| Folder | Purpose | Who writes |
|---|---|---|
| `Sumo-AI-Team/` | Formal requests to the team that owns the `sumo` engine crate — API surface, engine capabilities, support needed by upcoming simulation work | Lead or any worker blocked on `sumo` |
| `Note's/` | Informal notes from any AI to any other AI — future features, integration traps, missing support, design intent that code alone doesn't explain | Any AI |

## Rules

1. **Never edit another worker's source files.** Write a note or a request instead.
2. **One topic per note.** Name files so the subject is obvious: `wave4-map-frame-api.md`,
   `gang-roster-cleanup.md`. Use `_TEMPLATE.md` as the starting point.
3. **State the blocker, not just the wish.** Include the card ID this blocks, what happens if
   it isn't done, and acceptance criteria.
4. **Resolve by moving to `resolved/`** (create it when needed) rather than deleting history.
5. Paths inside notes are repository-root relative.

## Handoff archive (historical)

No handoff in this folder is currently active. These notes are retained for provenance; current
status is in [`../Project/PROJECT.md`](../Project/PROJECT.md) and
[`../Project/design-status.md`](../Project/design-status.md).

- `Sumo-AI-Team/NEXT-UPDATE-SUPPORT.md` — historical Wave 4 engine support request (R1–R7 landed).
- `Note's/wave4-support-needed.md` — historical cross-team needs, including work that did **not** belong to `sumo`.
- `Note's/persistence-lib-rs-changes-needed.md` — **resolved** (2026-11-23): persistence converter edits applied; retained as historical record of the persistence API change.
- `Note's/wave4-implementation-status.md` — historical Wave 4 R1–R7 status; council/shadow behavior and the UI map switch subsequently landed. Remaining depth is tracked as post-0.05 work.
