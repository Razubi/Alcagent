# Overnight Alcagent 0.05 development loop

This directory implements a spec-driven, fresh-context autonomous development loop (Ralph Wiggum methodology) for Project Alcagent.

## Start

```powershell
# Unlimited overnight run
powershell -ExecutionPolicy Bypass -File dev\ralph\ralph-loop.ps1

# Bounded run
powershell -ExecutionPolicy Bypass -File dev\ralph\ralph-loop.ps1 -MaxIterations 8

# Dry run (prints the opencode command, no agent launched)
powershell -ExecutionPolicy Bypass -File dev\ralph\ralph-loop.ps1 -DryRun -MaxIterations 1
```

The loop picks the first spec whose `Status:` is `pending`, marks it `in_progress`, launches a fresh `opencode run` session with `--auto --archived`, and only continues when the agent emits `<promise>DONE</promise>` and the spec is `done`.

## Stop

- Press `Ctrl+C` in the terminal running the loop.
- Or create a file named `dev/ralph/STOP` — the loop checks for it before each iteration.

## Logs

- `logs/ralph/session_*.log` — full loop output.
- `logs/ralph/iter_*.log` — per-iteration agent output.

## Safety

- No pushes, resets, or history rewrites.
- Every spec requires build + test validation before `DONE`.
- Rate-limit detection rotates to the next configured free model.
- Use `-Model` / `-Models` to override models.

## Game inspection

`.codegpt-game.json` at the repo root configures the game-development skill:

```powershell
node C:\Users\matth\.claude\skills\game-development\game.mjs run --dir .
node C:\Users\matth\.claude\skills\game-development\game.mjs logs --errors --dir .
```
