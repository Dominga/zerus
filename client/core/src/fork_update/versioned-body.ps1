
function Point([string]$folder) {
    # rmdir removes the junction itself and never the build it points to.
    if (Test-Path -LiteralPath $Link) { cmd /c rmdir "$Link" | Out-Null }
    New-Item -ItemType Junction -Path $Link -Target $folder | Out-Null
}
$gui = Join-Path $Link 'hgs-tray.exe'
$previous = ([string]((Get-Item -LiteralPath $Link).Target | Select-Object -First 1)).TrimEnd('\')
Step 'Waiting for Zerus to close...' 10
if (-not (WaitExit $ParentPid 120)) { Finish 'Zerus did not quit; the update waits for the next restart.' $true; exit 1 }
Step 'Switching to the new build...' 50
try {
    if (Test-Path -LiteralPath $Target) { Remove-Item -LiteralPath $Target -Recurse -Force }
    Move-Item -LiteralPath $New -Destination $Target
    Point $Target
} catch {
    $problem = "$_"
    if (-not (Test-Path -LiteralPath $Link)) { Point $previous }
    Start-Process -FilePath $gui; Finish "Could not switch to the new build: $problem" $true; exit 1
}
Step 'Starting Zerus...' 70
$started = Start-Process -FilePath $gui -PassThru
if (-not (Started $started)) {
    $code = $started.ExitCode
    Point $previous
    Start-Process -FilePath $gui
    Finish "The new build exited with code $code; current points to $previous again." $true
    exit 1
}
Log "Updated: current is $Target, the previous build stays in $previous."
Remove-Item -LiteralPath $Stage -Recurse -Force -ErrorAction SilentlyContinue
Get-ChildItem -LiteralPath $Root -Directory -Filter 'app-*' | Where-Object { $_.Name -ne (Split-Path -Leaf $Target) -and $_.Name -ne (Split-Path -Leaf $previous) } | ForEach-Object {
    try { Remove-Item -LiteralPath $_.FullName -Recurse -Force } catch { Log "Kept $($_.FullName): $_" }
}
if ($banner -and $banner.Visible) { Finish "Zerus build $Build is running." $false } elseif ($banner) { $banner.Close() }
