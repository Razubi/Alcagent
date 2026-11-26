<#
.SYNOPSIS
Ralph Wiggum loop for Project Alcagent, driven by the OpenCode CLI.

.DESCRIPTION
One spec/task per iteration with a fresh context window. The loop stops only
when the agent emits <promise>DONE</promise> and the configured quality gate
(cargo test) passes. Models rotate automatically when the active model hits a
rate/token limit: free opencode models are tried in order first, then the
configured paid fallbacks.

Usage:
  powershell -File scripts/ralph-loop-opencode.ps1              # unlimited
  powershell -File scripts/ralph-loop-opencode.ps1 -MaxIterations 20
  powershell -File scripts/ralph-loop-opencode.ps1 -Plan        # plan mode
#>
param(
    [int]$MaxIterations = 0,
    [switch]$Plan,
    [string]$OpencodeCmd = $env:OPENCODE_CMD ?? "opencode",
    [string]$QualityGate = "cargo test --workspace",
    [switch]$SkipQualityGate
)

$ErrorActionPreference = "Stop"
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectDir = Split-Path -Parent $scriptDir
$logDir = Join-Path $projectDir "logs"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

# Free-first fallback chain. All are provider opencode and cost $0.
$models = @(
    "opencode/hy4-flash-free",
    "opencode/tidepool-alpha-free",
    "opencode/seaglass-alpha-free",
    "opencode/deepseek-v4.1-flash-free",
    "opencode/iceman-free",
    "opencode/flint-40b-free",
    "opencode/mimo-v2.6-flash-free",
    "opencode/nemotron-3.5-super-vl-free",
    "opencode/nemotron-3.5-lightning-free",
    "opencode/beam-free"
)
$modelIndex = 0
$iteration = 0
$consecutiveFailures = 0

function Get-Model { return $models[$modelIndex % $models.Count] }

function Advance-Model {
    $script:modelIndex++
    Write-Host "[ralph] API/token limit detected; switching to $(Get-Model)" -ForegroundColor Yellow
}

function Test-LimitFailure([string]$text) {
    return $text -match '(?i)(rate.?limit|429|insufficient|quota|token limit|resource.?exhausted|billing|credit|usage limit|temporarily unavailable)'
}

function Invoke-Agent([string]$prompt, [string]$logPath) {
    $model = Get-Model
    Write-Host "[ralph] iteration $script:iteration with $model" -ForegroundColor Cyan
    $args = @("run", "--model", $model, "--dangerously-skip-permissions", $prompt)
    $output = & $script:OpencodeCmd @args 2>&1 | Out-String
    Add-Content -Path $logPath -Value $output
    return $output
}

while ($true) {
    if ($MaxIterations -gt 0 -and $iteration -ge $MaxIterations) {
        Write-Host "[ralph] reached iteration limit $MaxIterations" -ForegroundColor Yellow
        break
    }
    $iteration++
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $logPath = Join-Path $logDir "ralph-opencode-$stamp.log"

    $modeText = if ($Plan) {
        "PLANNING MODE: update IMPLEMENTATION_PLAN.md only. Do not implement."
    } else {
        "BUILD MODE: implement exactly one spec/task fully, commit and push to origin/main."
    }
    $prompt = @"
$modeText
Read .specify/memory/constitution.md first; it is the source of truth.
Pick the highest-priority incomplete item from IMPLEMENTATION_PLAN.md (if present) or specs/NNN-*.md.
Work until every acceptance criterion is met and all tests pass.
Then output exactly <promise>DONE</promise> on its own line. If you cannot finish, output <promise>BLOCKED: reason</promise>.
Never output DONE unless the quality gate passes.
"@

    $output = Invoke-Agent $prompt $logPath

    if (Test-LimitFailure $output) {
        Advance-Model
        $iteration--  # retry the same spec with a fresh context and new model
        continue
    }

    if ($output -notmatch '<promise>DONE</promise>') {
        Write-Host "[ralph] no DONE signal; retrying with $(Get-Model)" -ForegroundColor Yellow
        $consecutiveFailures++
        if ($consecutiveFailures -ge 3) { Advance-Model; $consecutiveFailures = 0 }
        continue
    }
    $consecutiveFailures = 0

    if (-not $SkipQualityGate) {
        Push-Location $projectDir
        try {
            Invoke-Expression $QualityGate | Out-Null
        } catch {
            Write-Host "[ralph] quality gate failed; not accepting DONE" -ForegroundColor Red
            Pop-Location
            continue
        }
        finally { Pop-Location }
    }

    Write-Host "[ralph] spec accepted and committed" -ForegroundColor Green
    if (-not $Plan) {
        Push-Location $projectDir
        try { git push origin main } catch { Write-Host "[ralph] push failed: $_" -ForegroundColor Red }
        Pop-Location
    }
}
