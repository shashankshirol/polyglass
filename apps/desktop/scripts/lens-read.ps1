# Windows-only manual check for live mode (ADR-0003). Run while the app is running.
# Opens a borderless window with known Swedish and English text at -X/-Y (physical px; put it
# under the lens interior), waits -Seconds for the pipeline, then prints the lens webview's text
# via UI Automation. The lens is excluded from screen capture, so screenshots can't show it.
# Expected in live mode with the models installed: "Do you want to save the changes before you
# close?" and the English line unchanged; without models, the model-missing message and the ⬇
# button. Saved with a BOM: Windows PowerShell 5.1 reads BOM-less UTF-8 as ANSI.
param([int]$X, [int]$Y, [int]$W = 468, [int]$H = 164, [int]$Seconds = 3)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class D { [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v); }
"@
[D]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
$f = New-Object System.Windows.Forms.Form
$f.FormBorderStyle = 'None'; $f.StartPosition = 'Manual'; $f.ShowInTaskbar = $false
$f.BackColor = [System.Drawing.Color]::White
$f.Bounds = New-Object System.Drawing.Rectangle($X, $Y, $W, $H)
$lines = @("Vill du spara ändringarna innan du stänger?", "", "Your changes have been saved.")
$top = 12
foreach ($t in $lines) {
  $l = New-Object System.Windows.Forms.Label
  $l.Text = $t; $l.AutoSize = $true; $l.Font = New-Object System.Drawing.Font("Segoe UI", 12)
  $l.ForeColor = [System.Drawing.Color]::Black; $l.Location = New-Object System.Drawing.Point(12, $top)
  $f.Controls.Add($l); $top += 34
}
$f.Show(); $f.Refresh()
$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 50 }
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, "Polyglass lens")
$lens = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $cond)
if (-not $lens) { "no lens in UIA tree" } else {
  $all = $lens.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  $names = foreach ($e in $all) { $n = $e.Current.Name; if ($n) { "$($e.Current.ControlType.ProgrammaticName): $n" } }
  $names | Select-Object -Unique
}
$f.Close()
