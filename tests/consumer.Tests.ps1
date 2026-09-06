# Real Codex CLI probes. Run with PowerShell 7; no Pester dependency.
# Agent verification is opt-in because it performs bounded model requests.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$NativeCodex,
    [switch]$RunAgent,
    [string]$AuthSource,
    [switch]$KeepProbe
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
$repo = Split-Path $PSScriptRoot -Parent
$probe = Join-Path ([IO.Path]::GetTempPath()) ('codex-kit-consumer-' + [guid]::NewGuid().ToString('N'))
$codexHome = Join-Path $probe 'user with spaces/.codex'
$neutral = Join-Path $probe 'neutral'
$project = Join-Path $probe 'project with spaces'
$utf8 = [Text.UTF8Encoding]::new($false)
$checks = [Collections.Generic.List[string]]::new()
$server = $null

function Assert-Consumer([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    $checks.Add($Message)
    Write-Host "PASS: $Message"
}
function Write-Fixture([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, $utf8) }
function Link-Fixture([string]$Path, [string]$Target) {
    $null = New-Item -ItemType SymbolicLink -Path $Path -Target $Target
}
function Invoke-ConsumerCli([string[]]$Arguments, [int]$TimeoutSeconds = 45) {
    $start = [Diagnostics.ProcessStartInfo]::new($NativeCodex)
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $start.WorkingDirectory = $neutral
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.RedirectStandardInput = $true
    $start.Environment['CODEX_HOME'] = $codexHome
    $process = [Diagnostics.Process]::Start($start)
    $process.StandardInput.Close()
    $outTask = $process.StandardOutput.ReadToEndAsync()
    $errorTask = $process.StandardError.ReadToEndAsync()
    try {
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $process.Kill($true)
            throw "Codex consumer probe timed out: $($Arguments[0])"
        }
        $stdout = $outTask.GetAwaiter().GetResult()
        $stderr = $errorTask.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) { throw "Codex consumer probe exited $($process.ExitCode): $stderr" }
        return $stdout
    } finally { $process.Dispose() }
}
function Remove-ConsumerTree([string]$Path) {
    # Every visited path must stay inside the explicitly created probe root.
    # Reparse points are removed themselves; their targets are never enumerated.
    $full = [IO.Path]::GetFullPath($Path)
    $root = [IO.Path]::GetFullPath($probe)
    if ($full -ne $root -and -not $full.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Cleanup escaped probe boundary: $full"
    }
    $item = Get-Item -LiteralPath $full -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        if ($item.PSIsContainer) { [IO.Directory]::Delete($full, $false) } else { [IO.File]::Delete($full) }
    } elseif ($item.PSIsContainer) {
        foreach ($child in Get-ChildItem -LiteralPath $full -Force) { Remove-ConsumerTree $child.FullName }
        # Native child exit can precede release of its Windows cwd handle.
        # Retry only that transient nonrecursive deletion; keep other failures.
        for ($attempt = 0; ; $attempt++) {
            try { [IO.Directory]::Delete($full, $false); break }
            catch [IO.IOException] {
                if ($attempt -ge 20) { throw }
                Start-Sleep -Milliseconds 100
            }
        }
    } else { [IO.File]::Delete($full) }
}

try {
    foreach ($directory in @($codexHome, "$codexHome/skills", "$codexHome/agents", $neutral, $project, "$project/.codex", "$probe/fixture-skill", "$probe/live-agent")) {
        $null = New-Item -ItemType Directory -Path $directory -Force
    }
    Write-Fixture "$codexHome/config.toml" 'developer_instructions = "HARNESS_LOCAL_BASE_MARKER"'
    Write-Fixture "$codexHome/auth-sentinel" 'unrelated authentication sentinel'
    Write-Fixture "$codexHome/history.jsonl" 'unrelated history sentinel'
    $profileSource = Join-Path $repo 'global/harness.config.toml'
    $profileHash = (Get-FileHash -LiteralPath $profileSource).Hash
    Link-Fixture "$codexHome/harness.config.toml" $profileSource
    Link-Fixture "$codexHome/AGENTS.md" (Join-Path $repo 'global/principles-of-work.md')
    foreach ($skill in Get-ChildItem -LiteralPath (Join-Path $repo '.agents/skills') -Directory) {
        # Windows native resolves the user's home through Known Folders, not a
        # fake USERPROFILE. CODEX_HOME/skills is the native isolated user root.
        Link-Fixture "$codexHome/skills/$($skill.Name)" $skill.FullName
    }
    Write-Fixture "$probe/fixture-skill/SKILL.md" "---`nname: harness-live-consumer-fixture`ndescription: HARNESS_SKILL_V1`n---`nHarmless fixture body."
    Link-Fixture "$codexHome/skills/harness-live-consumer-fixture" "$probe/fixture-skill"
    Write-Fixture "$project/AGENTS.md" 'HARNESS_PROJECT_INSTRUCTIONS_MARKER'
    Write-Fixture "$project/.codex/config.toml" 'developer_instructions = "HARNESS_PROJECT_CONFIG_MARKER"'
    $git = [Diagnostics.ProcessStartInfo]::new('git')
    $git.UseShellExecute=$false; $git.CreateNoWindow=$true; $git.RedirectStandardOutput=$true; $git.RedirectStandardError=$true
    foreach ($arg in @('init','--quiet',$project)) { $git.ArgumentList.Add($arg) }
    $gitProcess=[Diagnostics.Process]::Start($git); $gitProcess.WaitForExit()
    if ($gitProcess.ExitCode -ne 0) { throw $gitProcess.StandardError.ReadToEnd() }
    $gitProcess.Dispose()

    $version = (Invoke-ConsumerCli @('--version')).Trim()
    Write-Host "Consumer: $version; PowerShell $($PSVersionTable.PSVersion); probe $probe"
    $prompt = Invoke-ConsumerCli @('--profile','harness','debug','prompt-input','bounded fixture')
    Assert-Consumer ($prompt.Contains('danger-full-access') -and $prompt.Contains('Approval policy is currently never')) 'linked repository profile supplies Full Access'
    Assert-Consumer ($prompt.Contains('HARNESS_LOCAL_BASE_MARKER')) 'local base configuration coexists with profile'
    Assert-Consumer ($prompt.Contains('Working principles') -and $prompt.Contains('Protect correctness, data integrity')) 'full repository global instructions are in actual initial context'
    Assert-Consumer ($prompt.Contains('openspec-apply-change') -and $prompt.Contains('HARNESS_SKILL_V1')) 'linked managed skills coexist with an unrelated user fixture'

    $null = Invoke-ConsumerCli @('mcp','add','harness-consumer-fixture','--url','http://127.0.0.1:9/mcp')
    Assert-Consumer ((Get-Content -LiteralPath "$codexHome/config.toml" -Raw).Contains('harness-consumer-fixture')) 'native mcp add persists in local base'
    $null = Invoke-ConsumerCli @('mcp','remove','harness-consumer-fixture')

    $server = Start-ConsumerServer $NativeCodex $codexHome $neutral
    $models = Invoke-ConsumerRpc $server 'config/batchWrite' @{edits=@(@{keyPath='model';value='gpt-6-astra';mergeStrategy='upsert'},@{keyPath='model_reasoning_effort';value='high';mergeStrategy='upsert'})}
    Assert-Consumer ([IO.Path]::GetFullPath($models.filePath) -eq [IO.Path]::GetFullPath("$codexHome/config.toml")) 'native runtime model-selection writer targets local base'
    $trust = Invoke-ConsumerRpc $server 'config/value/write' @{keyPath='projects';value=@{$project=@{trust_level='trusted'}};mergeStrategy='upsert'}
    Assert-Consumer ([IO.Path]::GetFullPath($trust.filePath) -eq [IO.Path]::GetFullPath("$codexHome/config.toml")) 'native runtime project-trust writer targets local base'
    $skills = Invoke-ConsumerRpc $server 'skills/list' @{cwds=@($neutral,$repo);forceReload=$true}
    foreach ($entry in $skills.data) {
        $managed = @($entry.skills | Where-Object name -Like 'openspec-*')
        Assert-Consumer ($managed.Count -eq 6 -and @($managed | Group-Object name | Where-Object Count -ne 1).Count -eq 0) "native skills/list reports six unique managed sources in $($entry.cwd)"
        Assert-Consumer (@($managed | Where-Object { -not $_.path.StartsWith($repo,[StringComparison]::OrdinalIgnoreCase) }).Count -eq 0) 'native skill identities resolve to repository source paths'
    }
    Stop-ConsumerServer $server; $server = $null
    $projectPrompt = Invoke-ConsumerCli @('--profile','harness','-C',$project,'debug','prompt-input','bounded project fixture')
    Assert-Consumer ($projectPrompt.Contains('HARNESS_PROJECT_INSTRUCTIONS_MARKER') -and $projectPrompt.Contains('HARNESS_PROJECT_CONFIG_MARKER') -and $projectPrompt.Contains('Working principles')) 'trusted separate project config and instructions compose with global source'
    Assert-Consumer ((Get-FileHash -LiteralPath $profileSource).Hash -eq $profileHash -and (Get-Item -LiteralPath "$codexHome/harness.config.toml").LinkType -eq 'SymbolicLink') 'configuration writers preserve repository profile contents and connection'

    Write-Fixture "$probe/live-profile.toml" "approval_policy = 'never'`nsandbox_mode = 'danger-full-access'`ndeveloper_instructions = 'HARNESS_PROFILE_V1'"
    Link-Fixture "$codexHome/live.config.toml" "$probe/live-profile.toml"
    $first = Invoke-ConsumerCli @('--profile','live','debug','prompt-input')
    Write-Fixture "$probe/live-profile.toml" "approval_policy = 'never'`nsandbox_mode = 'danger-full-access'`ndeveloper_instructions = 'HARNESS_PROFILE_V2'"
    Write-Fixture "$probe/fixture-skill/SKILL.md" "---`nname: harness-live-consumer-fixture`ndescription: HARNESS_SKILL_V2`n---`nHarmless updated fixture body."
    $second = Invoke-ConsumerCli @('--profile','live','debug','prompt-input')
    Assert-Consumer ($first.Contains('HARNESS_PROFILE_V1') -and $second.Contains('HARNESS_PROFILE_V2') -and -not $second.Contains('HARNESS_PROFILE_V1')) 'next real CLI launch reads changed profile source without reinstall'
    Assert-Consumer ($second.Contains('HARNESS_SKILL_V2') -and -not $second.Contains('HARNESS_SKILL_V1')) 'next real CLI launch reads changed skill source without reinstall'
    Write-Fixture "$probe/live-instructions.md" 'HARNESS_INSTRUCTIONS_V1'
    [IO.File]::Delete("$codexHome/AGENTS.md")
    Link-Fixture "$codexHome/AGENTS.md" "$probe/live-instructions.md"
    $instructionsFirst = Invoke-ConsumerCli @('--profile','harness','debug','prompt-input')
    Write-Fixture "$probe/live-instructions.md" 'HARNESS_INSTRUCTIONS_V2'
    $instructionsSecond = Invoke-ConsumerCli @('--profile','harness','debug','prompt-input')
    Assert-Consumer ($instructionsFirst.Contains('HARNESS_INSTRUCTIONS_V1') -and $instructionsSecond.Contains('HARNESS_INSTRUCTIONS_V2') -and -not $instructionsSecond.Contains('HARNESS_INSTRUCTIONS_V1')) 'next real CLI launch reads changed global instructions without reinstall'
    [IO.File]::Delete("$codexHome/AGENTS.md")
    Link-Fixture "$codexHome/AGENTS.md" (Join-Path $repo 'global/principles-of-work.md')
    $override = Invoke-ConsumerCli @('--profile','harness','-c','sandbox_mode="read-only"','debug','prompt-input')
    Assert-Consumer ($override.Contains('`sandbox_mode` is `read-only`')) 'explicit native configuration override wins over shared profile'

    if ($RunAgent) {
        if ([string]::IsNullOrWhiteSpace($AuthSource) -or -not (Test-Path -LiteralPath $AuthSource -PathType Leaf)) { throw '-RunAgent requires an explicit existing -AuthSource auth.json path.' }
        Link-Fixture "$codexHome/auth.json" ([IO.Path]::GetFullPath($AuthSource))
        Link-Fixture "$codexHome/agents/harness-consumer" (Join-Path $PSScriptRoot 'fixtures')
        Write-Fixture "$codexHome/agents/unrelated-consumer.toml" "name = 'unrelated_consumer_fixture'`ndescription = 'Unrelated test-only personal agent'`ndeveloper_instructions = 'Return UNRELATED_AGENT_OK without tools.'"
        $agentPrompt = "Bounded integration test: spawn exactly one custom agent of type harness_consumer_fixture. Its task is to use fixture directory $codexHome/agents/harness-consumer. Wait for its final reply and return only that reply verbatim. You must actually invoke the custom agent. Do not inspect or modify files yourself. No other work."
        $agentResult = Invoke-ConsumerCli @('--profile','harness','exec','--ephemeral','--skip-git-repo-check','--json',$agentPrompt) 120
        Write-Fixture "$probe/repository-agent.jsonl" $agentResult
        Assert-Consumer ($agentResult.Contains('AGENT_V1:RESOURCE_V1') -and $agentResult.Contains('collab_tool_call')) 'actual spawned repository agent consumes resource through namespaced directory link'

        # Separate generated fixtures test live edits without touching checked-in files.
        Write-Fixture "$probe/live-agent/live-consumer.toml" "name = 'live_consumer_fixture'`ndescription = 'Test-only live role'`ndeveloper_instructions = 'Return LIVE_AGENT_V1 without tools.'"
        Link-Fixture "$codexHome/agents/live-consumer" "$probe/live-agent"
        $liveTask='Bounded integration test: spawn exactly one custom agent of type live_consumer_fixture, ask for its marker, wait for its reply, and return only that reply. Do not read or modify files. No other work.'
        $liveFirst=Invoke-ConsumerCli @('--profile','harness','exec','--ephemeral','--skip-git-repo-check','--json',$liveTask) 120
        Write-Fixture "$probe/live-agent/live-consumer.toml" "name = 'live_consumer_fixture'`ndescription = 'Test-only live role'`ndeveloper_instructions = 'Return LIVE_AGENT_V2 without tools.'"
        $liveSecond=Invoke-ConsumerCli @('--profile','harness','exec','--ephemeral','--skip-git-repo-check','--json',$liveTask) 120
        Assert-Consumer ($liveFirst.Contains('LIVE_AGENT_V1') -and $liveSecond.Contains('LIVE_AGENT_V2') -and $liveFirst.Contains('collab_tool_call') -and $liveSecond.Contains('collab_tool_call')) 'new actual spawned agents read edited definition through existing directory link'
        Write-Fixture "$probe/live-agent-v1.jsonl" $liveFirst; Write-Fixture "$probe/live-agent-v2.jsonl" $liveSecond
    }
    Assert-Consumer ((Get-Content -LiteralPath "$codexHome/auth-sentinel" -Raw).Trim() -eq 'unrelated authentication sentinel' -and (Get-Content -LiteralPath "$codexHome/history.jsonl" -Raw).Trim() -eq 'unrelated history sentinel') 'unrelated host state remains intact'
    Write-Host "PASS: $($checks.Count) actual-consumer assertions; agent model requests: $([bool]$RunAgent)."
} finally {
    if ($null -ne $server) { Stop-ConsumerServer $server }
    if ($KeepProbe) { Write-Host "Probe retained for inspection: $probe" }
    elseif (Test-Path -LiteralPath $probe) { Remove-ConsumerTree $probe; Write-Host 'Disposable registrations removed without following source links.' }
}
