#Requires -Version 7.4
<#!
Runs real adopted Nuphus through the source proxy. Creates a hidden/off-screen
owned window, resizes it by exact HWND, serves one owned page on loopback and
types/clicks only its DOM. No user windows/tabs/cookies are inspected or changed.
Browser/MCP/window/server processes are closed; private evidence is retained.
Model downloads are disabled; missing OCR prerequisites are reported separately.
#>
param([Parameter(Mandatory)][string]$Registry,[string]$EvidenceRoot)
$ErrorActionPreference = 'Stop'
$source = Split-Path $PSScriptRoot -Parent
$inventory = Get-Content -LiteralPath $Registry -Raw | ConvertFrom-Json -AsHashtable
$python = ($inventory.mcp | Where-Object id -EQ 'serena').paths.python
if (-not $EvidenceRoot) { $EvidenceRoot = Join-Path ([IO.Path]::GetTempPath()) ('harness-nuphus-operations-' + [guid]::NewGuid().ToString('N')) }
if (Test-Path -LiteralPath $EvidenceRoot) { throw 'Evidence directory must be a new owned path' }
& $python -B (Join-Path $PSScriptRoot 'fixtures/code-tools-native/nuphus-operations.py') $Registry $source $EvidenceRoot
if ($LASTEXITCODE -ne 0) { throw 'Nuphus real operation probe failed; inspect the preserved owned evidence path printed above' }
$report = Get-Content -LiteralPath (Join-Path $EvidenceRoot 'report.json') -Raw | ConvertFrom-Json
Write-Output "Nuphus actual operation assertions: $($report.checks.Count)"
if ($report.ocr_is_error) { Write-Output 'OCR prerequisite unresolved: see private perceive.json. This is not full task acceptance.' }
