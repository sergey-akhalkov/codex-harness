#requires -Version 7.4
<# Isolated host-entry recovery checks. Copies the production entry unchanged
beside a fake runtime module in a unique private fixture. Fast checks record sleep
requests; opt-in native cases use real one-minute waits. No scheduler, model,
network, credentials or global configuration mutations. Owned child execution is
bounded by the existing Windows Job runner; fixture evidence is retained. #>
[CmdletBinding()]
param([ValidateSet('None','Recovery','Exhaustion')][string]$NativeCase = 'None')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'The host recovery checks require Windows.' }
$repository = Split-Path $PSScriptRoot
$powershell = Join-Path $env:ProgramFiles 'PowerShell/7/pwsh.exe'
if (-not (Test-Path -LiteralPath $powershell -PathType Leaf)) { throw 'Ordinary Program Files PowerShell 7 is required.' }
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('codex-host-recovery-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixture)
$report = [ordered]@{status='running';fixture=$fixture;executable=$powershell;nativeCase=$NativeCase;hostSha256=(Get-FileHash -LiteralPath (Join-Path $repository 'tools/opencodex-service.ps1')).Hash;cases=@()}
Write-Output "Host recovery evidence: $fixture"

function Assert-HostRecovery([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Invoke-HostRecoveryCase([string]$Name, [string[]]$Outcomes, [int]$ExpectedAttempts, [bool]$Success, [bool]$RealWait = $false) {
    $caseRoot = Join-Path $fixture $Name
    $stateDirectory = Join-Path $caseRoot 'harness/subscriptions'
    [void][IO.Directory]::CreateDirectory($stateDirectory)
    $state = Join-Path $stateDirectory 'service.json'
    [IO.File]::WriteAllText($state, '{}')
    $hostScript = Join-Path $caseRoot 'opencodex-service.ps1'
    Copy-Item -LiteralPath (Join-Path $repository 'tools/opencodex-service.ps1') -Destination $hostScript
    [IO.File]::WriteAllText((Join-Path $caseRoot 'outcomes.json'), (ConvertTo-Json -InputObject $Outcomes))
    $fakeModule = @'
Set-StrictMode -Version Latest
$script:attempt = 0
function Invoke-SubscriptionServiceHost {
    param([string]$StatePath)
    $script:attempt++
    $outcomes = @(Get-Content -LiteralPath (Join-Path $PSScriptRoot 'outcomes.json') -Raw | ConvertFrom-Json)
    $outcome = $outcomes[[Math]::Min($script:attempt - 1, $outcomes.Count - 1)]
    [IO.File]::AppendAllText((Join-Path $PSScriptRoot 'attempts.jsonl'), ((@{attempt=$script:attempt;time=[DateTime]::UtcNow.ToString('o');outcome=$outcome;pid=$PID} | ConvertTo-Json -Compress) + [Environment]::NewLine))
    if ($outcome -eq 'success') { return }
    $failure = [InvalidOperationException]::new('fixture-secret-must-not-reach-logs')
    if ($outcome -eq 'runtime') { $failure.Data['SubscriptionRuntimeRetryable'] = $true }
    if ($outcome -eq 'false-marker') { $failure.Data['SubscriptionRuntimeRetryable'] = $false }
    if ($outcome -eq 'string-marker') { $failure.Data['SubscriptionRuntimeRetryable'] = 'true' }
    throw $failure
}
Export-ModuleMember -Function Invoke-SubscriptionServiceHost
'@
    if (-not $RealWait) {
        $fakeModule += @'

function Start-Sleep {
    param([int]$Seconds)
    [IO.File]::AppendAllText((Join-Path $PSScriptRoot 'sleeps.jsonl'), ((@{seconds=$Seconds;time=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json -Compress) + [Environment]::NewLine))
}
Export-ModuleMember -Function Invoke-SubscriptionServiceHost,Start-Sleep
'@
    }
    [IO.File]::WriteAllText((Join-Path $caseRoot 'subscription-routing.psm1'), $fakeModule)
    $timeout = if ($RealWait) { if ($Name -eq 'native-exhaustion') { 270 } else { 120 } } else { 30 }
    $request = @{executable=$powershell;arguments=@('-NoLogo','-NoProfile','-File',$hostScript,'-StatePath',$state);workingDirectory=$caseRoot;stdoutPath=(Join-Path $caseRoot 'stdout.log');stderrPath=(Join-Path $caseRoot 'stderr.log');environment=@{};memoryLimitMiB=512;timeoutSeconds=$timeout}
    $requestPath = Join-Path $caseRoot 'request.json'
    [IO.File]::WriteAllText($requestPath, ($request | ConvertTo-Json -Depth 6))
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $native = & (Join-Path $repository 'tools/opencodex-process.ps1') -RequestPath $requestPath -ResultPath (Join-Path $caseRoot 'result.json') -PassThru
    $attempts = @(Get-Content -LiteralPath (Join-Path $caseRoot 'attempts.jsonl') | ForEach-Object { $_ | ConvertFrom-Json })
    $sleeps = if (Test-Path -LiteralPath (Join-Path $caseRoot 'sleeps.jsonl')) { @(Get-Content -LiteralPath (Join-Path $caseRoot 'sleeps.jsonl') | ForEach-Object { $_ | ConvertFrom-Json }) } else { @() }
    $entries = @(Get-ChildItem -LiteralPath (Join-Path $stateDirectory 'runs') -Filter '*.host.jsonl')
    $logs = @(Get-Content -LiteralPath $entries[0].FullName | ForEach-Object { $_ | ConvertFrom-Json })
    $caseReport = [ordered]@{name=$Name;status='asserting';native=$native;elapsedSeconds=$clock.Elapsed.TotalSeconds;attempts=$attempts;sleeps=@($sleeps);hostLog=$entries[0].FullName}
    $report.cases += $caseReport
    Assert-HostRecovery ($native.Status -eq 'exited') "$Name did not naturally exit: $($native.Status)."
    Assert-HostRecovery (($native.ExitCode -eq 0) -eq $Success) "$Name returned unexpected exit code $($native.ExitCode)."
    Assert-HostRecovery ($attempts.Count -eq $ExpectedAttempts) "$Name expected $ExpectedAttempts attempts, observed $($attempts.Count)."
    Assert-HostRecovery (@($attempts.pid | Select-Object -Unique).Count -eq 1) "$Name changed host process between retries."
    $retryCount = $ExpectedAttempts - 1
    if (-not $RealWait) {
        Assert-HostRecovery (@($sleeps).Count -eq $retryCount) "$Name requested the wrong number of waits."
        foreach ($sleep in $sleeps) { Assert-HostRecovery ($sleep.seconds -eq 60) "$Name must request exactly 60 seconds per retry." }
    } else {
        for ($index = 1; $index -lt $attempts.Count; $index++) {
            $gap = ([DateTimeOffset]::Parse($attempts[$index].time) - [DateTimeOffset]::Parse($attempts[$index - 1].time)).TotalSeconds
            Assert-HostRecovery ($gap -ge 59.5) "$Name retried before the real one-minute delay."
        }
    }
    $attemptLogs = @($logs | Where-Object stage -eq 'service-attempt')
    $retryLogs = @($logs | Where-Object stage -eq 'service-retry')
    Assert-HostRecovery ($attemptLogs.Count -eq $ExpectedAttempts -and $retryLogs.Count -eq $retryCount) "$Name has incorrect structured attempt/retry counts."
    for ($index = 0; $index -lt $ExpectedAttempts; $index++) {
        Assert-HostRecovery ($attemptLogs[$index].attempt -eq ($index + 1) -and $attemptLogs[$index].retryCount -eq $index -and $attemptLogs[$index].maxRetries -eq 3) "$Name has incorrect structured budget fields."
    }
    foreach ($retry in $retryLogs) { Assert-HostRecovery ($retry.retryDelaySeconds -eq 60) "$Name logged an incorrect retry interval." }
    $expectedExhausted = if ($Name -in @('exhaustion','native-exhaustion')) { 1 } else { 0 }
    Assert-HostRecovery (@($logs | Where-Object stage -eq 'service-exhausted').Count -eq $expectedExhausted) "$Name has an incorrect exhaustion stage."
    Assert-HostRecovery (@($logs | Where-Object stage -eq 'completed').Count -eq [int]$Success) "$Name has an incorrect completion stage."
    $allOutput = (Get-Content -LiteralPath $entries[0].FullName,(Join-Path $caseRoot 'stdout.log'),(Join-Path $caseRoot 'stderr.log') -Raw) -join ''
    Assert-HostRecovery (-not $allOutput.Contains('fixture-secret-must-not-reach-logs')) "$Name exposed raw exception text."
    $caseReport.status = 'passed'
    Write-Output "PASS: $Name ($ExpectedAttempts attempts, $retryCount retries)"
}
try {
    if ($NativeCase -eq 'Recovery') { Invoke-HostRecoveryCase 'native-recovery' @('runtime','success') 2 $true $true }
    elseif ($NativeCase -eq 'Exhaustion') { Invoke-HostRecoveryCase 'native-exhaustion' @('runtime') 4 $false $true }
    else {
        Invoke-HostRecoveryCase 'recovery' @('runtime','success') 2 $true
        Invoke-HostRecoveryCase 'success' @('success') 1 $true
        Invoke-HostRecoveryCase 'last-retry-success' @('runtime','runtime','runtime','success') 4 $true
        Invoke-HostRecoveryCase 'exhaustion' @('runtime') 4 $false
        Invoke-HostRecoveryCase 'foreign' @('foreign') 1 $false
        Invoke-HostRecoveryCase 'false-marker' @('false-marker') 1 $false
        Invoke-HostRecoveryCase 'string-marker' @('string-marker') 1 $false
        Invoke-HostRecoveryCase 'foreign-after-runtime' @('runtime','foreign') 2 $false
    }
    $report.status = 'passed'
} catch {
    $report.status = 'failed'
    $report.failure = $_.Exception.Message
    throw
} finally {
    [IO.File]::WriteAllText((Join-Path $fixture 'report.json'), ($report | ConvertTo-Json -Depth 12))
}
