# Sumo AI Team — handoff channel

Target team: whoever owns the `sumo` crate (the engine/control surface over `alc-world`).

## How to use this folder

- `NEXT-UPDATE-SUPPORT.md` is a **historical Wave 4 request**, retained for provenance; it is not active.
- Add new requests as separate files named `<topic>-request.md`; do not append unrelated items
  to the active request.
- When a request is satisfied, move the file into `resolved/` and add a one-line pointer in the
  request's header stating the commit/PR that closed it.

## Ownership boundary

The sumo team owns:

- `crates/sumo/src/**` (Sim API, observation, drivers, decisions, persistence glue)
- `crates/sumo/tests/**` (engine-level integration tests)

Simulation state and behavior live in `alc-world`, `alc-prisoner`, `alc-agents`, `alc-social`,
`alc-guard`, `alc-warden` and `alc-economy`. If a request needs changes there, it is **not** a
sumo-team task — leave a note in `../Note's/` instead and let the lead coordinate.

## Standing constraints

- Never make CI or the 730-day soak depend on an external model or network call.
- Keep `Sim` the honest read/write surface. If the UI starts reaching into `World` directly,
  the engine has failed its contract.
- Preserve determinism: seeded RNG, `trace_hash()`, save/load replay equality.
