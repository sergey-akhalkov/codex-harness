#requires -Version 7.4
# Real native Codex + linked hooks + installed TypeScript LSP. All registration,
# auth links, transcripts and source edits are confined to a disposable directory.
# -RunAgent explicitly enables the model-backed patch/error/correction scenario.
[CmdletBinding()]
param(
    [string]$NativeCodex,
    [string]$InventoryPath,
    [string]$AuthSource,
    [string]$ProbeRoot,
    [ValidateSet('typescript','javascript','rust','powershell','python','pascal','cpp','csharp','json','markdown','toml','xml','cmake','bash','css','html')][string]$Language = 'typescript',
    [string]$LspRegistryPath,
    [ValidateSet('patch','shell','yielded','child','roots','mcp','config','delayed','failed')][string]$Scenario = 'patch',
    [switch]$RequiredMcp,
    [switch]$UseGlobalHome,
    [switch]$ChildDiscoverTools,
    [switch]$ChildReadMcps,
    [switch]$RunAgent,
    [switch]$MarkdownSibling,
    [switch]$KeepProbe
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($MarkdownSibling -and ($Language -ne 'markdown' -or $Scenario -ne 'patch')) { throw '-MarkdownSibling requires the Markdown patch scenario.' }
. (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
. (Join-Path $PSScriptRoot 'native-hook-trust.ps1')
$repository = Split-Path -Parent $PSScriptRoot
$hostHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
if (-not $NativeCodex) {
    $installed = Get-Content -LiteralPath (Join-Path $hostHome 'harness/installation.json') -Raw | ConvertFrom-Json
    $vendor = Join-Path (Split-Path -Parent $installed.codexCommand) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor'
    $NativeCodex = (Get-ChildItem -LiteralPath $vendor -Recurse -Filter codex.exe | Select-Object -First 1).FullName
}
if (-not $InventoryPath) { $InventoryPath = Join-Path $hostHome 'harness/code-tools.json' }
if (-not $AuthSource) { $AuthSource = Join-Path $hostHome 'auth.json' }
$inventory = Get-Content -LiteralPath $InventoryPath -Raw | ConvertFrom-Json
$python = ($inventory.mcp | Where-Object id -EQ 'serena').paths.python
if (-not (Test-Path -LiteralPath $python -PathType Leaf)) { throw 'Existing Serena Python is required.' }
if (-not $ProbeRoot) { $ProbeRoot = Join-Path ([IO.Path]::GetTempPath()) ('lsp-native-' + [guid]::NewGuid().ToString('N')) }
if ($Scenario -in @('mcp','delayed','failed') -and $UseGlobalHome) { throw 'Disposable MCP fixtures are registered only in an isolated home.' }
$ProbeRoot = [IO.Path]::GetFullPath($ProbeRoot)
if (Test-Path -LiteralPath $ProbeRoot) { throw 'Use a fresh disposable probe root; refusing to overwrite existing state.' }
$codexHome = if ($UseGlobalHome) { $hostHome } else { Join-Path $ProbeRoot 'codex home' }
$workspace = Join-Path $ProbeRoot 'workspace with spaces'
$editWorkspace = $workspace
$oldWorkspaceRoots = $env:HARNESS_LSP_WORKSPACE_ROOTS
$server = $null
$utf8 = [Text.UTF8Encoding]::new($false)
$checks = [Collections.Generic.List[string]]::new()
function Assert-Lsp([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    $checks.Add($Message)
    Write-Output "PASS: $Message"
}
function Write-LspFile([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, $utf8) }
function Remove-LspProbe {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Private fixture cleanup checks absolute containment and reparse points; prompting would leave owned probe resources behind.')]
    param([string]$Path)
    $full = [IO.Path]::GetFullPath($Path)
    if ($full -ne $ProbeRoot -and -not $full.StartsWith($ProbeRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Cleanup escaped disposable probe.' }
    $item = Get-Item -LiteralPath $full -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        if ($item.PSIsContainer) { [IO.Directory]::Delete($full, $false) } else { [IO.File]::Delete($full) }
    } elseif ($item.PSIsContainer) {
        foreach ($child in Get-ChildItem -LiteralPath $full -Force) { Remove-LspProbe $child.FullName }
        [IO.Directory]::Delete($full, $false)
    } else { [IO.File]::Delete($full) }
}
try {
    $directories = if ($UseGlobalHome) { @($workspace) } else { @($codexHome, $workspace, (Join-Path $codexHome 'harness/bin')) }
    foreach ($directory in $directories) { $null = New-Item -ItemType Directory -Path $directory -Force }
    Write-Output "Isolated LSP probe: $ProbeRoot"
    Write-LspFile (Join-Path $workspace 'index.ts') "export const value: number = 1;`n"
    Write-LspFile (Join-Path $workspace 'tsconfig.json') '{"compilerOptions":{"strict":true,"noEmit":true},"include":["*.ts"]}'
    if ($Scenario -eq 'shell') {
        Write-LspFile (Join-Path $workspace 'index.ts') "export const value: number = 0;`n"
        & git -C $workspace init --quiet
        if ($LASTEXITCODE -ne 0) { throw 'Disposable Git initialization failed.' }
        & git -C $workspace add index.ts tsconfig.json
        & git -C $workspace -c user.name=LspFixture -c user.email=lsp-fixture@example.invalid commit --quiet -m baseline
        if ($LASTEXITCODE -ne 0) { throw 'Disposable Git baseline commit failed.' }
        Write-LspFile (Join-Path $workspace 'index.ts') "export const value: number = 1;`n"
        Assert-Lsp ((& git -C $workspace diff --name-only) -contains 'index.ts') 'shell scenario starts with an actual pre-existing dirty source'
    }
    if ($Scenario -eq 'roots') {
        $editWorkspace = Join-Path $ProbeRoot 'approved additional root'
        $null = New-Item -ItemType Directory -Path $editWorkspace
        Write-LspFile (Join-Path $editWorkspace 'index.ts') "export const value: number = 1;`n"
        Write-LspFile (Join-Path $editWorkspace 'tsconfig.json') '{"compilerOptions":{"strict":true,"noEmit":true},"include":["*.ts"]}'
        # This fixture exercises the launcher's explicit approved-root contract.
        # Separate launcher tests verify --add-dir parsing and propagation.
        $env:HARNESS_LSP_WORKSPACE_ROOTS = ConvertTo-Json @($editWorkspace) -Compress
    }
    $sourceFile = 'index.ts'
    $expectedGood = 'export const value: number = 2;'
    if ($Scenario -eq 'config') {
        $expectedGood = 'export const value: string = null;'
        Write-LspFile (Join-Path $workspace 'index.ts') ($expectedGood + "`n")
        Write-LspFile (Join-Path $workspace 'tsconfig.json') '{"compilerOptions":{"strict":false,"noEmit":true},"include":["*.ts"]}'
    } elseif ($Scenario -in @('delayed','failed')) {
        $expectedGood = 'export const value: number = "wrong";'
    }
    $languageCase = $null
    if ($Language -ne 'typescript') {
        if ($Scenario -ne 'patch') { throw 'Non-TypeScript cases currently exercise native patch only.' }
        $caseMap = (& $python -B (Join-Path $repository 'tests/lsp-languages.py') $Language --describe | ConvertFrom-Json -AsHashtable)
        if ($LASTEXITCODE -ne 0) { throw 'Could not read language fixture.' }
        $languageCase = $caseMap[$Language]
        if ($MarkdownSibling) {
            $sibling = Join-Path $ProbeRoot 'sibling docs'
            $null = New-Item -ItemType Directory -Path $sibling
            Write-LspFile (Join-Path $sibling 'guide.md') "# Topic`n"
            $languageCase.good = "# Example`n`n[sibling](../sibling%20docs/guide.md#topic)`n"
            $languageCase.bad = "# Example`n`n[sibling](../sibling%20docs/guide.md#absent)`n"
        }
        $sourceFile = $languageCase.file
        $expectedGood = $languageCase.good.Trim()
        foreach ($name in @($languageCase.support.Keys) + @($sourceFile)) {
            $target = Join-Path $workspace $name
            $null = New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force
            $content = if ($name -eq $sourceFile) { $languageCase.good } else { $languageCase.support[$name] }
            Write-LspFile $target $content
        }
        if ($Language -eq 'csharp') {
            $emptyFeed = Join-Path $ProbeRoot 'empty-nuget-feed'
            $null = New-Item -ItemType Directory -Path $emptyFeed
            & dotnet restore (Join-Path $workspace 'Example.csproj') --source $emptyFeed
            if ($LASTEXITCODE -ne 0) { throw 'Disposable C# fixture offline restore failed.' }
        }
    }
    if (-not $UseGlobalHome) {
    Write-LspFile (Join-Path $codexHome 'harness/installation.json') (@{sourceRoot=$repository} | ConvertTo-Json -Compress)
    Write-LspFile (Join-Path $codexHome 'harness/code-tools.json') ($inventory | ConvertTo-Json -Depth 50)
    if ($LspRegistryPath) {
        Write-LspFile (Join-Path $codexHome 'harness/lsp-servers.json') (Get-Content -LiteralPath $LspRegistryPath -Raw)
    } else {
        $generated = & $python -B (Join-Path $repository 'tools/lsp/registry.py') --inventory $InventoryPath
        if ($LASTEXITCODE -ne 0) { throw 'LSP registry generation failed.' }
        Write-LspFile (Join-Path $codexHome 'harness/lsp-servers.json') ($generated -join "`n")
    }
    if ($Scenario -eq 'failed') {
        $registryFile = Join-Path $codexHome 'harness/lsp-servers.json'
        $failedRegistry = Get-Content -LiteralPath $registryFile -Raw | ConvertFrom-Json -AsHashtable
        $failedRegistry.servers.typescript.command = @($python, '-c', 'import sys; sys.exit(37)')
        Write-LspFile $registryFile ($failedRegistry | ConvertTo-Json -Depth 40)
    }
    $null = New-Item -ItemType SymbolicLink -Path (Join-Path $codexHome 'harness/bin/hook.ps1') -Target (Join-Path $repository 'tools/hook.ps1')
    $null = New-Item -ItemType SymbolicLink -Path (Join-Path $codexHome 'hooks.json') -Target (Join-Path $repository 'global/hooks.json')
    if (Test-Path -LiteralPath $AuthSource) { $null = New-Item -ItemType SymbolicLink -Path (Join-Path $codexHome 'auth.json') -Target $AuthSource }
    $workspaceToml = ConvertTo-Json $workspace -Compress
    $pythonToml = ConvertTo-Json $python -Compress
    $entryToml = ConvertTo-Json (Join-Path $repository 'tools/lsp/server.py') -Compress
    $extraLspArgs = ''
    if ($Scenario -eq 'delayed') {
        $realEntryToml = $entryToml
        $entryToml = ConvertTo-Json (Join-Path $PSScriptRoot 'fixtures/lsp/delayed-mcp.py') -Compress
        $markerToml = ConvertTo-Json (Join-Path $ProbeRoot 'delayed-handshake.json') -Compress
        $extraLspArgs = ", $realEntryToml, $markerToml"
    }
    $requiredToml = if ($RequiredMcp) { 'true' } else { 'false' }
    Write-LspFile (Join-Path $codexHome 'config.toml') @"
model = "gpt-6-astra"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
[windows]
sandbox = "unelevated"
[projects.$workspaceToml]
trust_level = "trusted"
[mcp_servers.harness-lsp]
command = $pythonToml
args = ["-B", "-u", $entryToml$extraLspArgs]
startup_timeout_sec = 60
tool_timeout_sec = 30
env_vars = ["CODEX_HOME"]
required = $requiredToml
"@
    if ($Scenario -eq 'mcp') {
        $fixturePath = ConvertTo-Json (Join-Path $PSScriptRoot 'fixtures/lsp/mcp-edit.py') -Compress
        [IO.File]::AppendAllText((Join-Path $codexHome 'config.toml'), @"

[mcp_servers.lsp-edit-fixture]
command = $pythonToml
args = ["-B", "-u", $fixturePath, $workspaceToml]
startup_timeout_sec = 30
required = true
"@, $utf8)
    }
    $trust = Invoke-NativeHookTrust -NativeCodex $NativeCodex -CodexHome $codexHome -Workspace $workspace -TracePath (Join-Path $ProbeRoot 'private-trust-transcript.txt')
    Assert-Lsp ($trust.TrustedCount -ge 7) 'all linked pre/post/completion handlers receive native trust'
    }
    $server = Start-ConsumerServer $NativeCodex $codexHome $workspace
    $listed = Invoke-ConsumerRpc $server 'hooks/list' @{cwds=@($workspace)}
    Assert-Lsp (@($listed.data[0].hooks | Where-Object trustStatus -NE 'trusted').Count -eq 0) 'fresh unprofiled app-server loads trusted live hook sources'
    if ($RunAgent) {
        # Native delegation requires a persisted parent thread in this version.
        $thread = Invoke-ConsumerRpc $server 'thread/start' @{cwd=$workspace;model='gpt-6-astra';ephemeral=($Scenario -ne 'child')}
        $runtime = Join-Path $codexHome 'harness/runtime/lsp'
        $previousReports = @{}
        if ($UseGlobalHome -and (Test-Path -LiteralPath $runtime)) {
            Get-ChildItem -LiteralPath $runtime -Filter 'report-*.json' -Recurse | ForEach-Object { $previousReports[$_.FullName] = $true }
        }
        $prompt = @'
This is a disposable integration fixture. Use only the native apply_patch tool for two edits, and do not run shell commands, manually call diagnostics tools, or write any other file.
First change index.ts from `export const value: number = 1;` to `export const value: number = "wrong";`.
Read the automatic tool feedback from that edit. Then use a second native apply_patch call to correct the assignment to the number 2.
Read the automatic feedback again. In your final reply report the actual diagnostic code received for the first edit and the actual clearance status received for the correction. If no automatic diagnostics arrived, state that fact explicitly. Do not infer or invent a verification result.
'@
        if ($languageCase) {
            $prompt = @"
This is a disposable $Language integration fixture. Perform two successful native apply_patch edits of $sourceFile. Use *** Update File and unified diff hunks, not delete/add operations. If a patch is rejected before editing, correct its syntax and retry that same step. Do not run shell commands, manually call diagnostic tools, or edit any other file.
The current exact contents are:
$($languageCase.good)
First replace its contents with this intentional error:
$($languageCase.bad)
Read the automatic tool feedback. Then use a second native apply_patch call to replace its contents with this correction:
$($languageCase.good)
Read the automatic feedback again. In your final reply report the exact diagnostic message or code actually received for the first edit and the exact clearance status received for the correction. If no automatic diagnostics arrived, state that fact. Do not infer or invent results.
"@
            if ($Language -eq 'rust') {
                $prompt += @'

Rust cold startup may report pending after the first edit. In that case, before correcting the error, you may use up to two read-only native PowerShell commands: Start-Sleep -Seconds 10; Get-Content src/lib.rs. Read their automatic feedback. These commands must not edit files or invoke diagnostic tools. Once actual diagnostics arrive, apply the correction; if still pending after two waits, report the unresolved result honestly.
'@
            }
        }
        if ($Scenario -eq 'shell') {
            $prompt = @'
This is a disposable native integration fixture. Use only two native shell tool calls; do not manually invoke diagnostics, apply_patch or any other tool.
First use PowerShell to write index.ts with exactly `export const value: number = "wrong";` and create untracked.ts with exactly `export const label: string = 123;`, print `PARTIAL_FAILURE_SENTINEL`, then exit with code 7 in that same shell command. The intentional exit 7 is part of this test.
Read the automatic diagnostics for both files from that failed command, then use a second native shell call to overwrite index.ts with exactly `export const value: number = 2;` and untracked.ts with exactly `export const label: string = "correct";`.
Report the actual original exit code, diagnostic code and actual clearance status received through automatic feedback. Do not infer a verification result; explicitly say if diagnostics did not arrive.
'@
        } elseif ($Scenario -eq 'config') {
            $prompt = @'
This is a disposable compiler configuration fixture. Use two native apply_patch calls targeting only tsconfig.json. Leave index.ts unchanged and do not invoke diagnostics manually.
First change compilerOptions.strict from false to true; read the automatic diagnostics for the unchanged index.ts file. Then change strict back to false and read the automatic clearance.
Report the actual diagnostic code and clearance status received automatically. State unavailable if missing; never infer verification.
'@
        } elseif ($Scenario -in @('delayed','failed')) {
            $prompt = @'
This is a disposable first-edit/startup-race fixture. Immediately perform exactly one native apply_patch changing index.ts from export const value: number = 1; to export const value: number = "wrong";. Do not run shell, invoke diagnostics manually, wait intentionally, or correct the error.
Immediately attempt to finish after that single edit. Report the actual automatic diagnostic code or unresolved status you received. Never infer or invent a check; preserve the intentional error for inspection.
'@
        } elseif ($Scenario -eq 'mcp') {
            $sourceFile = 'renamed.ts'
            $prompt = @'
This is a disposable native MCP edit fixture. Use only the two lsp-edit-fixture MCP tools: first rename_with_error, then correct_renamed_source. Discover their metadata if needed. Do not use shell or apply_patch, invoke diagnostics manually, or edit other files.
Read the automatic feedback after rename_with_error, including the deleted original source and new dependent file. Then call correct_renamed_source and read its automatic feedback.
Report both original MCP sentinel values and the actual automatic diagnostic code and clearance status. State unavailable if feedback is absent; never infer a check.
'@
        } elseif ($Scenario -eq 'roots') {
            $targetSource = Join-Path $editWorkspace 'index.ts'
            $prompt = @"
This is a disposable integration fixture with an explicitly approved additional workspace: $editWorkspace.
Use two native apply_patch calls, both targeting the absolute path $targetSource. Do not edit index.ts in the main current directory or any other file; do not invoke diagnostics manually.
First change the exact line export const value: number = 1; to export const value: number = "wrong"; in that additional root. Read the automatic feedback, then correct it to export const value: number = 2; using a second native patch.
Report the actual diagnostic code and clearance status received automatically, including the additional workspace identity. State unavailable if no feedback arrives; never infer a check.
"@
        } elseif ($Scenario -eq 'yielded') {
            $prompt = @'
This is a disposable native integration fixture. Use only native shell tools; do not manually invoke diagnostics or apply_patch.
First run a PowerShell shell command with yield_time_ms=1000. It must print `YIELDED_SENTINEL`, sleep 15 seconds, then write index.ts with exactly `export const value: number = "wrong";`. Poll that yielded command to completion using the native shell continuation tool.
Read the automatic diagnostic feedback. Then use a separate native shell call to write index.ts with exactly `export const value: number = 2;`.
Report the actual diagnostic code and clearance status from automatic feedback. If missing, explicitly say unverified; do not infer them.
'@
        } elseif ($Scenario -eq 'child') {
            $prompt = @'
This is a disposable native subagent integration fixture. Delegate exactly one bounded task to a native subagent: use native apply_patch to change index.ts from `export const value: number = 1;` to `export const value: number = "wrong";`, then use a separate native apply_patch to correct it to number 2. The child must report the actual automatic diagnostics code and actual clearance status received, without manually invoking any diagnostics tools. The parent must not edit any files. Wait for that child and report its actual results. Explicitly say unavailable if native delegation or automatic diagnostics are missing.
'@
            if ($ChildDiscoverTools) {
                $prompt += @'

Before the child edits, ask it to discover available MCP tool metadata using native tool_search if present, or functions.exec with ALL_TOOLS filtered by /harness|serena|codebase|graphify|nuphus/i. The child must report matching tool names only and must not call diagnostics manually. It may run one read-only shell command to print its CODEX_HOME and the names of mcp_servers from that home's config.toml (no credentials or config values). Include its observed discovery results in the final report, then perform the same two patches and wait for automatic feedback.
'@
            }
            if ($ChildReadMcps) {
                $prompt += @'

Before the same child performs the two patches, ask it to call these four actual read operations through its available MCP tools: Serena initial_instructions; Codebase Memory list_projects; Graphify graph_stats; Nuphus browser_evaluate with {"script":"6 * 7","confirm":true} in the harness-owned empty browser. Discover tool metadata if needed. Report each actual server/tool result, including the returned browser value and graph counts, and preserve actual errors if any. Do not use these reads to substitute for automatic diagnostics; after them the child must still perform the two native patches and report their automatic error and clearance feedback without manually calling diagnostics.
'@
            }
        }
        $turn = Invoke-ConsumerRpc $server 'turn/start' @{threadId=$thread.thread.id;input=@(@{type='text';text=$prompt})}
        $events = [Collections.Generic.List[object]]::new()
        $deadline = [DateTime]::UtcNow.AddSeconds($(if($ChildReadMcps){240}else{180}))
        $finished = $false
        while ([DateTime]::UtcNow -lt $deadline) {
            $line = $server.Process.StandardOutput.ReadLineAsync()
            if (-not $line.Wait([Math]::Max(1,[int]($deadline-[DateTime]::UtcNow).TotalMilliseconds))) { throw 'Actual LSP agent probe timed out.' }
            if ($null -eq $line.Result) { throw 'Native consumer exited before completing the LSP probe.' }
            $rpcEvent = $line.Result | ConvertFrom-Json -AsHashtable
            $events.Add($rpcEvent)
            # Preserve this fixture's own protocol evidence on bounded timeout,
            # without reading unrelated session transcripts.
            [IO.File]::AppendAllText((Join-Path $ProbeRoot 'private-agent-events.jsonl'), $line.Result + "`n", $utf8)
            if ($rpcEvent.ContainsKey('method') -and $rpcEvent.method -eq 'turn/completed' -and $rpcEvent.params.turn.id -eq $turn.turn.id) { $finished=$true; break }
        }
        Write-LspFile (Join-Path $ProbeRoot 'private-agent-events.json') ($events | ConvertTo-Json -Depth 70)
        Assert-Lsp $finished 'real Codex patch/error/correction turn finishes'
        $changes = @($events | Where-Object { $_.ContainsKey('method') -and $_.method -eq 'item/completed' -and $_.params.item.type -eq 'fileChange' })
        if ($Scenario -in @('patch','roots','config')) {
            Assert-Lsp ($changes.Count -ge 2 -and @($changes | Where-Object {$_.params.item.status -ne 'completed'}).Count -eq 0) 'two actual native patch results remain completed'
        }
        $shells = @($events | Where-Object { $_.ContainsKey('method') -and $_.method -eq 'item/completed' -and $_.params.item.type -eq 'commandExecution' })
        if ($Scenario -eq 'shell') {
            Assert-Lsp (@($shells | Where-Object {$_.params.item.exitCode -eq 7 -and $_.params.item.aggregatedOutput -match 'PARTIAL_FAILURE_SENTINEL'}).Count -eq 1) 'partial failure preserves its actual exit 7 and stdout'
        }
        $reports = @(Get-ChildItem -LiteralPath $runtime -Filter 'report-*.json' -Recurse |
            Where-Object {-not $previousReports.ContainsKey($_.FullName)} |
            ForEach-Object {Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json} |
            Where-Object {$_.workspace -eq $workspace})
        Write-LspFile (Join-Path $ProbeRoot 'scoped-diagnostics.json') ($reports | ConvertTo-Json -Depth 50)
        $results = @($reports | ForEach-Object {$_.results})
        $errorResults = @($results | Where-Object {$_.file -eq $sourceFile -and $_.status -eq 'diagnostics' -and $_.diagnostics.Count -gt 0})
        if ($Scenario -eq 'failed') {
            Assert-Lsp (@($results | Where-Object {$_.file -eq $sourceFile -and $_.status -in @('unavailable','failed','pending')}).Count -ge 1) 'crashed backend produces an explicit unresolved check'
            Assert-Lsp (@($results | Where-Object {$_.file -eq $sourceFile -and $_.status -eq 'clean'}).Count -eq 0) 'a crashed backend never produces false clearance'
        } else {
            Assert-Lsp ($errorResults.Count -ge 1) "the automatic path reports actual $Language diagnostics"
        }
        if ($Scenario -notin @('delayed','failed')) {
            Assert-Lsp (@($results | Where-Object {$_.file -eq $sourceFile -and $_.status -eq 'clean'}).Count -ge 1) 'the corrected revision receives an authoritative empty diagnostic set'
        }
        $final = @($events | Where-Object { $_.ContainsKey('method') -and $_.method -eq 'item/completed' -and $_.params.item.type -eq 'agentMessage' } | ForEach-Object {$_.params.item.text}) -join "`n"
        if ($ChildReadMcps) {
            $calls = @($events | Where-Object {$_['method'] -eq 'item/completed' -and $_['params']['item']['type'] -eq 'mcpToolCall'})
            $expectedCalls = @{serena='initial_instructions';'codebase-memory'='list_projects';graphify='graph_stats';nuphus='browser_evaluate'}
            foreach ($name in $expectedCalls.Keys) {
                $completedCalls = @($calls | Where-Object {$_['params']['item']['server'] -eq $name -and $_['params']['item']['tool'] -eq $expectedCalls[$name] -and $_['params']['item']['status'] -eq 'completed' -and $null -eq $_['params']['item']['error'] -and $_['params']['threadId'] -ne $thread.thread.id})
                Assert-Lsp ($completedCalls.Count -ge 1) "child performs actual $name MCP read through the native consumer"
            }
            $browserCalls = @($calls | Where-Object {$_['params']['item']['server'] -eq 'nuphus' -and $_['params']['item']['tool'] -eq 'browser_evaluate'})
            Assert-Lsp (@($browserCalls | ForEach-Object {$_['params']['item']['result']['content']} | Where-Object {$_.text -eq '42'}).Count -ge 1) 'actual child browser read returns 42'
        }
        $receivedError = $false
        foreach ($diagnostic in $(if($errorResults.Count){$errorResults[0].diagnostics}else{@()})) {
            if (($diagnostic.PSObject.Properties.Name -contains 'code' -and "$($diagnostic.code)" -and $final.Contains("$($diagnostic.code)")) -or
                ($diagnostic.message -and $final.Contains(($diagnostic.message -split "`n")[0].Trim()))) { $receivedError = $true }
        }
        if ($Scenario -eq 'failed') {
            Assert-Lsp ($final -match 'failed|unavailable|unresolved|unverified') 'the agent receives the actual backend failure before completion'
        } else {
            Assert-Lsp ($receivedError -and ($Scenario -eq 'delayed' -or $final -match 'clean')) 'the agent receives the required actual diagnostic feedback'
        }
        Assert-Lsp ((Get-Content -LiteralPath (Join-Path $editWorkspace $sourceFile) -Raw).Trim() -eq $expectedGood) 'only the intended corrected source remains'
        if ($MarkdownSibling) {
            $stops = @($events | Where-Object { $_['method'] -eq 'hook/completed' -and $_['params']['run']['eventName'] -eq 'stop' })
            Assert-Lsp ($stops.Count -eq 2 -and @($stops | Where-Object { $_['params']['run']['status'] -ne 'completed' }).Count -eq 0) 'native and command Stop handlers complete once without a continuation'
            Assert-Lsp (@($reports | Where-Object status -EQ 'unresolved').Count -eq 0) 'sibling Markdown links never produce a root-access failure'
        }
        if ($Scenario -eq 'roots') {
            Assert-Lsp ((Get-Content -LiteralPath (Join-Path $workspace 'index.ts') -Raw).Trim() -eq 'export const value: number = 1;') 'same-named source in the primary root remains unchanged'
            Assert-Lsp (@($errorResults | Where-Object {$_.workspace -eq $editWorkspace}).Count -ge 1) 'automatic result retains the approved additional root identity'
        }
        if ($Scenario -eq 'mcp') {
            Assert-Lsp ($final -match 'MCP_RENAME_SENTINEL' -and $final -match 'MCP_CORRECTION_SENTINEL') 'original MCP edit results remain model-visible'
            Assert-Lsp (-not (Test-Path -LiteralPath (Join-Path $workspace 'index.ts'))) 'MCP rename deletes the original file identity'
            Assert-Lsp (@($results | Where-Object {$_.file -eq 'index.ts' -and $_.status -eq 'deleted'}).Count -ge 1) 'automatic reconciliation clears the deleted source identity'
            Assert-Lsp (@($results | Where-Object {$_.file -eq 'consumer.ts' -and $_.status -eq 'clean'}).Count -ge 1) 'new untracked dependent source is checked automatically'
        }
        if ($Scenario -eq 'shell') {
            Assert-Lsp (@($results | Where-Object {$_.file -eq 'untracked.ts' -and $_.status -eq 'diagnostics'}).Count -ge 1) 'partial shell failure checks the newly created untracked file'
            Assert-Lsp (@($results | Where-Object {$_.file -eq 'untracked.ts' -and $_.status -eq 'clean'}).Count -ge 1) 'the second shell edit clears the untracked file diagnostics'
        }
        if ($Scenario -eq 'delayed') {
            $marker = Get-Content -LiteralPath (Join-Path $ProbeRoot 'delayed-handshake.json') -Raw | ConvertFrom-Json
            $firstPatch = @($events | Where-Object {$_['method'] -eq 'item/completed' -and $_['params']['item']['type'] -eq 'fileChange'})[0]
            Assert-Lsp ($firstPatch['params']['completedAtMs'] / 1000 -lt $marker.started + $marker.delay_seconds) 'the first real patch completes before the deliberately delayed MCP handshake'
            Assert-Lsp (@($events | Where-Object {$_['method'] -eq 'hook/completed' -and $_['params']['run']['handlerType'] -eq 'command' -and $_['params']['run']['eventName'] -eq 'postToolUse' -and $_['params']['run']['status'] -eq 'completed'}).Count -ge 1) 'independent command fallback delivers the preserved first edit before immediate completion'
        }
    }
    $report = @{nativeVersion=(& $NativeCodex --version);checks=$checks;modelProbe=[bool]$RunAgent;probe=$ProbeRoot;source='global/hooks.json + tools/lsp/server.py';scope=$Scenario;language=$Language;globalHome=[bool]$UseGlobalHome}
    Write-LspFile (Join-Path $ProbeRoot 'report.json') ($report | ConvertTo-Json -Depth 10)
    Write-Output ($report | ConvertTo-Json -Depth 10)
} finally {
    $env:HARNESS_LSP_WORKSPACE_ROOTS = $oldWorkspaceRoots
    if ($server) { Stop-ConsumerServer $server }
    if (Test-Path -LiteralPath $ProbeRoot) {
        if ($KeepProbe) { Write-Output "Retained private LSP probe: $ProbeRoot" }
        else { Remove-LspProbe $ProbeRoot }
    }
}
