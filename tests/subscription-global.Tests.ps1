#requires -Version 7.4
# Read-only global configuration consumer; owned app-server, no model prompts.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot
. (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
$hostCodexHome = Join-Path $env:USERPROFILE '.codex'
$state = Get-Content (Join-Path $hostCodexHome 'harness/installation.json') -Raw | ConvertFrom-Json
$vendor = Join-Path (Split-Path $state.codexCommand) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor'
$native = (Get-ChildItem $vendor -Filter codex.exe -Recurse -File | Select-Object -First 1).FullName
$root = Join-Path $env:TEMP ('harness-subscription-global-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
$report = @{ passed = $false; native = $native; cwd = $root; source = $repo; checks = @{} }
$priorCodexHome = $env:CODEX_HOME
$server = $null
try {
    $env:CODEX_HOME = $hostCodexHome
    $tokenPath=Join-Path $hostCodexHome 'harness/token-workflow.json'
    $rtkSelected=(Test-Path $tokenPath) -and (Get-Content $tokenPath -Raw | ConvertFrom-Json).enabled
    $features = & $native -C $root features list
    $expectedHooks=([bool]$rtkSelected).ToString().ToLowerInvariant()
    if ($LASTEXITCODE -ne 0 -or -not ($features -match ('^hooks\s+\S+\s+'+$expectedHooks+'\s*$'))) { throw 'Base hook feature disagrees with accepted selection' }
    # 0.153.4 rejects --profile on features list. Check the parsed live profile
    # and exercise its supported native MCP configuration consumer separately.
    $pythonExe = Join-Path $env:USERPROFILE 'AppData/Roaming/uv/tools/serena-agent/Scripts/python.exe'
    & $pythonExe -B -c 'import sys,tomllib; from pathlib import Path; expected=sys.argv[2]=="true"; assert tomllib.loads(Path(sys.argv[1]).read_text()).get("features",{}).get("hooks",expected) is expected' (Join-Path $hostCodexHome 'harness.config.toml') $expectedHooks
    if ($LASTEXITCODE) { throw 'Linked profile disagrees with accepted base hook selection' }
    $profileMcp = & $native -C $root --profile harness mcp list --json | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or 'harness-lsp' -in @($profileMcp | ForEach-Object name)) { throw 'Profile configuration did not load the selection' }
    $listed = & $native -C $root mcp list --json | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Native registration discovery failed' }
    $names = @($listed | ForEach-Object name)
    foreach ($name in @('serena','codebase-memory','graphify','nuphus')) {
        if ($name -notin $names) { throw "Missing selected registration: $name" }
    }
    if ('harness-lsp' -in $names) { throw 'Retired registration reappeared' }
    $report.checks.nativeRegistrations = $names
    $report.checks.baseAndProfileHooks = [bool]$rtkSelected
    $server = Start-ConsumerServer $native $hostCodexHome $root
    $hooks = Invoke-ConsumerRpc $server 'hooks/list' @{ cwds = @($root) }
    $report.checks.hooks = $hooks
    $serialized = $hooks | ConvertTo-Json -Depth 30 -Compress
    if ($rtkSelected) {
        $registered=@($hooks.data | ForEach-Object {$_.hooks})
        if ($registered.Count -ne 1 -or $serialized -notmatch 'harness-rtk.exe hook') { throw 'Native consumer lists an unaccepted hook selection' }
    } elseif ($serialized -notmatch '"hooks":\[\]') { throw 'Native consumer lists hooks during suspension' }
    $registry = Get-Content (Join-Path $hostCodexHome 'harness/lsp-servers.json') -Raw | ConvertFrom-Json -AsHashtable
    if ($registry.servers.Count) { throw 'Retired harness backends are provisioned in the registry' }
    $full = & (Join-Path $repo 'install.ps1') -Mode Check -CodeToolsOnly -Detailed
    $full | ConvertTo-Json -Depth 50 | Set-Content (Join-Path $root 'details.json') -Encoding utf8
    if ($full.status -ne 'protocol-ready' -or $full.health.servers.Count -ne 4) { throw 'Selected protocol checks failed' }
    $report.checks.protocol = @($full.health.servers)
    $report.checks.harnessBackends = 0
    $report.checks.detailsCharacters = ($full | ConvertTo-Json -Depth 50 -Compress).Length
    $report.passed = $true
} finally {
    Stop-ConsumerServer $server
    $env:CODEX_HOME = $priorCodexHome
    $report | ConvertTo-Json -Depth 30 | Set-Content (Join-Path $root 'report.json') -Encoding utf8
    [pscustomobject]@{ passed = $report.passed; evidence = (Join-Path $root 'report.json') }
}
