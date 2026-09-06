param([Parameter(Mandatory)][string]$StatePath,[Parameter(Mandatory)][string]$StopPath)
Add-Type -AssemblyName System.Windows.Forms
$form = [Windows.Forms.Form]::new()
$form.Text = 'Harness owned Nuphus fixture'
$form.ShowInTaskbar = $false
$form.StartPosition = 'Manual'
$form.Location = [Drawing.Point]::new(-10000,-10000)
$form.Size = [Drawing.Size]::new(320,180)
$form.Add_Shown({
    [IO.File]::WriteAllText($StatePath, (@{pid=$PID;hwnd=$form.Handle.ToInt64()} | ConvertTo-Json -Compress))
})
$timer = [Windows.Forms.Timer]::new()
$timer.Interval = 100
$timer.Add_Tick({ if (Test-Path -LiteralPath $StopPath) { $form.Close() } })
$timer.Start()
try { $null = $form.ShowDialog() } finally { $timer.Dispose(); $form.Dispose() }
