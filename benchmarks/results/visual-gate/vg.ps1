# Visual gate driver: one action per call so screenshots can be inspected between steps.
# Safety: every launch uses a fresh temp data dir passed as --data-dir AND as LOCALAPPDATA for the
# child; only the PID this script started (recorded in the state file) and the WebView2 children
# using its temp data dir are ever stopped. Keys/clicks are only sent while that PID is foreground.
#   vg.ps1 launch [-W 1280 (0 = app default)] [-Hgt 800] [-Url ...] [-Exe ...] [-Theme light|dark]
#   vg.ps1 shot -Name 01-foo [-Crop "x,y,w,h"]      (union rect of all visible Bowser windows)
#   vg.ps1 keys -Text '^t' [-Wait 600]
#   vg.ps1 type -Text 'https://example.com' [-Wait 600]
#   vg.ps1 click -X 100 -Y 20 [-Right] [-Double]    (window-relative physical px)
#   vg.ps1 move -X 100 -Y 20                        (hover)
#   vg.ps1 drag -X 100 -Y 20 -X2 300 -Y2 20
#   vg.ps1 size -W 1600 -Hgt 900 | snapleft | max | restore
#   vg.ps1 windows | quit
param(
  [Parameter(Position = 0)][string]$Action = 'windows',
  [string]$Name = '', [string]$Text = '', [int]$Wait = 600,
  [int]$X = 0, [int]$Y = 0, [int]$X2 = 0, [int]$Y2 = 0, [int]$W = 1280, [int]$Hgt = 800,
  [string]$Url = 'https://example.com/', [string]$Exe = '', [string]$Theme = '', [string]$Crop = '',
  [switch]$Right, [switch]$Double, [switch]$Main
)
$ErrorActionPreference = 'Stop'
$root = 'C:\Users\charlie\Documents\ChatGPT\Bowser'
if (-not $Exe) { $Exe = "$root\target-director\release\bowser.exe" }
$Out = "$root\benchmarks\results\visual-gate"
$stateFile = Join-Path $env:TEMP 'bowser-visual-gate-state.json'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
if (-not ('VG' -as [type])) {
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices; using System.Collections.Generic;
public static class VG {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  public delegate bool EP(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int ht, uint f);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, int e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, int e);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct PT { public int X, Y; }
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(PT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
  public static uint PidAt(int x, int y) { PT p; p.X = x; p.Y = y; IntPtr h = GetAncestor(WindowFromPoint(p), 2); uint pid; GetWindowThreadProcessId(h, out pid); return pid; }
  [DllImport("user32.dll")] static extern bool EnumWindows(EP p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  public static IntPtr Find(uint pid) { IntPtr found = IntPtr.Zero; EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var s = new StringBuilder(64); GetClassName(h, s, 64); if (s.ToString() == "BowserNativeWindow") { found = h; return false; } } return true; }, IntPtr.Zero); return found; }
  public static List<string> Visible(uint pid) { var list = new List<string>(); EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { var s = new StringBuilder(64); GetClassName(h, s, 64); RECT r; GetWindowRect(h, out r); list.Add(s.ToString() + "|" + r.L + "|" + r.T + "|" + r.R + "|" + r.B); } return true; }, IntPtr.Zero); return list; }
  public static void Force(IntPtr h) { IntPtr fg = GetForegroundWindow(); uint p; uint ft = GetWindowThreadProcessId(fg, out p); uint me = GetCurrentThreadId(); AttachThreadInput(me, ft, true); SetForegroundWindow(h); AttachThreadInput(me, ft, false); }
}
"@ }
[VG]::SetProcessDPIAware() | Out-Null

function Load { if (-not (Test-Path $stateFile)) { throw 'no state; launch first' }; return Get-Content $stateFile -Raw | ConvertFrom-Json }
function Hwnd($st) { $hw = [VG]::Find([uint32]$st.pid); if ($hw -eq [IntPtr]::Zero) { throw "Bowser window for pid $($st.pid) not found" }; return $hw }
function IsFg($st) { $fp = [uint32]0; [VG]::GetWindowThreadProcessId([VG]::GetForegroundWindow(), [ref]$fp) | Out-Null; return ($fp -eq [uint32]$st.pid) }
function EnsureFg($st, $hw) {
  # Other agents' test browsers may steal the foreground: wait (up to ~3 min) rather than type into them.
  for ($i = 0; $i -lt 400; $i++) { if (IsFg $st) { return }; [VG]::Force($hw); Start-Sleep -Milliseconds 450 }
  if (-not (IsFg $st)) { throw 'Bowser is not foreground; refusing input' }
}
function WinRect($hw) { $r = New-Object VG+RECT; [VG]::GetWindowRect($hw, [ref]$r) | Out-Null; return $r }
function Grab([int]$x, [int]$y, [int]$w, [int]$ht, [string]$file) {
  $bmp = New-Object Drawing.Bitmap $w, $ht
  $g = [Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
  $bmp.Save($file, [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
}

switch ($Action) {
  'launch' {
    if (Test-Path $stateFile) { $old = Get-Content $stateFile -Raw | ConvertFrom-Json; if (Get-Process -Id $old.pid -ErrorAction SilentlyContinue) { throw "previous run pid $($old.pid) still alive; quit first" } }
    $data = Join-Path $env:TEMP ("bowser-visual-gate-" + (Get-Date -Format 'HHmmss') + '-' + (Get-Random))
    New-Item -ItemType Directory -Force $data | Out-Null
    $env:LOCALAPPDATA = $data; $env:BOWSER_UI = 'html'; $env:BOWSER_DATA_DIR = $data
    if ($Theme) { $env:BOWSER_FORCE_THEME = $Theme }
    $a = @('--data-dir', "`"$data`""); if ($Url -ne 'none') { $a += @('--open-url', $Url) }
    $p = Start-Process $Exe -ArgumentList $a -PassThru
    @{ pid = $p.Id; data = $data; exe = $Exe } | ConvertTo-Json | Set-Content $stateFile -Encoding utf8
    $hw = [IntPtr]::Zero
    for ($i = 0; $i -lt 300 -and $hw -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 50; $hw = [VG]::Find([uint32]$p.Id) }
    if ($hw -eq [IntPtr]::Zero) { throw 'No Bowser window' }
    Start-Sleep -Milliseconds 1500; $hw = (Get-Process -Id $p.Id).MainWindowHandle; if ($W -gt 0) { [VG]::SetWindowPos($hw, [IntPtr]::Zero, 40, 30, $W, $Hgt, 0x14) | Out-Null }
    Write-Host ("launched pid {0} dpi {1} data {2}" -f $p.Id, [VG]::GetDpiForWindow($hw), $data)
  }
  'shot' {
    $st = Load; $hw = Hwnd $st; EnsureFg $st $hw; Start-Sleep -Milliseconds 400
    $wr = WinRect $hw
    # Refuse to capture while another process's window covers ours (other agents run test browsers too).
    for ($k = 0; $k -lt 200; $k++) {
      $bad = $false
      foreach ($pt in @(@(0.5,0.5), @(0.1,0.1), @(0.9,0.1), @(0.1,0.9), @(0.9,0.9), @(0.5,0.05), @(0.3,0.6), @(0.7,0.4))) {
        $px = [int]($wr.L + ($wr.R - $wr.L) * $pt[0]); $py = [int]($wr.T + ($wr.B - $wr.T) * $pt[1])
        if ([VG]::PidAt($px, $py) -ne [uint32]$st.pid) { $bad = $true }
      }
      if (-not $bad) { break }
      [VG]::Force($hw); Start-Sleep -Milliseconds 700
    }
    if ($bad) { throw 'another window covers Bowser; not capturing' }
    Start-Sleep -Milliseconds 150
    $wr = WinRect $hw; $x0 = $wr.L; $y0 = $wr.T; $x1 = $wr.R; $y1 = $wr.B
    if (-not $Main) {
      foreach ($v in [VG]::Visible([uint32]$st.pid)) { $f = $v.Split('|'); $l = [int]$f[1]; $t = [int]$f[2]; $rr = [int]$f[3]; $bb = [int]$f[4]; if ($rr - $l -lt 2 -or $bb - $t -lt 2) { continue }; if ($l -lt $x0) { $x0 = $l }; if ($t -lt $y0) { $y0 = $t }; if ($rr -gt $x1) { $x1 = $rr }; if ($bb -gt $y1) { $y1 = $bb } }
    }
    if ($Crop) { $c = $Crop.Split(',') | % { [int]$_ }; $x0 = $wr.L + $c[0]; $y0 = $wr.T + $c[1]; $x1 = $x0 + $c[2]; $y1 = $y0 + $c[3] }
    $sb = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    if ($x0 -lt 0) { $x0 = 0 }; if ($y0 -lt 0) { $y0 = 0 }; if ($x1 -gt $sb.Width) { $x1 = $sb.Width }; if ($y1 -gt $sb.Height) { $y1 = $sb.Height }
    $file = Join-Path $Out ("$Name.png")
    # Grab, then paint everything outside Bowser's own windows a flat magenta so other desktop windows never leak in.
    $bmp = New-Object Drawing.Bitmap ($x1 - $x0), ($y1 - $y0)
    $g = [Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($x0, $y0, 0, 0, $bmp.Size)
    $outside = New-Object Drawing.Region (New-Object Drawing.Rectangle 0, 0, ($x1 - $x0), ($y1 - $y0))
    $outside.Exclude((New-Object Drawing.Rectangle ($wr.L - $x0), ($wr.T - $y0), ($wr.R - $wr.L), ($wr.B - $wr.T)))
    foreach ($v in [VG]::Visible([uint32]$st.pid)) { $f = $v.Split('|'); $outside.Exclude((New-Object Drawing.Rectangle ([int]$f[1] - $x0), ([int]$f[2] - $y0), ([int]$f[3] - [int]$f[1]), ([int]$f[4] - [int]$f[2]))) }
    $g.FillRegion((New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb(255, 96, 0, 96))), $outside)
    $bmp.Save($file, [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
    Write-Host ("saved {0} ({1}x{2} at {3},{4}; window {5},{6})" -f $Name, ($x1 - $x0), ($y1 - $y0), $x0, $y0, $wr.L, $wr.T)
  }
  'keys' { $st = Load; $hw = Hwnd $st; EnsureFg $st $hw; [Windows.Forms.SendKeys]::SendWait($Text); Start-Sleep -Milliseconds $Wait }
  'type' {
    $st = Load; $hw = Hwnd $st
    foreach ($ch in $Text.ToCharArray()) { $s = [string]$ch; if ('+^%~(){}[]'.Contains($s)) { $s = '{' + $s + '}' }; EnsureFg $st $hw; [Windows.Forms.SendKeys]::SendWait($s); Start-Sleep -Milliseconds 35 }
    Start-Sleep -Milliseconds $Wait
  }
  'click' {
    $st = Load; $hw = Hwnd $st; EnsureFg $st $hw; $r = WinRect $hw
    for ($k = 0; $k -lt 300 -and [VG]::PidAt($r.L + $X, $r.T + $Y) -ne [uint32]$st.pid; $k++) { [VG]::Force($hw); Start-Sleep -Milliseconds 600 }
    if ([VG]::PidAt($r.L + $X, $r.T + $Y) -ne [uint32]$st.pid) { throw 'click point is covered by another process; refusing' }
    [VG]::SetCursorPos($r.L + $X, $r.T + $Y) | Out-Null; Start-Sleep -Milliseconds 150
    $n = 1; if ($Double) { $n = 2 }
    for ($i = 0; $i -lt $n; $i++) { if ($Right) { [VG]::mouse_event(8,0,0,0,0); [VG]::mouse_event(16,0,0,0,0) } else { [VG]::mouse_event(2,0,0,0,0); [VG]::mouse_event(4,0,0,0,0) }; Start-Sleep -Milliseconds 60 }
    Start-Sleep -Milliseconds $Wait
  }
  'move' {
    $st = Load; $hw = Hwnd $st; EnsureFg $st $hw; $wr = WinRect $hw; $sb = [Windows.Forms.Screen]::PrimaryScreen.Bounds
    $tx = $wr.L + $X; $ty = $wr.T + $Y
    for ($i = 4; $i -ge 0; $i--) { $px = $tx - $i * 3; $ax = [int]([double]$px * 65535 / ($sb.Width - 1)); $ay = [int]([double]$ty * 65535 / ($sb.Height - 1)); [VG]::mouse_event(0x8001, $ax, $ay, 0, 0); Start-Sleep -Milliseconds 40 }
    Start-Sleep -Milliseconds $Wait
  }
  'drag' {
    $st = Load; $hw = Hwnd $st; EnsureFg $st $hw; $r = WinRect $hw
    for ($k = 0; $k -lt 300 -and [VG]::PidAt($r.L + $X, $r.T + $Y) -ne [uint32]$st.pid; $k++) { [VG]::Force($hw); Start-Sleep -Milliseconds 600 }
    if ([VG]::PidAt($r.L + $X, $r.T + $Y) -ne [uint32]$st.pid) { throw 'click point is covered by another process; refusing' }
    [VG]::SetCursorPos($r.L + $X, $r.T + $Y) | Out-Null; Start-Sleep -Milliseconds 150; [VG]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 150
    for ($i = 1; $i -le 20; $i++) { [VG]::SetCursorPos($r.L + $X + [int](($X2 - $X) * $i / 20), $r.T + $Y + [int](($Y2 - $Y) * $i / 20)) | Out-Null; Start-Sleep -Milliseconds 30 }
    Start-Sleep -Milliseconds 200; [VG]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $Wait
  }
  'size' { $st = Load; $hw = (Get-Process -Id $st.pid).MainWindowHandle; if ([VG]::IsZoomed($hw)) { [VG]::ShowWindow($hw, 9) | Out-Null; Start-Sleep -Milliseconds 300 }; [VG]::SetWindowPos($hw, [IntPtr]::Zero, 40, 30, $W, $Hgt, 0x14) | Out-Null; Start-Sleep -Milliseconds 900; $r = WinRect $hw; Write-Host "rect $($r.L),$($r.T) $($r.R - $r.L)x$($r.B - $r.T)" }
  'snapleft' { $st = Load; $hw = Hwnd $st; EnsureFg $st $hw; [VG]::keybd_event(0x5B,0,0,0); [VG]::keybd_event(0x25,0,0,0); [VG]::keybd_event(0x25,0,2,0); [VG]::keybd_event(0x5B,0,2,0); Start-Sleep -Milliseconds 300; [VG]::keybd_event(0x1B,0,0,0); [VG]::keybd_event(0x1B,0,2,0); Start-Sleep -Milliseconds $Wait; $r = WinRect $hw; Write-Host "rect $($r.L),$($r.T) $($r.R - $r.L)x$($r.B - $r.T) zoomed=$([VG]::IsZoomed($hw))" }
  'rect' { $st = Load; $hw = Hwnd $st; $r = WinRect $hw; Write-Host "rect $($r.L),$($r.T) $($r.R - $r.L)x$($r.B - $r.T) zoomed=$([VG]::IsZoomed($hw))" }
  'windows' { $st = Load; [VG]::Visible([uint32]$st.pid) | % { Write-Host $_ }; Write-Host "fg=$(IsFg $st)" }
  'quit' {
    $st = Load
    if (Get-Process -Id $st.pid -ErrorAction SilentlyContinue) { Stop-Process -Id $st.pid -Force }
    Start-Sleep -Milliseconds 800
    Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$($st.data)*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    Remove-Item $stateFile; Write-Host "quit pid $($st.pid)"
  }
}
