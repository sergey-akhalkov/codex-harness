#requires -Version 7.4
[CmdletBinding()]
param([Parameter(Mandatory)][ValidateSet('pre','post','stop')][string]$Event)
# Retired diagnostic entry point; the RTK exception has its own native handler.
# Keep this linked compatibility entry point for
# cached sessions: no stdin scan, runtime discovery, process startup or feedback.
return
