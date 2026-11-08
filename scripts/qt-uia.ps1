# Task 205: steer one process's Qt windows through UI Automation patterns
# (Invoke, SelectionItem, ExpandCollapse), never mouse or keyboard input, so a
# side-by-side capture cannot touch another program.
#   pwsh scripts/qt-uia.ps1 -ProcessId <pid> -List [-Depth n]
#   pwsh scripts/qt-uia.ps1 -ProcessId <pid> -Press "<name>" [-Type Button|TabItem|ListItem|...] [-Index n]
#   pwsh scripts/qt-uia.ps1 -ProcessId <pid> -Window "<title part>" ...   (limits to one window)
param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [switch]$List,
    [int]$Depth = 6,
    [string]$Press,
    [string]$Type,
    [int]$Index = 0,
    [string]$Window,
    [string]$SetValue,
    [double]$ScrollPercent,
    [switch]$Collapse,
    [switch]$Close,
    [switch]$Maximize
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$root = $A::RootElement
$byPid = New-Object System.Windows.Automation.PropertyCondition($A::ProcessIdProperty, $ProcessId)
$windows = @($root.FindAll([System.Windows.Automation.TreeScope]::Children, $byPid))
if ($Window) { $windows = @($windows | Where-Object { $_.Current.Name -like "*$Window*" }) }
if ($windows.Count -eq 0) { throw "no window of process $ProcessId" }

function Show($element, $level) {
    if ($level -gt $Depth) { return }
    $c = $element.Current
    $kind = $c.ControlType.ProgrammaticName -replace '^ControlType\.', ''
    $flag = if ($c.IsOffscreen) { " (offscreen)" } else { "" }
    "{0}{1} '{2}' [{3}]{4}" -f ('  ' * $level), $kind, $c.Name, $c.AutomationId, $flag
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $child = $walker.GetFirstChild($element)
    while ($child) { Show $child ($level + 1); $child = $walker.GetNextSibling($child) }
}

if ($List) { foreach ($w in $windows) { Show $w 0 }; return }

# -Maximize: the process's main window fills its screen (WindowPattern).
if ($Maximize) {
    $pattern = $null
    $w = $windows | Where-Object { $_.Current.AutomationId -like '*sessionsWindow' } | Select-Object -First 1
    if (-not $w) { $w = $windows[0] }
    if ($w.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$pattern)) {
        $pattern.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized); "maximized"
    }
    return
}

# -Close -Window <automation id part>: closes a popup or dialog of the process.
if ($Close) {
    foreach ($w in @($root.FindAll([System.Windows.Automation.TreeScope]::Children, $byPid))) {
        if ($Window -and $w.Current.AutomationId -notlike "*$Window*" -and $w.Current.Name -notlike "*$Window*") { continue }
        $pattern = $null
        if ($w.TryGetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern, [ref]$pattern)) { $pattern.Close(); "closed $($w.Current.AutomationId)" }
    }
    return
}

if ($Press) {
    $found = @()
    foreach ($w in $windows) {
        $all = $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($e in $all) {
            $c = $e.Current
            $kind = $c.ControlType.ProgrammaticName -replace '^ControlType\.', ''
            # A name, its first line (list rows add details below it) or an
            # object name (the AutomationId is the dotted object path).
            $named = $c.Name -eq $Press -or ($c.Name -split "`n")[0] -eq $Press -or $c.AutomationId -eq $Press -or $c.AutomationId.EndsWith(".$Press")
            if ($named -and (-not $Type -or $kind -eq $Type)) { $found += $e }
        }
    }
    if ($found.Count -le $Index) { throw "no control '$Press' ($Type) number $Index; found $($found.Count)" }
    $e = $found[$Index]
    $pattern = $null
    if ($Collapse) {
        if (-not $e.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$pattern)) {
            $names = ($e.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName }) -join ', '
            throw "'$Press' has no ExpandCollapse pattern (has: $names)"
        }
        $pattern.Collapse(); "collapsed $Press"; return
    }
    if ($PSBoundParameters.ContainsKey('ScrollPercent')) {
        if (-not $e.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern, [ref]$pattern)) { throw "'$Press' has no Scroll pattern" }
        $pattern.SetScrollPercent(-1, $ScrollPercent); "scrolled $Press to $ScrollPercent"; return
    }
    if ($PSBoundParameters.ContainsKey('SetValue')) {
        if (-not $e.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) { throw "'$Press' has no Value pattern" }
        $pattern.SetValue($SetValue); "set $Press"; return
    }
    if ($e.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { $pattern.Invoke(); "invoked $Press"; return }
    if ($e.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { $pattern.Select(); "selected $Press"; return }
    if ($e.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) { $pattern.Toggle(); "toggled $Press"; return }
    if ($e.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$pattern)) { $pattern.Expand(); "expanded $Press"; return }
    throw "'$Press' has no Invoke, SelectionItem, Toggle or ExpandCollapse pattern"
}
