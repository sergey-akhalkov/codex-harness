#requires -Version 7.4
<#
Opt-in native CLI contracts. Writes only a fresh host-temp evidence root and its
owned homes/workspaces. Reuses live ChatGPT auth in a PRIVATE COPY and the exact
existing OpenCodex route; no service, installer, global config, or model switch.
Worker is bounded by the existing Windows Job observer (2048 MiB / 540 seconds).
ConPTY performs native hook review; RPC is used only for read-only inspection.
Every attempt, including failures, is retained. No automatic retry.
#>
[CmdletBinding()]
param(
    [switch]$RunModelProbes,
    [switch]$NoModelSetup,
    [ValidateSet('skills','pilot')][string]$Scenario = 'skills',
    [ValidateSet('tui','exec')][string]$Transport = 'tui',
    [string]$EvidenceRoot,
    [switch]$Worker,
    [ValidateRange(4000,100000)][int]$AutoCompactTokens = 14000
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot
if (-not $Worker -and -not $RunModelProbes -and -not $NoModelSetup) { 'SKIP: pass -RunModelProbes to authorize the bounded Astra consumer.'; return }
if (-not $IsWindows) { throw 'Windows ConPTY required.' }
$liveHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
if (-not $Worker) {
    if (-not $EvidenceRoot) { $EvidenceRoot = Join-Path $env:TEMP ('native-context-contracts-' + [guid]::NewGuid().ToString('N')) }
    $EvidenceRoot = [IO.Path]::GetFullPath($EvidenceRoot)
    if ((Test-Path -LiteralPath $EvidenceRoot) -or -not $EvidenceRoot.StartsWith([IO.Path]::GetFullPath($env:TEMP), [StringComparison]::OrdinalIgnoreCase)) {
        throw 'EvidenceRoot must be a NEW host-temp directory.'
    }
    [IO.Directory]::CreateDirectory($EvidenceRoot) | Out-Null
    $request = @{executable=(Join-Path $PSHOME 'pwsh.exe');arguments=@('-NoLogo','-NoProfile','-File',$PSCommandPath,'-Worker','-Scenario',$Scenario,'-Transport',$Transport,'-EvidenceRoot',$EvidenceRoot,'-AutoCompactTokens',"$AutoCompactTokens");workingDirectory=$repo;
        stdoutPath=(Join-Path $EvidenceRoot 'worker-stdout.txt');stderrPath=(Join-Path $EvidenceRoot 'worker-stderr.txt');startedPath=(Join-Path $EvidenceRoot 'process-started.json');memoryLimitMiB=2048;timeoutSeconds=540}
    $requestPath = Join-Path $EvidenceRoot 'process-request.json'
    if($NoModelSetup){$request.arguments += '-NoModelSetup'}
    $request | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $requestPath
    Write-Output "Evidence: $EvidenceRoot"
    $result = & (Join-Path $repo 'tools/opencodex-process.ps1') -RequestPath $requestPath -ResultPath (Join-Path $EvidenceRoot 'process-result.json') -PassThru
    $result | ConvertTo-Json -Depth 8
    if ($result.Status -ne 'exited' -or $result.ExitCode -ne 0) { throw "Native probe infrastructure failed; inspect $EvidenceRoot/process-result.json" }
    return
}
$report = [ordered]@{status='running';scenario=$Scenario;transport=$Transport;started=[DateTime]::UtcNow.ToString('o');root=$EvidenceRoot;stages=@();autoCompactTokens=$AutoCompactTokens}
$terminal = $null
$server = $null
$codexHome = Join-Path $EvidenceRoot 'home'
$workspace = Join-Path $EvidenceRoot 'workspace'
$oldHome = $env:CODEX_HOME
$oldPath = $env:PATH
function Save-Report { $report | ConvertTo-Json -Depth 35 | Set-Content -LiteralPath (Join-Path $EvidenceRoot 'report.json') }
function Stage([string]$Name, $Detail) {
    $report.stages += @{name=$Name;at=[DateTime]::UtcNow.ToString('o');detail=$Detail}
    Save-Report
    Write-Output $Name
}
function Read-Rows {
    $files = @(Get-ChildItem -LiteralPath (Join-Path $codexHome 'sessions') -Filter '*.jsonl' -File -Recurse -ErrorAction SilentlyContinue)
    foreach ($f in $files) {
        $stream = [IO.FileStream]::new($f.FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
        $reader = [IO.StreamReader]::new($stream)
        try { $snapshot = $reader.ReadToEnd() } finally { $reader.Dispose() }
        foreach ($line in $snapshot -split '\r?\n' | Where-Object { $_ }) {
            try { $line | ConvertFrom-Json -AsHashtable -Depth 100 } catch { Write-Verbose 'An in-flight rollout line is incomplete; the next poll rereads it.' }
        }
    }
}
function Get-CompletedTurnCount {
    return @(Read-Rows | Where-Object { $_.type -eq 'event_msg' -and $_.payload.type -eq 'task_complete' -and -not [string]::IsNullOrEmpty($_.payload.last_agent_message) }).Count
}
function Wait-ManualCompact([int]$Before, [int]$Seconds=100) {
    $until=[DateTime]::UtcNow.AddSeconds($Seconds)
    while([DateTime]::UtcNow -lt $until) {
        $compact=@(Read-Rows | Where-Object { $_.type -eq 'compacted' -or ($_.type -eq 'event_msg' -and $_.payload.type -eq 'context_compacted') })
        if($compact.Count -gt $Before){return}
        if($terminal.HasExited){throw 'CLI exited before manual compaction.'}
        Start-Sleep -Milliseconds 400
    }
    throw 'No new native compaction record after /compact; terminal text alone is insufficient.'
}
function Wait-Text([string]$Pattern, [int]$Seconds=30) {
    $until = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $until) {
        $plain = $terminal.Transcript -replace '\x1B\[[0-?]*[ -/]*[@-~]', '' -replace '\s', ''
        if ($plain -match $Pattern) { return }
        if ($terminal.HasExited) { throw "CLI exited waiting for $Pattern (exit $($terminal.ExitCode))" }
        Start-Sleep -Milliseconds 200
    }
    throw "Timed out waiting for terminal: $Pattern"
}
function Send-Line([string]$Text) {
    if($Text.Length -gt 100){$terminal.Send("$([char]27)[200~" + $Text + "$([char]27)[201~");Start-Sleep -Milliseconds 800}
    else{$terminal.Send($Text);Start-Sleep -Milliseconds 250}
    $terminal.Send("`r")
}
function Wait-Turn([int]$Before, [int]$Seconds=140) {
    $until = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $until) {
        $rows = @(Read-Rows)
        [IO.File]::WriteAllText((Join-Path $EvidenceRoot 'live-terminal.txt'),$terminal.Transcript)
        # /compact completes a native task with no assistant answer. It must not
        # satisfy the wait for a subsequent model turn or queue another prompt.
        $done = @($rows | Where-Object { $_.type -eq 'event_msg' -and $_.payload.type -eq 'task_complete' -and -not [string]::IsNullOrEmpty($_.payload.last_agent_message) })
        if ($done.Count -gt $Before) { Start-Sleep -Milliseconds 600; return $done[-1].payload }
        if ($terminal.HasExited) { throw "CLI exited before turn completion: $($terminal.ExitCode)" }
        Start-Sleep -Milliseconds 400
    }
    throw 'Turn deadline elapsed; inspect terminal, hook events and native rollout. No retry.'
}
function Start-Terminal([string]$ResumeId='') {
    $env:CODEX_HOME = $codexHome
    $argsText = if ($ResumeId) { ' resume ' + $ResumeId + ' --no-alt-screen' } else { ' --no-alt-screen' }
    $script:terminal = [Harness.Tests.ConPty]::new($native, ('"' + $native + '"' + $argsText), $workspace)
    Stage 'cli-started' @{pid=$terminal.ProcessId;resume=$ResumeId}
    Wait-Text 'gpt-6-astra|Hooksneedreview|Doyoutrust' 35
    Start-Sleep -Milliseconds 1200
    $plain = $terminal.Transcript -replace '\x1B\[[0-?]*[ -/]*[@-~]', '' -replace '\s', ''
    if ($plain -match 'Doyoutrust') { Send-Line ''; Wait-Text 'Hooksneedreview|gpt-6-astralow' 30 }
    $plain = $terminal.Transcript -replace '\x1B\[[0-?]*[ -/]*[@-~]', '' -replace '\s', ''
    if ($plain -match 'Hooksneedreview') {
        Wait-Text '2\.Trustallandcontinue'; $terminal.Send("$([char]27)[B"); Start-Sleep -Milliseconds 200; Send-Line ''
        Start-Sleep -Milliseconds 1000
    }
    Wait-Text 'gpt-6-astralow' 40
    $plain = $terminal.Transcript -replace '\x1B\[[0-?]*[ -/]*[@-~]', '' -replace '\s', ''
    if ($plain -match 'Setupdefaultsandbox|Inputdisableduntilsetup') { throw 'Unexpected sandbox onboarding; do not submit prompt or authorize setup.' }
    Start-Sleep -Milliseconds 3000
    # Native TUI otherwise starts an auxiliary Luna task-title request. Assign a
    # literal name before any model prompt, through the supported CLI command.
    Send-Line '/rename Native context contracts'
    $nameDeadline=[DateTime]::UtcNow.AddSeconds(12)
    $named=$false
    while([DateTime]::UtcNow -lt $nameDeadline){
        $index=Join-Path $codexHome 'session_index.jsonl'
        if(Test-Path -LiteralPath $index){
            $names=@(Get-Content -LiteralPath $index | ForEach-Object { $_ | ConvertFrom-Json })
            if(@($names | Where-Object thread_name -eq 'Native context contracts').Count){$named=$true;break}
        }
        if($terminal.HasExited){throw 'CLI exited while naming the owned session.'}
        Start-Sleep -Milliseconds 250
    }
    if(-not $named){throw 'Owned native thread name was not confirmed; no model prompt may be submitted.'}
    Stage 'native-thread-named' @{name='Native context contracts';pid=$terminal.ProcessId}
}
function Close-Terminal([string]$Name) {
    Send-Line '/quit'
    $natural = $terminal.Wait(20000)
    Stage 'cli-exit' @{name=$Name;natural=$natural;exit=$terminal.ExitCode;pid=$terminal.ProcessId}
    [IO.File]::WriteAllText((Join-Path $EvidenceRoot ($Name + '-terminal.txt')), $terminal.Transcript)
    $terminal.Dispose(); $script:terminal=$null
    if (-not $natural) { throw 'Native CLI did not exit naturally.' }
}
try {
    # Native no-model comparison proved packaged PowerShell fails restricted-token
    # startup. A child-only PATH filter selects the already-installed desktop PS7.
    $removedPathEntries=@($env:PATH -split ';' | Where-Object { $_ -match '(?i)\\WindowsApps(?:\\|$)' })
    $env:PATH=($env:PATH -split ';' | Where-Object { $_ -notin $removedPathEntries }) -join ';'
    $report.shellSelection=@{method='native default after child-only WindowsApps PATH exclusion';removedPathEntries=$removedPathEntries;comparison='native-context-contracts-shell-78981da418b04e3b9064245efa98a4a9'}
    foreach ($d in @($codexHome,$workspace)) { [IO.Directory]::CreateDirectory($d) | Out-Null }
    $meta = Get-Content -LiteralPath (Join-Path $liveHome 'harness/installation.json') -Raw | ConvertFrom-Json
    $native = Join-Path (Split-Path $meta.codexCommand) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe'
    $configText = [IO.File]::ReadAllText((Join-Path $liveHome 'config.toml'))
    $route = [regex]::Match($configText, '(?m)^openai_base_url\s*=\s*"([^"]+)"').Groups[1].Value
    if ($route -ne 'http://127.0.0.1:10100/v1') { throw 'Expected existing subscription route is absent; no fallback.' }
    $auth = Get-Content -LiteralPath (Join-Path $liveHome 'auth.json') -Raw | ConvertFrom-Json
    if ($auth.auth_mode -ne 'chatgpt' -or $auth.OPENAI_API_KEY) { throw 'Existing ChatGPT subscription auth required.' }
    Copy-Item -LiteralPath (Join-Path $liveHome 'auth.json') -Destination (Join-Path $codexHome 'auth.json')
    Copy-Item -LiteralPath (Join-Path $liveHome 'opencodex-catalog.json') -Destination (Join-Path $codexHome 'models.json')
    $report.native = @{path=$native;version=(& $native --version);sha256=(Get-FileHash -LiteralPath $native).Hash}
    $report.route = @{model='gpt-6-astra';provider='openai';base_url=$route;auth_mode='chatgpt';api_key=$false}
    $report.source = @{head=(git -C $repo rev-parse HEAD);scripts=@(foreach($p in @($PSCommandPath,(Join-Path $PSScriptRoot 'native-context-contracts-hook.ps1'),(Join-Path $PSScriptRoot 'ConPty.cs'),(Join-Path $repo 'tools/opencodex-process.ps1'),(Join-Path $repo 'tools/opencodex-process.cs'))) { @{path=$p;sha256=(Get-FileHash -LiteralPath $p).Hash} })}
    $sourceSnapshot=Join-Path $EvidenceRoot 'source'
    [IO.Directory]::CreateDirectory($sourceSnapshot)|Out-Null
    foreach($p in $report.source.scripts){Copy-Item -LiteralPath $p.path -Destination (Join-Path $sourceSnapshot (Split-Path $p.path -Leaf))}
    $pilot = if ($Scenario -eq 'pilot') { 'true' } else { 'false' }
    $hookEnabled = if ($Scenario -eq 'pilot' -and $Transport -eq 'exec') { 'false' } else { 'true' }
    $tomlWorkspace = $workspace.Replace('\','/')
    $tomlModels = (Join-Path $codexHome 'models.json').Replace('\','/')
    @"
model = "gpt-6-astra"
model_provider = "openai"
model_reasoning_effort = "low"
model_catalog_json = "$tomlModels"
openai_base_url = "$route"
approval_policy = "never"
sandbox_mode = "read-only"
model_auto_compact_token_limit = $AutoCompactTokens
model_auto_compact_token_limit_scope = "body_after_prefix"
web_search = "disabled"
[windows]
sandbox = "unelevated"
[features]
hooks = $hookEnabled
apps = false
multi_agent = false
multi_agent_v2 = false
memories = false
[features.context_management]
experimental_mode = $pilot
[projects.'$tomlWorkspace']
trust_level = "trusted"
"@ | Set-Content -LiteralPath (Join-Path $codexHome 'config.toml')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'native-context-contracts-hook.ps1') -Destination (Join-Path $EvidenceRoot 'hook.ps1')
    @{workspace=$workspace} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $EvidenceRoot 'hook-state.json')
    $hooks=@{}
    foreach($name in @('SessionStart','UserPromptSubmit','PostToolUse','PreCompact','PostCompact')) {
        $command = '& "C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo -NoProfile -File "' + (Join-Path $EvidenceRoot 'hook.ps1') + '"'
        $handler=@{type='command';command=$command;timeout=10}
        if($name -notin @('PreCompact','PostCompact')){$handler.additionalContextLimit=1600}
        $hooks[$name]=@(@{hooks=@($handler)})
    }
    @{hooks=$hooks} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $codexHome 'hooks.json')
    $skillToken1='REV1_'+[guid]::NewGuid().ToString('N').Substring(0,12)
    $skillToken2='REV2_'+[guid]::NewGuid().ToString('N').Substring(0,12)
    foreach($v in 1..2) {
        $delta=if($v -eq 1){41}else{73}; $token=if($v -eq 1){$skillToken1}else{$skillToken2}
        "---`nname: contract-adjustment`ndescription: Calculate the contract adjustment for a numeric input in this owned runtime fixture.`n---`nFor adjustment input N, return N plus $delta. Include revision receipt $token. Read no other skill. This is an inspection fixture and authorizes no writes." | Set-Content -LiteralPath (Join-Path $EvidenceRoot "skill-v$v.md")
    }
    '17' | Set-Content -LiteralPath (Join-Path $workspace 'seed.txt')
    '17' | Set-Content -LiteralPath (Join-Path $workspace 'update-seed.txt')
    # Meaningful decision input, not padding: the winning allowed row is known independently.
    $csv=@('id,category,score')
    for($i=1;$i -le 900;$i++){ $category=if($i % 7 -eq 0){'red'}else{'blue'}; $csv += "item-$i,$category,$($i*13 % 10007)" }
    $csv += 'winning-allowed,blue,10008','tempting-forbidden,red,99999'
    $csv | Set-Content -LiteralPath (Join-Path $workspace 'candidates.csv')
    'Now select the highest scoring eligible candidate from the preceding data.' | Set-Content -LiteralPath (Join-Path $workspace 'late-step.txt')
    @{created_answer=58;updated_answer=90;revision1=$skillToken1;revision2=$skillToken2;eligible_winner='winning-allowed';forbidden='tempting-forbidden'} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $EvidenceRoot 'oracle.json')
    Add-Type -Path (Join-Path $PSScriptRoot 'ConPty.cs')
    . (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
    $env:CODEX_HOME=$codexHome
    & $native features list -c "features.context_management.experimental_mode=$pilot" 2>&1 | Set-Content -LiteralPath (Join-Path $EvidenceRoot 'parser-features.txt')
    Stage 'parser' @{exit=$LASTEXITCODE;setting=$pilot}
    $server=Start-ConsumerServer $native $codexHome $workspace
    $effective=Invoke-ConsumerRpc $server 'config/read' @{cwd=$workspace;includeLayers=$true}
    $effective | ConvertTo-Json -Depth 60 | Set-Content -LiteralPath (Join-Path $EvidenceRoot 'effective-config.json')
    $account=Invoke-ConsumerRpc $server 'account/read' @{refreshToken=$false}
    # Remove identifying fields before saving account eligibility evidence.
    if($account.account){$account.account.Remove('email')|Out-Null;$account.account.Remove('accountId')|Out-Null}
    Stage 'eligibility' $account
    Stop-ConsumerServer $server; $server=$null
    if($hookEnabled -eq 'true') {
    Start-Terminal
    Stage 'reviewed-fixture-hooks' @{pid=$terminal.ProcessId}
    $server=Start-ConsumerServer $native $codexHome $workspace
    $trust=Invoke-ConsumerRpc $server 'hooks/list' @{cwds=@($workspace)}
    $trust | ConvertTo-Json -Depth 35 | Set-Content -LiteralPath (Join-Path $EvidenceRoot 'hook-trust.json')
    Stop-ConsumerServer $server; $server=$null
    $registered=@($trust.data | ForEach-Object { $_.hooks })
    if($registered.Count -ne 5 -or @($registered | Where-Object trustStatus -NE 'trusted').Count) { throw 'Every owned hook must be natively trusted before a model prompt.' }
    } else { Stage 'pilot-hooks-disabled' 'Independent task 2.1 runtime probe; no TUI hook delivery claim.' }
    if($NoModelSetup){
        if($terminal){Close-Terminal 'setup'}
        Stage 'no-model-setup-complete' @{}
    } elseif($Transport -eq 'exec') {
        if($terminal){Close-Terminal 'review'}
        $prompt=if($Scenario -eq 'skills') {
            'Bounded read-only runtime fixture. Read seed.txt with one shell call. Use the newly delivered adjustment procedure and remember its result. Then read update-seed.txt in a separate shell call. Read the newly updated procedure before calculating again. Return both adjustment results, both revision receipts and latest context receipts. Do not delegate, browse, write files, inspect hook scripts or evidence, or use other skills.'
        } else {
            'Bounded read-only context continuation probe. EARLY INVARIANT: red candidates are NEVER eligible, regardless of score. Keep this rule for the whole task. First use a shell tool to read ALL candidates.csv (request max_output_tokens 14000); retain the best eligible id. After that call returns, use a SEPARATE shell call to read late-step.txt, and only then give the final eligible candidate id and latest context receipts. Do not combine reads. Do not read hook scripts or evidence, write files, delegate, browse or use other skills.'
        }
        Stage 'exec-prompt' $prompt
        $execRequest=@{executable=$native;arguments=@('exec','--skip-git-repo-check','--json','--output-last-message',(Join-Path $EvidenceRoot 'exec-final.txt'),$prompt);workingDirectory=$workspace;
            environment=@{CODEX_HOME=$codexHome};stdoutPath=(Join-Path $EvidenceRoot 'exec-events.jsonl');stderrPath=(Join-Path $EvidenceRoot 'exec-stderr.txt');startedPath=(Join-Path $EvidenceRoot 'exec-started.json');memoryLimitMiB=2048;timeoutSeconds=210}
        $execPath=Join-Path $EvidenceRoot 'exec-request.json'
        $execRequest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $execPath
        $result=& (Join-Path $repo 'tools/opencodex-process.ps1') -RequestPath $execPath -ResultPath (Join-Path $EvidenceRoot 'exec-process.json') -PassThru
        Stage 'exec-result' $result
        if($result.Status -ne 'exited' -or $result.ExitCode -ne 0){throw 'Native exec failed; preserve its exact error and do not retry the same failure.'}
    } elseif($Scenario -eq 'skills') {
        $prompt='This is a bounded runtime inspection fixture. First read seed.txt with one shell call. Then calculate the contract adjustment of its number, following any freshly delivered adjustment procedure. Do not delegate, browse, write files, inspect hook scripts or evidence, or read other skills. Return the number, revision receipt, and latest context receipts in one final line.'
        Stage 'prompt-create' $prompt; Send-Line $prompt; Stage 'turn-create-complete' (Wait-Turn 0)
        $skillPath=Join-Path $workspace '.agents/skills/contract-adjustment/SKILL.md'
        Copy-Item -LiteralPath (Join-Path $EvidenceRoot 'skill-v2.md') -Destination $skillPath
        Stage 'skill-updated' @{path=$skillPath;sha256=(Get-FileHash -LiteralPath $skillPath).Hash;pid=$terminal.ProcessId}
        $prompt='Calculate the contract adjustment of 17 using the current procedure. Return the number, revision receipt and latest context receipts. Same inspection boundaries apply.'
        Stage 'prompt-update' $prompt; Send-Line $prompt; Stage 'turn-update-complete' (Wait-Turn 1)
        $compactBefore=@(Read-Rows | Where-Object { $_.type -eq 'compacted' -or ($_.type -eq 'event_msg' -and $_.payload.type -eq 'context_compacted') }).Count
        Stage 'manual-compact-request' @{pid=$terminal.ProcessId}; Send-Line '/compact'
        Wait-ManualCompact $compactBefore
        Stage 'manual-compact-observed' @{}
        $before=Get-CompletedTurnCount
        Send-Line $prompt; Stage 'turn-after-manual-complete' (Wait-Turn $before)
        # A separate automatic mid-turn case in the SAME native process/session.
        # The independent oracle must see actual compaction and subsequent output;
        # the configured threshold never establishes that compaction occurred.
        $automaticPrompt='EARLY INVARIANT: red candidates are never eligible regardless of score. Read ALL candidates.csv with max_output_tokens 14000. After that tool returns, read late-step.txt in a separate call. After that call returns, read the current adjustment procedure and calculate the adjustment of 17. Then return Eligible candidate id: followed by the highest scoring eligible id, adjustment answer, revision receipt and latest context receipts. Do not combine reads, write files, delegate, browse, inspect evidence or hooks, or read other skills.'
        $before=Get-CompletedTurnCount
        Stage 'prompt-automatic' @{prompt=$automaticPrompt;pid=$terminal.ProcessId}
        Send-Line $automaticPrompt; Stage 'turn-after-automatic-complete' (Wait-Turn $before 210)
        $rows=@(Read-Rows); $id=@($rows|Where-Object type -eq 'session_meta')[-1].payload.id
        Close-Terminal 'main'
        Start-Terminal $id
        $before=Get-CompletedTurnCount
        Send-Line $prompt; Stage 'turn-resume-complete' (Wait-Turn $before)
        Close-Terminal 'resume'
    } else {
        $prompt='This is a bounded read-only context continuation probe. EARLY INVARIANT: red candidates are NEVER eligible, regardless of score. Keep this rule for the whole task. First use a shell tool to read ALL candidates.csv (request max_output_tokens 14000); retain the best eligible id. After that call returns, use a SEPARATE shell call to read late-step.txt, and only then give your final candidate id and latest context receipts. Do not combine the two reads. Do not read hook scripts or evidence, write files, delegate, browse or use other skills.'
        Stage 'prompt-pilot' $prompt; Send-Line $prompt; Stage 'turn-pilot-complete' (Wait-Turn 0 210)
        Close-Terminal 'pilot'
    }
    $report.status='observed'
} catch {
    $report.status='failed'; $report.error=$_.ToString(); Write-Output $report.error
} finally {
    if($server){Stop-ConsumerServer $server}
    if($terminal){[IO.File]::WriteAllText((Join-Path $EvidenceRoot 'partial-terminal.txt'),$terminal.Transcript);$terminal.Dispose()}
    $env:CODEX_HOME=$oldHome
    $env:PATH=$oldPath
    $ownedAuth=Join-Path $codexHome 'auth.json'
    if(Test-Path -LiteralPath $ownedAuth){Remove-Item -LiteralPath $ownedAuth -Force}
    $report.ended=[DateTime]::UtcNow.ToString('o'); Save-Report
}
if($report.status -eq 'failed'){exit 1}
