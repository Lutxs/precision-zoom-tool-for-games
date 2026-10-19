# Captures the settings window into docs\settings.png (the README screenshot).
# Shows the current registry settings. Builds first unless -NoBuild.
param([string]$Out = 'docs\settings.png', [switch]$NoBuild)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

if (Get-Process precision-zoom -ErrorAction SilentlyContinue) {
    throw 'Precision Zoom is running. Exit it first (tray icon -> Exit).'
}
if (-not $NoBuild) {
    cargo build --release; if ($LASTEXITCODE) { throw 'build failed' }
}

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class PzShot {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int size);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@

# Without per-monitor DPI awareness, GetWindowRect returns scaled-down sizes
# on high-DPI displays while the DWM frame bounds stay physical.
[void][PzShot]::SetThreadDpiAwarenessContext([IntPtr](-4))

$proc = Start-Process target\release\precision-zoom.exe -ArgumentList '--settings' -PassThru
try {
    # The egui window is the visible "Precision Zoom" window whose class isn't
    # PrecisionZoom*. winit also owns a visible 22x22 helper window, with no title.
    $hwnd = [IntPtr]::Zero
    for ($i = 0; $i -lt 50 -and $hwnd -eq [IntPtr]::Zero; $i++) {
        Start-Sleep -Milliseconds 200
        [void][PzShot]::EnumWindows({
            param($h, $l)
            $owner = 0; [void][PzShot]::GetWindowThreadProcessId($h, [ref]$owner)
            if ($owner -eq $proc.Id -and [PzShot]::IsWindowVisible($h)) {
                $class = New-Object System.Text.StringBuilder 256
                [void][PzShot]::GetClassName($h, $class, 256)
                $title = New-Object System.Text.StringBuilder 256
                [void][PzShot]::GetWindowText($h, $title, 256)
                if (-not $class.ToString().StartsWith('PrecisionZoom') -and $title.ToString() -eq 'Precision Zoom') {
                    $script:hwnd = $h; return $false
                }
            }
            return $true
        }, [IntPtr]::Zero)
    }
    if ($hwnd -eq [IntPtr]::Zero) { throw 'settings window did not appear' }
    Start-Sleep -Seconds 1 # let egui paint its first frames

    # PrintWindow renders the window itself, even if other windows cover it.
    # Never capture the screen instead: it would pick up whatever else is shown.
    $win = New-Object PzShot+RECT; [void][PzShot]::GetWindowRect($hwnd, [ref]$win)
    $bmp = New-Object System.Drawing.Bitmap ($win.R - $win.L), ($win.B - $win.T)
    $g = [System.Drawing.Graphics]::FromImage($bmp); $hdc = $g.GetHdc()
    [void][PzShot]::PrintWindow($hwnd, $hdc, 2) # PW_RENDERFULLCONTENT
    $g.ReleaseHdc($hdc); $g.Dispose()

    # Crop the invisible resize borders (DWMWA_EXTENDED_FRAME_BOUNDS).
    $frame = New-Object PzShot+RECT; [void][PzShot]::DwmGetWindowAttribute($hwnd, 9, [ref]$frame, 16)
    $crop = New-Object System.Drawing.Rectangle ($frame.L - $win.L), ($frame.T - $win.T), ($frame.R - $frame.L), ($frame.B - $frame.T)
    $shot = $bmp.Clone($crop, $bmp.PixelFormat)
    $shot.Save((Join-Path $PSScriptRoot $Out), [System.Drawing.Imaging.ImageFormat]::Png)
    $shot.Dispose(); $bmp.Dispose()
    Write-Host "Saved $Out ($($crop.Width)x$($crop.Height))"
} finally {
    # Tray Exit (WM_COMMAND id 3) to the controller window.
    $controller = [PzShot]::FindWindow('PrecisionZoomController', [NullString]::Value)
    if ($controller -ne [IntPtr]::Zero) { [void][PzShot]::PostMessage($controller, 0x111, [IntPtr]3, [IntPtr]::Zero) }
    if (-not $proc.WaitForExit(5000)) { Stop-Process -Id $proc.Id }
}
