# Drives a real Bowser build (HTML chrome) to capture the new-tab surface, the address bar while a
# start URL is still loading, and a suggestion list for a query made of many emoji.
# Every keystroke is gated on Bowser (the PID this script started) being the foreground window; the
# script only ever stops that PID and the WebView2 children that use its throwaway data folder.
# Usage:
#   powershell -File capture-newtab.ps1 [-Exe path] [-Out dir] [-Label prefix] [-Only regex]
param(
  [string]$Exe = "$PSScriptRoot\..\..\target-director\debug\bowser.exe",
  [string]$Out = "$PSScriptRoot\..\results\surfaces\newtab",
  [string]$Label = "",
  [string]$Only = "",
  [int]$visitWait = 2200
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
  [DllImport("user32.dll")] static extern bool EnumWindows(EP p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  public static IntPtr Find(uint pid) { IntPtr found = IntPtr.Zero; EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var s = new StringBuilder(64); GetClassName(h, s, 64); if (s.ToString() == "BowserNativeWindow") { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
  public static void Force(IntPtr h) { IntPtr fg = GetForegroundWindow(); uint p; uint ft = GetWindowThreadProcessId(fg, out p); uint me = GetCurrentThreadId(); AttachThreadInput(me, ft, true); ShowWindow(h, 9); SetForegroundWindow(h); AttachThreadInput(me, ft, false); }
}
"@ }
[OU]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path; $Exe = (Resolve-Path $Exe).Path
$env:BOWSER_UI = 'html'
$script:p = $null; $script:h = [IntPtr]::Zero; $script:data = $null

function Launch([string]$url = 'https://example.com/') {
  $script:data = Join-Path $env:TEMP ("bowser-newtab-" + (Get-Random))
  New-Item -ItemType Directory -Force $script:data | Out-Null
  $env:LOCALAPPDATA = $script:data  # belt and braces: nothing may touch the real app data
  $args2 = @('--data-dir', $script:data); if ($url) { $args2 += @('--open-url', $url) }
  $script:p = Start-Process $Exe -ArgumentList $args2 -PassThru
  $script:h = [IntPtr]::Zero
  for ($i = 0; $i -lt 300 -and $script:h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 50; $script:h = [OU]::Find([uint32]$script:p.Id) }
  if ($script:h -eq [IntPtr]::Zero) { throw "No Bowser window" }
  Write-Host ("window (pid {0}, dpi {1})" -f $script:p.Id, [OU]::GetDpiForWindow($script:h))
}
function Quit {
  if ($script:p) { Stop-Process -Id $script:p.Id -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 800
  Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$($script:data)*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  $script:p = $null
}
function IsFg { $fp = [uint32]0; [OU]::GetWindowThreadProcessId([OU]::GetForegroundWindow(), [ref]$fp) | Out-Null; return ($fp -eq [uint32]$script:p.Id) }
function EnsureFg { for ($i = 0; $i -lt 6; $i++) { if (IsFg) { return $true }; [OU]::Force($script:h); Start-Sleep -Milliseconds 250 }; return (IsFg) }
function Keys([string]$k, [int]$wait = 500) {
  if (-not (EnsureFg)) { throw "Bowser is not the foreground window; refusing to send keys" }
  [Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds $wait
}
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
function Size([int]$w, [int]$ht) { [OU]::ShowWindow($script:h, 9) | Out-Null; Start-Sleep -Milliseconds 300; [OU]::MoveWindow($script:h, 40, 40, $w, $ht, $true) | Out-Null; Start-Sleep -Milliseconds 900 }
function WinRect { $r = New-Object OU+RECT; [OU]::GetWindowRect($script:h, [ref]$r) | Out-Null; return $r }
function Grab([int]$x, [int]$y, [int]$w, [int]$ht, [string]$file) {
  $bmp = New-Object Drawing.Bitmap $w, $ht
  $g = [Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
  $bmp.Save($file, [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
}
function Shot([string]$name, [int]$maxH = 0) {
  if (-not (EnsureFg)) { Write-Host "SKIPPED $name (Bowser not foreground)"; return }
  Start-Sleep -Milliseconds 350
  $r = WinRect; $w = $r.R - $r.L; $ht = $r.B - $r.T; if ($maxH -gt 0 -and $ht -gt $maxH) { $ht = $maxH }
  $file = Join-Path $Out ("$Label$name.png"); Grab $r.L $r.T $w $ht $file
  Write-Host "saved $file ($w x $ht)"
}
# Sends one key, then grabs the top of the window as fast as it can for $ms: open/close frames.
function Burst([string]$key, [string]$name, [int]$ms = 320, [int]$maxH = 420) {
  if (-not (EnsureFg)) { Write-Host "SKIPPED $name (Bowser not foreground)"; return }
  $r = WinRect; $w = $r.R - $r.L
  $frames = New-Object System.Collections.ArrayList
  [Windows.Forms.SendKeys]::SendWait($key)
  $sw = [Diagnostics.Stopwatch]::StartNew()
  while ($sw.ElapsedMilliseconds -lt $ms) {
    $bmp = New-Object Drawing.Bitmap $w, $maxH
    $g = [Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size); $g.Dispose()
    [void]$frames.Add(@($sw.ElapsedMilliseconds, $bmp))
  }
  $i = 0
  foreach ($f in $frames) {
    $file = Join-Path $Out ("{0}{1}-{2:D2}-{3:D3}ms.png" -f $Label, $name, $i, $f[0]); $f[1].Save($file, [Drawing.Imaging.ImageFormat]::Png); $f[1].Dispose(); $i++
  }
  Write-Host "burst $name : $i frames"
}

try {
  # 1. Start URL still loading: the bar must already show it (grab before the page can finish).
  Launch 'https://example.com/'
  $r = WinRect; $w = $r.R - $r.L; $sw = [Diagnostics.Stopwatch]::StartNew(); $n = 0
  while ($sw.ElapsedMilliseconds -lt 3200) {
    $f = Join-Path $Out ("{0}launch-{1:D2}-{2:D4}ms.png" -f $Label, $n, $sw.ElapsedMilliseconds)
    Grab $r.L $r.T $w 90 $f; $n++; Start-Sleep -Milliseconds 120
  }
  Write-Host "launch frames: $n"
  Start-Sleep -Seconds 3
  Size 1280 800
  # 2. A new tab: opaque themed surface, focused empty address bar.
  Keys '^t' 900; Shot 'newtab' 
  # 3. A query of many emoji must keep the suggestion list (the search row alone) on screen.
  Set-Clipboard -Value (([char]::ConvertFromUtf32(0x1F600)) * 140 + ' pizza')
  Keys '^v' 1200; Shot 'emoji-suggest' 420
  Keys '{ESC}' 300; Keys '{ESC}' 300
  Keys '^l' 300; Set-Clipboard -Value ('rust ' + ([char]::ConvertFromUtf32(0x1F980)) * 200)
  Keys '^v' 1200; Shot 'emoji-suggest-2' 420
  Keys '{ESC}' 300; Keys '{ESC}' 300
  Keys '^l' 300; Set-Clipboard -Value ((([string][char]0x6f22 + [char]0x5b57 + ' ') * 60) + (([string][char]0x5e9 + [char]0x5dc + [char]0x5d5 + [char]0x5dd + ' ') * 20))
  Keys '^v' 1200; Shot 'cjk-rtl-suggest' 420
} finally { Quit }
