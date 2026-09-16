[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$RepoRoot,
  [Parameter(Mandatory = $true)][string]$LiveCodexHome
)
$ErrorActionPreference = "Stop"

$root = Join-Path ([IO.Path]::GetTempPath()) ("xai-profile-verify-" + [guid]::NewGuid().ToString("N"))
$codexHomeDir = Join-Path $root "codex-home"
$user = Join-Path $root "user-home"
$workspace = Join-Path $root "workspace"
foreach ($directory in @($codexHomeDir, $user, $workspace, (Join-Path $codexHomeDir "harness"), (Join-Path $codexHomeDir "harness/subscriptions"))) { [void][IO.Directory]::CreateDirectory($directory) }
$report = [ordered]@{
  status = "running"
  startedAt = [DateTime]::UtcNow.ToString("o")
  evidence = $root
  repo = $RepoRoot
  codexHome = $codexHomeDir
  checks = [Collections.Generic.List[string]]::new()
}
function Assert-Check([bool]$Condition, [string]$Message) {
  if (-not $Condition) { throw "FAIL: $Message" }
  $report.checks.Add($Message) | Out-Null
  Write-Output "PASS: $Message"
}
try {
  $installation = Get-Content -Raw (Join-Path $LiveCodexHome "harness/installation.json") | ConvertFrom-Json
  $bridge = $installation.configBridge
  Copy-Item -LiteralPath (Join-Path $LiveCodexHome "harness/installation.json") -Destination (Join-Path $codexHomeDir "harness/installation.json")
  Copy-Item -LiteralPath (Join-Path $LiveCodexHome "harness/subscriptions/xai-oauth.json") -Destination (Join-Path $codexHomeDir "harness/subscriptions/xai-oauth.json")
  Assert-Check (Test-Path -LiteralPath (Join-Path $codexHomeDir "harness/subscriptions/xai-oauth.json")) "private xAI store staged in isolated home"

  $connected = & $bridge install --subscriptions-only --source $RepoRoot --codex-home $codexHomeDir --user-home $user
  if ($LASTEXITCODE -ne 0) { throw "subscriptions install failed" }
  $profilePath = Join-Path $codexHomeDir "xai.config.toml"
  $catalogPath = Join-Path $codexHomeDir "xai.models.json"
  Assert-Check (Test-Path -LiteralPath $profilePath) "installer wrote xai.config.toml"
  $profile = Get-Content -Raw $profilePath
  Assert-Check ($profile -notmatch "openai_base_url") "profile contains no openai_base_url"
  $catalog = Get-Item -LiteralPath $catalogPath
  $catalogTarget = ([string]$catalog.Target).Replace("\", "/")
  Assert-Check ($catalog.LinkType -eq "SymbolicLink" -and $catalogTarget -like "*global/codex-profiles/xai.models.json") "xai catalog is a kit source link"
  Assert-Check (-not (Test-Path -LiteralPath (Join-Path $codexHomeDir "config.toml"))) "ordinary config.toml was not created or injected"
  $report.profileAuthCommand = ([regex]::Match($profile, 'command = "([^"]+)"')).Groups[1].Value
  Assert-Check ($report.profileAuthCommand.StartsWith("C:/Users/noilw/.codex/harness/config-bridge/builds/")) "auth command resolves to the recorded kit manager"

  $marker = "XAI-PROFILE-" + [guid]::NewGuid().ToString("N").Substring(0, 12)
  $codexCli = $installation.codexCommand
  $environment = @{ CODEX_HOME = $codexHomeDir }
  $prompt = "Use one shell command to write exactly $marker into proof.txt in the current directory. Then read the file and print its content in your final message. No other work."
  $stdout = Join-Path $root "codex-stdout.txt"
  $stderr = Join-Path $root "codex-stderr.txt"
  $scriptBlock = {
    param($CodexCli, $Workspace, $Prompt, $Stdout, $Stderr, $HomeDir)
    $env:CODEX_HOME = $HomeDir
    & $CodexCli exec --profile xai --skip-git-repo-check -C $Workspace --dangerously-bypass-approvals-and-sandbox -c 'model_reasoning_effort="low"' $Prompt 1> $Stdout 2> $Stderr
    return $LASTEXITCODE
  }
  $job = Start-Job -ScriptBlock $scriptBlock -ArgumentList $codexCli, $workspace, $prompt, $stdout, $stderr, $codexHomeDir
  if (Wait-Job $job -Timeout 300) { $exitCode = Receive-Job $job; Stop-Job $job -ErrorAction SilentlyContinue } else { Stop-Job $job; throw "codex exec timed out after 300s" }
  Remove-Job $job -Force
  $report.codexExit = $exitCode
  Assert-Check ($exitCode -eq 0) "codex --profile xai exec exited successfully"
  Assert-Check ((Test-Path -LiteralPath (Join-Path $workspace "proof.txt")) -and ((Get-Content -Raw (Join-Path $workspace "proof.txt")).Trim() -eq $marker)) "Grok completed a real tool call producing proof.txt"
  $visible = Get-Content -Raw $stdout
  Assert-Check ($visible -match [regex]::Escape($marker)) "final answer carries the marker"

  $rollouts = Get-ChildItem -LiteralPath (Join-Path $codexHomeDir "sessions") -Recurse -Filter *.jsonl -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending
  Assert-Check ($rollouts.Count -gt 0) "native rollout evidence retained in isolated home"
  $rollout = $rollouts[0].FullName
  $report.rollout = $rollout
  $lines = Get-Content -LiteralPath $rollout
  $turnModels = @($lines | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.type -eq "turn_context" } | ForEach-Object { $_.payload.model })
  Assert-Check ($turnModels.Count -gt 0 -and (@($turnModels | Sort-Object -Unique) -eq "grok-4.6")) "session model identity is grok-4.6"
  $text = $lines -join "`n"
  Assert-Check ($text -notmatch "127\.0\.0\.1:10100") "rollout shows no localhost proxy routing"

  $disconnected = & $bridge disconnect --subscriptions-only --source $RepoRoot --codex-home $codexHomeDir --user-home $user
  if ($LASTEXITCODE -ne 0) { throw "subscriptions disconnect failed" }
  Assert-Check (-not (Test-Path -LiteralPath $profilePath)) "disconnect removed the installed xai profile"
  $report.status = "passed"
} catch {
  $report.status = "failed"
  $report.failure = $_.Exception.Message
  try {
    & $bridge disconnect --subscriptions-only --source $RepoRoot --codex-home $codexHomeDir --user-home $user | Out-Null
  } catch { }
  throw
} finally {
  $report.completedAt = [DateTime]::UtcNow.ToString("o")
  [IO.File]::WriteAllText((Join-Path $root "report.json"), ($report | ConvertTo-Json -Depth 10))
  Write-Output ("Private evidence: " + $root)
}
