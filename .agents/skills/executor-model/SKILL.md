---
name: executor-model
description: Switch the model used by kit executors in one action by pointing executor_profiles in checkout orchestration.toml files at an installed Codex profile, then show the resolved model/effort. Use when the user asks to change the executor/delegation model (for example "executors to ds/deepseek/xai/zai") or wants it done instantly.
---

# Executor model switch (one action)

The executor model is the executor profile: `global/orchestration.toml` in each
checkout names `executor_profiles`, and the profile file under CODEX_HOME holds
the model, provider and reasoning effort. Switching the profile switches the
model. No reinstall is needed; the installed launcher reads the kit checkout's
config live, and consumer checkouts read their own file.

This skill is kit-owned and source-linked; it ships no executable file. Run the
canonical block below once, setting `$ProfileId` and `$Roots` to the user's
target model profile and the checkouts they named:

```powershell
$ProfileId = 'ds'
$Roots = 'D:\path\to\kit', 'D:\path\to\consumer'
$codexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { "$HOME\.codex" }
$installed = @(Get-ChildItem $codexHome -Filter '*.config.toml' -File |
    ForEach-Object { $_.Name -replace '\.config\.toml$', '' })
if (Test-Path "$codexHome\config.toml") {
    $installed += @(Select-String -Path "$codexHome\config.toml" -Pattern '^\s*\[profiles\.([^\]]+)\]' |
        ForEach-Object { $_.Matches[0].Groups[1].Value })
}
if ($installed -notcontains $ProfileId) {
    throw "profile '$ProfileId' is not installed in $codexHome. Installed: $(($installed | Sort-Object -Unique) -join ', ')"
}
if (Test-Path "$codexHome\$ProfileId.config.toml") {
    Get-Content "$codexHome\$ProfileId.config.toml" |
        Select-String -Pattern '^(model|model_provider|model_reasoning_effort)\s*='
}
foreach ($root in $Roots) {
    $file = Join-Path $root 'global\orchestration.toml'
    $raw = Get-Content -Raw $file
    $updated = $raw -replace 'executor_profiles\s*=\s*\[[^\]]*\]', "executor_profiles = [`"$ProfileId`"]"
    if ($updated -eq $raw -and $raw -notmatch "executor_profiles\s*=\s*\[`"$ProfileId`"\]") {
        throw "no executor_profiles line to update in $file"
    }
    Set-Content -Path $file -Value $updated -NoNewline
    Select-String -Path $file -Pattern 'executor_profiles' |
        ForEach-Object { '{0}:{1}: {2}' -f $_.Path, $_.LineNumber, $_.Line }
}
```

The whole block is one action: it refuses an uninstalled profile (and lists
what is installed), prints the resolved model/provider/effort, rewrites only
the `executor_profiles` line in every named checkout, and reads each change
back with file and line.

- `-ProfileId` accepts any installed profile (`ds`, `xai`, `zai`, ...). If the
  user names a model slug instead, find the profile whose `<id>.config.toml`
  (or `[profiles.<id>]` section in `config.toml`) sets that `model =` value and
  run the script with that profile id.
- The script refuses a missing profile or a target without the
  `executor_profiles` line, edits nothing else, and prints every updated line
  plus the resolved model/effort.
- Never touch `lead_profile`, `successor_lead_profile`, provider/auth blocks or
  tests. In the harness kit checkout some unit tests pin the committed executor
  id; report that instead of editing tests silently.
