
# The programs that still run from the installation, by name.
function Holders {
    $prefix = $Install.TrimEnd('\') + '\'
    @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -and $_.Path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) } catch { $false }
    } | ForEach-Object { $_.ProcessName + '.exe' } | Sort-Object -Unique)
}
function WaitIdle([int]$seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        $holders = @(Holders)
        if ($holders.Count -eq 0) { return $true }
        Step ('Waiting for programs that run from the installation: ' + ($holders -join ', ')) 25
        Pump 500
    }
    @(Holders).Count -eq 0
}
$gui = Join-Path $Install 'hgs-tray.exe'
Step 'Waiting for Zerus to close...' 10
[void](WaitExit $ParentPid 120)
if (-not (WaitIdle 120)) { Start-Process -FilePath $gui; Finish 'Programs still run from the installation; the update waits for the next restart.' $true; exit 1 }
Step 'Setting the current build aside...' 45
# The old rollback copy goes only once the current build can move; a scanner or
# indexer may hold the folder for a moment, so the move is tried for 20 seconds.
$aside = $Install.TrimEnd('') + '.aside'
$problem = $null
for ($try = 0; $try -lt 40; $try++) {
    try { Move-Item -LiteralPath $Install -Destination $aside; $problem = $null; break }
    catch { $problem = "$_"; Pump 500 }
}
if ($problem) { Start-Process -FilePath $gui; Finish "Could not set the current build aside: $problem" $true; exit 1 }
try {
    if (Test-Path -LiteralPath $Previous) { Remove-Item -LiteralPath $Previous -Recurse -Force }
    Move-Item -LiteralPath $aside -Destination $Previous
} catch { Log "Kept the current build as $aside`: $_"; $Previous = $aside }
Step 'Moving the new build in...' 60
try { Move-Item -LiteralPath $New -Destination $Install }
catch {
    $problem = "$_"
    Move-Item -LiteralPath $Previous -Destination $Install
    Start-Process -FilePath $gui; Finish "Could not move the new build in: $problem" $true; exit 1
}
Step 'Starting Zerus...' 70
$started = Start-Process -FilePath $gui -PassThru
if (-not (Started $started)) {
    $code = $started.ExitCode
    Step 'The new build exited; restoring the previous build...' 90
    [void](WaitIdle 30)
    $failed = $Install.TrimEnd('\') + '.failed'
    if (Test-Path -LiteralPath $failed) { Remove-Item -LiteralPath $failed -Recurse -Force }
    Move-Item -LiteralPath $Install -Destination $failed
    Move-Item -LiteralPath $Previous -Destination $Install
    Start-Process -FilePath $gui
    Finish "The new build exited with code $code; the previous build runs again." $true
    exit 1
}
Log "Updated to the build in $Install."
Remove-Item -LiteralPath $Stage -Recurse -Force -ErrorAction SilentlyContinue
if ($banner -and $banner.Visible) { Finish "Zerus build $Build is running." $false } elseif ($banner) { $banner.Close() }
