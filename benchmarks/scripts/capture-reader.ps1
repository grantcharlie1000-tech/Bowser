# Drives a real Bowser build (HTML chrome) through reader view: a local HTTP fixture article and a
# live Wikipedia page, before / reader / after-Esc screenshots, plus checks that toggling fetches
# nothing and adds no history entry.
# Safety: the app runs with --data-dir and LOCALAPPDATA both pointing at a fresh temp folder (never the
# real %LOCALAPPDATA%\Bowser). Every keystroke is gated on Bowser (the PID this script started) being
# the foreground window. The script only stops the PIDs it started and the WebView2 children that use
# its throwaway data folder.
# Usage:
#   powershell -File capture-reader.ps1 [-Exe path] [-Out dir] [-Port 8765] [-SkipWikipedia]
param(
  [string]$Exe = "$PSScriptRoot\..\..\target-reader\release\bowser.exe",
  [string]$Out = "$PSScriptRoot\..\results\surfaces\reader",
  [int]$Port = 8765,
  [switch]$SkipWikipedia,
  [string]$Only = ''   # regex over: semantic wiki overlay history wikipedia
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
if (-not ('OU' -as [type])) {
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class OU {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  public delegate bool EP(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int ht, uint flags);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] static extern bool EnumWindows(EP p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  public static IntPtr Find(uint pid) { IntPtr found = IntPtr.Zero; EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var s = new StringBuilder(64); GetClassName(h, s, 64); if (s.ToString() == "BowserNativeWindow") { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
  public static string Title(IntPtr h) { var s = new StringBuilder(512); GetWindowText(h, s, 512); return s.ToString(); }
  public static void Force(IntPtr h) { IntPtr fg = GetForegroundWindow(); uint p; uint ft = GetWindowThreadProcessId(fg, out p); uint me = GetCurrentThreadId(); AttachThreadInput(me, ft, true); keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x12, 0, 2, UIntPtr.Zero); ShowWindow(h, 9); SetForegroundWindow(h); AttachThreadInput(me, ft, false); }
  public static void Topmost(IntPtr h) { SetWindowPos(h, new IntPtr(-1), 0, 0, 0, 0, 0x53); }
  public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(2, 0, 0, 0, UIntPtr.Zero); mouse_event(4, 0, 0, 0, UIntPtr.Zero); }
}
"@ }
[OU]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path; $Exe = (Resolve-Path $Exe).Path
$env:BOWSER_UI = 'html'
$script:p = $null; $script:h = [IntPtr]::Zero; $script:data = $null; $script:server = $null
$serverLog = Join-Path $Out 'server.log'
$base = "http://127.0.0.1:$Port"

function Launch([string]$url) {
  $script:data = Join-Path $env:TEMP ("bowser-reader-" + (Get-Random))
  New-Item -ItemType Directory -Force $script:data | Out-Null
  $env:LOCALAPPDATA = $script:data  # belt and braces: nothing may touch the real app data
  $args2 = @('--data-dir', $script:data); if ($url) { $args2 += @('--open-url', $url) }
  $script:p = Start-Process $Exe -ArgumentList $args2 -PassThru
  $script:h = [IntPtr]::Zero
  for ($i = 0; $i -lt 300 -and $script:h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 50; $script:h = [OU]::Find([uint32]$script:p.Id) }
  if ($script:h -eq [IntPtr]::Zero) { throw "No Bowser window" }
  Write-Host ("window (pid {0}, dpi {1}, data {2})" -f $script:p.Id, [OU]::GetDpiForWindow($script:h), $script:data)
}
function QuitApp {
  if ($script:p) { Stop-Process -Id $script:p.Id -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 800
  if ($script:data) {
    Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$($script:data)*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  }
  $script:p = $null
}
function Quit {
  QuitApp
  if ($script:server) { Stop-Process -Id $script:server.Id -Force -ErrorAction SilentlyContinue }
  $script:server = $null
}
function IsFg { $fp = [uint32]0; [OU]::GetWindowThreadProcessId([OU]::GetForegroundWindow(), [ref]$fp) | Out-Null; return ($fp -eq [uint32]$script:p.Id) }
function EnsureFg { for ($i = 0; $i -lt 20; $i++) { if (IsFg) { return $true }; [OU]::Force($script:h); Start-Sleep -Milliseconds 250 }; return (IsFg) }
function Keys([string]$k, [int]$wait = 500) {
  if (-not (EnsureFg)) { throw "Bowser is not the foreground window; refusing to send keys" }
  [Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds $wait
}
function TypeText([string]$text, [int]$wait = 600) {
  foreach ($ch in $text.ToCharArray()) {
    $s = [string]$ch
    if ('+^%~(){}[]'.Contains($s)) { $s = '{' + $s + '}' }
    if (-not (EnsureFg)) { throw "Bowser is not the foreground window; refusing to type" }
    [Windows.Forms.SendKeys]::SendWait($s); Start-Sleep -Milliseconds 25
  }
  Start-Sleep -Milliseconds $wait
}
function Visit([string]$url, [int]$wait = 2500) { Keys '^l' 300; Keys '^a' 150; TypeText $url 150; Keys '{ENTER}' $wait }
function Size([int]$w, [int]$ht) { [OU]::ShowWindow($script:h, 9) | Out-Null; [OU]::Topmost($script:h);  # stay above other windows so screenshots are clean
  Start-Sleep -Milliseconds 300; [OU]::MoveWindow($script:h, 40, 40, $w, $ht, $true) | Out-Null; Start-Sleep -Milliseconds 900 }
function WinRect { $r = New-Object OU+RECT; [OU]::GetWindowRect($script:h, [ref]$r) | Out-Null; return $r }
function Shot([string]$name) {
  if (-not (EnsureFg)) { Write-Host "SKIPPED $name (Bowser not foreground)"; return $null }
  [OU]::SetCursorPos(2, 2) | Out-Null   # keep the pointer off the page so no hover state differs
  Start-Sleep -Milliseconds 500
  $r = WinRect; $w = $r.R - $r.L; $ht = $r.B - $r.T
  $file = Join-Path $Out ("$name.png")
  $bmp = New-Object Drawing.Bitmap $w, $ht
  $g = [Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save($file, [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
  Write-Host "saved $file ($w x $ht)"
  return $file
}
# Share of pixels that differ by more than a rounding error in the page area (below the 84 px chrome).
function PageDiff([string]$a, [string]$b) {
  $x = [Drawing.Bitmap]::FromFile($a); $y = [Drawing.Bitmap]::FromFile($b)
  $n = 0; $total = 0
  for ($py = 90; $py -lt [Math]::Min($x.Height, $y.Height) - 4; $py += 2) {
    for ($px = 4; $px -lt [Math]::Min($x.Width, $y.Width) - 4; $px += 2) {
      $total++
      $c1 = $x.GetPixel($px, $py); $c2 = $y.GetPixel($px, $py)
      if ([Math]::Abs($c1.R - $c2.R) + [Math]::Abs($c1.G - $c2.G) + [Math]::Abs($c1.B - $c2.B) -gt 24) { $n++ }
    }
  }
  $x.Dispose(); $y.Dispose()
  return [Math]::Round(100.0 * $n / [Math]::Max(1, $total), 3)
}
function Title { return [OU]::Title($script:h) }
function GetCount([string]$path) { return @(Get-Content $serverLog | Where-Object { $_ -eq "GET $path" }).Count }

try {
  $script:server = Start-Process node -ArgumentList @("`"$PSScriptRoot\serve-reader-fixtures.mjs`"", $Port, "`"$serverLog`"") -PassThru -WindowStyle Hidden
  Start-Sleep -Milliseconds 800

  # Before / reader / after for one page. Escape is sent after a click, as a user reading the page would.
  function Scenario([string]$url, [string]$name, [int]$wait) {
    if ($Only -and $name -notmatch $Only) { return }
    Launch $url
    Start-Sleep -Milliseconds $wait
    Size 1280 800
    $before = Shot "$name-1-before"
    Keys '^%r' 1800
    $null = Shot "$name-2-reader"
    $rc = WinRect; [OU]::Click($rc.L + 640, $rc.T + 400); Start-Sleep -Milliseconds 400
    Keys '{ESC}' 1500
    $after = Shot "$name-3-after-esc"
    Write-Host ("{0}: page pixels changed after leaving reader view with Esc: {1}%" -f $name, (PageDiff $before $after))
    Keys '^%r' 1500
    Keys '^%r' 1500
    $null = Shot "$name-4-after-chord"
    Write-Host ("{0}: after Ctrl+Alt+R on, Ctrl+Alt+R off: {1}%" -f $name, (PageDiff $before (Join-Path $Out "$name-4-after-chord.png")))
    QuitApp
  }

  Scenario "$base/article-semantic.html" 'semantic' 4000
  Scenario "$base/article-wiki.html" 'wiki' 3000
  Scenario "$base/calm-overlay.html" 'overlay' 3000
  Write-Host ("server saw: " + ((Get-Content $serverLog) -join ', '))

  # History: A then B, reader on and off on B, one Back must reach A (so reader view added no entry).
  if (-not $Only -or 'history' -match $Only) {
  Launch "$base/start.html"; Start-Sleep -Seconds 3; Size 1280 800
  $rc = WinRect; [OU]::Click($rc.L + 90, $rc.T + 114); Start-Sleep -Milliseconds 2500
  Write-Host ("title after clicking the link (expect the article): " + (Title))
  Keys '^%r' 1500; $null = Shot 'history-divs-reader'
  Keys '^%r' 1500
  Keys '%{LEFT}' 2000
  Write-Host ("title after one Back (expect Start page): " + (Title))
  Write-Host ("requests: " + ((Get-Content $serverLog) -join ', '))
  QuitApp
  }

  if (-not $SkipWikipedia) {
    Scenario 'https://en.wikipedia.org/wiki/Web_browser' 'wikipedia' 7000
  }
  Write-Host "done"
} finally { Quit }
