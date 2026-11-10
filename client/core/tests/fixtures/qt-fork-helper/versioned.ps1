param([int]$ParentPid, [string]$Root, [string]$Link, [string]$New, [string]$Target, [string]$Stage, [int]$Build, [switch]$Quiet)
$ErrorActionPreference = 'Stop'
$log = Join-Path $Root 'update.log'
function Log([string]$message) { try { Add-Content -LiteralPath $log -Value ("{0:u} {1}" -f (Get-Date), $message) } catch { } }

# -Quiet is for tests only; the updaters never pass it. No banner shows, and
# every build starts without a window.
if ($Quiet) { $PSDefaultParameterValues['Start-Process:WindowStyle'] = 'Hidden' }
$banner = $null
try {
    if ($Quiet) { throw 'quiet' }
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    try { Add-Type -Namespace ZerusUpdate -Name Dpi -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();'; [void][ZerusUpdate.Dpi]::SetProcessDPIAware() } catch { }
    $graphics = [System.Drawing.Graphics]::FromHwnd([IntPtr]::Zero); $scale = $graphics.DpiX / 96; $graphics.Dispose()
    function Px([double]$value) { [int][Math]::Round($value * $scale) }
    function Rgb([int]$r, [int]$g, [int]$b) { [System.Drawing.Color]::FromArgb($r, $g, $b) }
    $banner = New-Object System.Windows.Forms.Form
    $banner.Text = 'Zerus update'
    $banner.FormBorderStyle = 'None'; $banner.TopMost = $true; $banner.ShowInTaskbar = $true; $banner.StartPosition = 'Manual'
    try { $banner.Icon = [System.Drawing.Icon]::ExtractAssociatedIcon((Join-Path $New 'hgs-tray.exe')) } catch { }
    $banner.BackColor = Rgb 58 61 68
    $banner.Padding = New-Object System.Windows.Forms.Padding(1)
    $banner.ClientSize = New-Object System.Drawing.Size((Px 380), (Px 104))
    $area = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
    $banner.Location = New-Object System.Drawing.Point(($area.Right - $banner.Width - (Px 16)), ($area.Bottom - $banner.Height - (Px 16)))
    $body = New-Object System.Windows.Forms.Panel
    $body.Dock = 'Fill'; $body.BackColor = Rgb 31 33 38
    $banner.Controls.Add($body)
    $heading = New-Object System.Windows.Forms.Label
    $heading.Text = "Updating Zerus to build $Build"; $heading.ForeColor = Rgb 236 238 241
    $heading.Font = New-Object System.Drawing.Font('Segoe UI Semibold', 10)
    $heading.SetBounds((Px 16), (Px 12), (Px 346), (Px 22))
    $status = New-Object System.Windows.Forms.Label
    $status.ForeColor = Rgb 178 182 189
    $status.Font = New-Object System.Drawing.Font('Segoe UI', 9)
    $status.SetBounds((Px 16), (Px 36), (Px 346), (Px 38))
    $track = New-Object System.Windows.Forms.Panel
    $track.BackColor = Rgb 51 54 61
    $track.SetBounds((Px 16), (Px 82), (Px 346), (Px 6))
    $fill = New-Object System.Windows.Forms.Panel
    $fill.BackColor = Rgb 76 141 255
    $fill.SetBounds(0, 0, 0, (Px 6))
    $track.Controls.Add($fill)
    $body.Controls.AddRange(@($heading, $status, $track))
    foreach ($control in @($banner, $body, $heading, $status, $track, $fill)) { $control.Add_Click({ $banner.Hide() }) }
    $banner.Show()
    [System.Windows.Forms.Application]::DoEvents()
} catch { $banner = $null }
# Waits while the banner stays responsive.
function Pump([int]$milliseconds) {
    $until = (Get-Date).AddMilliseconds($milliseconds)
    do {
        if ($banner -and $banner.Visible) { [System.Windows.Forms.Application]::DoEvents() }
        Start-Sleep -Milliseconds 50
    } while ((Get-Date) -lt $until)
}
function Step([string]$text, [int]$percent) {
    if (-not $banner) { return }
    $status.Text = $text
    $fill.Width = [int]($track.Width * [Math]::Min(100, [Math]::Max(0, $percent)) / 100)
    $banner.Refresh()
    [System.Windows.Forms.Application]::DoEvents()
}
# The result: a failure is logged and stays on screen longer, even after the
# banner was hidden.
function Finish([string]$text, [bool]$failed) {
    if ($failed) { Log $text }
    if (-not $banner) { return }
    if ($failed) {
        $fill.BackColor = Rgb 224 82 82; $status.ForeColor = Rgb 255 160 160
        if (-not $banner.Visible) { $banner.Show() }
    }
    Step $text 100
    Pump $(if ($failed) { 10000 } else { 2500 })
    $banner.Close()
}
function WaitExit([int]$id, [int]$seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Process -Id $id -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Pump 200 }
    -not (Get-Process -Id $id -ErrorAction SilentlyContinue)
}
# The new build counts as started once it runs 15 seconds. The banner leaves
# as soon as its window is up; a build that exits later still reports.
function Started($process) {
    for ($second = 1; $second -le 15; $second++) {
        Pump 1000
        if ($process.HasExited) { return $false }
        $process.Refresh()
        if ($banner -and $banner.Visible -and $second -ge 2 -and $process.MainWindowHandle -ne [IntPtr]::Zero) {
            Step "Zerus build $Build is running." 100; Pump 1200; $banner.Hide()
        } else { Step 'Starting Zerus...' (70 + 2 * $second) }
    }
    -not $process.HasExited
}

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
Get-ChildItem -LiteralPath $Root -Directory -Filter 'app-*' | Where-Object { $_.FullName -ne $Target -and $_.FullName -ne $previous } | ForEach-Object {
    try { Remove-Item -LiteralPath $_.FullName -Recurse -Force } catch { Log "Kept $($_.FullName): $_" }
}
if ($banner -and $banner.Visible) { Finish "Zerus build $Build is running." $false } elseif ($banner) { $banner.Close() }
