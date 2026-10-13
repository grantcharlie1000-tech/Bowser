# Drives a real Bowser build (HTML chrome) to check per-site zoom and the permissions dashboard.
# Everything runs against a throwaway data folder: --data-dir AND LOCALAPPDATA point at it, so the
# real %LOCALAPPDATA%\Bowser is never read or written. The script only ever stops the PIDs it
# started (Bowser, the fixture server) and the WebView2 children that use its own data folder.
# The page reports its zoom in the window title (dpr=...), so zoom and persistence are measured, not
# just looked at.
# Usage: powershell -File capture-prefs.ps1 [-Exe path] [-Out dir] [-Stage zoom|dashboard|all]
param(
  [string]$Exe = "$PSScriptRoot\..\..\target-prefs\release\bowser.exe",
  [string]$Out = "$PSScriptRoot\..\results\surfaces\prefs",
  [string]$Stage = "all",
  [int]$Port = 18765
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
if (-not ('PF' -as [type])) {
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class PF {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  public delegate bool EP(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] static extern bool EnumWindows(EP p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  public static IntPtr Find(uint pid) { IntPtr found = IntPtr.Zero; EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var s = new StringBuilder(64); GetClassName(h, s, 64); if (s.ToString() == "BowserNativeWindow") { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
  public static string Dump(uint pid) { var sb = new StringBuilder(); EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid) { var s = new StringBuilder(64); GetClassName(h, s, 64); RECT r; GetWindowRect(h, out r); sb.Append(s.ToString() + (IsWindowVisible(h) ? " vis " : " hid ") + r.L + "," + r.T + "," + (r.R-r.L) + "x" + (r.B-r.T) + " | "); } return true; }, IntPtr.Zero); return sb.ToString(); }
  public static string Title(IntPtr h) { var s = new StringBuilder(512); GetWindowText(h, s, 512); return s.ToString(); }
  public static void Force(IntPtr h) { IntPtr fg = GetForegroundWindow(); uint p; uint ft = GetWindowThreadProcessId(fg, out p); uint me = GetCurrentThreadId(); AttachThreadInput(me, ft, true); ShowWindow(h, 9); SetForegroundWindow(h); AttachThreadInput(me, ft, false); }
}
"@ }
[PF]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path; $Exe = (Resolve-Path $Exe).Path
$env:BOWSER_UI = 'html'
$script:p = $null; $script:h = [IntPtr]::Zero
$script:data = Join-Path $env:TEMP ("bowser-prefs-" + (Get-Random))
New-Item -ItemType Directory -Force $script:data | Out-Null
$env:LOCALAPPDATA = $script:data   # belt and braces: nothing may touch the real app data
$url = "http://localhost:$Port/"
$origin = "http://localhost:$Port"
$prefsFile = Join-Path $script:data 'site-prefs.txt'

# The dashboard shot wants a few stored sites next to the real denied requests the fixture page makes.
if ($Stage -in 'dashboard') {
  @("https://meet.example allow camera", "https://meet.example allow microphone", "http://localhost:8080 block location", "https://news.example allow popups") | Set-Content (Join-Path $script:data 'permissions.txt')
}
if ($Stage -eq 'popups') { Set-Content (Join-Path $script:data 'permissions.txt') @("$origin allow popups") }
# --- fixture server ---
$serverLog = Join-Path $script:data 'server.out'
$server = Start-Process node -ArgumentList @("`"$PSScriptRoot\prefs-fixture-server.mjs`"", "$Port") -PassThru -WindowStyle Hidden -RedirectStandardOutput $serverLog
Start-Sleep -Milliseconds 800

function Launch {
  $a = @('--data-dir', "`"$script:data`"", '--open-url', $url)
  $script:p = Start-Process $Exe -ArgumentList $a -PassThru
  $script:h = [IntPtr]::Zero
  for ($i = 0; $i -lt 300 -and $script:h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 50; $script:h = [PF]::Find([uint32]$script:p.Id) }
  if ($script:h -eq [IntPtr]::Zero) { throw "No Bowser window" }
  [PF]::MoveWindow($script:h, 40, 40, 1200, 820, $true) | Out-Null
  for ($i = 0; $i -lt 60 -and (Title) -notmatch "dpr="; $i++) { Start-Sleep -Milliseconds 500 }
  Start-Sleep -Milliseconds 500
  Write-Host ("window pid {0}" -f $script:p.Id)
}
function Quit {
  if ($script:p) { $script:p.CloseMainWindow() | Out-Null; if (-not $script:p.WaitForExit(8000)) { Stop-Process -Id $script:p.Id -Force -ErrorAction SilentlyContinue } }
  Start-Sleep -Milliseconds 1200
  Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$($script:data)*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  $script:p = $null
}
function Title { [PF]::Title($script:h) }
function Focus { [PF]::Force($script:h); Start-Sleep -Milliseconds 300 }
function Gate { if ([PF]::GetForegroundWindow() -ne $script:h) { Focus } }
function Shot($name, [switch]$Toolbar) {
  Start-Sleep -Milliseconds 700
  $r = New-Object PF+RECT; [PF]::GetWindowRect($script:h, [ref]$r) | Out-Null
  $w = $r.R - $r.L; $ht = if ($Toolbar) { 56 } else { $r.B - $r.T }
  $bmp = New-Object Drawing.Bitmap $w, $ht
  $g = [Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save("$Out\$name.png", [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
  Write-Host "saved $name.png"
}
function Chord([byte[]]$mods, [byte]$vk, [int]$wait = 700) {
  Gate
  foreach ($m in $mods) { [PF]::keybd_event($m, 0, 0, [UIntPtr]::Zero) }
  $ext = if ($vk -eq 0x2E) { 1 } else { 0 }   # Delete is an extended key
  [PF]::keybd_event($vk, 0, $ext, [UIntPtr]::Zero); [PF]::keybd_event($vk, 0, (2 -bor $ext), [UIntPtr]::Zero)
  [array]::Reverse($mods)
  foreach ($m in $mods) { [PF]::keybd_event($m, 0, 2, [UIntPtr]::Zero) }
  Start-Sleep -Milliseconds $wait
}
function CtrlWheel([int]$notches) {
  Gate
  $r = New-Object PF+RECT; [PF]::GetWindowRect($script:h, [ref]$r) | Out-Null
  [PF]::SetCursorPos($r.L + 600, $r.T + 450) | Out-Null; Start-Sleep -Milliseconds 200
  [PF]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
  for ($i = 0; $i -lt [math]::Abs($notches); $i++) { [PF]::mouse_event(0x0800, 0, 0, [math]::Sign($notches) * 120, 0); Start-Sleep -Milliseconds 120 }
  [PF]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 800
}
function Click([int]$rx, [int]$ry) {
  Gate
  $r = New-Object PF+RECT; [PF]::GetWindowRect($script:h, [ref]$r) | Out-Null
  [PF]::SetCursorPos($r.L + $rx, $r.T + $ry) | Out-Null; Start-Sleep -Milliseconds 200
  [PF]::mouse_event(2, 0, 0, 0, 0); [PF]::mouse_event(4, 0, 0, 0, 0)
  Start-Sleep -Milliseconds 900
}
function Wheel([int]$notches, [int]$rx = 600, [int]$ry = 450) {
  Gate
  $r = New-Object PF+RECT; [PF]::GetWindowRect($script:h, [ref]$r) | Out-Null
  [PF]::SetCursorPos($r.L + $rx, $r.T + $ry) | Out-Null; Start-Sleep -Milliseconds 200
  for ($i = 0; $i -lt [math]::Abs($notches); $i++) { [PF]::mouse_event(0x0800, 0, 0, [math]::Sign($notches) * 120, 0); Start-Sleep -Milliseconds 80 }
  Start-Sleep -Milliseconds 500
}
function Prefs { if (Test-Path $prefsFile) { (Get-Content $prefsFile | Where-Object { $_ -match '^http' }) -join '; ' } else { '(no file)' } }
function Dpr([string]$t) { if ($t -match 'dpr=([0-9.]+)') { [double]$Matches[1] } else { 0.0 } }
function Check([string]$what, [bool]$ok) { Write-Host ("[{0}] {1}" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $what) }

$VK_1 = 0x31; $VK_T = 0x54; $VK_CTRL = 0x11; $VK_SHIFT = 0x10; $VK_PLUS = 0xBB; $VK_MINUS = 0xBD; $VK_0 = 0x30; $VK_DEL = 0x2E
try {
  Write-Host "data dir: $script:data"
  Launch
  Focus
  $t0 = Title; $base = Dpr $t0
  Write-Host "start title: $t0 (base dpr $base)"
  Shot 'zoom-00-start'

  if ($Stage -in 'zoom', 'all') {
    Chord @($VK_CTRL) $VK_0 1200                               # primes focus: the first chord after a launch can be lost
    foreach ($i in 1..3) { Chord @($VK_CTRL) $VK_PLUS }       # 100 -> 110 -> 125 -> 150
    $t1 = Title; $d1 = Dpr $t1
    Write-Host "after 3x Ctrl+Plus: $t1 ratio=$([math]::Round($d1 / $base, 3)) prefs: $(Prefs)"
    Check 'Ctrl+Plus x3 gives 150%' ([math]::Abs($d1 / $base - 1.5) -lt 0.01)
    Check 'site-prefs.txt stores the origin at 150' ((Prefs) -eq "$origin zoom 150")
    Shot 'zoom-01-150' ; Shot 'zoom-01-150-toolbar' -Toolbar
    Chord @($VK_CTRL, $VK_SHIFT) $VK_PLUS                      # Ctrl+Shift+= also zooms in: 175
    Chord @($VK_CTRL) $VK_MINUS; Chord @($VK_CTRL) $VK_MINUS   # 150 -> 125
    $d2 = Dpr (Title); Write-Host "after Ctrl+Shift+=, 2x Ctrl+Minus: ratio=$([math]::Round($d2 / $base, 3)) prefs: $(Prefs)"
    Check 'Ctrl+Shift+= then 2x Ctrl+Minus gives 125%' ([math]::Abs($d2 / $base - 1.25) -lt 0.01)
    Chord @($VK_CTRL) $VK_PLUS; Chord @($VK_CTRL) $VK_PLUS      # 125 -> 150 -> 175 (kept for the restart check)
    Chord @($VK_CTRL) $VK_MINUS                                  # back to 150
    $expect = Title; $dExpect = Dpr $expect
    Quit; Start-Sleep -Seconds 1
    Write-Host "restart with the same data dir; prefs file: $(Prefs)"
    Launch; Focus
    $t3 = Title; $d3 = Dpr $t3
    Write-Host "after restart: $t3 ratio=$([math]::Round($d3 / $base, 3))"
    Check 'zoom persisted across restart (150%)' ([math]::Abs($d3 / $base - 1.5) -lt 0.01)
    Shot 'zoom-02-restart' ; Shot 'zoom-02-restart-toolbar' -Toolbar
    # Site information shows the zoom row with a Reset button.
    Click 315 22 ; Start-Sleep -Milliseconds 800
    Shot 'zoom-03-siteinfo'
    [Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep -Milliseconds 500
    # Ctrl+wheel is WebView2's own zoom; the result is stored like a key press.
    CtrlWheel 2
    $d4 = Dpr (Title); Write-Host "after Ctrl+wheel up x2: ratio=$([math]::Round($d4 / $base, 3)) prefs: $(Prefs)"
    Check 'Ctrl+wheel zoomed the page' ($d4 / $base -gt 1.55)
    Check 'Ctrl+wheel result is stored for the origin' ((Prefs) -match [regex]::Escape("$origin zoom "))
    Chord @($VK_CTRL) $VK_0
    $d5 = Dpr (Title); Write-Host "after Ctrl+0: ratio=$([math]::Round($d5 / $base, 3)) prefs: $(Prefs)"
    Check 'Ctrl+0 resets to 100% and clears the stored line' (([math]::Abs($d5 / $base - 1.0) -lt 0.01) -and ((Prefs) -eq '(no file)' -or (Prefs) -eq ''))
    Shot 'zoom-04-reset-toolbar' -Toolbar
  }


  if ($Stage -eq 'popups') {
    # Allow popups for the fixture origin: its script popup now becomes a tab. Without the stored
    # Allow the same page opens no tab (see the default run, where only one tab exists).
    Write-Host "popups: permissions.txt = $(Get-Content (Join-Path $script:data 'permissions.txt'))"
    Write-Host "popups: title after load = $(Title)"
    Shot 'popups-00-allowed'
    Chord @($VK_CTRL) $VK_1 800
    Click 315 22; Start-Sleep -Milliseconds 800
    Shot 'popups-01-siteinfo'
    [Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep -Milliseconds 500
  }
  if ($Stage -in 'dashboard', 'all') {
    Chord @($VK_CTRL) $VK_T 1500                               # a blank tab: the dashboard overlay is a child of the window and the page view paints over it
    Start-Sleep -Seconds 2
    Chord @($VK_CTRL, $VK_SHIFT) $VK_DEL 6000
    Write-Host ("windows: " + [PF]::Dump([uint32]$script:p.Id))
    Shot "dashboard-00-top"
    Wheel -6
    Shot 'dashboard-01-scrolled'
    Wheel -6
    Shot 'dashboard-02-denied'
    [Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep -Milliseconds 400
    Write-Host "permissions file: $(Get-Content (Join-Path $script:data 'permissions.txt') -ErrorAction SilentlyContinue)"
  }
}
finally {
  if (Test-Path (Join-Path $script:data "crash.log")) { Write-Host "CRASH LOG:"; Get-Content (Join-Path $script:data "crash.log") -TotalCount 25 }
  Quit
  if ($server) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 500
  Remove-Item -Recurse -Force $script:data -ErrorAction SilentlyContinue
}
