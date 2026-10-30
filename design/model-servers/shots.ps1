# Renders every view of index.html to shots/<view>-<theme>.png with headless
# Chrome or Edge at twice the CSS size. Uses its own temporary browser profile.
# -Only servers,settings renders just those views. Keep the sizes equal to V.<view>.w/h.
param([string]$Browser = "", [string[]]$Only = @())
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Browser) {
    $Browser = @("$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
                 "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe") |
        Where-Object { Test-Path $_ } | Select-Object -First 1
}
$views = [ordered]@{ servers = '1440,760'; session = '1440,820'; down = '1440,820'; settings = '1440,840'; states = '1440,1160' }
$out = Join-Path $here 'shots'
New-Item -ItemType Directory -Force $out | Out-Null
$profileDir = Join-Path ([IO.Path]::GetTempPath()) "zerus-model-servers-shots"
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
