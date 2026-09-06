# Helpers for isolated, real-CLI app-server probes. No mock server is used.
# Requires PowerShell 7 for ProcessStartInfo.ArgumentList and bounded async reads.
function Start-ConsumerServer {
    param([string]$NativeCodex, [string]$CodexHome, [string]$WorkingDirectory)
    $start = [Diagnostics.ProcessStartInfo]::new($NativeCodex)
    $start.ArgumentList.Add('app-server')
    $start.ArgumentList.Add('--stdio')
    $start.WorkingDirectory = $WorkingDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $start.Environment['CODEX_HOME'] = $CodexHome
    $process = [Diagnostics.Process]::Start($start)
    $server = @{ Process = $process; NextId = 1; ErrorRead = $process.StandardError.ReadToEndAsync() }
    $null = Invoke-ConsumerRpc $server 'initialize' @{clientInfo=@{name='codex-harness-consumer-tests';version='1'};capabilities=@{experimentalApi=$true}}
    $process.StandardInput.WriteLine('{"method":"initialized"}')
    $process.StandardInput.Flush()
    return $server
}

function Invoke-ConsumerRpc {
    param([hashtable]$Server, [string]$Method, [hashtable]$Parameters, [int]$TimeoutSeconds = 30)
    $id = $Server.NextId++
    $request = @{id=$id;method=$Method;params=$Parameters} | ConvertTo-Json -Depth 30 -Compress
    $Server.Process.StandardInput.WriteLine($request)
    $Server.Process.StandardInput.Flush()
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $lineTask = $Server.Process.StandardOutput.ReadLineAsync()
        $remaining = [Math]::Max(1, [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds)
        if (-not $lineTask.Wait($remaining)) { throw "Timed out waiting for app-server $Method." }
        if ($null -eq $lineTask.Result) {
            $detail = if ($Server.ErrorRead.IsCompleted) { [string]$Server.ErrorRead.GetAwaiter().GetResult() } else { 'stderr is still pending' }
            if ($detail.Length -gt 2500) { $detail = $detail.Substring($detail.Length - 2500) }
            $detail = $detail -replace '(?i)((?:authorization|api[_-]?key|access[_-]?token|password|secret)\s*[:=]\s*)[^\s,;]+', '$1[redacted]'
            throw "App-server exited during ${Method}: $detail"
        }
        $response = $lineTask.Result | ConvertFrom-Json -AsHashtable
        if ($Server.ContainsKey('TracePath')) { [IO.File]::AppendAllText($Server.TracePath, $lineTask.Result + "`n", [Text.UTF8Encoding]::new($false)) }
        if ($response.ContainsKey('id') -and $response.id -eq $id) {
            if ($response.ContainsKey('error')) { throw "App-server ${Method}: $($response.error | ConvertTo-Json -Compress)" }
            return $response.result
        }
    }
    throw "Timed out waiting for app-server $Method."
}

function Stop-ConsumerServer {
    param([hashtable]$Server)
    if ($null -eq $Server) { return }
    $Server.Process.StandardInput.Close()
    if (-not $Server.Process.WaitForExit(3000)) { $Server.Process.Kill($true); $Server.Process.WaitForExit() }
    $Server.Process.Dispose()
}
