param(
    [int]$MaxIterations = 0,
    [int]$MaxRetriesPerSpec = 3,
    [string[]]$Models = @(
        'opencode/tidepool-alpha-free',
        'opencode/hy4-flash-free',
        'opencode/mimo-v2.6-flash-free',
        'opencode/big-pickle'
    ),
    [string]$OpenCodeExe = 'C:\Users\matth\AppData\Roaming\npm\node_modules\@opencode\cli\bin\opencode.exe',
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$SpecDir = Join-Path $Root 'dev\ralph\specs'
$PromptTemplate = Join-Path $Root 'dev\ralph\prompt.txt'
$History = Join-Path $Root 'dev\ralph\history.txt'
$StopFile = Join-Path $Root 'dev\ralph\STOP'
$LogDir = Join-Path $Root 'logs\ralph'
$SessionLog = Join-Path $LogDir ("session_{0}.log" -f (Get-Date -Format 'yyyyMMdd_HHmmss'))

New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
if (-not (Test-Path $OpenCodeExe)) {
    throw "OpenCode executable not found: $OpenCodeExe"
}

function Write-Log {
    param([string]$Message, [string]$Color = 'Gray')
    $line = '[{0}] {1}' -f (Get-Date -Format 'HH:mm:ss'), $Message
    Write-Host $line -ForegroundColor $Color
    Add-Content -Path $SessionLog -Value $line
}

function Get-NextSpec {
    Get-ChildItem -Path $SpecDir -Filter '*.md' | Sort-Object Name | ForEach-Object {
        $text = Get-Content $_.FullName -Raw
        if ($text -match '(?m)^Status:\s*(pending|in_progress)\s*$') { $_ }
    } | Select-Object -First 1
}

function Set-SpecStatus {
    param([System.IO.FileInfo]$Spec, [string]$Status)
    $text = Get-Content $Spec.FullName -Raw
    $updated = $text -replace '(?m)^Status:\s*.*$', "Status: $Status"
    Set-Content -Path $Spec.FullName -Value $updated -Encoding UTF8
}

function Spec-HasStatus {
    param([System.IO.FileInfo]$Spec, [string]$Status)
    $text = Get-Content $Spec.FullName -Raw
    return $text -match ('(?m)^Status:\s*{0}\s*$' -f [regex]::Escape($Status))
}

function Append-History {
    param([string]$Message)
    Add-Content -Path $History -Value $Message -Encoding UTF8
}

function Commit-DevState {
    param([string]$Message)
    $staged = & git -C $Root status --porcelain -- dev/ralph
    if ($staged) {
        & git -C $Root add -- dev/ralph
        & git -C $Root commit -m $Message | Out-Null
    }
}

$modelIndex = 0
$iteration = 0
$retry = @{}
Write-Log ('Ralph loop started. Root={0} Models={1}' -f $Root, ($Models -join ', ')) 'Cyan'

while ($true) {
    if (Test-Path $StopFile) {
        Write-Log 'STOP file found. Exiting cleanly.' 'Yellow'
        break
    }
    if ($MaxIterations -gt 0 -and $iteration -ge $MaxIterations) {
        Write-Log ('Reached MaxIterations={0}. Exiting.' -f $MaxIterations) 'Yellow'
        break
    }

    $spec = Get-NextSpec
    if (-not $spec) {
        Write-Log 'No pending specs. 0.05 backlog complete.' 'Green'
        break
    }

    $iteration++
    $name = $spec.Name
    $previous = 0
    if ($retry.ContainsKey($name)) { $previous = [int]$retry[$name] }
    $attempt = $previous + 1
    $retry[$name] = $attempt
    $currentModel = $Models[$modelIndex % $Models.Count]
    Set-SpecStatus -Spec $spec -Status 'in_progress'

    $prompt = (Get-Content $PromptTemplate -Raw).
        Replace('{{SPEC_PATH}}', $spec.FullName).
        Replace('{{ITERATION}}', ('{0}.{1}' -f $iteration, $attempt)).
        Replace('{{MODEL}}', $currentModel)

    $iterLog = Join-Path $LogDir ("iter_{0:d3}_{1}_{2}.log" -f $iteration, $name.Replace('.md', ''), (Get-Date -Format 'HHmmss'))
    Write-Log ('Iteration {0}: {1} (attempt {2}, model {3})' -f $iteration, $name, $attempt, $currentModel) 'Magenta'

    if ($DryRun) {
        Write-Log ('DRY RUN: & "{0}" run --auto --archived --model "{1}" --title "Ralph {2}" <prompt>' -f $OpenCodeExe, $currentModel, $name) 'Yellow'
        break
    }

    & $OpenCodeExe run --auto --archived --model $currentModel --title ("Ralph {0}" -f $name) $prompt 2>&1 |
        Tee-Object -FilePath $iterLog
    $output = Get-Content $iterLog -Raw

    Add-Content -Path $SessionLog -Value $output -Encoding UTF8

    $doneSignal = $output -match '<promise>DONE</promise>'
    $blockedSignal = $output -match '<promise>BLOCKED</promise>'
    $statusDone = Spec-HasStatus -Spec $spec -Status 'done'
    $statusBlocked = Spec-HasStatus -Spec $spec -Status 'blocked'

    if ($output -match 'usage limit|rate limit|free usage limit|429') {
        $modelIndex++
        Write-Log ('Rate limit detected; rotating to {0}' -f $Models[$modelIndex % $Models.Count]) 'Yellow'
    }

    if ($doneSignal -and $statusDone) {
        $gitStatus = & git -C $Root status --porcelain
        if ($gitStatus) {
            Write-Log 'DONE but working tree is dirty; agent must commit. Retrying spec.' 'Red'
            Append-History ('- ITER {0}.{1} {2}: DONE signal without clean commit; retrying.' -f $iteration, $attempt, $name)
        } else {
            Write-Log ('PASS: {0} completed and committed.' -f $name) 'Green'
            Append-History ('- ITER {0}.{1} {2}: DONE. Validation and commit verified.' -f $iteration, $attempt, $name)
            $retry[$name] = 0
            continue
        }
    }

    if ($blockedSignal -and $statusBlocked) {
        Write-Log ('BLOCKED: {0}. Moving to next spec.' -f $name) 'Yellow'
        Append-History ('- ITER {0}.{1} {2}: BLOCKED. See spec for reason.' -f $iteration, $attempt, $name)
        $retry[$name] = 0
        continue
    }

    if ($attempt -ge $MaxRetriesPerSpec) {
        Set-SpecStatus -Spec $spec -Status 'blocked'
        Append-History ('- ITER {0}.{1} {2}: BLOCKED after {3} failed attempts. No completion signal.' -f $iteration, $attempt, $name, $MaxRetriesPerSpec)
        Commit-DevState -Message ('chore(ralph): mark {0} blocked' -f $name.Replace('.md', ''))
        Write-Log ('BLOCKED after {0} attempts: {1}. Moving on.' -f $MaxRetriesPerSpec, $name) 'Red'
        $retry[$name] = 0
        continue
    }

    $currentStatus = if (Spec-HasStatus -Spec $spec -Status 'done') { 'done' } elseif (Spec-HasStatus -Spec $spec -Status 'blocked') { 'blocked' } else { 'in_progress' }
    Write-Log ('RETRY: {0} did not complete cleanly (signal={1}, status={2}).' -f $name, $doneSignal, $currentStatus) 'Yellow'
}

Write-Log 'Ralph loop finished.' 'Cyan'
