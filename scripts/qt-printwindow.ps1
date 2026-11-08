# Task 205: save the visible top-level windows of one process with PrintWindow
# (PW_RENDERFULLCONTENT), so a capture never contains another program's window
# and needs no OS input or screen copy.
#   pwsh scripts/qt-printwindow.ps1 -ProcessId <pid> -Out <folder> -Name <prefix>
param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Name = "qt",
    [int]$MinWidth = 200
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class ZerusPrintWindow {
    public delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public static List<IntPtr> Windows(uint pid) {
        var found = new List<IntPtr>();
        EnumWindows((hwnd, l) => {
            uint owner; GetWindowThreadProcessId(hwnd, out owner);
            if (owner == pid && IsWindowVisible(hwnd)) found.Add(hwnd);
            return true;
        }, IntPtr.Zero);
        return found;
    }
    public static string Title(IntPtr hwnd) { var b = new StringBuilder(512); GetWindowText(hwnd, b, b.Capacity); return b.ToString(); }
}
"@
New-Item -ItemType Directory -Force $Out | Out-Null
$i = 0
foreach ($hwnd in [ZerusPrintWindow]::Windows([uint32]$ProcessId)) {
    $rect = New-Object ZerusPrintWindow+RECT
    [void][ZerusPrintWindow]::GetWindowRect($hwnd, [ref]$rect)
    $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
    if ($width -lt $MinWidth -or $height -lt 100) { continue }
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $hdc = $graphics.GetHdc()
    $ok = [ZerusPrintWindow]::PrintWindow($hwnd, $hdc, 2)
    $graphics.ReleaseHdc($hdc); $graphics.Dispose()
    $suffix = if ($i -eq 0) { "" } else { "-$i" }
    $file = Join-Path $Out "$Name$suffix.png"
    $bitmap.Save($file, [System.Drawing.Imaging.ImageFormat]::Png); $bitmap.Dispose()
    [pscustomobject]@{ file = $file; title = [ZerusPrintWindow]::Title($hwnd); width = $width; height = $height; printed = $ok }
    $i++
}
if ($i -eq 0) { Write-Error "no visible window of process $ProcessId" }
