# Records Bowser's first boot on a fresh profile: the window opening, the first-run welcome
# (bowser://settings/welcome, docs/design/welcome.md) and a walk through every step with the real
# mouse, then "Start browsing". Frames are screen captures (about 12 a second, JPEG) with the
# cursor drawn in; a later step turns them into a video.
#
# Safety: everything runs on a throwaway --data-dir. Buttons are found by their accessible names
# through UI Automation inside the Bowser window this script started, and the mouse only moves while
# that window is in front. "Set as default" is never pressed (it would change the machine's default
# browser); the step's "Not now" is used instead.
#
# Usage: powershell -File record-first-boot.ps1 [-Exe path] [-Out dir]
param(
  [string]$Exe = "$PSScriptRoot\..\..\target\debug\bowser.exe",
  [string]$Out = "$PSScriptRoot\..\results\first-boot",
  [int]$Fps = 12
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
if (-not ('FB' -as [type])) {
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class FB {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
  public delegate bool EP(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int k);
  [DllImport("user32.dll")] static extern bool EnumWindows(EP p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  public static IntPtr Find(uint pid) { IntPtr found = IntPtr.Zero; EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var s = new StringBuilder(64); GetClassName(h, s, 64); if (s.ToString() == "BowserNativeWindow") { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
  public static void Force(IntPtr h) { IntPtr fg = GetForegroundWindow(); uint p; uint ft = GetWindowThreadProcessId(fg, out p); uint me = GetCurrentThreadId(); AttachThreadInput(me, ft, true); ShowWindow(h, 9); SetForegroundWindow(h); AttachThreadInput(me, ft, false); }
}
"@ }
[FB]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path; $Exe = (Resolve-Path $Exe).Path
$frames = Join-Path $Out 'frames'
New-Item -ItemType Directory -Force $frames | Out-Null
$log = Join-Path $Out 'record.log'
function Note([string]$text) { $line = "{0:HH:mm:ss.fff} {1}" -f (Get-Date), $text; Write-Host $line; Add-Content -Path $log -Value $line }

# A bigger desktop where the runner allows it (Windows Server's own cmdlet; ignored elsewhere).
try { Set-DisplayResolution -Width 1600 -Height 900 -Force -ErrorAction Stop; Note 'display 1600x900' } catch { Note "display unchanged: $($_.Exception.Message)" }
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Note ("screen {0}x{1}" -f $screen.Width, $screen.Height)

# ---- the frame grabber: its own process, until the stop file appears ----
$stop = Join-Path $Out 'stop'
Remove-Item $stop -ErrorAction SilentlyContinue
$grabber = Start-Job -ArgumentList $frames, $stop, $Fps -ScriptBlock {
  param($frames, $stop, $fps)
  Add-Type -AssemblyName System.Windows.Forms, System.Drawing
  Add-Type @"
using System; using System.Runtime.InteropServices;
public static class G { [StructLayout(LayoutKind.Sequential)] public struct P { public int X,Y; }
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out P p);
  [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int k);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }
"@
  [G]::SetProcessDPIAware() | Out-Null
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' }
  $params = New-Object System.Drawing.Imaging.EncoderParameters 1
  $params.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter ([System.Drawing.Imaging.Encoder]::Quality), 88L
  $white = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(235, 255, 255, 255))
  $blue = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(240, 120, 170, 255))
  $edge = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(150, 0, 0, 0)), 2
  $n = 0; $period = [int](1000 / $fps); $clock = [Diagnostics.Stopwatch]::StartNew()
  while (-not (Test-Path $stop)) {
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $p = New-Object G+P; [G]::GetCursorPos([ref]$p) | Out-Null
    $down = ([G]::GetAsyncKeyState(1) -band 0x8000) -ne 0
    $r = if ($down) { 7 } else { 9 }
    $g.SmoothingMode = 'AntiAlias'
    $g.FillEllipse($(if ($down) { $blue } else { $white }), $p.X - $r, $p.Y - $r, 2 * $r, 2 * $r)
    $g.DrawEllipse($edge, $p.X - $r, $p.Y - $r, 2 * $r, 2 * $r)
    $bmp.Save((Join-Path $frames ('f{0:D5}.jpg' -f $n)), $codec, $params)
    $g.Dispose(); $bmp.Dispose(); $n++
    $wait = $n * $period - $clock.ElapsedMilliseconds
    if ($wait -gt 0) { Start-Sleep -Milliseconds $wait }
  }
  "$n frames"
}

# ---- Bowser on a fresh profile ----
$data = Join-Path $env:RUNNER_TEMP ("bowser-first-boot-" + (Get-Random))
if (-not $env:RUNNER_TEMP) { $data = Join-Path $env:TEMP ("bowser-first-boot-" + (Get-Random)) }
New-Item -ItemType Directory -Force $data | Out-Null
$env:BOWSER_UI = 'html'
Start-Sleep -Milliseconds 1500   # a moment of empty desktop before the launch
Note 'launch'
$p = Start-Process $Exe -ArgumentList @('--data-dir', $data) -PassThru
$h = [IntPtr]::Zero
for ($i = 0; $i -lt 400 -and $h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 50; $h = [FB]::Find([uint32]$p.Id) }
if ($h -eq [IntPtr]::Zero) { throw 'No Bowser window' }
Note "window pid $($p.Id)"
$w = [Math]::Min(1440, $screen.Width - 40); $ht = [Math]::Min(860, $screen.Height - 40)
[FB]::MoveWindow($h, [int](($screen.Width - $w) / 2), [int](($screen.Height - $ht) / 2), $w, $ht, $true) | Out-Null

function IsFg { $fp = [uint32]0; [FB]::GetWindowThreadProcessId([FB]::GetForegroundWindow(), [ref]$fp) | Out-Null; return ($fp -eq [uint32]$p.Id) }
function EnsureFg { for ($i = 0; $i -lt 8; $i++) { if (IsFg) { return $true }; [FB]::Force($h); Start-Sleep -Milliseconds 250 }; return (IsFg) }

$root = $null
function Root { if (-not $script:root) { $script:root = [System.Windows.Automation.AutomationElement]::FromHandle($h) }; return $script:root }
# The first on-screen element in Bowser's window whose accessible name is $name (or starts with it).
function FindNamed([string]$name, [int]$timeoutMs = 12000) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.ElapsedMilliseconds -lt $timeoutMs) {
    try {
      $all = (Root).FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
      foreach ($el in $all) {
        $n = $el.Current.Name
        if ($n -and ($n -eq $name -or $n.StartsWith($name)) -and -not $el.Current.IsOffscreen) {
          $r = $el.Current.BoundingRectangle
          if ($r.Width -gt 4 -and $r.Height -gt 4) { return $el }
        }
      }
    } catch { }
    Start-Sleep -Milliseconds 300
  }
  return $null
}
function Glide([int]$x, [int]$y, [int]$ms = 450) {
  $from = New-Object FB+POINT; [FB]::GetCursorPos([ref]$from) | Out-Null
  $steps = [Math]::Max(10, [int]($ms / 16))
  for ($i = 1; $i -le $steps; $i++) {
    $t = $i / $steps; $e = 1 - [Math]::Pow(1 - $t, 3)
    [FB]::SetCursorPos([int]($from.X + ($x - $from.X) * $e), [int]($from.Y + ($y - $from.Y) * $e)) | Out-Null
    Start-Sleep -Milliseconds 16
  }
}
function Hover([string]$name, [int]$dwell = 450) {
  $el = FindNamed $name
  if (-not $el) { Note "not found: $name"; return $false }
  $r = $el.Current.BoundingRectangle
  Glide ([int]($r.X + $r.Width / 2)) ([int]($r.Y + $r.Height / 2))
  Start-Sleep -Milliseconds $dwell
  return $true
}
function Press([string]$name, [int]$after = 900) {
  if (-not (EnsureFg)) { throw 'Bowser is not the foreground window; refusing to click' }
  if (-not (Hover $name 250)) { return $false }
  [FB]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 120; [FB]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero)
  Note "pressed: $name"
  Start-Sleep -Milliseconds $after
  return $true
}
function Esc { if (EnsureFg) { [Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep -Milliseconds 600 } }

try {
  # The window paints, then the welcome takes the blank first tab (about two seconds after).
  if (-not (FindNamed 'Get started' 25000)) { Note 'the welcome did not appear'; Start-Sleep -Seconds 4; throw 'no welcome' }
  Note 'welcome shown'
  Start-Sleep -Milliseconds 2200
  Hover 'Skip setup' 600 | Out-Null
  Press 'Get started' 1700 | Out-Null

  # Import: hover the browsers, try one (on a fresh runner there is nothing to import, and Bowser says so).
  foreach ($b in 'Chrome', 'Edge', 'Brave', 'Firefox', 'Opera', 'Vivaldi', 'Helium') { Hover $b 220 | Out-Null }
  Press 'Edge' 2600 | Out-Null
  Esc
  Press 'Continue' 1600 | Out-Null

  # Look
  Press 'Light' 1500 | Out-Null
  Press 'Dark' 1500 | Out-Null
  foreach ($c in 'Teal', 'Rose', 'Violet') { Press $c 1000 | Out-Null }
  Press 'Windows accent' 1000 | Out-Null
  Press 'System' 1300 | Out-Null
  Press 'Continue' 1600 | Out-Null

  # Search
  Press 'DuckDuckGo' 1200 | Out-Null
  Press 'Brave' 1200 | Out-Null
  Press 'Google' 1300 | Out-Null
  Press 'Continue' 1600 | Out-Null

  # Ad blocking
  Press 'No, show me everything' 1200 | Out-Null
  Press 'Yes, block ads for me' 1300 | Out-Null
  Press 'Continue' 1700 | Out-Null

  # Default browser: never pressed on a shared machine.
  Hover 'Set as default' 900 | Out-Null
  Press 'Not now' 2200 | Out-Null

  # Done
  Press 'Start browsing' 3500 | Out-Null
  Note 'finished'
} catch {
  Note "stopped: $($_.Exception.Message)"
} finally {
  New-Item -ItemType File -Force $stop | Out-Null
  $result = Receive-Job -Job $grabber -Wait -AutoRemoveJob
  Note "grabber: $result"
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 800
  Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$data*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
