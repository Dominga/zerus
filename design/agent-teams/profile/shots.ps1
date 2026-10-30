# Renders every view of index.html to shots/<view>-<theme>.png with headless
# Chrome or Edge at twice the CSS size. Uses its own temporary browser profile.
# -Only profile,heat renders just those views. Keep the sizes equal to V.<view>.w/h.
param([string]$Browser = "", [string[]]$Only = @())
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Browser) {
    $Browser = @("$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
                 "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe") |
        Where-Object { Test-Path $_ } | Select-Object -First 1
}
$views = [ordered]@{ profile = '1600,820'; tokens = '1600,640'; members = '1600,560'; compare = '1600,640'; heat = '1600,860'; heatboard = '1600,520' }
$out = Join-Path $here 'shots'
New-Item -ItemType Directory -Force $out | Out-Null
$profileDir = Join-Path ([IO.Path]::GetTempPath()) "zerus-profile-shots"
$page = ([Uri](Join-Path $here 'index.html')).AbsoluteUri
foreach ($view in $views.Keys) {
    if ($Only.Count -and $view -notin $Only) { continue }
    foreach ($theme in 'dark', 'light') {
        $png = Join-Path $out "$view-$theme.png"
        & $Browser --headless=new --disable-gpu --no-first-run --user-data-dir="$profileDir" `
            --force-device-scale-factor=2 --window-size=$($views[$view]) --screenshot="$png" `
            --virtual-time-budget=2000 "$page`?view=$view&theme=$theme" 2>$null | Out-Null
        Write-Output "$view-$theme.png"
    }
}
