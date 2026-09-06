#requires -Version 7.4
# Actual full installer and native MCP consumers from a copied/moved source.
# Connections use an owned home; the dependency owner is explicitly the existing
# account. PATH changes affect this test process only. No model request.
[CmdletBinding()]
param([string]$DependencyUserHome = [Environment]::GetFolderPath('UserProfile'), [switch]$KeepProbe)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
$realCodexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $DependencyUserHome '.codex' }
$existing = Get-Content -LiteralPath (Join-Path $realCodexHome 'harness/installation.json') -Raw | ConvertFrom-Json
$inventory = Get-Content -LiteralPath (Join-Path $realCodexHome 'harness/code-tools.json') -Raw | ConvertFrom-Json
$python = ($inventory.mcp | Where-Object id -EQ 'serena').paths.python
$native = @(Get-ChildItem (Join-Path (Split-Path $existing.codexCommand) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor') -Filter codex.exe -Recurse -File)[0].FullName
$root = Join-Path ([IO.Path]::GetTempPath()) ('harness-full-move-' + [guid]::NewGuid().ToString('N'))
$source = Join-Path $root 'checkout источник'
$bindingHome = Join-Path $root 'binding home'
$codexPath = Join-Path $root 'codex'
$workspace = Join-Path $root 'outside TS project'
$utf8 = [Text.UTF8Encoding]::new($false)
$lf = [string][char]10
$initialPath = $env:Path
$userPath = [Environment]::GetEnvironmentVariable('Path','User')
$originalCodexHome = $env:CODEX_HOME
$server = $null
$connected = $false
$succeeded = $false
$checks = [Collections.Generic.List[string]]::new()
$protected = @{}
$realLinks = @{}
foreach ($item in Get-ChildItem -LiteralPath (Join-Path $DependencyUserHome '.agents/skills') -Force) { $realLinks[$item.FullName] = @($item.LinkType,$item.LinkTarget) -join '|' }
foreach ($path in @((Join-Path $realCodexHome 'config.toml'),(Join-Path $realCodexHome 'harness/installation.json'),(Join-Path $realCodexHome 'harness/code-tools-registration.json'),(Join-Path $realCodexHome 'harness/code-tools.json'),(Join-Path $DependencyUserHome '.serena/serena_config.yml'))) {
    if (Test-Path -LiteralPath $path -PathType Leaf) { $protected[$path] = (Get-FileHash -LiteralPath $path).Hash }
}
foreach ($record in @($inventory.mcp) + @($inventory.languages)) {
    if ($record.PSObject.Properties.Name -notcontains 'paths') { continue }
    foreach ($property in $record.paths.PSObject.Properties) {
        $path = $property.Value
        if ($path -is [string] -and (Test-Path -LiteralPath $path -PathType Leaf)) { $protected[$path] = (Get-FileHash -LiteralPath $path).Hash }
    }
}
function Assert-Move([bool]$Condition,[string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    $checks.Add($Message); Write-Output "PASS: $Message"
}
function Write-Move([string]$Path,[string]$Text) {
    $null = New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force
    [IO.File]::WriteAllText($Path,$Text,$utf8)
}
function Assert-SharedPreserved {
    foreach ($path in $protected.Keys) { if ((Get-FileHash -LiteralPath $path).Hash -ne $protected[$path]) { throw "Shared file changed; preserve it for review: $path" } }
    foreach ($path in $realLinks.Keys) { $item=Get-Item -LiteralPath $path -Force; if ((@($item.LinkType,$item.LinkTarget) -join '|') -ne $realLinks[$path]) { throw "Real personal skill link changed: $path" } }
    if ([Environment]::GetEnvironmentVariable('Path','User') -cne $userPath) { throw 'Persistent user PATH changed.' }
}
function Invoke-FullMoveInstall([string]$Mode) {
    $result = & (Join-Path $script:source 'install.ps1') -Mode $Mode -CodexHome $codexPath -UserHome $bindingHome -DependencyUserHome $DependencyUserHome -CodexCommand $existing.codexCommand -PathScope Process
    return $result
}
function Invoke-NativeMoveTool([string]$ServerName,[string]$Name,[hashtable]$Arguments) {
    $result = Invoke-ConsumerRpc $script:server 'mcpServer/tool/call' @{threadId=$script:threadId;server=$ServerName;tool=$Name;arguments=$Arguments} -TimeoutSeconds 75
    if ($result.isError) { throw "Native $ServerName/$Name failed: $($result | ConvertTo-Json -Depth 20 -Compress)" }
    return ($result.content | Where-Object type -EQ text | ForEach-Object text) -join $lf
}
function Verify-MovedConsumer([string]$Phase) {
    $check = Invoke-FullMoveInstall Check
    Write-Move (Join-Path $root ($Phase + '-check.json')) ($check | ConvertTo-Json -Depth 60)
    Assert-Move ($check.status -eq 'Connected' -and $check.codeTools.health.status -eq 'protocol-ready' -and $check.codeTools.health.servers.Count -eq 5) "$Phase actual Check starts all five MCPs from the connected source"
    $script:server = Start-ConsumerServer $native $codexPath $workspace
    $thread = Invoke-ConsumerRpc $script:server 'thread/start' @{cwd=$workspace;ephemeral=$true}
    $script:threadId = $thread.thread.id
    $config = Invoke-ConsumerRpc $script:server 'config/read' @{includeLayers=$false;cwd=$workspace}
    foreach ($name in @('serena','codebase-memory','graphify','nuphus','harness-lsp')) {
        $args = @($config.config.mcp_servers[$name].args)
        Assert-Move ($args -contains (Join-Path $script:source 'tools/mcp.ps1')) "$Phase native registration directly names current source: $name"
    }
    $null = Invoke-NativeMoveTool serena initial_instructions @{}
    $null = Invoke-NativeMoveTool serena activate_project @{project=$workspace}
    $before = Invoke-NativeMoveTool serena get_diagnostics_for_file @{relative_path='index.ts'}
    Assert-Move ($before.Trim() -eq '{}') "$Phase existing TypeScript server starts clean"
    $edit = Invoke-NativeMoveTool serena replace_in_files @{relative_path='index.ts';mode='literal';needle='export const value: number = 1;';repl='export const value: number = "wrong";';expected_count=1}
    $errorResult = Invoke-NativeMoveTool serena get_diagnostics_for_file @{relative_path='index.ts'}
    Assert-Move ($edit -match 'Replaced 1 occurrence' -and $errorResult -match '2322') "$Phase actual native Serena edit yields TypeScript code 2322"
    $null = Invoke-NativeMoveTool serena replace_in_files @{relative_path='index.ts';mode='literal';needle='export const value: number = "wrong";';repl='export const value: number = 1;';expected_count=1}
    $clean = Invoke-NativeMoveTool serena get_diagnostics_for_file @{relative_path='index.ts'}
    Assert-Move ($clean.Trim() -eq '{}') "$Phase actual correction clears diagnostics"
    Stop-ConsumerServer $script:server; $script:server=$null
}
try {
    # This is a source fixture, not a copied deployment. Every installed
    # capability must remain a direct link or native path registration to it.
    foreach ($relative in @('tools','global','.agents')) {
        $destination = Join-Path $source $relative
        $null = New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force
        Copy-Item -LiteralPath (Join-Path $repository $relative) -Destination $destination -Recurse
    }
    Copy-Item -LiteralPath (Join-Path $repository 'install.ps1') -Destination $source
    $foreignConfig = 'model = "gpt-6-astra"' + $lf + 'model_verbosity = "low"' + $lf + '[mcp_servers.foreign_fixture]' + $lf + 'command = "foreign-disabled"' + $lf + 'enabled = false' + $lf
    Write-Move (Join-Path $codexPath 'config.toml') $foreignConfig
    Write-Move (Join-Path $codexPath 'foreign.txt') 'foreign data'
    Write-Move (Join-Path $bindingHome '.agents/skills/foreign-fixture/SKILL.md') ('---' + $lf + 'name: foreign-fixture' + $lf + 'description: Existing foreign fixture' + $lf + '---')
    Write-Move (Join-Path $workspace '.serena/project.yml') ('project_name: full-move-fixture' + $lf + 'languages: [typescript]' + $lf + 'encoding: utf-8' + $lf)
    Write-Move (Join-Path $workspace 'index.ts') ('export const value: number = 1;' + $lf)
    Write-Move (Join-Path $workspace 'tsconfig.json') '{"compilerOptions":{"strict":true,"noEmit":true},"include":["*.ts"]}'
    $env:CODEX_HOME = $codexPath
    Write-Output "Full source lifecycle fixture: $root"
    $result = Invoke-FullMoveInstall Install
    $connected = $true
    Write-Move (Join-Path $root 'install-first.json') ($result | ConvertTo-Json -Depth 60)
    Assert-Move ($result.status -eq 'Connected') 'full Install connects the fresh source with explicitly shared dependencies'
    Assert-Move (@($result.codeTools.dependencies.results | Where-Object state -In @('installed','installed-unverified','updated','pending','failed')).Count -eq 0) 'full Install only reuses already installed compatible packages'
    $state = Get-Content -LiteralPath (Join-Path $codexPath 'harness/installation.json') -Raw | ConvertFrom-Json
    Assert-Move ($state.dependencyUserHome -eq [IO.Path]::GetFullPath($DependencyUserHome) -and $state.userHome -eq $bindingHome) 'binding and dependency owners persist separately'
    foreach ($link in $state.links) {
        Assert-Move ((Get-Item -LiteralPath $link.destination -Force).LinkType -eq 'SymbolicLink' -and [IO.Path]::GetFullPath($link.source).StartsWith($source + '\',[StringComparison]::OrdinalIgnoreCase)) 'fresh source capability is connected directly'
    }
    Verify-MovedConsumer first
    $moved = Join-Path $root 'moved checkout источник'
    foreach ($path in @($source,$moved)) { if (-not [IO.Path]::GetFullPath($path).StartsWith($root + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Source move escaped owned fixture.' } }
    Move-Item -LiteralPath $source -Destination $moved
    $source = $moved
    $result = Invoke-FullMoveInstall Install
    Assert-Move ($result.status -eq 'Connected') 'full Install reconnects all sources after the owned checkout moves'
    $state = Get-Content -LiteralPath (Join-Path $codexPath 'harness/installation.json') -Raw | ConvertFrom-Json
    Assert-Move (@($state.links | Where-Object { -not $_.source.StartsWith($source + '\',[StringComparison]::OrdinalIgnoreCase) }).Count -eq 0) 'all direct source links name only the moved checkout'
    Verify-MovedConsumer moved
    $result = Invoke-FullMoveInstall Disconnect
    $connected = $false
    Assert-Move ($result.status -eq 'Disconnected') 'full Disconnect removes owned connections after relocation'
    Assert-Move (-not (Test-Path -LiteralPath (Join-Path $codexPath 'harness/installation.json')) -and -not (Test-Path -LiteralPath (Join-Path $codexPath 'harness/code-tools-registration.json')) -and -not (Test-Path -LiteralPath (Join-Path $codexPath 'hooks.json'))) 'owned lifecycle state and hook source connection are removed'
    Assert-Move ((Get-Content -LiteralPath (Join-Path $codexPath 'config.toml') -Raw).Contains('[mcp_servers.foreign_fixture]') -and (Get-Content -LiteralPath (Join-Path $codexPath 'foreign.txt') -Raw) -eq 'foreign data' -and (Test-Path -LiteralPath (Join-Path $bindingHome '.agents/skills/foreign-fixture/SKILL.md'))) 'foreign configuration, data and pre-existing skill survive Disconnect'
    Assert-Move ($env:Path -ceq $initialPath) 'the test process PATH is exactly restored'
    Assert-SharedPreserved
    Assert-Move $true 'actual user configuration, personal skill links, persistent PATH and adopted entrypoint files are unchanged'
    Write-Move (Join-Path $root 'report.json') (@{status='passed';checks=$checks;assertions=$checks.Count;nativeVersion=(& $native --version);root=$root;modelProbe=$false;dependencyOwner=$DependencyUserHome;bindingHome=$bindingHome} | ConvertTo-Json -Depth 10)
    Write-Output "Full source lifecycle checks passed: $($checks.Count) assertions."
    $succeeded = $true
} finally {
    if ($server) { Stop-ConsumerServer $server }
    if ($connected) { Write-Warning 'Owned installation retained after failure; use its recorded owners for Recover/Disconnect.' }
    $env:CODEX_HOME = $originalCodexHome
    $env:Path = $initialPath
    Assert-SharedPreserved
    Write-Output "Retained owned source lifecycle evidence: $root"
}
