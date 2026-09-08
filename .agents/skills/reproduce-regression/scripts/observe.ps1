#requires -Version 7.4
param([Parameter(Mandatory)][string]$Supervisor, [Parameter(Mandatory)][string]$CaseRoot,
    [int]$ReadySeconds = 0, [long]$OutputLimit = 16777216)
$ErrorActionPreference = 'Stop'
$observation = @{ ready = $false; status = 'running'; native = $null; error = $null }
$clock = [Diagnostics.Stopwatch]::new()
$observer = {
    param([uint32]$ChildId)
    if (-not $clock.IsRunning) { $clock.Start() }
    $ready = Join-Path $CaseRoot 'ready.txt'
    if ((Test-Path -LiteralPath $ready) -and [IO.File]::ReadAllText($ready).Trim() -ceq 'READY') {
        $observation.ready = $true
    }
    foreach ($name in @('stdout.txt', 'stderr.txt')) {
        $file = Get-Item -LiteralPath (Join-Path $CaseRoot $name) -ErrorAction SilentlyContinue
        if ($file -and $file.Length -gt $OutputLimit) { $observation.status = 'output-limit'; throw 'Observed output limit exceeded' }
    }
    if ($ReadySeconds -gt 0 -and -not $observation.ready -and $clock.Elapsed.TotalSeconds -ge $ReadySeconds) {
        $observation.status = 'readiness-timeout'; throw 'Readiness deadline exceeded'
    }
}.GetNewClosure()
try {
    $observation.native = & $Supervisor -RequestPath (Join-Path $CaseRoot 'request.json') -PassThru -OnRunning $observer
    $observation.status = $observation.native.Status
    # A short-lived process can exit between observer ticks. Check its receipt too.
    $ready = Join-Path $CaseRoot 'ready.txt'
    if ((Test-Path -LiteralPath $ready) -and [IO.File]::ReadAllText($ready).Trim() -ceq 'READY') { $observation.ready = $true }
    if ($ReadySeconds -gt 0 -and -not $observation.ready -and $observation.status -eq 'exited') { $observation.status = 'readiness-failure' }
} catch {
    if ($observation.status -eq 'running') { $observation.status = 'infrastructure-failure' }
    $observation.error = $_.Exception.Message
} finally {
    # The supervisor closes its owned Job handle before returning/throwing; that
    # reaps descendants without looking up or killing any process by name/PID.
    $observation | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $CaseRoot 'observed.json') -Encoding utf8
}
