# Windows-only measurement of live-mode CPU (ADR-0003). Run while the app is running.
# Covers the lens with a window showing -Scene, waits -Settle seconds, then reports the CPU time
# the polyglass process used over -Seconds and how many pipeline cycles it processed (with -Log,
# the app's log file; start the app with RUST_LOG=info,pg::timings=debug and its output
# redirected there).
#   static  Swedish and English text that doesn't change
#   caret   the same text in a text box focused by a real click (the lens interior is
#           click-through): the Windows caret blinks. Moves the cursor and takes the keyboard focus.
#   video   the text over an animation repainted at ~30 fps
#   none    no window: whatever is under the lens
# Saved with a BOM: Windows PowerShell 5.1 reads BOM-less UTF-8 as ANSI.
param([string]$Scene = "static", [int]$Seconds = 10, [int]$Settle = 3, [string]$Log = "",
      [string]$Process = "polyglass")
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class C {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, IntPtr e);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
[C]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
$h = [C]::FindWindow([NullString]::Value, "Polyglass lens")
if ($h -eq [IntPtr]::Zero) { "no lens window"; exit 1 }
$r = New-Object C+RECT; [C]::GetWindowRect($h, [ref]$r) | Out-Null
$proc = Get-Process $Process -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) { "no $Process process"; exit 1 }

$text = "Vill du spara ändringarna innan du stänger?`r`n`r`nYour changes have been saved."
$f = $null
if ($Scene -ne "none") {
  $f = New-Object System.Windows.Forms.Form
  $f.FormBorderStyle = 'None'; $f.StartPosition = 'Manual'; $f.ShowInTaskbar = $false; $f.TopMost = $true
  $f.BackColor = [System.Drawing.Color]::White
  $f.Bounds = New-Object System.Drawing.Rectangle($r.L, $r.T, ($r.R - $r.L), ($r.B - $r.T))
  $font = New-Object System.Drawing.Font("Segoe UI", 12)
  if ($Scene -eq "caret") {
    $tb = New-Object System.Windows.Forms.TextBox
    $tb.Multiline = $true; $tb.BorderStyle = 'None'; $tb.Font = $font; $tb.Text = $text
    # Below the lens toolbar, like the text of the other scenes.
    $tb.Location = New-Object System.Drawing.Point(12, 48)
    $tb.Size = New-Object System.Drawing.Size(($r.R - $r.L - 24), ($r.B - $r.T - 60))
    $f.Controls.Add($tb)
  } else {
    $script:frame = 0
    $f.Add_Paint({
      param($s, $e)
      $g = $e.Graphics
      if ($Scene -eq "video") {
        for ($x = 0; $x -lt $f.ClientSize.Width; $x += 8) {
          $v = ($x + $script:frame * 7) % 256
          $b = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb($v, [int]($v / 2) + 64, 255 - $v))
          $g.FillRectangle($b, $x, 0, 8, $f.ClientSize.Height); $b.Dispose()
        }
      }
      $g.TextRenderingHint = 'ClearTypeGridFit'
      $g.DrawString($text, $font, [System.Drawing.Brushes]::Black, 12, 48)
    })
    if ($Scene -eq "video") {
      $t = New-Object System.Windows.Forms.Timer
      $t.Interval = 33
      $t.Add_Tick({ $script:frame++; $f.Invalidate() })
      $t.Start()
    }
  }
  $f.Show(); $f.Activate()
  if ($Scene -eq "caret") {
    # A real click: Windows doesn't let a background process take the focus otherwise.
    [System.Windows.Forms.Application]::DoEvents()
    $p0 = New-Object C+POINT; [C]::GetCursorPos([ref]$p0) | Out-Null
    [C]::SetCursorPos($r.L + 60, [int](($r.T + $r.B) / 2)) | Out-Null
    [C]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero); [C]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero)
    [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 100
    [C]::SetCursorPos($p0.X, $p0.Y) | Out-Null
    [System.Windows.Forms.Application]::DoEvents()
    "caret: text box focused=$($tb.Focused)"
  }
}

function Pump([double]$s) {
  $end = (Get-Date).AddSeconds($s)
  while ((Get-Date) -lt $end) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 10 }
}
function Cycles { if ($Log) { (Select-String -Path $Log -Pattern "pg::timings" -SimpleMatch).Count } else { 0 } }

Pump $Settle
$proc.Refresh(); $cpu0 = $proc.TotalProcessorTime; $c0 = Cycles
Pump $Seconds
$proc.Refresh(); $cpu1 = $proc.TotalProcessorTime; $c1 = Cycles
if ($f) { $f.Close() }
$ms = [int]($cpu1 - $cpu0).TotalMilliseconds
"scene=$Scene lens=$($r.L),$($r.T) $($r.R - $r.L)x$($r.B - $r.T) cpu_ms=$ms over ${Seconds}s cycles=$($c1 - $c0)"
