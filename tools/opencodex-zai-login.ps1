#requires -Version 7.4
# Host-private Z.AI Coding Plan key login. Does not invoke ocx, does not read
# the local zai file profile, and never writes the key into linked config.json.
[CmdletBinding()]
param(
    [string]$CodexHome = $(if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }),
    [string]$UserHome = $env:USERPROFILE,
    [string]$SourceRoot,
    [string]$KeyFile,
    [switch]$NoOpenBrowser
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'This login entry requires Windows.' }
Import-Module (Join-Path $PSScriptRoot 'subscription-routing.psm1') -Force
if (-not $SourceRoot) {
    $servicePath = Join-Path $CodexHome 'harness/subscriptions/service.json'
    if (Test-Path -LiteralPath $servicePath -PathType Leaf) {
        $descriptor = Get-Content -LiteralPath $servicePath -Raw | ConvertFrom-Json
        if ($descriptor.owner -ne 'codex-harness-subscriptions') { throw 'Unexpected subscription service owner.' }
        $SourceRoot = $descriptor.source
    } else {
        $SourceRoot = Split-Path $PSScriptRoot
    }
}
$paths = Get-SubscriptionPaths -SourceRoot $SourceRoot -UserHome $UserHome -CodexHome $CodexHome
Assert-SubscriptionZaiSource $paths
$profileBefore = Get-SubscriptionLocalZaiProfileState $paths
function Read-ZaiKeyFromStdin {
    if ([Console]::IsInputRedirected) {
        $raw = [Console]::In.ReadToEnd()
        if ($null -eq $raw) { return $null }
        return $raw.Trim()
    }
    return $null
}
function Protect-ZaiKeyFile([string]$Path) {
    $acl = Get-Acl -LiteralPath $Path
    $acl.SetAccessRuleProtection($true, $false)
    $account = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    $rule = [Security.AccessControl.FileSystemAccessRule]::new($account, 'FullControl', 'Allow')
    $acl.SetAccessRule($rule)
    Set-Acl -LiteralPath $Path -AclObject $acl
}
$key = $null
if ($KeyFile) {
    if (-not [IO.Path]::IsPathFullyQualified($KeyFile)) { throw 'KeyFile must be an absolute path.' }
    $key = [Text.UTF8Encoding]::new($false).GetString([IO.File]::ReadAllBytes($KeyFile)).Trim()
} else {
    $key = Read-ZaiKeyFromStdin
}
if (-not $key) {
    $listen = [Net.HttpListener]::new()
    $listen.Prefixes.Add('http://127.0.0.1:0/')
    try {
        $listen.Start()
    } catch {
        $listen.Prefixes.Clear()
        $listen.Prefixes.Add('http://127.0.0.1:56131/')
        $listen.Start()
    }
    $prefix = @($listen.Prefixes)[0]
    $page = @"
<!doctype html><html lang="ru"><meta charset="utf-8"><title>Z.AI login</title>
<style>body{font:18px system-ui;max-width:640px;margin:70px auto;padding:24px}</style>
<h1>Подключить Z.AI GLM Coding Plan</h1>
<p>Вставьте ключ Coding Plan. Он останется только на этом ПК и не будет записан в Git.</p>
<form method="post" action="$prefix"><label>API key <input name="key" type="password" required autocomplete="off"></label><button type="submit">Save</button></form>
</html>
"@
    $pagePath = Join-Path $paths.runtime ('zai-login-' + [guid]::NewGuid().ToString('N') + '.html')
    [void][IO.Directory]::CreateDirectory($paths.runtime)
    [IO.File]::WriteAllText($pagePath, $page)
    if (-not $NoOpenBrowser) {
        $browser = [Diagnostics.ProcessStartInfo]::new($pagePath)
        $browser.UseShellExecute = $true
        [void][Diagnostics.Process]::Start($browser)
        Write-Output "Local Z.AI login page opened: $pagePath"
    } else {
        Write-Output "Open this local login page: $pagePath"
    }
    $context = $listen.GetContext()
    $reader = [IO.StreamReader]::new($context.Request.InputStream, $context.Request.ContentEncoding)
    try { $body = $reader.ReadToEnd() } finally { $reader.Dispose() }
    if ($body -match 'key=([^&]+)') { $key = [Uri]::UnescapeDataString($Matches[1]).Trim() }
    $bytes = [Text.Encoding]::UTF8.GetBytes('Saved. You can close this tab.')
    $context.Response.OutputStream.Write($bytes, 0, $bytes.Length)
    $context.Response.Close()
    $listen.Stop()
    if (Test-Path -LiteralPath $pagePath) { Remove-Item -LiteralPath $pagePath -Force }
}
if (-not $key -or $key -match '[\r\n]' -or $key.Length -gt 8192) { throw 'Z.AI key must be a single non-empty line.' }
if ($key -match '(?i)zai\.config\.toml') { throw 'Login must not use the local zai profile as the secret source.' }
[void][IO.Directory]::CreateDirectory((Split-Path $paths.zaiKey))
$temporary = $paths.zaiKey + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
try {
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $payload = [Text.UTF8Encoding]::new($false).GetBytes($key)
        $stream.Write($payload, 0, $payload.Length)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
    Protect-ZaiKeyFile $temporary
    if (Test-Path -LiteralPath $paths.zaiKey) { [IO.File]::Replace($temporary, $paths.zaiKey, [NullString]::Value) }
    else { [IO.File]::Move($temporary, $paths.zaiKey) }
} finally {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
}
Protect-ZaiKeyFile $paths.zaiKey
Assert-SubscriptionLocalZaiProfilePreserved $paths $profileBefore
[pscustomobject]@{ Authorized = $true; Provider = 'zai'; Store = $paths.zaiKey; ProfileUnchanged = $true }
