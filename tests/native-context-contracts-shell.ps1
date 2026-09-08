#requires -Version 7.4
<# No model calls. Compare native default shell selection with inherited PATH and
with only packaged PowerShell/WindowsApps lookup directories removed from PATH.
Each arm uses a new owned home. thread/shellCommand is unsandboxed by its schema:
its fixed command only reports its executable and reads owned seed.txt. The same
selected executable is then checked through command/exec with readOnly sandbox.
Receipts are retained; neither arm modifies the parent/global environment. #>
[CmdletBinding()]
param([Parameter(Mandatory)][string]$EvidenceRoot, [ValidateSet('baseline','filtered')][string]$Arm)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot
$EvidenceRoot=[IO.Path]::GetFullPath($EvidenceRoot)
$tempPrefix=[IO.Path]::GetFullPath($env:TEMP).TrimEnd('\')+'\'
if(-not $EvidenceRoot.StartsWith($tempPrefix,[StringComparison]::OrdinalIgnoreCase)){throw 'Owned host-temp root required.'}
$armRoot=Join-Path $EvidenceRoot $Arm
if(Test-Path -LiteralPath $armRoot){throw 'Arm must be new.'}
$probeHome=Join-Path $armRoot 'home';$workspace=Join-Path $armRoot 'workspace'
foreach($path in @($probeHome,$workspace)){[IO.Directory]::CreateDirectory($path)|Out-Null}
$meta=Get-Content -LiteralPath "$env:USERPROFILE/.codex/harness/installation.json" -Raw|ConvertFrom-Json
$native=Join-Path (Split-Path $meta.codexCommand) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe'
Copy-Item -LiteralPath "$env:USERPROFILE/.codex/opencodex-catalog.json" -Destination (Join-Path $probeHome 'models.json')
$models=(Join-Path $probeHome 'models.json').Replace('\','/')
@"
model = "gpt-6-astra"
model_provider = "openai"
model_catalog_json = "$models"
openai_base_url = "http://127.0.0.1:10100/v1"
approval_policy = "never"
sandbox_mode = "read-only"
[windows]
sandbox = "unelevated"
[features]
hooks = false
apps = false
multi_agent = false
multi_agent_v2 = false
"@|Set-Content -LiteralPath (Join-Path $probeHome 'config.toml')
'17'|Set-Content -LiteralPath (Join-Path $workspace 'seed.txt')
$originalPath=$env:PATH
$removed=@($originalPath -split ';'|Where-Object{$_ -match '(?i)\\WindowsApps(?:\\|$)'})
$result=[ordered]@{arm=$Arm;root=$armRoot;native=$native;nativeHash=(Get-FileHash $native).Hash;removedPathEntries=@();modelCalls=0}
$server=$null
try{
    if($Arm -eq 'filtered'){$env:PATH=($originalPath -split ';'|Where-Object{$_ -notin $removed}) -join ';';$result.removedPathEntries=$removed}
    . (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
    $server=Start-ConsumerServer $native $probeHome $workspace
    $server.TracePath=Join-Path $armRoot 'rpc.jsonl'
    $thread=Invoke-ConsumerRpc $server 'thread/start' @{model='gpt-6-astra';modelProvider='openai';allowProviderModelFallback=$false;cwd=$workspace;sandbox='read-only';approvalPolicy='never'}
    $result.threadId=$thread.thread.id;$result.serverPid=$server.Process.Id
    $command='[Diagnostics.Process]::GetCurrentProcess().MainModule.FileName; Get-Content -LiteralPath seed.txt'
    $null=Invoke-ConsumerRpc $server 'thread/shellCommand' @{threadId=$thread.thread.id;command=$command;timeoutMs=10000}
    $deadline=[DateTime]::UtcNow.AddSeconds(15)
    while([DateTime]::UtcNow -lt $deadline){
        $read=$server.Process.StandardOutput.ReadLineAsync()
        if(-not $read.Wait([Math]::Max(1,[int]($deadline-[DateTime]::UtcNow).TotalMilliseconds))){throw 'Shell event deadline.'}
        if($null -eq $read.Result){throw 'Server exited before shell completion.'}
        [IO.File]::AppendAllText($server.TracePath,$read.Result+"`n")
        $row=$read.Result|ConvertFrom-Json -AsHashtable
        if($row.method -eq 'item/completed' -and $row.params.item.type -eq 'commandExecution'){
            $result.selection=$row.params.item
            break
        }
    }
    if(-not $result.selection){throw 'No completed shell item.'}
    $selected=($result.selection.aggregatedOutput -split '\r?\n'|Where-Object{$_ -match '(?i)^[A-Z]:.*\.exe$'}|Select-Object -First 1)
    if(-not $selected){throw 'Default shell did not report an absolute executable.'}
    $result.selectedShell=$selected
    try{
        $result.sandbox=Invoke-ConsumerRpc $server 'command/exec' @{command=@($selected,'-NoLogo','-NoProfile','-Command','Get-Content -LiteralPath seed.txt');cwd=$workspace;sandboxPolicy=@{type='readOnly'};timeoutMs=10000}
    }catch{$result.sandboxError=$_.ToString()}
    $result.status='observed'
}catch{$result.status='failed';$result.error=$_.ToString()}
finally{
    if($server){Stop-ConsumerServer $server;$server.ErrorRead.GetAwaiter().GetResult()|Set-Content -LiteralPath (Join-Path $armRoot 'server-stderr.txt')}
    $env:PATH=$originalPath
    $result|ConvertTo-Json -Depth 20|Set-Content -LiteralPath (Join-Path $armRoot 'result.json')
}
$result|ConvertTo-Json -Depth 20
if($result.status -ne 'observed'){exit 1}
