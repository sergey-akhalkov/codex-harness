#requires -Version 7.4
# Only local, bounded Node fixtures; does not run Bun, OpenCodex or any service.
[CmdletBinding()]
param([string]$NodePath = (Get-Command node -CommandType Application -ErrorAction Stop).Source)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'These Job Object integration tests require Windows.' }
$runner = Join-Path (Split-Path $PSScriptRoot) 'tools/opencodex-process.ps1'
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('codex-subscription-process-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixtureRoot)
$fixture = Join-Path $fixtureRoot 'fixture.cjs'
$ownedProcesses = [Collections.Generic.List[Diagnostics.Process]]::new()
$passed = 0
function Assert-True([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function New-Request([string]$Name, [string[]]$Arguments, [int]$Memory = 160, [int]$Timeout = 10, [hashtable]$Environment = @{}) {
    $request = @{
        executable = $NodePath; arguments = @($fixture) + $Arguments; workingDirectory = $fixtureRoot
        stdoutPath = Join-Path $fixtureRoot "$Name.stdout"; stderrPath = Join-Path $fixtureRoot "$Name.stderr"
        memoryLimitMiB = $Memory; timeoutSeconds = $Timeout; environment = $Environment
        startedPath = Join-Path $fixtureRoot "$Name.started.json"
    }
    $path = Join-Path $fixtureRoot "$Name.request.json"
    [IO.File]::WriteAllText($path, ($request | ConvertTo-Json -Depth 5))
    @{ Path = $path; Result = Join-Path $fixtureRoot "$Name.result.json"; Data = $request }
}
function Assert-ProcessStopped([int]$ProcessId) {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($process) {
        try { Assert-True ($process.WaitForExit(3000)) "Fixture process $ProcessId survived job termination." }
        finally { $process.Dispose() }
    }
}
function Wait-PidFile([string]$Path) {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-Path -LiteralPath $Path) {
            $value = [IO.File]::ReadAllText($Path)
            if ($value -match '^\d+$') { return [int]$value }
        }
        Start-Sleep -Milliseconds 50
    }
    throw 'Timed out waiting for a bounded fixture PID.'
}
$fixtureSource = @'
const fs = require('node:fs');
const {spawn} = require('node:child_process');
const [mode, ...args] = process.argv.slice(2);
if (mode === 'normal') {
  console.log(JSON.stringify({argv: args, env: process.env.CODEX_BOUNDED_TEST, zai: process.env.ZAI_API_KEY || null, stdin: fs.readFileSync(0, 'utf8')}));
  console.error('fixture stderr');
  process.exitCode = 7;
} else if (mode === 'linger') {
  fs.writeFileSync(args[0], String(process.pid));
  setInterval(() => {}, 1000);
} else if (mode === 'child') {
  fs.writeFileSync(args[0], String(process.pid));
  const held = [];
  let blocks = 0;
  fs.writeFileSync(args[0] + '.allocated', '0');
  if (args[1] === 'allocate') setInterval(() => {
    // Independent hard ceiling, even if runner containment were defective.
    if (blocks++ < 24) {
      held.push(Buffer.alloc(4 * 1024 * 1024, 0xa5));
      fs.writeFileSync(args[0] + '.allocated', String(held.length * 4 * 1024 * 1024));
    }
  }, 15);
  else setInterval(() => {}, 1000);
} else if (mode === 'parent') {
  fs.writeFileSync(args[0], String(process.pid));
  const held = args[2] === 'allocate' ? Buffer.alloc(40 * 1024 * 1024, 0x5a) : null;
  const child = spawn(process.execPath, [__filename, 'child', args[1], args[2]], {stdio: 'ignore', windowsHide: true});
  child.on('error', () => process.exit(8));
  setInterval(() => { if (held && held[0] !== 0x5a) process.exit(9); }, 1000);
  if (args[2] === 'exit') {
    const poll = setInterval(() => { if (fs.existsSync(args[1])) process.exit(0); }, 20);
  }
} else throw new Error('Unknown fixture mode');
'@
try {
    [IO.File]::WriteAllText($fixture, $fixtureSource)
    $arguments = @('space value', '', 'русский', 'trailing\', 'a"b', 'literal$(value)`value')
    $normal = New-Request normal (@('normal') + $arguments) -Environment @{ CODEX_BOUNDED_TEST = 'private-environment-sentinel' }
    $result = & $runner -RequestPath $normal.Path -ResultPath $normal.Result -PassThru
    Assert-True ($result.Status -eq 'exited' -and $result.ExitCode -eq 7 -and $result.AssignedBeforeResume) 'Normal exit/assignment contract failed.'
    $receipt = Get-Content -LiteralPath $normal.Data.startedPath -Raw | ConvertFrom-Json
    Assert-True ($receipt.processId -eq $result.ProcessId -and $receipt.assignedBeforeResume) 'Started receipt did not identify the contained process.'
    $output = Get-Content -LiteralPath $normal.Data.stdoutPath -Raw | ConvertFrom-Json
    Assert-True (($output.argv | ConvertTo-Json -Compress) -ceq ($arguments | ConvertTo-Json -Compress)) 'Windows argv quoting changed arguments.'
    Assert-True ($output.env -eq 'private-environment-sentinel' -and $output.stdin -eq '') 'Environment or stdin EOF contract failed.'
    Assert-True ((Get-Content -LiteralPath $normal.Data.stderrPath -Raw).Trim() -eq 'fixture stderr') 'Stderr was not separately redirected.'
    Assert-True (-not ([IO.File]::ReadAllText($normal.Result).Contains('private-environment-sentinel'))) 'Result exposed environment values.'
    Assert-True ($result.PeakJobMemoryBytes -gt 0 -and $result.PeakJobMemoryBytes -le $result.MemoryLimitBytes) 'Normal job memory accounting failed.'
    $passed++; Write-Output 'PASS normal exit, argv, private environment, redirected stdio, memory accounting'

    $secretPath = Join-Path $fixtureRoot 'zai-key.txt'
    [IO.File]::WriteAllText($secretPath, 'synthetic-zai-secret-must-not-print')
    $secretRequest = New-Request secret @('normal') -Environment @{ CODEX_BOUNDED_TEST = 'private-environment-sentinel' }
    $secretJson = Get-Content -LiteralPath $secretRequest.Path -Raw | ConvertFrom-Json -AsHashtable
    $secretJson.secretFiles = @{ ZAI_API_KEY = $secretPath }
    [IO.File]::WriteAllText($secretRequest.Path, ($secretJson | ConvertTo-Json -Depth 8))
    $secretResult = & $runner -RequestPath $secretRequest.Path -ResultPath $secretRequest.Result -PassThru
    Assert-True ($secretResult.Status -eq 'exited' -and $secretResult.AssignedBeforeResume) 'Secret injection did not run the fixture.'
    $secretOut = Get-Content -LiteralPath $secretRequest.Data.stdoutPath -Raw
    $secretPayload = $secretOut | ConvertFrom-Json
    Assert-True ($secretPayload.env -eq 'private-environment-sentinel' -and $secretPayload.zai -eq 'synthetic-zai-secret-must-not-print') 'Secret was not injected into child env.'
    Assert-True (-not $secretOut.Contains('zai-key.txt') -or $secretPayload.zai -eq 'synthetic-zai-secret-must-not-print') 'Secret injection stdout contract failed.'
    $requestText = [IO.File]::ReadAllText($secretRequest.Path)
    $resultText = [IO.File]::ReadAllText($secretRequest.Result)
    Assert-True ($requestText.Contains('secretFiles') -and -not $requestText.Contains('synthetic-zai-secret-must-not-print')) 'Secret value leaked into the request JSON.'
    Assert-True (-not $resultText.Contains('synthetic-zai-secret-must-not-print')) 'Secret value leaked into the result JSON.'
    $passed++; Write-Output 'PASS secretFiles injects child env without writing the secret into request JSON'

    $before = [IO.File]::ReadAllText($normal.Data.stdoutPath)
    $rejected = $false
    try { & $runner -RequestPath $normal.Path -PassThru | Out-Null } catch { $rejected = $_.Exception.Message -like '*distinct new file paths*' }
    Assert-True ($rejected -and [IO.File]::ReadAllText($normal.Data.stdoutPath) -ceq $before) 'Existing logs were not protected.'
    $passed++; Write-Output 'PASS repeated launch refuses existing output without overwriting'

    $timeoutPid = Join-Path $fixtureRoot 'timeout.pid'
    $timeout = New-Request timeout @('linger', $timeoutPid) -Timeout 1
    $result = & $runner -RequestPath $timeout.Path -ResultPath $timeout.Result -PassThru
    Assert-True ($result.Status -eq 'timeout' -and $result.ExitCode -eq 124 -and $result.ElapsedMilliseconds -lt 5000) 'Timeout did not terminate promptly.'
    Assert-ProcessStopped $result.ProcessId
    $passed++; Write-Output 'PASS command timeout terminates its process'

    $parentPid = Join-Path $fixtureRoot 'normal-parent.pid'; $childPid = Join-Path $fixtureRoot 'normal-child.pid'
    $family = New-Request family @('parent', $parentPid, $childPid, 'exit')
    $result = & $runner -RequestPath $family.Path -ResultPath $family.Result -PassThru
    Assert-True ($result.Status -eq 'exited' -and $result.ExitCode -eq 0) 'Parent fixture did not exit normally.'
    Assert-ProcessStopped (Wait-PidFile $childPid)
    $passed++; Write-Output 'PASS normal parent exit closes job and terminates lingering descendant'

    $parentPid = Join-Path $fixtureRoot 'observer-parent.pid'; $childPid = Join-Path $fixtureRoot 'observer-child.pid'
    $observed = New-Request observer @('parent', $parentPid, $childPid, 'linger')
    $rejected = $false
    try {
        & $runner -RequestPath $observed.Path -ResultPath $observed.Result -PassThru -OnRunning {
            param($OwnedProcessId)
            if (Test-Path -LiteralPath $childPid) { throw "fixture observer failure ($OwnedProcessId)" }
        } | Out-Null
    } catch { $rejected = $_.Exception.Message -like '*fixture observer failure*' }
    Assert-True $rejected 'Observer did not run synchronously or its original error was hidden.'
    Assert-ProcessStopped (Wait-PidFile $parentPid); Assert-ProcessStopped (Wait-PidFile $childPid)
    $passed++; Write-Output 'PASS trusted observer failure closes job and terminates parent and descendant'

    $parentPid = Join-Path $fixtureRoot 'memory-parent.pid'; $childPid = Join-Path $fixtureRoot 'memory-child.pid'
    $memory = New-Request memory @('parent', $parentPid, $childPid, 'allocate') -Memory 128 -Timeout 8
    $result = & $runner -RequestPath $memory.Path -ResultPath $memory.Result -PassThru
    Assert-True ($result.Status -eq 'memory-limit' -and $result.ExitCode -eq 125) 'Aggregate memory limit did not stop the process group.'
    # Windows can report a peak above JobMemoryLimit when it sends the denied
    # allocation notification. Preserve that measurement; check it is bounded
    # by the one attempted 4 MiB allocation, and that intended allocation never
    # completed, instead of misrepresenting this telemetry as an exact RSS cap.
    Assert-True ($result.PeakJobMemoryBytes -le $result.MemoryLimitBytes + 4MB) 'Job memory peak was not bounded by the configured limit and one attempted allocation.'
    $allocated = [long][IO.File]::ReadAllText($childPid + '.allocated')
    Assert-True ($allocated -lt 96MB) 'Child completed allocations which must exceed the group limit.'
    Assert-ProcessStopped $result.ProcessId
    Assert-ProcessStopped (Wait-PidFile $childPid)
    $passed++; Write-Output ("PASS aggregate parent+child limit; reported peak {0:N1} MiB, configured 128 MiB, child allocated {1:N0}/96 MiB; both terminated" -f ($result.PeakJobMemoryBytes / 1MB), ($allocated / 1MB))

    $parentPid = Join-Path $fixtureRoot 'host-parent.pid'; $childPid = Join-Path $fixtureRoot 'host-child.pid'
    $hostRequest = New-Request host @('parent', $parentPid, $childPid, 'linger') -Timeout 20
    $startInfo = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
    $startInfo.UseShellExecute = $false; $startInfo.CreateNoWindow = $true
    foreach ($argument in @('-NoLogo','-NoProfile','-File',$runner,'-RequestPath',$hostRequest.Path,'-ResultPath',$hostRequest.Result)) { $startInfo.ArgumentList.Add($argument) }
    $runnerProcess = [Diagnostics.Process]::Start($startInfo)
    $ownedProcesses.Add($runnerProcess)
    $rootId = Wait-PidFile $parentPid; $childId = Wait-PidFile $childPid
    foreach ($processId in @($rootId,$childId)) { $ownedProcesses.Add([Diagnostics.Process]::GetProcessById($processId)) }
    # Kill only the exact runner process handle; no process-tree kill API. The
    # kernel must close its noninherited job handle and terminate both Nodes.
    $runnerProcess.Kill(); Assert-True ($runnerProcess.WaitForExit(5000)) 'Runner fixture failed to exit.'
    Assert-ProcessStopped $rootId; Assert-ProcessStopped $childId
    $passed++; Write-Output 'PASS killing runner alone closes its job and terminates parent and descendant'
    Write-Output "$passed subscription process integration checks passed."
}
finally {
    foreach ($process in $ownedProcesses) {
        try { if (-not $process.HasExited) { $process.Kill(); [void]$process.WaitForExit(3000) } } finally { $process.Dispose() }
    }
    $resolvedRoot = [IO.Path]::GetFullPath($fixtureRoot)
    $expectedParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
    if ((Split-Path $resolvedRoot) -ine $expectedParent -or (Split-Path $resolvedRoot -Leaf) -notlike 'codex-subscription-process-*') { throw 'Refusing fixture cleanup outside the explicit temporary workspace.' }
    Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
}

