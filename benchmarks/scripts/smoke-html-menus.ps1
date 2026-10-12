# Smoke test for HTML popup menus: launches Bowser (HTML chrome) with an isolated LOCALAPPDATA,
# opens the main menu and a tab context menu, records which window class rendered each menu
# (BowserHtmlPopup = HTML, BowserMainMenu = native fallback), captures screenshots and checks a pick.
param(
  [string]$Exe = "$PSScriptRoot\..\..\target-menus\release\bowser.exe",
  [string]$Out = "$PSScriptRoot\..\results\menus",
  [string]$Url = "https://example.com/",
  [string]$Step = "all"
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices; using System.Collections.Generic;
public static class M {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool r);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern void mouse_event(uint f,int x,int y,uint d,int e);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  public static int CountClass(uint pid, string cls) {
    int n = 0;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); var s = new StringBuilder(128); GetClassName(h, s, 128); if (p == pid && s.ToString() == cls) n++; return true; }, IntPtr.Zero);
    return n;
  }
  public static List<string> Visible(uint pid) {
    var found = new List<string>();
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) {
        var s = new StringBuilder(128); GetClassName(h, s, 128);
        RECT r; GetWindowRect(h, out r);
        found.Add(s.ToString() + " " + r.L + "," + r.T + "," + (r.R - r.L) + "x" + (r.B - r.T) + (h == GetForegroundWindow() ? " fg" : ""));
      }
      return true;
    }, IntPtr.Zero);
    return found;
  }
}
"@
[M]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path; $Exe = (Resolve-Path $Exe).Path
$data = Join-Path $env:TEMP ("bowser-menus-" + (Get-Random))
New-Item -ItemType Directory -Force $data | Out-Null
$env:LOCALAPPDATA = $data
$env:BOWSER_UI = 'html'

$p = Start-Process $Exe -ArgumentList @('--data-dir', "`"$data`"", '--open-url', $Url) -PassThru
$h = [IntPtr]::Zero
for ($i = 0; $i -lt 60 -and $h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500; $p.Refresh(); $h = $p.MainWindowHandle }
if ($h -eq [IntPtr]::Zero) { throw "No window" }
[M]::ShowWindow($h, 9) | Out-Null
[M]::MoveWindow($h, 40, 40, 1200, 800, $true) | Out-Null
# The popup host prewarms ~2 s after the toolbar is live; allow for a slow, loaded machine.
$wait = 20; if ($env:START_WAIT) { $wait = [int]$env:START_WAIT }; Start-Sleep -Seconds $wait

function Focus { [M]::SetForegroundWindow($h) | Out-Null; Start-Sleep -Milliseconds 300 }
function Windows($label) { $list = [M]::Visible([uint32]$p.Id); Write-Host "[$label] visible: $($list -join ' | ')"; return $list }
function Shot($name) {
  Start-Sleep -Milliseconds 700
  # Screen copy of the browser window's area: the popup is a separate top-level window over it.
  $r = New-Object M+RECT; [M]::GetWindowRect($h, [ref]$r) | Out-Null
  $w = $r.R - $r.L; $ht = $r.B - $r.T
  $bmp = New-Object Drawing.Bitmap $w, $ht
  $g = [Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save("$Out\$name.png", [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
  Write-Host "saved $name.png"
}
function ShotPopup($name) {
  # PrintWindow of the HTML popup itself (PW_RENDERFULLCONTENT so the WebView2 content is included).
  $popup = [IntPtr]::Zero
  $cb = [M+EnumProc]{ param($hh, $l)
    $s = New-Object Text.StringBuilder 128; [M]::GetClassName($hh, $s, 128) | Out-Null
    $pp = 0; [M]::GetWindowThreadProcessId($hh, [ref]$pp) | Out-Null
    if ($pp -eq $p.Id -and $s.ToString() -eq 'BowserHtmlPopup' -and [M]::IsWindowVisible($hh)) { $script:popup = $hh }
    return $true }
  $script:popup = [IntPtr]::Zero
  [M]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
  if ($script:popup -eq [IntPtr]::Zero) { Write-Host "no visible HTML popup for $name"; return }
  $r = New-Object M+RECT; [M]::GetWindowRect($script:popup, [ref]$r) | Out-Null
  $bmp = New-Object Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
  $g = [Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc()
  [M]::PrintWindow($script:popup, $dc, 2) | Out-Null
  $g.ReleaseHdc($dc); $bmp.Save("$Out\$name.png", [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
  Write-Host "saved $name.png (PrintWindow)"
}
function Keys($k, $wait = 600) { [Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds $wait }
function Click($rx, $ry, $right = $false) {
  $r = New-Object M+RECT; [M]::GetWindowRect($h, [ref]$r) | Out-Null
  [M]::SetCursorPos($r.L + $rx, $r.T + $ry) | Out-Null; Start-Sleep -Milliseconds 200
  if ($right) { [M]::mouse_event(8,0,0,0,0); [M]::mouse_event(16,0,0,0,0) } else { [M]::mouse_event(2,0,0,0,0); [M]::mouse_event(4,0,0,0,0) }
  Start-Sleep -Milliseconds 900
}

try {
  Focus
  Shot '00-start'
  Windows 'start' | Out-Null
  if ($Step -eq 'start') { return }
  if ($Step -eq 'wait') {
    foreach ($i in 1..40) { Write-Host ("+{0}s popup windows: {1}" -f $i, [M]::CountClass([uint32]$p.Id, 'BowserHtmlPopup')); Start-Sleep -Seconds 1 }
    return
  }
  if ($Step -eq 'probe') {
    Write-Host ("popup windows (any visibility): " + [M]::CountClass([uint32]$p.Id, 'BowserHtmlPopup'))
    foreach ($round in 1..3) {
      Click ([int]$env:MENU_X) ([int]$env:MENU_Y)
      $t0 = Get-Date
      for ($i = 0; $i -lt 12; $i++) { $v = [M]::Visible([uint32]$p.Id) -join ' | '; Write-Host ("round $round +{0:n0}ms: $v" -f ((Get-Date) - $t0).TotalMilliseconds); Start-Sleep -Milliseconds 250 }
    }
    return
  }

  # Main menu: the three-dot button sits at the right end of the toolbar row.
  Click ([int]$env:MENU_X) ([int]$env:MENU_Y)
  Windows 'main menu' | Out-Null
  Shot '01-main-menu'
  ShotPopup '01-main-menu-popup'
  # Keyboard: typeahead to "Workspaces" (drill-in page), then back out with Left, then Escape.
  Keys 'w' 400; Keys '{RIGHT}' 900
  Shot '02-main-menu-submenu'
  Keys '{LEFT}' 600; Keys '{ESC}' 900
  Windows 'after escape' | Out-Null

  # Reopen guard: clicking the button while its menu is open closes it and does not reopen it.
  Start-Sleep -Milliseconds 400
  Click ([int]$env:MENU_X) ([int]$env:MENU_Y)
  Click ([int]$env:MENU_X) ([int]$env:MENU_Y)
  Windows 'button clicked twice' | Out-Null

  # Pick: reopen and choose "New tab" (first row) with Enter; a second tab should appear.
  Start-Sleep -Milliseconds 400
  Click ([int]$env:MENU_X) ([int]$env:MENU_Y)
  Keys '{HOME}' 300; Keys '{ENTER}' 1500
  Windows 'after pick' | Out-Null
  Shot '03-after-new-tab-pick'

  # Tab context menu: right-click the first tab in the HTML tab strip.
  Focus
  Click ([int]$env:TAB_X) ([int]$env:TAB_Y) $true
  Windows 'tab menu' | Out-Null
  Shot '04-tab-context-menu'
  ShotPopup '04-tab-context-menu-popup'
  # Pick "Duplicate tab" by typeahead.
  Keys 'du' 300; Keys '{ENTER}' 2000
  Windows 'after duplicate' | Out-Null
  Shot '05-after-duplicate-pick'
} finally {
  taskkill /PID $p.Id /T /F | Out-Null
  Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$data*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
