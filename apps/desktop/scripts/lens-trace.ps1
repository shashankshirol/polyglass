# Windows-only measurement of PERF-01/02/06 in the running app (M1-APP-10). Start the app with
# `--trace-timings=uncached` (every result after coming to rest is translated afresh; plain
# `--trace-timings` for the cached case) and its log redirected, e.g.
#   Start-Process target\release\polyglass.exe -ArgumentList '--trace-timings=uncached' `
#     -RedirectStandardOutput trace.log
# This script sizes the lens to a 600x300 interior (SPEC §5) at -X/-Y on a 100% monitor, shows
# the fixture's text (tests/fixtures/<Fixture>/truth.json, Segoe UI 12-16 px like the fixture
# images) in a window under it, then moves the lens by 1 px -Moves times, -Pause ms apart. Each
# move brings the lens to rest again; the log's `drawn` lines carry at_rest_ms (at rest -> drawn).
# Saved with a BOM: Windows PowerShell 5.1 reads BOM-less UTF-8 as ANSI.
param([string]$Fixture = "ui-dialog-sv", [int]$X = 300, [int]$Y = 300, [int]$Moves = 30,
      [int]$Pause = 1500)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class T {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int h2, uint f);
}
"@
[T]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
$h = [T]::FindWindow([NullString]::Value, "Polyglass lens")
if ($h -eq [IntPtr]::Zero) { "no lens window"; exit 1 }
$truth = Get-Content -Raw -Encoding UTF8 (Join-Path $PSScriptRoot "..\..\..\tests\fixtures\$Fixture\truth.json") | ConvertFrom-Json

# Lens layout (lens.rs LAYOUT): 6 px frame, 30 px toolbar above the interior.
$frame = 6; $toolbar = 30
$ix = $X + $frame; $iy = $Y + $frame + $toolbar
$f = New-Object System.Windows.Forms.Form
$f.FormBorderStyle = 'None'; $f.StartPosition = 'Manual'; $f.ShowInTaskbar = $false
$f.BackColor = [System.Drawing.Color]::FromArgb(243, 243, 243)
$f.Bounds = New-Object System.Drawing.Rectangle($ix, $iy, 600, 300)
$top = 12; $i = 0
foreach ($b in $truth.blocks) {
  $l = New-Object System.Windows.Forms.Label
  $l.Text = $b.text; $l.AutoSize = $true; $l.MaximumSize = New-Object System.Drawing.Size(576, 0)
  $l.Font = New-Object System.Drawing.Font("Segoe UI", [single](12 + $i % 5), [System.Drawing.GraphicsUnit]::Pixel)
  $l.ForeColor = [System.Drawing.Color]::FromArgb(27, 27, 27); $l.Location = New-Object System.Drawing.Point(12, $top)
  $f.Controls.Add($l); $top += $l.PreferredHeight + 10; $i++
}
$f.Show(); $f.Refresh()
# SWP_NOZORDER|SWP_NOACTIVATE; then the lens back on top (HWND_TOPMOST, no move/size): a covered
# webview may not paint, and the trace waits for the paint.
[T]::SetWindowPos($h, [IntPtr]::Zero, $X, $Y, 600 + 2 * $frame, 300 + $toolbar + 2 * $frame, 0x14) | Out-Null
[T]::SetWindowPos($h, [IntPtr](-1), 0, 0, 0, 0, 0x13) | Out-Null
function Pump([int]$ms) {
  $end = (Get-Date).AddMilliseconds($ms)
  while ((Get-Date) -lt $end) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 }
}
Pump 3000
for ($n = 1; $n -le $Moves; $n++) {
  [T]::SetWindowPos($h, [IntPtr]::Zero, $X + ($n % 2), $Y, 0, 0, 0x15) | Out-Null  # + SWP_NOSIZE
  Pump $Pause
}
$f.Close()
"moved the lens $Moves times over $Fixture"
