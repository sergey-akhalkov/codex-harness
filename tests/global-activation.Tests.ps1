#requires -Version 7.4
# Read-only verification after a real User-PATH installation. No model requests.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$NativeCodex,
    [string]$CodexHome
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
$repository = Split-Path $PSScriptRoot -Parent
$actualUserHome = [Environment]::GetFolderPath('UserProfile')
if (-not $CodexHome) { $CodexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $actualUserHome '.codex' } }
$CodexHome = [IO.Path]::GetFullPath($CodexHome)
$statePath = Join-Path $CodexHome 'harness/installation.json'
if (-not (Test-Path -LiteralPath $statePath -PathType Leaf)) { throw 'Real installation metadata is missing. Install the kit before running this check.' }
$state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json -AsHashtable
$probe = Join-Path ([IO.Path]::GetTempPath()) ('codex-harness-global-check-' + [guid]::NewGuid().ToString('N'))
$neutral = Join-Path $probe 'neutral'
$project = Join-Path $probe 'project with spaces'
$server = $null
$assertions = 0
$snapshots = @{}
$utf8 = [Text.UTF8Encoding]::new($false)

function Same-ActivationPath([string]$Left, [string]$Right) {
    [string]::Equals([IO.Path]::GetFullPath($Left).TrimEnd('\'), [IO.Path]::GetFullPath($Right).TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
}
function Assert-Activation([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    $script:assertions++
    Write-Host "PASS: $Message"
}
function Save-ActivationFingerprint([string]$Path) {
    # Authentication is only hashed; its body and digest are never printed.
    $full = [IO.Path]::GetFullPath($Path)
    if (-not $snapshots.ContainsKey($full)) {
        $snapshots[$full] = if (Test-Path -LiteralPath $full -PathType Leaf) { (Get-FileHash -LiteralPath $full -Algorithm SHA256).Hash } else { $null }
    }
}
function Assert-ActivationFingerprints {
    foreach ($entry in $snapshots.GetEnumerator()) {
        $after = if (Test-Path -LiteralPath $entry.Key -PathType Leaf) { (Get-FileHash -LiteralPath $entry.Key -Algorithm SHA256).Hash } else { $null }
        if ($after -cne $entry.Value) { throw "Read-only activation check observed a changed protected file: $($entry.Key). Inspect concurrent activity; this test never writes that file." }
    }
}
function Invoke-FreshOrdinaryCodex([string]$Directory, [string[]]$Arguments) {
    # Payload is data, never interpolated into a shell command. The child resolves
    # the ordinary command using a fresh machine+user PATH, with no profile flag.
    $payloadPath = Join-Path $probe 'child-arguments.json'
    [IO.File]::WriteAllText($payloadPath, (ConvertTo-Json -InputObject @{arguments=$Arguments} -Depth 8 -Compress), $utf8)
    $start = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
    foreach ($argument in @('-NoProfile','-File',(Join-Path $probe 'invoke-ordinary.ps1'),$payloadPath)) { $start.ArgumentList.Add($argument) }
    $start.WorkingDirectory = $Directory
    $start.UseShellExecute=$false; $start.CreateNoWindow=$true
    $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true; $start.RedirectStandardInput=$true
    $start.StandardOutputEncoding=$utf8; $start.StandardErrorEncoding=$utf8
    $start.Environment['CODEX_HOME']=$CodexHome
    $start.Environment['Path']=[Environment]::ExpandEnvironmentVariables([Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User'))
    $process=[Diagnostics.Process]::Start($start)
    $process.StandardInput.Close()
    $outTask=$process.StandardOutput.ReadToEndAsync(); $errorTask=$process.StandardError.ReadToEndAsync()
    try {
        if (-not $process.WaitForExit(45000)) { $process.Kill($true); throw 'Fresh ordinary Codex invocation timed out.' }
        $stdout=$outTask.GetAwaiter().GetResult(); $stderr=$errorTask.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) { throw "Fresh ordinary Codex invocation failed with exit $($process.ExitCode): $stderr" }
        $result=$stdout | ConvertFrom-Json -AsHashtable
        Assert-Activation (Same-ActivationPath $result.entry (Join-Path $CodexHome 'harness/bin/codex.ps1')) 'fresh PowerShell resolves ordinary codex to the installed linked entry point'
        return $result
    } finally { $process.Dispose() }
}
function Remove-ActivationFixture([string]$Path) {
    $full=[IO.Path]::GetFullPath($Path); $root=[IO.Path]::GetFullPath($probe)
    if ($full -ne $root -and -not $full.StartsWith($root+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture cleanup escaped its explicitly created root.' }
    $item=Get-Item -LiteralPath $full -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        if ($item.PSIsContainer) { [IO.Directory]::Delete($full,$false) } else { [IO.File]::Delete($full) }
    } elseif ($item.PSIsContainer) {
        foreach ($child in Get-ChildItem -LiteralPath $full -Force) { Remove-ActivationFixture $child.FullName }
        [IO.Directory]::Delete($full,$false)
    } else { [IO.File]::Delete($full) }
}

try {
    Assert-Activation ($state.schemaVersion -eq 1 -and $state.pathScope -eq 'User') 'metadata describes a real persistent User-PATH installation'
    Assert-Activation ((Same-ActivationPath $state.sourceRoot $repository) -and (Same-ActivationPath $state.userHome $actualUserHome) -and (Same-ActivationPath $state.codexHome $CodexHome)) 'installation sources and user directories identify this checkout and real Windows account'
    foreach ($path in @($statePath,(Join-Path $CodexHome 'config.toml'),(Join-Path $CodexHome 'auth.json'),(Join-Path $CodexHome 'history.jsonl'))) { Save-ActivationFingerprint $path }
    foreach ($registration in $state.links) {
        $item=Get-Item -LiteralPath $registration.destination -Force
        Assert-Activation ($item.LinkType -eq 'SymbolicLink' -and (Same-ActivationPath $item.ResolveLinkTarget($true).FullName $registration.source)) "live registration resolves to its recorded $($registration.kind) source"
        if (Test-Path -LiteralPath $registration.source -PathType Leaf) { Save-ActivationFingerprint $registration.source }
        else { foreach ($sourceFile in Get-ChildItem -LiteralPath $registration.source -Recurse -File) { Save-ActivationFingerprint $sourceFile.FullName } }
    }
    foreach ($toolFile in Get-ChildItem -LiteralPath (Join-Path $repository 'tools') -File) { Save-ActivationFingerprint $toolFile.FullName }
    $skills=@($state.links | Where-Object kind -eq 'skill')
    Assert-Activation ($skills.Count -eq 6) 'installation records all six managed OpenSpec skills'
    foreach ($skill in $skills) {
        Assert-Activation (Same-ActivationPath (Split-Path $skill.destination -Parent) (Join-Path $actualUserHome '.agents/skills')) 'managed skill uses the real Windows user discovery directory'
    }
    foreach ($directory in @($neutral,$project,(Join-Path $project '.codex'))) { $null=New-Item -ItemType Directory -Path $directory -Force }
    [IO.File]::WriteAllText((Join-Path $project 'AGENTS.md'),'HARNESS_REAL_PROJECT_INSTRUCTIONS',$utf8)
    [IO.File]::WriteAllText((Join-Path $project '.codex/config.toml'),'developer_instructions = "HARNESS_REAL_PROJECT_CONFIG"',$utf8)
    $childScript=@'
param([string]$PayloadPath)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$payload=Get-Content -LiteralPath $PayloadPath -Raw | ConvertFrom-Json
$arguments=@($payload.arguments)
$entry=(Get-Command codex -ErrorAction Stop).Source
$output=@(& codex @arguments)
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
@{entry=$entry;output=($output -join "`n")} | ConvertTo-Json -Depth 5 -Compress
'@
    [IO.File]::WriteAllText((Join-Path $probe 'invoke-ordinary.ps1'),$childScript,$utf8)
    $gitStart=[Diagnostics.ProcessStartInfo]::new((Get-Command git -CommandType Application).Source)
    foreach ($argument in @('init','--quiet',$project)) { $gitStart.ArgumentList.Add($argument) }
    $gitStart.UseShellExecute=$false; $gitStart.CreateNoWindow=$true; $gitStart.RedirectStandardError=$true
    $gitProcess=[Diagnostics.Process]::Start($gitStart)
    try { if (-not $gitProcess.WaitForExit(10000)) { $gitProcess.Kill($true); throw 'Temporary git init timed out.' }; if ($gitProcess.ExitCode -ne 0) { throw $gitProcess.StandardError.ReadToEnd() } } finally { $gitProcess.Dispose() }
    $version=Invoke-FreshOrdinaryCodex $neutral @('--version')
    Write-Host "Actual global consumer: $($version.output); PowerShell $($PSVersionTable.PSVersion)"
    $neutralResult=Invoke-FreshOrdinaryCodex $neutral @('debug','prompt-input')
    # Trust exists only in this invocation. No writer or interactive session is run.
    # The CLI's dotted override key parser does not interpret TOML quotes in
    # the key. Put the quoted path inside a TOML value instead; otherwise the
    # project stays untrusted and its .codex/config.toml is silently skipped.
    $trustOverride="projects={ '$project'={trust_level='trusted'} }"
    $projectResult=Invoke-FreshOrdinaryCodex $project @('-c',$trustOverride,'debug','prompt-input')
    $instructionsRegistration=@($state.links | Where-Object kind -eq 'instructions')[0]
    $fullInstructions=(Get-Content -LiteralPath $instructionsRegistration.source -Raw).Replace("`r`n","`n").Trim()
    foreach ($result in @($neutralResult,$projectResult)) {
        $prompt=$result.output | ConvertFrom-Json -AsHashtable
        $text=(@($prompt | ForEach-Object { $_.content } | ForEach-Object { $_.text }) -join "`n").Replace("`r`n","`n")
        Assert-Activation ($text.Contains($fullInstructions)) 'ordinary startup consumes the entire repository global instruction source'
        Assert-Activation ($text.Contains('danger-full-access') -and $text.Contains('Approval policy is currently never')) 'ordinary startup selects Full Access without a manual profile flag'
        foreach ($skill in $skills) { Assert-Activation ($text.Contains($skill.name)) "ordinary startup discovers $($skill.name)" }
    }
    Assert-Activation ($projectResult.output.Contains('HARNESS_REAL_PROJECT_INSTRUCTIONS') -and $projectResult.output.Contains('HARNESS_REAL_PROJECT_CONFIG')) 'separate project instructions and invocation-trusted project config compose with global sources'

    $server=Start-ConsumerServer $NativeCodex $CodexHome $neutral
    $listed=Invoke-ConsumerRpc $server 'skills/list' @{cwds=@($neutral,$project,$repository);forceReload=$true}
    foreach ($entry in $listed.data) {
        $managed=@($entry.skills | Where-Object { $_.name -in $skills.name })
        Assert-Activation ($managed.Count -eq 6 -and @($managed | Group-Object name | Where-Object Count -ne 1).Count -eq 0) "real-user skills/list has six unique managed sources in $($entry.cwd)"
        foreach ($skill in $skills) {
            $found=@($managed | Where-Object name -eq $skill.name)
            Assert-Activation ($found.Count -eq 1 -and (Same-ActivationPath $found[0].path (Join-Path $skill.source 'SKILL.md'))) "real-user skill identity resolves directly to checkout: $($skill.name)"
        }
    }
    # Retain only origin metadata and a fixed whitelist of non-secret scalar
    # preferences for these assertions. Never print or persist config/read data.
    $read=Invoke-ConsumerRpc $server 'config/read' @{includeLayers=$true;cwd=$neutral}
    $baseLayers=@($read.layers | Where-Object { $_.name.type -eq 'user' })
    Assert-Activation ($baseLayers.Count -ge 1) 'native configuration consumer retains the host-local user configuration layer'
    $safeKeys=@('model_provider','model_verbosity','file_opener','check_for_update_on_startup','hide_agent_reasoning','show_raw_agent_reasoning')
    $checkedLocal=0
    foreach ($layer in $baseLayers) {
        foreach ($key in $safeKeys) {
            if ($layer.config.ContainsKey($key) -and $read.config.ContainsKey($key)) {
                Assert-Activation ($read.config[$key] -ceq $layer.config[$key]) "unmanaged local preference remains effective: $key"
                $checkedLocal++
            }
        }
    }
    Write-Host "Non-secret unmanaged local scalar preferences observable on this host: $checkedLocal"
    $read=$null
    Stop-ConsumerServer $server; $server=$null
    Assert-ActivationFingerprints
    Assert-Activation $true 'base configuration, authorization, history, installation metadata and connected sources remained unchanged'
    Write-Host "PASS: $assertions real-global activation assertions; no model requests."
} finally {
    if ($null -ne $server) { Stop-ConsumerServer $server }
    # Also detect accidental changes if a preceding consumer assertion fails.
    try { Assert-ActivationFingerprints } finally {
        if (Test-Path -LiteralPath $probe) { Remove-ActivationFixture $probe; Write-Host 'Own temporary project removed; real-user registrations were untouched.' }
    }
}
