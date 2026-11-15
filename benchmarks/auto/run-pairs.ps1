# Windows PowerShell 5.1. Automated alternating baseline/candidate timing driver.
#   .\run-pairs.ps1 -BaselineExe C:\b\bowser.exe -CandidateExe C:\c\bowser.exe [-Pairs 10] [-Workloads startup,new_tab,...]
# Both executables must carry benchmarks/auto/instrumentation.patch. Then: node analyze.mjs <OutDir>
#
# SAFETY: keyboard/mouse input is sent only while GetForegroundWindow() is the main window of the
# process THIS script launched, re-verified before every individual input call; otherwise the
# sample is aborted. Only processes this script started (by object/PID) are ever stopped.
# Timed intervals contain only Start-Sleep and cheap fixture-file reads: no CIM, no process
# enumeration. Window lookup (EnumWindows) happens outside the timed intervals except startup,
# where it begins only after the fixture's ready receipt has arrived.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BaselineExe,
    [Parameter(Mandatory)][string]$CandidateExe,
    [ValidateRange(1,200)][int]$Pairs = 10,
    [ValidateSet('startup','new_tab','tab_switch','omnibox_enter','settings_open','conversation')][string[]]$Workloads = @('startup','new_tab','omnibox_enter','settings_open','tab_switch','conversation'),
    [string]$OutDir,
    [string]$TempRoot = $env:TEMP,
    [int]$SleepWaitSeconds = 30,
    [int]$IdleSeconds = 5,
    [switch]$KeepData
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not [Diagnostics.Stopwatch]::IsHighResolution) { throw 'High-resolution QPC required' }
$FreqTicks = [Diagnostics.Stopwatch]::Frequency
foreach ($exe in @($BaselineExe, $CandidateExe)) { if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Missing executable: $exe" } }
$BaselineExe = [IO.Path]::GetFullPath($BaselineExe); $CandidateExe = [IO.Path]::GetFullPath($CandidateExe)
if ($BaselineExe -ieq $CandidateExe) { throw 'Baseline and candidate must differ' }
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (-not $OutDir) { $OutDir = Join-Path $TempRoot "bowser-auto-$stamp" }
$OutDir = [IO.Path]::GetFullPath($OutDir)
$DataRoot = Join-Path $TempRoot "bowser-auto-data-$stamp"
New-Item -ItemType Directory -Force -Path (Join-Path $OutDir 'logs') | Out-Null
New-Item -ItemType Directory -Force -Path $DataRoot | Out-Null
$SamplesPath = Join-Path $OutDir 'samples.jsonl'
$FixtureLog = Join-Path $OutDir 'fixture-log.jsonl'

if (-not ('BowserAutoInput' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class BowserAutoInput {
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x,y; }
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx,dy; public uint mouseData,dwFlags,time; public UIntPtr dwExtraInfo; }
 [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk,wScan; public uint dwFlags,time; public UIntPtr dwExtraInfo; }
 [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
 [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public UNION u; }
 delegate bool EnumProc(IntPtr h, IntPtr l);
 [DllImport("user32.dll", SetLastError=true)] static extern uint SendInput(uint n, INPUT[] inputs, int size);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
 [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
 [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint flags);
 [DllImport("user32.dll")] static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
 static int Size() { return Marshal.SizeOf(typeof(INPUT)); }
 static INPUT Key(ushort vk, ushort scan, uint flags) { var i = new INPUT { type = 1 }; i.u.ki = new KEYBDINPUT { wVk = vk, wScan = scan, dwFlags = flags }; return i; }
 static INPUT Mouse(uint flags, uint data) { var i = new INPUT { type = 0 }; i.u.mi = new MOUSEINPUT { dwFlags = flags, mouseData = data }; return i; }
 // Main native window of a process, or Zero.
 public static IntPtr FindMain(uint pid) {
  IntPtr found = IntPtr.Zero;
  EnumWindows((h, l) => {
   uint p; GetWindowThreadProcessId(h, out p);
   if (p != pid || !IsWindowVisible(h)) return true;
   var sb = new StringBuilder(128); GetClassName(h, sb, 128);
   if (sb.ToString() == "BowserNativeWindow") { found = h; return false; }
   return true;
  }, IntPtr.Zero);
  return found;
 }
 public static uint ProcessOf(IntPtr h) { uint p; if (h == IntPtr.Zero) return 0; GetWindowThreadProcessId(h, out p); return p; }
 public static int HeldKeys() {
  int n = 0; foreach (int k in new[] { 1, 2, 4, 0x10, 0x11, 0x12, 0x5B, 0x5C }) if ((GetAsyncKeyState(k) & 0x8000) != 0) n++;
  return n;
 }
 public static uint Dpi(IntPtr h) { return GetDpiForWindow(h); }
 // Screen-space client rectangle: left, top, width, height.
 public static int[] ClientScreen(IntPtr h) {
  RECT r; if (!GetClientRect(h, out r)) return null;
  POINT p = new POINT { x = 0, y = 0 }; if (!ClientToScreen(h, ref p)) return null;
  return new[] { p.x, p.y, r.Right - r.Left, r.Bottom - r.Top };
 }
 public static bool PointOwnedBy(IntPtr root, int x, int y) {
  IntPtr hit = WindowFromPoint(new POINT { x = x, y = y });
  return hit != IntPtr.Zero && GetAncestor(hit, 2) == root;
 }
 // One atomic chord: modifiers/keys down in order, then up in reverse.
 public static uint Chord(ushort[] vks) {
  var list = new INPUT[vks.Length * 2];
  for (int i = 0; i < vks.Length; i++) { list[i] = Key(vks[i], 0, 0); list[vks.Length * 2 - 1 - i] = Key(vks[i], 0, 2); }
  return SendInput((uint)list.Length, list, Size());
 }
 public static uint Unicode(char c) { return SendInput(2, new[] { Key(0, c, 4), Key(0, c, 4 | 2) }, Size()); }
 public static uint Wheel(int delta) { return SendInput(1, new[] { Mouse(0x0800, (uint)delta) }, Size()); }
 public static uint Click() { return SendInput(2, new[] { Mouse(0x0002, 0), Mouse(0x0004, 0) }, Size()); }
 public static bool MoveCursor(int x, int y) { return SetCursorPos(x, y); }
 public static bool Close(IntPtr h) { return PostMessage(h, 0x0010, IntPtr.Zero, IntPtr.Zero); }
}
'@
}

function Get-Qpc { [Diagnostics.Stopwatch]::GetTimestamp() }

# ---------- fixture server (production-fixture-server.mjs + benchmarks/auto/auto-fixture.mjs) ----------
$script:Server = $null
$script:FixtureOffset = 0L
function Start-Fixture {
    $node = (Get-Command node -ErrorAction Stop).Source
    $portFile = Join-Path $OutDir 'fixture-port.txt'
    $serverJs = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\scripts\production-fixture-server.mjs'))
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $node; $psi.Arguments = "`"$serverJs`" `"$portFile`" `"$FixtureLog`""
    $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
    $script:Server = [Diagnostics.Process]::Start($psi)
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    while (-not (Test-Path -LiteralPath $portFile)) {
        if ($script:Server.HasExited -or $deadline.Elapsed.TotalSeconds -gt 10) { throw 'Fixture server did not start' }
        Start-Sleep -Milliseconds 50
    }
    Start-Sleep -Milliseconds 50
    'http://127.0.0.1:' + (Get-Content -LiteralPath $portFile -Raw).Trim()
}
function Stop-Fixture { if ($script:Server -and -not $script:Server.HasExited) { try { $script:Server.Kill() } catch {} } }

# Rows appended to the fixture log since the previous call.
function Read-FixtureRows {
    if (-not (Test-Path -LiteralPath $FixtureLog)) { return @() }
    $fs = New-Object IO.FileStream($FixtureLog, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    try {
        if ($fs.Length -le $script:FixtureOffset) { return @() }
        [void]$fs.Seek($script:FixtureOffset, [IO.SeekOrigin]::Begin)
        $bytes = New-Object byte[] ($fs.Length - $script:FixtureOffset)
        $read = $fs.Read($bytes, 0, $bytes.Length)
        $text = [Text.Encoding]::UTF8.GetString($bytes, 0, $read)
        $cut = $text.LastIndexOf("`n")
        if ($cut -lt 0) { return @() }
        $script:FixtureOffset += [Text.Encoding]::UTF8.GetByteCount($text.Substring(0, $cut + 1))
        @($text.Substring(0, $cut).Split("`n") | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json })
    } finally { $fs.Dispose() }
}
function Wait-Fixture([scriptblock]$Match, [int]$TimeoutMs, [string]$What) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    do {
        foreach ($row in (Read-FixtureRows)) { if (& $Match $row) { return $row } }
        Start-Sleep -Milliseconds 40
    } while ($clock.ElapsedMilliseconds -lt $TimeoutMs)
    throw "ABORT:fixture_timeout:$What"
}
function Wait-Ready([string]$Token, [int]$TimeoutMs = 30000) {
    Wait-Fixture { param($r) $r.path -eq '/ready' -and $r.token -eq $Token -and $r.event -eq 'request_arrival' } $TimeoutMs "ready_$Token" | Out-Null
}

# ---------- per-sample context and guarded input ----------
function New-Token { 't' + [guid]::NewGuid().ToString('N').Substring(0, 20) }

function Assert-Owned($ctx) {
    if ($ctx.proc.HasExited) { throw 'ABORT:process_exited' }
    $fg = [BowserAutoInput]::GetForegroundWindow()
    if ($ctx.hwnd -eq [IntPtr]::Zero -or $fg -ne $ctx.hwnd -or [BowserAutoInput]::ProcessOf($fg) -ne [uint32]$ctx.proc.Id) { throw 'ABORT:foreground_not_owned' }
    if ([BowserAutoInput]::HeldKeys() -ne 0) { throw 'ABORT:nonneutral_keys_or_buttons' }
}
function Mark($ctx, [string]$Name) { $ctx.marks.Add(@{ name = $Name; qpc = (Get-Qpc) }) }

function Send-Chord($ctx, [uint16[]]$Keys, [string]$MarkName) {
    Assert-Owned $ctx
    if ($MarkName) { Mark $ctx $MarkName }
    if ([BowserAutoInput]::Chord($Keys) -ne ($Keys.Count * 2)) { throw 'ABORT:sendinput_failed' }
}
function Send-Text($ctx, [string]$Text, [int]$DelayMs = 8) {
    foreach ($c in $Text.ToCharArray()) {
        Assert-Owned $ctx
        if ([BowserAutoInput]::Unicode($c) -ne 2) { throw 'ABORT:sendinput_failed' }
        if ($DelayMs -gt 0) { Start-Sleep -Milliseconds $DelayMs }
    }
}
function Move-OwnedCursor($ctx, [double]$FracX, [double]$FracY) {
    Assert-Owned $ctx
    $r = [BowserAutoInput]::ClientScreen($ctx.hwnd)
    if (-not $r) { throw 'ABORT:client_rect' }
    $x = [int]($r[0] + $r[2] * $FracX); $y = [int]($r[1] + $r[3] * $FracY)
    if (-not [BowserAutoInput]::PointOwnedBy($ctx.hwnd, $x, $y)) { throw 'ABORT:point_not_owned' }
    [void][BowserAutoInput]::MoveCursor($x, $y)
}
function Ensure-Foreground($ctx) {
    $clock = [Diagnostics.Stopwatch]::StartNew(); $nudged = $false
    while ($clock.Elapsed.TotalSeconds -lt 6) {
        if ($ctx.proc.HasExited) { throw 'ABORT:process_exited' }
        $fg = [BowserAutoInput]::GetForegroundWindow()
        if ($fg -eq $ctx.hwnd) { return }
        # One plain SetForegroundWindow on the window we launched; no input is sent.
        if (-not $nudged -and $clock.ElapsedMilliseconds -gt 400) { [void][BowserAutoInput]::SetForegroundWindow($ctx.hwnd); $nudged = $true }
        Start-Sleep -Milliseconds 50
    }
    throw 'ABORT:never_foreground'
}
function Find-MainWindow($ctx) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.Elapsed.TotalSeconds -lt 15) {
        if ($ctx.proc.HasExited) { throw 'ABORT:process_exited' }
        $h = [BowserAutoInput]::FindMain([uint32]$ctx.proc.Id)
        if ($h -ne [IntPtr]::Zero) {
            $ctx.hwnd = $h
            if ([BowserAutoInput]::Dpi($h) -ne 96) { throw 'ABORT:dpi_not_96' }
            return
        }
        Start-Sleep -Milliseconds 100
    }
    throw 'ABORT:no_main_window'
}

function Start-Bowser([string]$Exe, [string]$Id, [string[]]$ExtraArgs) {
    $data = Join-Path $DataRoot $Id; $logRel = "logs\$Id.csv"; $log = Join-Path $OutDir $logRel
    New-Item -ItemType Directory -Force -Path $data | Out-Null
    # Both arms: pre-answer the first-run import offer (default profile root == data dir) so no modal steals input.
    [IO.File]::WriteAllText((Join-Path $data 'import-offer.txt'), "answered`n", (New-Object Text.UTF8Encoding($false)))
    $argv = @('--data-dir', $data, '--diagnostic-log', $log) + $ExtraArgs
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe; $psi.WorkingDirectory = Split-Path -Parent $Exe; $psi.UseShellExecute = $false
    $psi.Arguments = ($argv | ForEach-Object { '"' + ($_ -replace '"', '\"') + '"' }) -join ' '
    $ctx = @{ proc = $null; hwnd = [IntPtr]::Zero; marks = (New-Object 'System.Collections.Generic.List[object]'); log = $logRel; logPath = $log; data = $data }
    Mark $ctx 'launch'
    $ctx.proc = [Diagnostics.Process]::Start($psi)
    $ctx
}
# Graceful WM_CLOSE to the verified window of OUR process so PerfLog flushes on drop; kill only our object.
function Stop-Bowser($ctx) {
    $flushed = $false
    if ($ctx.proc) {
        try {
            if (-not $ctx.proc.HasExited) {
                if ($ctx.hwnd -ne [IntPtr]::Zero -and [BowserAutoInput]::IsWindow($ctx.hwnd) -and [BowserAutoInput]::ProcessOf($ctx.hwnd) -eq [uint32]$ctx.proc.Id) { [void][BowserAutoInput]::Close($ctx.hwnd) }
                if (-not $ctx.proc.WaitForExit(20000)) { $ctx.proc.Kill(); [void]$ctx.proc.WaitForExit(5000) }
                else { $flushed = $true }
            } else { $flushed = $true }
        } catch {}
    }
    $flushed -and (Test-Path -LiteralPath $ctx.logPath) -and ((Get-Item -LiteralPath $ctx.logPath).Length -gt 0)
}

function Add-Receipt($Sample) { [IO.File]::AppendAllText($SamplesPath, ((ConvertTo-Json -InputObject $Sample -Compress -Depth 8) + "`n"), (New-Object Text.UTF8Encoding($false))) }

# ---------- workload groups ----------
$VK_CTRL = [uint16]0x11; $VK_T = [uint16]0x54; $VK_L = [uint16]0x4C; $VK_1 = [uint16]0x31; $VK_ENTER = [uint16]0x0D

function Invoke-Group([string]$Arm, [string]$Exe, [int]$Pair, [string]$Group, [string]$Base) {
    $id = "$Pair-$Arm-$Group"
    $receipts = @{}   # workload -> receipt
    $status = 'ok'; $reason = ''
    $tok = @{}
    $extra = @(); $workloadsHere = @()
    switch ($Group) {
        'ui' { $workloadsHere = @($Workloads | Where-Object { $_ -in 'startup','new_tab','omnibox_enter','settings_open' }); $tok.startup = New-Token; $tok.target = New-Token; $extra = @('--open-url', "$Base/light?token=$($tok.startup)") }
        'tab_switch' { $workloadsHere = @('tab_switch'); $tok.sleeper = New-Token; $tok.other = New-Token; $extra = @('--benchmark-memory-pressure', 'on', '--open-url', "$Base/light?token=$($tok.sleeper)") }
        'conversation' { $workloadsHere = @('conversation'); $tok.conversation = New-Token; $extra = @('--open-url', "$Base/auto-conversation?token=$($tok.conversation)") }
    }
    if (-not $workloadsHere.Count) { return }
    $ctx = $null; $done = @()
    try {
        [void](Read-FixtureRows)   # skip rows from earlier samples
        $ctx = Start-Bowser $Exe $id $extra
        switch ($Group) {
            'ui' {
                Wait-Ready $tok.startup 40000
                $done += 'startup'
                Find-MainWindow $ctx; Ensure-Foreground $ctx; Start-Sleep -Milliseconds 800
                if ('new_tab' -in $workloadsHere) {
                    Send-Chord $ctx @($VK_CTRL, $VK_T) 'new_tab'; Start-Sleep -Milliseconds 1500; $done += 'new_tab'
                }
                if ('omnibox_enter' -in $workloadsHere) {
                    Send-Chord $ctx @($VK_CTRL, $VK_L) $null; Start-Sleep -Milliseconds 250
                    Send-Text $ctx "$Base/light?token=$($tok.target)" 6; Start-Sleep -Milliseconds 400
                    [void](Read-FixtureRows)
                    Send-Chord $ctx @($VK_ENTER) 'omnibox_enter'
                    Wait-Ready $tok.target 15000; Start-Sleep -Milliseconds 700; $done += 'omnibox_enter'
                }
                if ('settings_open' -in $workloadsHere) {
                    Send-Chord $ctx @($VK_CTRL, $VK_L) $null; Start-Sleep -Milliseconds 250
                    Send-Text $ctx 'bowser://settings' 6; Start-Sleep -Milliseconds 400
                    Send-Chord $ctx @($VK_ENTER) 'settings_enter'
                    Start-Sleep -Milliseconds 2500; $done += 'settings_open'
                }
            }
            'tab_switch' {
                Wait-Ready $tok.sleeper 40000
                Find-MainWindow $ctx; Ensure-Foreground $ctx; Start-Sleep -Milliseconds 800
                Send-Chord $ctx @($VK_CTRL, $VK_T) $null; Start-Sleep -Milliseconds 600
                Send-Chord $ctx @($VK_CTRL, $VK_L) $null; Start-Sleep -Milliseconds 250
                Send-Text $ctx "$Base/light?token=$($tok.other)" 6; Start-Sleep -Milliseconds 400
                Send-Chord $ctx @($VK_ENTER) $null
                Wait-Ready $tok.other 15000
                # Nothing at all happens while the first tab is hidden and discarded under forced memory pressure.
                Start-Sleep -Seconds $SleepWaitSeconds
                [void](Read-FixtureRows)
                Send-Chord $ctx @($VK_CTRL, $VK_1) 'tab_switch'
                Wait-Ready $tok.sleeper 20000; Start-Sleep -Milliseconds 700; $done += 'tab_switch'
            }
            'conversation' {
                Wait-Fixture { param($r) $r.path -eq '/auto-stats' -and $r.token -eq $tok.conversation -and $r.payload -and $r.payload.phase -eq 'ready' } 40000 'conversation_ready' | Out-Null
                Find-MainWindow $ctx; Ensure-Foreground $ctx; Start-Sleep -Milliseconds 800
                # Give the page keyboard focus with real clicks on the composer (verified by its focus note).
                $focused = $false
                foreach ($fromBottom in 0.86, 0.90, 0.82, 0.93) {
                    Move-OwnedCursor $ctx 0.66 $fromBottom
                    Assert-Owned $ctx
                    if ([BowserAutoInput]::Click() -ne 2) { throw 'ABORT:sendinput_failed' }
                    Start-Sleep -Milliseconds 250
                    foreach ($row in (Read-FixtureRows)) { if ($row.path -eq '/auto-stats' -and $row.token -eq $tok.conversation -and $row.payload -and $row.payload.phase -eq 'composer_focus') { $focused = $true } }
                    if ($focused) { break }
                }
                if (-not $focused) { throw 'ABORT:composer_not_focused' }
                Start-Sleep -Seconds $IdleSeconds
                Mark $ctx 'interaction_start'
                $text = 'benchmark typing 20!'.Substring(0, 20)
                for ($i = 0; $i -lt 20; $i++) {
                    Assert-Owned $ctx
                    if ([BowserAutoInput]::Unicode($text[$i]) -ne 2) { throw 'ABORT:sendinput_failed' }
                    if ($i -eq 0) { Move-OwnedCursor $ctx 0.66 0.40 }
                    Assert-Owned $ctx
                    if ([BowserAutoInput]::Wheel(($(if ($i % 8 -lt 4) { 120 } else { -120 }))) -ne 1) { throw 'ABORT:sendinput_failed' }
                    Start-Sleep -Milliseconds 55
                }
                for ($i = 0; $i -lt 20; $i++) {
                    Assert-Owned $ctx
                    if ([BowserAutoInput]::Wheel(($(if ($i % 8 -lt 4) { -120 } else { 120 }))) -ne 1) { throw 'ABORT:sendinput_failed' }
                    Start-Sleep -Milliseconds 40
                }
                Mark $ctx 'interaction_end'
                Wait-Fixture { param($r) $r.path -eq '/auto-stats' -and $r.token -eq $tok.conversation -and $r.payload -and $r.payload.phase -eq 'stats' } 20000 'conversation_stats' | Out-Null
                $done += 'conversation'
            }
        }
    } catch {
        $status = 'aborted'
        $reason = if ($_.Exception.Message -like 'ABORT:*') { $_.Exception.Message.Substring(6) } else { 'error:' + $_.Exception.Message }
    }
    $flushed = $false
    if ($ctx) { $flushed = Stop-Bowser $ctx }
    foreach ($w in $workloadsHere) {
        $ok = ($status -eq 'ok') -or ($w -in $done)
        $tokens = @{}
        foreach ($k in @($tok.Keys)) { $tokens[$k] = $tok[$k] }
        $receipt = [ordered]@{
            id = "$Pair-$Arm-$w"; pair = $Pair; arm = $Arm; workload = $w; group = $Group
            status = $(if ($ok) { 'ok' } else { 'aborted' }); reason = $(if ($ok) { '' } else { $reason })
            exe = $Exe; pid = $(if ($ctx) { $ctx.proc.Id } else { 0 }); freq = $FreqTicks
            log = $(if ($ctx) { $ctx.log } else { $null }); log_flushed = $flushed
            tokens = $tokens; expect = @{ chars = 20 }
            marks = $(if ($ctx) { @($ctx.marks | ForEach-Object { $_ }) } else { @() })
        }
        # tab_switch/omnibox analysis names its own token roles
        if ($w -eq 'omnibox_enter') { $receipt.tokens = @{ target = $tok.target } }
        if ($w -eq 'startup') { $receipt.tokens = @{ startup = $tok.startup } }
        if ($w -eq 'tab_switch') { $receipt.tokens = @{ sleeper = $tok.sleeper; other = $tok.other } }
        Add-Receipt $receipt
    }
    if (-not $KeepData -and $ctx) { Remove-Item -LiteralPath $ctx.data -Recurse -Force -ErrorAction SilentlyContinue }
    Write-Host ("{0} {1}: {2}{3}" -f $id, ($workloadsHere -join '+'), $status, $(if ($reason) { " ($reason)" } else { '' }))
}

# ---------- main ----------
$groups = @()
if (@($Workloads | Where-Object { $_ -in 'startup','new_tab','omnibox_enter','settings_open' }).Count) { $groups += 'ui' }
if ('tab_switch' -in $Workloads) { $groups += 'tab_switch' }
if ('conversation' -in $Workloads) { $groups += 'conversation' }
# new_tab/omnibox_enter/settings_open ride on one launched process that starts with the startup fixture.
if ('startup' -notin $Workloads -and $groups -contains 'ui') { Write-Host 'note: startup is always timed as the first step of the ui group' }

$manifest = [ordered]@{
    started = (Get-Date).ToString('o'); pairs = $Pairs; workloads = $Workloads; freq = $FreqTicks
    ps_version = $PSVersionTable.PSVersion.ToString(); os = [Environment]::OSVersion.VersionString
    baseline = @{ exe = $BaselineExe; sha256 = (Get-FileHash -LiteralPath $BaselineExe).Hash }
    candidate = @{ exe = $CandidateExe; sha256 = (Get-FileHash -LiteralPath $CandidateExe).Hash }
    sleep_wait_seconds = $SleepWaitSeconds; idle_seconds = $IdleSeconds
}
$manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $OutDir 'manifest.json') -Encoding UTF8
Write-Host "Output: $OutDir"
$base = $null
try {
    $base = Start-Fixture
    foreach ($pair in 1..$Pairs) {
        # Alternate which arm goes first so slow drift does not favour one arm.
        $order = if ($pair % 2 -eq 1) { @('baseline', 'candidate') } else { @('candidate', 'baseline') }
        foreach ($group in $groups) {
            foreach ($arm in $order) {
                $exe = if ($arm -eq 'baseline') { $BaselineExe } else { $CandidateExe }
                Invoke-Group $arm $exe $pair $group $base
                Start-Sleep -Milliseconds 1500
            }
        }
    }
} finally {
    Stop-Fixture
    if (-not $KeepData) { Remove-Item -LiteralPath $DataRoot -Recurse -Force -ErrorAction SilentlyContinue }
}
Write-Host "Done. Analyze with: node `"$PSScriptRoot\analyze.mjs`" `"$OutDir`""
