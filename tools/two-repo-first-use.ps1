[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $Secondary
)

$ErrorActionPreference = 'Stop'

function Read-ExpectedLine([string] $Path) {
    ([System.IO.File]::ReadAllText($Path)) -replace "(`r`n|`n|`r)\z", ''
}

try {
    $greeting = Read-ExpectedLine (Join-Path $PSScriptRoot 'two-repo-greeting.txt')
    $value = Read-ExpectedLine (Join-Path $Secondary 'value.txt')
}
catch {
    [Console]::Error.WriteLine("two-repo-first-use: cannot read inputs: $($_.Exception.Message)")
    exit 1
}

$combined = "$greeting $value"
if (-not [string]::Equals($combined, 'hello from both', [System.StringComparison]::Ordinal)) {
    [Console]::Error.WriteLine("two-repo-first-use: expected 'hello from both', got '$combined'")
    exit 1
}

Write-Output 'hello from both'
