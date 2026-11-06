param([int]$ParentPid, [string]$Install, [string]$New, [string]$Previous, [string]$Stage, [int]$Build, [switch]$Quiet)
$ErrorActionPreference = 'Stop'
$log = Join-Path (Split-Path -Parent $Install) 'zerus-fork-update.log'
function Log([string]$message) { try { Add-Content -LiteralPath $log -Value ("{0:u} {1}" -f (Get-Date), $message) } catch { } }
