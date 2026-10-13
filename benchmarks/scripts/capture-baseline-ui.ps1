# Launches Bowser (HTML chrome) with an isolated LOCALAPPDATA, one fresh process per scenario,
# drives it with keystrokes/clicks and captures window-rect screenshots.
param(
  [string]$Exe = "$PSScriptRoot\..\..\target-baseline\release\bowser.exe",
  [string]$Out = "$PSScriptRoot\..\results\baseline-ui",
  [string]$Url = "https://en.wikipedia.org/wiki/Web_browser",
  [string]$Only = ""
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern void mouse_event(uint f,int x,int y,uint d,int e);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
}
"@
[W]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path; $Exe = (Resolve-Path $Exe).Path
$dataRoot = Join-Path $env:TEMP "bowser-baseline-data"; $script:n = 0

$env:BOWSER_UI = 'html'
$script:p = $null
$global:h = [IntPtr]::Zero

function Launch {
  $script:n++; $script:data = Join-Path $dataRoot ("run" + $script:n + "-" + (Get-Random)); $env:LOCALAPPDATA = $script:data
  New-Item -ItemType Directory -Force $script:data | Out-Null
  $script:p = Start-Process $Exe -ArgumentList @('--open-url', $Url) -PassThru
  $global:h = [IntPtr]::Zero
  for ($i=0; $i -lt 60 -and $global:h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500; $script:p.Refresh(); $global:h = $script:p.MainWindowHandle }
  if ($global:h -eq [IntPtr]::Zero) { throw "No window" }
  [W]::ShowWindow($global:h, 9) | Out-Null
  Start-Sleep -Seconds 6
}
function Quit { taskkill /PID $script:p.Id /T /F | Out-Null; Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | ? { $_.CommandLine -like "*$($script:data)*" } | % { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }; Start-Sleep 2 }
function Focus { [W]::SetForegroundWindow($global:h) | Out-Null; Start-Sleep -Milliseconds 300 }
function Shot($name) {
  Start-Sleep -Milliseconds 900
  $r = New-Object W+RECT; [W]::GetWindowRect($global:h, [ref]$r) | Out-Null
  $w = $r.R-$r.L; $ht = $r.B-$r.T
  $bmp = New-Object Drawing.Bitmap $w, $ht
  $g = [Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save("$Out\$name.png", [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
  Write-Host "saved $name ($w x $ht)"
}
function Keys($k, $wait=700) { Focus; [Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds $wait }
function ClickAt($rx, $ry) {
  $r = New-Object W+RECT; [W]::GetWindowRect($global:h, [ref]$r) | Out-Null
  [W]::SetCursorPos($r.L+$rx, $r.T+$ry) | Out-Null; Start-Sleep -Milliseconds 200
  [W]::mouse_event(2,0,0,0,0); [W]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds 700
}
function Scenario($name, [scriptblock]$act) { if ($Only -and $name -notlike "*$Only*") { return }; Launch; Focus; & $act; Shot $name; Quit }

Scenario '01-main-horizontal' { }
Scenario '02-address-focused' { Keys '^l' 600 }
Scenario '03-new-tab' { Keys '^t' 1200 }
Scenario '04-two-tabs' { Keys '^t' 800; Keys 'https://example.com{ENTER}' 3500 }
Scenario '05-vertical-sidebar' { Keys '^t' 800; Keys 'https://example.com{ENTER}' 3000; Keys '^+v' 2500 }
Scenario '06-command-palette' { Keys '^+k' 1200 }
Scenario '07-history' { Keys '^h' 1500 }
Scenario '08-downloads' { Keys '^j' 1500 }
Scenario '09-notes-tasks' { Keys '^+n' 1500 }
Scenario '10-passwords' { Keys '^,' 1500 }
Scenario '11-privacy-dashboard' { Keys '^+{DELETE}' 1500 }
Scenario '12-extensions' { Keys '^+e' 1500 }
Scenario '13-add-bookmark' { Keys '^d' 1500 }
Scenario '14-shift-f10' { Keys '+{F10}' 1200 }
Scenario "15-main-menu-click" { Focus; ClickAt 812 23; Start-Sleep 2 }
Scenario "17-site-info" { ClickAt 190 23; Start-Sleep 1 }
Scenario "18-tab-context" { $r = New-Object W+RECT; [W]::GetWindowRect($global:h, [ref]$r) | Out-Null; [W]::SetCursorPos($r.L+500, $r.T+54) | Out-Null; Start-Sleep -Milliseconds 300; [W]::mouse_event(8,0,0,0,0); [W]::mouse_event(16,0,0,0,0); Start-Sleep 1 }
Scenario '16-split-view' { Keys '^+\' 2000 }
Write-Host done
