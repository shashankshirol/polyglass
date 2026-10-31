# Windows-only manual check for the lens window (ADR-0002). Run while the app is running.
#   info                 window rect, DPI and extended styles (topmost, WS_EX_TRANSPARENT, ...)
#   hit  -X -Y           move the cursor there, report how fast click-through switched and which
#                        window is under the point; restores the cursor afterwards
#   drag -X -Y -DX -DY   press, move and release the left button (drag / resize tests)
#   key  -X <vk> [-Y 1]  press a virtual key (Y=1 adds Shift), e.g. 0x27 = Right arrow
# It moves the real cursor and sends real input, so don't touch the mouse while it runs.
# Coordinates are physical virtual-desktop pixels.
param([string]$Action = "info", [int]$X = 0, [int]$Y = 0, [int]$DX = 0, [int]$DY = 0, [string]$Out = "")
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class W {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtr(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte sc, uint f, IntPtr e);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
[W]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null  # per-monitor v2, physical coords
$h = [W]::FindWindow([NullString]::Value, "Polyglass lens")
if ($h -eq [IntPtr]::Zero) { "no lens window"; exit 1 }
function ExStyle { [int64][W]::GetWindowLongPtr($h, -20) }
function Info {
  $r = New-Object W+RECT; [W]::GetWindowRect($h, [ref]$r) | Out-Null
  $ex = ExStyle
  [pscustomobject]@{ Rect = "$($r.L),$($r.T) $($r.R-$r.L)x$($r.B-$r.T)"; Dpi = [W]::GetDpiForWindow($h); Visible = [W]::IsWindowVisible($h);
    Topmost = [bool]($ex -band 0x8); Transparent = [bool]($ex -band 0x20); Layered = [bool]($ex -band 0x80000); ToolWindow = [bool]($ex -band 0x80); AppWindow = [bool]($ex -band 0x40000) }
}
switch ($Action) {
  "info" { Info | Format-List | Out-String }
  "hit" {
    # Move the cursor to (X,Y), then time how long until WS_EX_TRANSPARENT settles, and what window is under the point.
    $p0 = New-Object W+POINT; [W]::GetCursorPos([ref]$p0) | Out-Null
    $before = [bool]((ExStyle) -band 0x20)
    [W]::SetCursorPos($X, $Y) | Out-Null
    $sw = [Diagnostics.Stopwatch]::StartNew(); $changedAt = $null
    while ($sw.ElapsedMilliseconds -lt 300) { $now = [bool]((ExStyle) -band 0x20); if ($now -ne $before -and -not $changedAt) { $changedAt = $sw.Elapsed.TotalMilliseconds }; Start-Sleep -Milliseconds 1 }
    $after = [bool]((ExStyle) -band 0x20)
    $pt = New-Object W+POINT; $pt.X = $X; $pt.Y = $Y
    $under = [W]::GetAncestor([W]::WindowFromPoint($pt), 2)
    $sb = New-Object Text.StringBuilder 256; [W]::GetWindowText($under, $sb, 256) | Out-Null
    [W]::SetCursorPos($p0.X, $p0.Y) | Out-Null
    "point=($X,$Y) transparent_before=$before after=$after switched_ms=$changedAt window_under='$($sb.ToString())' is_lens=$($under -eq $h)"
  }
  "drag" {
    # Press at (X,Y), move by (DX,DY) in steps, release.
    [W]::SetCursorPos($X, $Y) | Out-Null; Start-Sleep -Milliseconds 120
    [W]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 120
    for ($i = 1; $i -le 10; $i++) { [W]::SetCursorPos($X + [int]($DX*$i/10), $Y + [int]($DY*$i/10)) | Out-Null; Start-Sleep -Milliseconds 30 }
    Start-Sleep -Milliseconds 100; [W]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 200
    Info | Format-List | Out-String
  }
  "key" {
    # VK in $X, with Shift if $Y=1
    if ($Y -eq 1) { [W]::keybd_event(0x10, 0, 0, [IntPtr]::Zero) }
    [W]::keybd_event([byte]$X, 0, 1, [IntPtr]::Zero); [W]::keybd_event([byte]$X, 0, 3, [IntPtr]::Zero)
    if ($Y -eq 1) { [W]::keybd_event(0x10, 0, 2, [IntPtr]::Zero) }
    Start-Sleep -Milliseconds 200; Info | Format-List | Out-String
  }

}
