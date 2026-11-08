param([int]$ParentPid, [string]$Root, [string]$Link, [string]$New, [string]$Target, [string]$Stage, [int]$Build, [switch]$Quiet)
$ErrorActionPreference = 'Stop'
$log = Join-Path $Root 'update.log'
function Log([string]$message) { try { Add-Content -LiteralPath $log -Value ("{0:u} {1}" -f (Get-Date), $message) } catch { } }
