#requires -Version 7.4
[CmdletBinding()]
param([Parameter(Mandatory)][ValidateSet('pre','post','stop')][string]$Event)
$ErrorActionPreference = 'Stop'
# Native hook JSON is UTF-8. Windows Console.In otherwise uses the OEM code
# page and corrupts non-ASCII workspace names before Python receives the bytes.
$utf8 = [Text.UTF8Encoding]::new($false)
[Console]::InputEncoding = $utf8
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8
$env:PYTHONUTF8 = '1'
$env:PYTHONIOENCODING = 'utf-8'
$payload = [Console]::In.ReadToEnd()
try {
    $hostRoot = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex' }
    $state = Get-Content -LiteralPath (Join-Path $hostRoot 'harness/installation.json') -Raw | ConvertFrom-Json
    $inventory = Get-Content -LiteralPath (Join-Path $hostRoot 'harness/code-tools.json') -Raw | ConvertFrom-Json
    $serena = @($inventory.mcp | Where-Object id -EQ 'serena')
    if ($serena.Count -ne 1 -or -not (Test-Path -LiteralPath $serena[0].paths.python -PathType Leaf)) { throw 'Shared Python unavailable.' }
    $payload | & $serena[0].paths.python -B -u (Join-Path $state.sourceRoot 'tools/lsp/journal.py') --event $Event
    if ($LASTEXITCODE -ne 0) { throw "Diagnostic journal failed (exit $LASTEXITCODE)." }
} catch {
    $eventName = try { ($payload | ConvertFrom-Json).hook_event_name } catch { $null }
    if (-not $eventName) { $eventName = switch ($Event) { pre { 'PreToolUse' } post { 'PostToolUse' } default { 'Stop' } } }
    # Hook infrastructure cannot turn an absent baseline into an apparently clean check.
    $message = 'Automatic LSP diagnostics unavailable: the pre-edit journal or shared runtime could not be opened. No clean diagnostic result is established.'
    if ($Event -in @('pre','post')) {
        @{ hookSpecificOutput = @{ hookEventName = $eventName; additionalContext = $message } } | ConvertTo-Json -Depth 4 -Compress
    } else {
        $active = try { [bool]($payload | ConvertFrom-Json).stop_hook_active } catch { $false }
        if ($active) { @{ systemMessage = $message } | ConvertTo-Json -Compress }
        else { @{ decision = 'block'; reason = $message } | ConvertTo-Json -Compress }
    }
}
