# Drives a real Bowser build (HTML chrome) to capture the omnibox suggestions dropdown and the tab
# strip. Every keystroke and click is gated on Bowser (the PID this script started) being the
# foreground window; the script only ever stops that PID and the WebView2 children that use its
# throwaway data folder. Usage:
#   powershell -File capture-suggest-polish.ps1 [-Exe path] [-Out dir] [-Label prefix] [-Only regex]
param(
  [string]$Exe = "$PSScriptRoot\..\..\target-polish\release\bowser.exe",
  [string]$Out = "$PSScriptRoot\..\results\suggest-polish",
  [string]$Label = "",
  [string]$Only = "",
  [int]$visitWait = 2200
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
if (-not ('SP' -as [type])) {
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class SP {
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
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, int e);
  [DllImport("user32.dll")] static extern bool EnumWindows(EP p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  public static IntPtr Find(uint pid) { IntPtr found = IntPtr.Zero; EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var s = new StringBuilder(64); GetClassName(h, s, 64); if (s.ToString() == "BowserNativeWindow") { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
  public static void Force(IntPtr h) { IntPtr fg = GetForegroundWindow(); uint p; uint ft = GetWindowThreadProcessId(fg, out p); uint me = GetCurrentThreadId(); AttachThreadInput(me, ft, true); ShowWindow(h, 9); SetForegroundWindow(h); AttachThreadInput(me, ft, false); }
}
"@ }
[SP]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path; $Exe = (Resolve-Path $Exe).Path
$env:BOWSER_UI = 'html'
$script:p = $null; $script:h = [IntPtr]::Zero; $script:data = $null

function Launch([string]$url = 'https://example.com/') {
  $script:data = Join-Path $env:TEMP ("bowser-suggest-polish-" + (Get-Random))
  New-Item -ItemType Directory -Force $script:data | Out-Null
  $env:BOWSER_DATA_DIR = $script:data
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $script:p = Start-Process $Exe -ArgumentList @('--open-url', $url) -PassThru
  $script:h = [IntPtr]::Zero
  for ($i = 0; $i -lt 300 -and $script:h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 50; $script:h = [SP]::Find([uint32]$script:p.Id) }
  if ($script:h -eq [IntPtr]::Zero) { throw "No Bowser window" }
  Write-Host ("window after {0} ms (pid {1})" -f $sw.ElapsedMilliseconds, $script:p.Id)
  return $sw
}
function Quit {
  if ($script:p) { Stop-Process -Id $script:p.Id -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 800
  Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$($script:data)*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  $script:p = $null
}
function IsFg { $fp = [uint32]0; [SP]::GetWindowThreadProcessId([SP]::GetForegroundWindow(), [ref]$fp) | Out-Null; return ($fp -eq [uint32]$script:p.Id) }
function EnsureFg { for ($i = 0; $i -lt 6; $i++) { if (IsFg) { return $true }; [SP]::Force($script:h); Start-Sleep -Milliseconds 250 }; return (IsFg) }
function Keys([string]$k, [int]$wait = 500) {
  if (-not (EnsureFg)) { throw "Bowser is not the foreground window; refusing to send keys" }
  [Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds $wait
}
# Types text one character at a time, re-checking the foreground before each one.
function TypeText([string]$text, [int]$wait = 600) {
  foreach ($ch in $text.ToCharArray()) {
    $s = [string]$ch
    if ('+^%~(){}[]'.Contains($s)) { $s = '{' + $s + '}' }
    if (-not (EnsureFg)) { throw "Bowser is not the foreground window; refusing to type" }
    [Windows.Forms.SendKeys]::SendWait($s); Start-Sleep -Milliseconds 40
  }
  Start-Sleep -Milliseconds $wait
}
function Visit([string]$url, [int]$wait = 2500) { Keys '^l' 300; Keys '^a' 150; TypeText $url 150; Keys '{ENTER}' $wait }
function NewTab([string]$url, [int]$wait = 1800) { Keys '^t' 500; TypeText $url 150; Keys '{ENTER}' $wait }
function Size([int]$w, [int]$ht) { [SP]::ShowWindow($script:h, 9) | Out-Null; Start-Sleep -Milliseconds 300; [SP]::MoveWindow($script:h, 40, 40, $w, $ht, $true) | Out-Null; Start-Sleep -Milliseconds 900 }
function Shot([string]$name, [int]$maxH = 0) {
  if (-not (EnsureFg)) { Write-Host "SKIPPED $name (Bowser not foreground)"; return }
  Start-Sleep -Milliseconds 350
  $r = New-Object SP+RECT; [SP]::GetWindowRect($script:h, [ref]$r) | Out-Null
  $w = $r.R - $r.L; $ht = $r.B - $r.T; if ($maxH -gt 0 -and $ht -gt $maxH) { $ht = $maxH }
  $bmp = New-Object Drawing.Bitmap $w, $ht
  $g = [Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $file = Join-Path $Out ("$Label$name.png"); $bmp.Save($file, [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
  Write-Host "saved $file ($w x $ht)"
}
function Want([string]$name) { return (-not $Only) -or ($name -match $Only) }

try {
  # ---- Startup: how soon the HTML chrome is visible (timed shots of the top of the window). ----
  if (Want 'startup') {
    $sw = Launch 'https://example.com/'
    [SP]::Force($script:h)
    foreach ($t in 400, 900, 1600) { while ($sw.ElapsedMilliseconds -lt $t) { Start-Sleep -Milliseconds 20 }; Shot ("startup-{0}ms" -f $t) 140 }
    Quit
  }

  if (Want 'suggest') {
    Launch 'https://example.com/' | Out-Null
    Start-Sleep -Seconds 4
    Size 1280 800
    foreach ($u in 'https://en.wikipedia.org/wiki/Web_browser', 'https://en.wikipedia.org/wiki/WebView', 'https://en.wikipedia.org/wiki/Microsoft_Edge',
                   'https://www.rust-lang.org/', 'https://doc.rust-lang.org/book/', 'https://developer.mozilla.org/en-US/docs/Web/CSS',
                   'https://en.wikipedia.org/wiki/Wiki', 'https://www.wikidata.org/wiki/Wikidata:Main_Page') { Visit $u $visitWait }
    # Typing: the dropdown opens under the pill.
    Keys '^l' 300; TypeText 'wiki' 900; Shot 'suggest-many'
    Keys '{DOWN}' 300; Keys '{DOWN}' 400; Shot 'suggest-arrow-down'
    Keys '{ESC}' 600; Shot 'suggest-after-esc'
    Keys '{ESC}' 400
    Keys '^l' 300; TypeText 'rust' 900; Shot 'suggest-few'
    Keys '{DOWN}' 400; Keys '{ENTER}' 2500; Shot 'suggest-after-enter'
    # Open tabs: their rows switch to them.
    NewTab 'https://developer.mozilla.org/en-US/'
    NewTab 'https://example.org/'
    Keys '^l' 300; TypeText 'mozilla' 900; Shot 'suggest-switch-tab'
    Keys '{ESC}' 300; Keys '{ESC}' 300
    Keys '^l' 300; TypeText 'a very long search query that keeps going well past the width of the dropdown to test clipping' 900; Shot 'suggest-long-query'
    Keys '{ESC}' 300; Keys '{ESC}' 300
    # Narrow window.
    Size 760 640
    Keys '^l' 300; TypeText 'wiki' 900; Shot 'suggest-narrow'
    Keys '{ESC}' 300; Keys '{ESC}' 300
    Quit
  }

  if (Want 'tabs') {
    Launch 'https://example.com/' | Out-Null
    Start-Sleep -Seconds 4
    Size 1280 800
    Shot 'tabs-1' 160
    foreach ($u in 'https://example.org/', 'https://www.rust-lang.org/', 'https://developer.mozilla.org/en-US/') { NewTab $u 1500 }
    Shot 'tabs-4' 160
    # Distinct hosts, so smart tab groups do not fold them into one island.
    foreach ($u in 'https://example.net/', 'https://www.iana.org/', 'https://www.w3.org/', 'https://en.wikipedia.org/wiki/Tab_key',
                   'https://httpbin.org/html', 'https://www.ietf.org/', 'https://docs.rs/', 'https://crates.io/') { NewTab $u 1100 }
    Start-Sleep -Seconds 2
    Shot 'tabs-12' 160
    Quit
  }
} finally {
  if ($script:p) { Quit }
}
Write-Host done

