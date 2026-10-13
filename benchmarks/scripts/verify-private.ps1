# Real-app check for private tabs. Every launch uses --data-dir <fresh temp dir> and LOCALAPPDATA set to
# that same directory, so the real %LOCALAPPDATA%\Bowser is never touched. Only PIDs started here are
# stopped. Run: powershell -NoProfile -ExecutionPolicy Bypass -File verify-private.ps1 -Exe <bowser.exe>
param(
  [string]$Exe = "$PSScriptRoot\..\..\target-private\release\bowser.exe",
  [string]$Out = "$PSScriptRoot\..\results\surfaces\private",
  [int]$Port = 47613,
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
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern void mouse_event(uint f,int x,int y,uint d,int e);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
}
"@
[W]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path; $Exe = (Resolve-Path $Exe).Path
$dataRoot = Join-Path $env:TEMP "bowser-private-verify"
New-Item -ItemType Directory -Force $dataRoot | Out-Null
$serverLog = Join-Path $dataRoot "server.log"
$report = Join-Path $Out "verification.txt"
Remove-Item $serverLog -ErrorAction SilentlyContinue
"Private tab verification $(Get-Date -Format s)" | Set-Content $report
function Note($text) {
  Write-Host $text
  for ($try = 0; $try -lt 10; $try++) { try { Add-Content $report $text -ErrorAction Stop; return } catch { Start-Sleep -Milliseconds 200 } }
}

$env:BOWSER_UI = 'html'
$script:p = $null; $global:h = [IntPtr]::Zero; $script:data = $null; $script:n = 0
$base = "http://127.0.0.1:$Port"

# The probe server is started by this script and stopped by PID.
$serverScript = Join-Path $PSScriptRoot "private-probe-server.py"
if (Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue) { throw "Port $Port is already in use; pass another -Port" }
$server = Start-Process python -ArgumentList @('-I', $serverScript, $Port, $serverLog) -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 2
$null = Invoke-WebRequest "$base/whoami?who=selfcheck" -UseBasicParsing -TimeoutSec 5
if (-not (Test-Path $serverLog)) { throw "The probe server did not log the self-check request" }
Remove-Item $serverLog

function Launch([string]$name) {
  $script:n++
  $script:data = Join-Path $dataRoot ("$name-" + (Get-Random))
  New-Item -ItemType Directory -Force $script:data | Out-Null
  $env:LOCALAPPDATA = $script:data
  $script:p = Start-Process $Exe -ArgumentList @('--data-dir', $script:data) -PassThru
  $global:h = [IntPtr]::Zero
  for ($i = 0; $i -lt 60 -and $global:h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500; $script:p.Refresh(); $global:h = $script:p.MainWindowHandle }
  if ($global:h -eq [IntPtr]::Zero) { throw "No window" }
  [W]::ShowWindow($global:h, 9) | Out-Null
  Start-Sleep -Seconds 6
}
# Other agents may be driving their own Bowser windows on the same desktop. Input is sent only while
# this run's own window is the foreground window; any lapse marks the run as tainted so it is repeated.
$script:tainted = $false
function OwnsForeground { $fg = [W]::GetForegroundWindow(); $procId = [uint32]0; [void][W]::GetWindowThreadProcessId($fg, [ref]$procId); return ($procId -eq [uint32]$script:p.Id) }
# Returns $true when this run's window is the foreground window; never sends input otherwise.
function Focus {
  for ($i = 0; $i -lt 40; $i++) {
    # A background process may not take the foreground unless it shares input state with the current
    # foreground thread (or a key was just pressed), so attach to it for the call.
    $fg = [W]::GetForegroundWindow(); $fgPid = [uint32]0
    $fgThread = [W]::GetWindowThreadProcessId($fg, [ref]$fgPid); $me = [W]::GetCurrentThreadId()
    if ($i -ge 3) { [W]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [W]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero) }
    if ($fgThread -ne 0 -and $fgThread -ne $me) { [void][W]::AttachThreadInput($me, $fgThread, $true) }
    [W]::ShowWindow($global:h, 9) | Out-Null
    [W]::BringWindowToTop($global:h) | Out-Null
    [W]::SetForegroundWindow($global:h) | Out-Null
    if ($fgThread -ne 0 -and $fgThread -ne $me) { [void][W]::AttachThreadInput($me, $fgThread, $false) }
    Start-Sleep -Milliseconds 150
    if (OwnsForeground) { return $true }
    Start-Sleep -Milliseconds 350
  }
  $script:tainted = $true; Note "TAINTED: could not take the foreground (another window has it)"
  return $false
}
function Keys($k, $wait = 800) {
  if (-not (Focus)) { return }
  [Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds $wait
}
function Shot($name, [switch]$NoFocus) {
  # Popups close when the main window is re-activated, so a popup shot must not re-focus.
  if (-not $NoFocus) { [void](Focus) }
  Start-Sleep -Milliseconds 900
  if (-not (OwnsForeground)) { $script:tainted = $true; Note "TAINTED: foreground changed before the '$name' screenshot" }
  $r = New-Object W+RECT; [W]::GetWindowRect($global:h, [ref]$r) | Out-Null
  $w = $r.R - $r.L; $ht = $r.B - $r.T
  $bmp = New-Object Drawing.Bitmap $w, $ht
  $g = [Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save("$Out\$name.png", [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
  Write-Host "saved $name ($w x $ht)"
}
function ClickAt($rx, $ry, [switch]$Right) {
  if (-not (Focus)) { return }
  $r = New-Object W+RECT; [W]::GetWindowRect($global:h, [ref]$r) | Out-Null
  [W]::SetCursorPos($r.L + $rx, $r.T + $ry) | Out-Null; Start-Sleep -Milliseconds 200
  if ($Right) { [W]::mouse_event(8,0,0,0,0); [W]::mouse_event(16,0,0,0,0) } else { [W]::mouse_event(2,0,0,0,0); [W]::mouse_event(4,0,0,0,0) }
  Start-Sleep -Milliseconds 800
}
# Graceful close so every store is flushed the way a user's exit would; force only if it hangs.
function Quit {
  $id = $script:p.Id
  [void]$script:p.CloseMainWindow()
  if (-not $script:p.WaitForExit(20000)) { taskkill /PID $id /T /F | Out-Null }
  Start-Sleep 3
  Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$($script:data)*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Start-Sleep 2
}
# A scenario is repeated (fresh data dir each time) until a whole attempt kept the foreground.
function Scenario($name, [scriptblock]$act) {
  if ($Only -and $name -notlike "*$Only*") { return }
  for ($attempt = 1; $attempt -le 20; $attempt++) {
    Launch $name; $script:tainted = $false; [void](Focus); & $act
    Note "Scenario '$name' attempt $attempt tainted: $script:tainted"
    Quit
    if (-not $script:tainted) { return }
    Remove-Item $serverLog -ErrorAction SilentlyContinue
  }
  throw "Scenario '$name' could not run without losing the foreground"
}

# Searches every file under the data dir for the probe markers and lists what exists.
function Scan([string]$label, [string[]]$markers) {
  Note ""
  Note "== Data directory scan: $label ($script:data) =="
  $files = Get-ChildItem $script:data -Recurse -File -Force -ErrorAction SilentlyContinue
  $hits = @()
  foreach ($f in $files) {
    try { $bytes = [IO.File]::ReadAllBytes($f.FullName) } catch { try { $fs = [IO.File]::Open($f.FullName, 'Open', 'Read', 'ReadWrite'); $bytes = New-Object byte[] $fs.Length; [void]$fs.Read($bytes, 0, $bytes.Length); $fs.Close() } catch { continue } }
    $text = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
    foreach ($marker in $markers) {
      if ($text.Contains($marker)) { $hits += "$($f.FullName.Substring($script:data.Length)) contains '$marker'" }
    }
  }
  $top = Get-ChildItem $script:data -File -Force | ForEach-Object { $_.Name }
  Note "Top-level files: $($top -join ', ')"
  Note "Files in total: $($files.Count)"
  Note "Markers searched in every file: $($markers -join ', ')"
  if ($hits.Count -eq 0) { Note "RESULT: none of the markers is in any file." }
  else { Note "RESULT: probe markers found:"; $hits | ForEach-Object { Note "  $_" } }
  return $hits.Count
}
function ServerLog { if (Test-Path $serverLog) { Get-Content $serverLog } else { @() } }

try {
  Scenario 'private' {
    Keys '^+p' 1200
    Shot '01-private-tab-new'
    Keys "$base/?who=private{ENTER}" 4500
    Shot '02-private-tab-loaded'
    Keys '{F5}' 3500
    Note "Private tab visited / and reloaded (second request must carry the cookie)."
    Keys '^t' 900
    Keys "$base/whoami?who=normal{ENTER}" 4000
    Shot '03-private-and-normal-tabs'
    Note "A normal tab then loaded /whoami (must NOT carry the private cookie)."
    Keys '^+v' 2200
    Shot '04-vertical-sidebar'
    Keys '^+v' 1800
    Keys '^2' 800
    ClickAt 190 23
    Start-Sleep 1
    Shot '05-site-info-private' -NoFocus
    Keys '{ESC}' 500
    $pr = New-Object W+RECT; [W]::GetWindowRect($global:h, [ref]$pr) | Out-Null
    Keys '^w' 1500
    Note "Last private tab closed."
    Keys '^+p' 1200
    Keys "$base/?who=private{ENTER}" 4500
    Note "A new private tab loaded / again (must NOT carry the old cookie: the profile was discarded)."
    Shot '06-second-private-session'
    Keys '^+{DELETE}' 1800
    Shot '07-privacy-panel' -NoFocus
    Keys '{ESC}' 600
  }
  Note ""
  Note "== Server log (request, cookie header received) =="
  ServerLog | ForEach-Object { Note $_ }
  $privateScan = Scan 'private run (the only normal-tab visit was /whoami?who=normal, which is expected to be recorded)' @('bowser_probe', 'who=private')
  $normalScan = Scan 'private run, normal-tab marker (expected: present in history.txt)' @('who=normal')
  Note "history.txt present: $(Test-Path (Join-Path $script:data 'history.txt'))"

  # Menus and popups with a private tab active (screenshots only; the data checks are above).
  Scenario 'surfaces' {
    Keys '^+p' 1000
    Keys "$base/?who=surfaces{ENTER}" 4000
    ClickAt 815 23
    Start-Sleep 1
    Shot '08-main-menu' -NoFocus
    Keys '{ESC}' 600
    ClickAt 450 54 -Right
    Start-Sleep 1
    Shot '09-private-tab-context-menu' -NoFocus
    Keys '{ESC}' 600
    Keys '^+k' 1200
    Shot '10-command-palette' -NoFocus
    Keys '{ESC}' 600
  }
  Scenario 'control-normal' {
    Keys "$base/?who=control{ENTER}" 4500
    Keys '{F5}' 3000
    Note "Control: the same two requests in a NORMAL tab."
  }
  Note ""
  Note "== Server log after the control run (the last two requests are the control's visit and reload) =="
  ServerLog | ForEach-Object { Note $_ }
  $controlScan = Scan 'control (normal tab)' @('bowser_probe', 'who=control')
  Note ""
  Note "Control hits: $controlScan (this must be > 0, otherwise the scan proves nothing). Private hits: $privateScan (must be 0)."
}
finally {
  if ($server) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }
}
Write-Host "done"
