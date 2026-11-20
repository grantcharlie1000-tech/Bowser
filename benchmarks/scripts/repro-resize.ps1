# Reproduces the "broken after a corner resize" report on a hosted Windows runner: Bowser on the Classic
# layout (tabs on top, toolbar below, bookmarks bar at its default: off) with a blank new tab in front, then real mouse
# drags of each window corner. After every drag it saves a screenshot of the window, the window's and the
# client's screen rectangles, and every child window's rectangle (the HTML chrome hosts) next to them.
# `BOWSER_PAINT_LOG` makes Bowser log where the overlay frames the page and where each child window is.
#
# Everything runs on a throwaway --data-dir. Evidence only.
#
# Usage: powershell -File repro-resize.ps1 [-Exe path] [-Out dir]
param(
  [string]$Exe = "$PSScriptRoot\..\..\target\release\bowser.exe",
  [string]$Out = "$PSScriptRoot\..\results\resize-repro"
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
if (-not ('RZ' -as [type])) {
Add-Type @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class RZ {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
  public delegate bool EP(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [DllImport("user32.dll")] static extern bool EnumWindows(EP p, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EP p, IntPtr l);
  [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  // The frame's hit code at a screen point, and the class of the window a press there would reach.
  public static int Hit(IntPtr h, int x, int y) { return (int)SendMessage(h, 0x84, IntPtr.Zero, (IntPtr)((y << 16) | (x & 0xFFFF))); }
  public static string Under(int x, int y) { POINT p = new POINT(); p.X = x; p.Y = y; return Cls(WindowFromPoint(p)); }
  public static string Cls(IntPtr h) { var s = new StringBuilder(128); GetClassName(h, s, 128); return s.ToString(); }
  public static IntPtr Find(uint pid) { IntPtr found = IntPtr.Zero; EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h) && Cls(h) == "BowserNativeWindow") { found = h; return false; } return true; }, IntPtr.Zero); return found; }
  public static void Force(IntPtr h) { IntPtr fg = GetForegroundWindow(); uint p; uint ft = GetWindowThreadProcessId(fg, out p); uint me = GetCurrentThreadId(); AttachThreadInput(me, ft, true); ShowWindow(h, 9); SetForegroundWindow(h); AttachThreadInput(me, ft, false); }
  // Direct children only, with their screen rectangles relative to the parent's client origin.
  public static string Children(IntPtr parent) {
    var sb = new StringBuilder(); POINT o = new POINT(); ClientToScreen(parent, ref o);
    EnumChildWindows(parent, (h, l) => {
      if (GetParent(h) != parent) return true;
      RECT r; GetWindowRect(h, out r);
      sb.AppendFormat("    {0,-40} vis={1} client=({2},{3},{4},{5})\n", Cls(h), IsWindowVisible(h) ? 1 : 0, r.L - o.X, r.T - o.Y, r.R - o.X, r.B - o.Y);
      return true; }, IntPtr.Zero);
    return sb.ToString();
  }
  // Bowser's own top-level windows (the click-through chrome overlay among them), in screen coordinates.
  public static string TopLevel(uint pid) {
    var sb = new StringBuilder();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { RECT r; GetWindowRect(h, out r); sb.AppendFormat("    {0,-40} screen=({1},{2},{3},{4})\n", Cls(h), r.L, r.T, r.R, r.B); } return true; }, IntPtr.Zero);
    return sb.ToString();
  }
}
"@ }
[RZ]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path; $Exe = (Resolve-Path $Exe).Path
$log = Join-Path $Out 'repro.log'
function Note([string]$text) { $line = "{0:HH:mm:ss.fff} {1}" -f (Get-Date), $text; Write-Host $line; Add-Content -Path $log -Value $line }

try { Set-DisplayResolution -Width 1600 -Height 900 -Force -ErrorAction Stop; Note 'display 1600x900' } catch { Note "display unchanged: $($_.Exception.Message)" }
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Note ("screen {0}x{1}" -f $screen.Width, $screen.Height)

# ---- Bowser on a fresh profile with the Classic layout ----
$data = Join-Path $(if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }) ("bowser-resize-" + (Get-Random))
New-Item -ItemType Directory -Force $data | Out-Null
$settings = "tab_layout=tabs-top`ncentred_address_bar=true`n"
[IO.File]::WriteAllText((Join-Path $data 'settings.txt'), $settings, (New-Object Text.UTF8Encoding $false))
# The first-run welcome is answered, so it never takes the new tab (profile.rs `import-offer.txt`).
[IO.File]::WriteAllText((Join-Path $data 'import-offer.txt'), "answered`n", (New-Object Text.UTF8Encoding $false))
# Dark apps, like the report's machine (a release build follows Windows).
$personalize = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
New-Item -Path $personalize -Force | Out-Null
Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value 0 -Type DWord
Set-ItemProperty -Path $personalize -Name SystemUsesLightTheme -Value 0 -Type DWord
$env:BOWSER_UI = 'html'
$env:BOWSER_PAINT_LOG = '1'
$paintLog = Join-Path $env:TEMP 'bowser-paint.log'
Remove-Item $paintLog -ErrorAction SilentlyContinue
$diag = Join-Path $Out 'bowser-diagnostic.log'
# A first page keeps the first-run welcome from taking the first tab (docs/design/welcome.md).
$p = Start-Process $Exe -ArgumentList @('--data-dir', $data, '--diagnostic-log', $diag, '--open-url', 'https://example.com/') -PassThru
$h = [IntPtr]::Zero
for ($i = 0; $i -lt 400 -and $h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 50; $h = [RZ]::Find([uint32]$p.Id) }
if ($h -eq [IntPtr]::Zero) { throw 'No Bowser window' }
Note "window pid $($p.Id)"

function IsFg { $fp = [uint32]0; [RZ]::GetWindowThreadProcessId([RZ]::GetForegroundWindow(), [ref]$fp) | Out-Null; return ($fp -eq [uint32]$p.Id) }
function EnsureFg { for ($i = 0; $i -lt 8; $i++) { if (IsFg) { return $true }; [RZ]::Force($h); Start-Sleep -Milliseconds 250 }; return (IsFg) }
function WinRect { $r = New-Object RZ+RECT; [RZ]::GetWindowRect($h, [ref]$r) | Out-Null; return $r }

$shot = 0
function Snap([string]$name, [switch]$Front) {
  $script:shot++
  if ($Front) { EnsureFg | Out-Null; Start-Sleep -Milliseconds 300 }
  $r = WinRect
  $c = New-Object RZ+RECT; [RZ]::GetClientRect($h, [ref]$c) | Out-Null
  $o = New-Object RZ+POINT; [RZ]::ClientToScreen($h, [ref]$o) | Out-Null
  $pad = 24
  $x = [Math]::Max(0, $r.L - $pad); $y = [Math]::Max(0, $r.T - $pad)
  $w = [Math]::Min($screen.Width, $r.R + $pad) - $x; $ht = [Math]::Min($screen.Height, $r.B + $pad) - $y
  $file = '{0:D2}-{1}.png' -f $script:shot, $name
  if ($w -gt 0 -and $ht -gt 0) {
    $bmp = New-Object System.Drawing.Bitmap $w, $ht
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($x, $y, 0, 0, (New-Object System.Drawing.Size $w, $ht))
    $bmp.Save((Join-Path $Out $file), [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
  }
  Note ("SNAP {0}: window=({1},{2},{3},{4}) client={5}x{6} client_origin=({7},{8}) shot_origin=({9},{10})" -f $file, $r.L, $r.T, $r.R, $r.B, $c.R, $c.B, $o.X, $o.Y, $x, $y)
  Add-Content -Path $log -Value ("  top-level windows:`n" + [RZ]::TopLevel([uint32]$p.Id) + "  children:`n" + [RZ]::Children($h))
}

function Glide([int]$x, [int]$y, [int]$ms = 600) {
  $from = New-Object RZ+POINT; [RZ]::GetCursorPos([ref]$from) | Out-Null
  $steps = [Math]::Max(10, [int]($ms / 16))
  for ($i = 1; $i -le $steps; $i++) {
    $t = $i / $steps
    [RZ]::SetCursorPos([int]($from.X + ($x - $from.X) * $t), [int]($from.Y + ($y - $from.Y) * $t)) | Out-Null
    Start-Sleep -Milliseconds 16
  }
}

# Drags the named corner by (dx, dy) with the real mouse, holding the button through the whole move.
function DragCorner([string]$corner, [int]$dx, [int]$dy) {
  if (-not (EnsureFg)) { Note 'not in front; skipping drag'; return }
  $r = WinRect
  # A point the frame answers with this corner's resize code (HTTOPLEFT 13, HTTOPRIGHT 14,
  # HTBOTTOMLEFT 16, HTBOTTOMRIGHT 17) and where a press reaches Bowser's own window or a resize grip
  # (`BowserFrameGrip`, 4 to 8 px in), never the page or a caption button.
  $want = @{ 'top-left' = 13; 'top-right' = 14; 'bottom-left' = 16; 'bottom-right' = 17 }[$corner]
  $found = $false
  foreach ($iy in 0, 1, 2, 3, 5, 6, 7) {
    foreach ($ix in 6, 5, 7, 2, 1, 0, 3) {
      $sx = if ($corner -like '*left') { $r.L + $ix } else { $r.R - 1 - $ix }
      $sy = if ($corner -like 'top*') { $r.T + $iy } else { $r.B - 1 - $iy }
      $code = [RZ]::Hit($h, $sx, $sy); $under = [RZ]::Under($sx, $sy)
      if ($code -eq $want -and ($under -eq 'BowserNativeWindow' -or $under -eq 'BowserFrameGrip')) { $found = $true; break }
    }
    if ($found) { break }
  }
  if (-not $found) { Note "no $corner resize point (last: hit $code under $under)"; return }
  Note "grab $corner at ($sx,$sy): hit $code on $under"
  [RZ]::SetCursorPos($sx, $sy) | Out-Null
  Start-Sleep -Milliseconds 400
  [RZ]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 150
  Glide ($sx + [int]($dx / 2)) ($sy + [int]($dy / 2)) 500
  Snap "$corner-mid-drag"
  Glide ($sx + $dx) ($sy + $dy) 500
  Start-Sleep -Milliseconds 200
  [RZ]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero)
  Note "dragged $corner by ($dx,$dy) from ($sx,$sy)"
  Start-Sleep -Milliseconds 1500
  Snap "$corner-after"
  # Off the window, so no hover state is in the picture.
  [RZ]::SetCursorPos(5, $screen.Height - 5) | Out-Null
}

function Reset {
  [RZ]::MoveWindow($h, 120, 80, 1200, 760, $true) | Out-Null
  Start-Sleep -Milliseconds 1500
}

try {
  Reset
  Start-Sleep -Seconds 5      # the HTML chrome comes up after the first native paint
  if (EnsureFg) { [Windows.Forms.SendKeys]::SendWait('^t'); Note 'new tab' }
  Start-Sleep -Seconds 3
  Snap 'start' -Front

  # The report: a corner dragged in until the window is about 700 px wide.
  foreach ($case in @(
      @('bottom-right', -500, -60), @('top-right', -500, 60), @('top-left', 500, 60), @('bottom-left', 500, -60),
      @('top-right', -500, -80), @('top-left', 500, -60))) {
    Reset
    DragCorner $case[0] $case[1] $case[2]
    # Back out again, in two steps, the way someone tries to get the window back.
    DragCorner $case[0] (-[int]($case[1] / 2)) (-[int]($case[2] / 2))
  }

  # The same sizes without the modal sizing loop.
  Reset
  [RZ]::MoveWindow($h, 120, 140, 700, 700, $true) | Out-Null
  Start-Sleep -Milliseconds 1500
  Snap 'movewindow-700' -Front
  Note 'finished'
} catch {
  Note "stopped: $($_.Exception.Message)"
} finally {
  try { $p.CloseMainWindow() | Out-Null; if (-not $p.WaitForExit(8000)) { Note 'no clean exit' } } catch { }
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 800
  Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$data*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  if (Test-Path $paintLog) { Copy-Item $paintLog (Join-Path $Out 'bowser-paint.log') }
}
