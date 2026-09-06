# Correlated diagnostics for the exact content supplied by the adapter. No edits,
# downloads, profile loading, or execution of the source being analyzed.
$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$diagnosticInput = [Console]::In.ReadToEnd() | ConvertFrom-Json -Depth 20
Import-Module -Name $diagnosticInput.analyzer -ErrorAction Stop
$analysisParameters = @{ ScriptDefinition = [string]$diagnosticInput.text; ErrorAction = 'Stop' }
if ($diagnosticInput.settings) {
    $analysisParameters.Settings = [string]$diagnosticInput.settings
}
$analysisResults = @(Invoke-ScriptAnalyzer @analysisParameters | ForEach-Object {
    $analysisSeverity = switch ([string]$_.Severity) { 'Error' { 1 } 'ParseError' { 1 } 'Warning' { 2 } default { 3 } }
    @{
        range = @{
            start = @{ line = [Math]::Max(0, $_.Extent.StartLineNumber - 1); character = [Math]::Max(0, $_.Extent.StartColumnNumber - 1) }
            end = @{ line = [Math]::Max(0, $_.Extent.EndLineNumber - 1); character = [Math]::Max(0, $_.Extent.EndColumnNumber - 1) }
        }
        message = [string]$_.Message
        severity = $analysisSeverity
        code = [string]$_.RuleName
        source = 'PSScriptAnalyzer'
    }
})
@{ diagnostics = $analysisResults; complete = $true } | ConvertTo-Json -Depth 20 -Compress
