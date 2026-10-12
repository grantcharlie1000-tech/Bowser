<#
.SYNOPSIS
  Cold/warm "process launch to a real page visible and usable" harness for Bowser.

.DESCRIPTION
  Launches bowser.exe with --benchmark-log, --benchmark-user-data-dir (isolated profile) and --open-url <url>,
  then records, per run, the Bowser event timeline (ms since process_start) plus outer wall clock.
  Marks reported (cumulative, ms from process_start, from Bowser's own event CSV):
    window_visible, chrome_painted, engine_started_for_chrome, env_ready (first webview_environment_result),
    chrome_controller_ready (first webview_controller_result), submitted (initial URL handed to the engine),
    content_controller_ready (last webview_controller_result before navigate_called), navigate_called,
    navigation_started, content_loading, navigation_completed (first), page_usable.
  Phase durations (ms) are derived from adjacent marks (see $PhaseDefs below).
  Outer wall clock: Stopwatch from just before Start-Process to (a) the BowserNativeWindow being visible and
  (b) the main window title first becoming the page title (polled every ~4 ms). Bowser buffers its event log
  until exit, so page_usable cannot be watched live; the harness waits -GraceMs after the title appears, then
  closes the window and reads the log. A run is valid only if the log shows page_usable or navigation_completed.

  -Mode cold : a brand-new profile directory every run (no WebView2 cache, no cookies, empty profile).
  -Mode warm : one untimed priming launch (same URL) builds a template profile; every timed run starts from a fresh
               copy of that template (HTTP cache, cookies and WebView2 state from the priming visit present).
  -Mode both : cold block then warm block per site.
  OS file caches are NOT flushed, so "cold" means a fresh Bowser/WebView2 profile, not a cold disk.

  Sites: 'fixture' is benchmarks\fixtures\light.html served from an in-process loopback server (pure Bowser
  overhead, no network). The others are real sites; their numbers include DNS, TLS, server and page weight.

  Output (under -OutputDirectory): runs.csv (every run), summary.json (median/p95/min/max per metric per
  site/mode), summary.md (tables), logs\ (event CSVs), plus machine metadata. Never kills processes: a Bowser
  process group that does not exit after WM_CLOSE is reported and its profile is kept.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $BowserExecutable,
    [string] $Label = 'current',
    [ValidateSet('cold','warm','both')] [string] $Mode = 'both',
    [string[]] $Sites = @('fixture','example','wikipedia','github','bbc'),
    [ValidateRange(1,500)] [int] $Runs = 15,
    [ValidateRange(1,500)] [int] $RealSiteRuns = 8,
    [ValidateRange(5,120)] [int] $UsableTimeoutSeconds = 45,
    [ValidateSet('html','native','default')] [string] $Ui = 'default',
    [ValidateRange(0,10000)] [int] $GraceMs = 2500,
    [ValidateRange(0,30000)] [int] $RealSiteGraceMs = 5000,
    [ValidateRange(1,100)] [int] $QuietCpuPct = 35,
    [ValidateRange(0,600)] [int] $QuietWaitSeconds = 60,
    [string] $OutputDirectory
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $PSScriptRoot ("results\launch-to-usable-{0}" -f (Get-Date -Format 'yyyyMMdd-HHmmss')) }

if (-not ('BowserL2U.Native' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
namespace BowserL2U {
public static class Native {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  public static string GetTitle(IntPtr h) { var sb = new StringBuilder(512); GetWindowText(h, sb, 512); return sb.ToString(); }
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public static IntPtr FindMain(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) { var sb = new StringBuilder(64); GetClassName(h, sb, 64);
        if (sb.ToString() == "BowserNativeWindow") { found = h; return false; } }
      return true; }, IntPtr.Zero);
    return found;
  }
}
// Loopback-only static server for files directly inside one directory (GET only).
public sealed class Fixture {
  private readonly TcpListener listener; private readonly string root; private volatile bool running;
  public Fixture(string root) { this.root = Path.GetFullPath(root).TrimEnd('\\') + "\\"; listener = new TcpListener(IPAddress.Loopback, 0); }
  public int Port { get { return ((IPEndPoint)listener.LocalEndpoint).Port; } }
  public void Start() { listener.Start(); running = true; var t = new Thread(Loop); t.IsBackground = true; t.Start(); }
  public void Stop() { running = false; try { listener.Stop(); } catch (Exception) { } }
  private void Loop() {
    while (running) {
      TcpClient c; try { c = listener.AcceptTcpClient(); } catch (Exception) { break; }
      ThreadPool.QueueUserWorkItem(delegate(object s) { Serve((TcpClient)s); }, c);
    }
  }
  private void Serve(TcpClient client) {
    using (client) { try {
      client.ReceiveTimeout = 5000; client.SendTimeout = 5000;
      var stream = client.GetStream(); var buf = new byte[8192]; int n = 0; string head = null;
      while (n < buf.Length) { int r = stream.Read(buf, n, buf.Length - n); if (r <= 0) break; n += r;
        string txt = Encoding.ASCII.GetString(buf, 0, n); if (txt.Contains("\r\n\r\n")) { head = txt; break; } }
      if (head == null) return;
      string[] rl = head.Split(new[] { "\r\n" }, StringSplitOptions.None)[0].Split(' ');
      if (rl.Length != 3 || rl[0] != "GET") { Send(stream, 405, "text/plain", new byte[0]); return; }
      string target = rl[1]; int q = target.IndexOfAny(new[] { '?', '#' }); if (q >= 0) target = target.Substring(0, q);
      string name = target.StartsWith("/") ? target.Substring(1) : "";
      if (name.Length == 0 || name.IndexOfAny(new[] { '/', '\\', ':', '%' }) >= 0 || name.Contains("..")) { Send(stream, 404, "text/plain", new byte[0]); return; }
      string full = Path.GetFullPath(Path.Combine(root, name));
      if (!full.StartsWith(root, StringComparison.OrdinalIgnoreCase) || !File.Exists(full)) { Send(stream, 404, "text/plain", new byte[0]); return; }
      Send(stream, 200, full.EndsWith(".html", StringComparison.OrdinalIgnoreCase) ? "text/html; charset=utf-8" : "application/octet-stream", File.ReadAllBytes(full));
    } catch (Exception) { } }
  }
  private static void Send(NetworkStream s, int code, string type, byte[] body) {
    byte[] h = Encoding.ASCII.GetBytes("HTTP/1.1 " + code + " X\r\nContent-Type: " + type + "\r\nContent-Length: " + body.Length + "\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n");
    s.Write(h, 0, h.Length); s.Write(body, 0, body.Length);
  }
}
}
'@
}

$WM_CLOSE = 0x0010
$exe = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($BowserExecutable)
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Bowser executable not found: $exe" }
$exeHash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
$out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
$logRoot = Join-Path $out 'logs'
New-Item -ItemType Directory -Force $logRoot | Out-Null
$work = Join-Path $env:TEMP ("bowser-l2u-{0}" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$script:allClean = $true
$Sites = @($Sites | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
$inv = [Globalization.CultureInfo]::InvariantCulture

$SiteUrls = [ordered]@{
    example   = 'https://example.com/'
    wikipedia = 'https://en.wikipedia.org/wiki/Web_browser'
    github    = 'https://github.com/'
    bbc       = 'https://www.bbc.co.uk/news'
}

# Phase durations: name, start mark, end mark (marks are cumulative ms from process_start).
$PhaseDefs = @(
    @('p_window_visible',        'process_start',            'window_visible'),
    @('p_chrome_paint',          'window_visible',           'chrome_painted'),
    @('p_env_create',            'env_requested',            'env_ready'),
    @('p_chrome_controller',     'chrome_controller_requested','chrome_controller_ready'),
    @('p_content_controller',    'content_controller_requested','content_controller_ready'),
    @('p_to_submitted',          'process_start',            'submitted'),
    @('p_submit_to_navigate',    'submitted',                'navigate_called'),
    @('p_navigate_to_started',   'navigate_called',          'navigation_started'),
    @('p_started_to_loading',    'navigation_started',       'content_loading'),
    @('p_loading_to_completed',  'content_loading',          'navigation_completed'),
    @('p_completed_to_usable',   'navigation_completed',     'page_usable'),
    @('p_submit_to_usable',      'submitted',                'usable')
)
$MarkOrder = @('window_visible','chrome_painted','engine_started_for_chrome','env_ready','chrome_controller_ready','submitted','content_controller_ready','navigate_called','navigation_started','content_loading','navigation_completed','page_usable','usable')

function Get-Group([int] $RootPid, [string] $ProfileDir) {
    $all = @(Get-CimInstance Win32_Process)
    $ids = New-Object 'System.Collections.Generic.HashSet[int]'; [void]$ids.Add($RootPid)
    foreach ($p in $all) { if ($p.CommandLine -and $p.CommandLine.IndexOf($ProfileDir, [StringComparison]::OrdinalIgnoreCase) -ge 0) { [void]$ids.Add([int]$p.ProcessId) } }
    $changed = $true
    while ($changed) { $changed = $false; foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId) -and -not $ids.Contains([int]$p.ProcessId)) { [void]$ids.Add([int]$p.ProcessId); $changed = $true } } }
    @($all | Where-Object { $ids.Contains([int]$_.ProcessId) })
}
# Posts WM_CLOSE and waits for the whole process group. If bowser.exe itself has exited but WebView2 helper
# processes for THIS run's profile directory (our own temp dir) linger for 8 s, they are orphans of a Bowser
# shutdown race; they are stopped so the profile can be removed/reused, and counted in $script:orphansStopped.
# Nothing else is ever killed, and a still-running bowser.exe is never killed.
$script:orphansStopped = 0
function Stop-Bowser($Run, [string] $ProfileDir) {
    if ($Run.Main.ToInt64() -ne 0) { [void][BowserL2U.Native]::PostMessage($Run.Main, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) }
    $deadline = (Get-Date).AddSeconds(60)
    $hostGoneAt = $null
    while ((Get-Date) -lt $deadline) {
        $g = @(Get-Group $Run.Process.Id $ProfileDir)
        if ($g.Count -eq 0) { return $true }
        $hostAlive = @($g | Where-Object { $_.ProcessId -eq $Run.Process.Id }).Count -gt 0
        if (-not $hostAlive) {
            if ($null -eq $hostGoneAt) { $hostGoneAt = Get-Date }
            elseif (((Get-Date) - $hostGoneAt).TotalSeconds -ge 8 -and @($g | Where-Object { $_.Name -ne 'msedgewebview2.exe' }).Count -eq 0) {
                foreach ($o in $g) { try { Stop-Process -Id $o.ProcessId -Force -ErrorAction Stop } catch { } }
                $script:orphansStopped += $g.Count
                Write-Warning ("bowser.exe exited but {0} WebView2 process(es) for its profile lingered 8 s; stopped them (orphan count so far {1})." -f $g.Count, $script:orphansStopped)
                Start-Sleep -Milliseconds 500
                return $true
            }
        }
        Start-Sleep -Milliseconds 200
    }
    $surv = @(Get-Group $Run.Process.Id $ProfileDir | ForEach-Object { $_.ProcessId })
    Write-Warning ("Bowser group did not exit after WM_CLOSE; surviving PIDs: {0}. Not killed; profile kept." -f ($surv -join ','))
    $script:allClean = $false
    return $false
}
function Remove-Profile([string] $Dir) {
    for ($i = 0; $i -lt 12; $i++) {
        if (-not (Test-Path -LiteralPath $Dir)) { return }
        try { Remove-Item -LiteralPath $Dir -Recurse -Force -ErrorAction Stop; return } catch { Start-Sleep -Milliseconds 1000 }
    }
    Write-Warning "Could not remove profile directory: $Dir"
}
# Returns ordered list of @{name; ms} from the event CSV (empty if unreadable). Shared read so Bowser can keep writing.
function Read-Events([string] $LogPath) {
    $list = New-Object 'System.Collections.Generic.List[object]'
    if (-not (Test-Path -LiteralPath $LogPath)) { return ,$list }
    try {
        $fs = [IO.File]::Open($LogPath, 'Open', 'Read', 'ReadWrite,Delete')
        $sr = New-Object IO.StreamReader($fs)
        try {
            [void]$sr.ReadLine()
            while ($null -ne ($line = $sr.ReadLine())) {
                $f = $line.Split(',')
                $ms = 0.0
                if ($f.Count -ge 2 -and [double]::TryParse($f[1], [Globalization.NumberStyles]::Float, $inv, [ref]$ms)) { $list.Add([pscustomobject]@{ name = $f[0]; ms = $ms }) }
            }
        } finally { $sr.Dispose(); $fs.Dispose() }
    } catch { }
    return ,$list
}
function Get-Marks($Events) {
    $m = @{}
    $first = { param($n) foreach ($e in $Events) { if ($e.name -eq $n) { return $e.ms } }; return $null }
    $m['process_start'] = & $first 'process_start'
    foreach ($n in 'window_visible','chrome_painted','engine_started_for_chrome','submitted','navigate_called','navigation_started','content_loading','navigation_completed','page_usable') { $m[$n] = & $first $n }
    # page_usable is only emitted by Bowser for the local fixtures (title contains BOWSER_BENCH_READY); real sites
    # have no such marker, so 'usable' falls back to the first navigation_completed (document loaded and shown).
    $m['usable'] = $(if ($null -ne $m['page_usable']) { $m['page_usable'] } else { $m['navigation_completed'] })
    $m['usable_source'] = $(if ($null -ne $m['page_usable']) { 'page_usable' } elseif ($null -ne $m['navigation_completed']) { 'navigation_completed' } else { $null })
    $m['env_requested'] = & $first 'webview_environment_requested'
    $m['env_ready'] = & $first 'webview_environment_result'
    $m['chrome_controller_requested'] = & $first 'webview_controller_requested'
    $m['chrome_controller_ready'] = & $first 'webview_controller_result'
    # Content controller = the last controller request/result pair that precedes the first navigate_called.
    $nav = $m['navigate_called']
    $req = $null; $res = $null
    foreach ($e in $Events) {
        if ($null -ne $nav -and $e.ms -ge $nav) { break }
        if ($e.name -eq 'webview_controller_requested') { $req = $e.ms }
        if ($e.name -eq 'webview_controller_result') { $res = $e.ms }
    }
    $m['content_controller_requested'] = $req
    $m['content_controller_ready'] = $res
    $m['controller_pairs_before_navigate'] = @($Events | Where-Object { $_.name -eq 'webview_controller_result' -and ($null -eq $nav -or $_.ms -lt $nav) }).Count
    $m
}
function Invoke-One([string] $Url, [string] $ProfileDir, [string] $LogPath) {
    $argList = @('--benchmark-log', "`"$LogPath`"", '--benchmark-user-data-dir', "`"$ProfileDir`"", '--open-url', $Url)
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = ($argList -join ' ')
    $psi.UseShellExecute = $false
    # Benchmark flags do not isolate Bowser's app data (history, workspaces, settings under %LOCALAPPDATA%\Bowser),
    # so every launch gets its own throwaway LOCALAPPDATA next to its profile directory.
    $appData = "$ProfileDir.appdata"
    New-Item -ItemType Directory -Force $appData | Out-Null
    $psi.EnvironmentVariables['LOCALAPPDATA'] = $appData
    if ($Ui -ne 'default') { $psi.EnvironmentVariables['BOWSER_UI'] = $Ui }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = [Diagnostics.Process]::Start($psi)
    $main = [IntPtr]::Zero; $outerVisible = $null; $outerUsable = $null
    $deadline = $UsableTimeoutSeconds * 1000
    while ($sw.ElapsedMilliseconds -lt $deadline) {
        if ($main.ToInt64() -eq 0) {
            $main = [BowserL2U.Native]::FindMain([uint32]$p.Id)
            if ($main.ToInt64() -ne 0) { $outerVisible = $sw.Elapsed.TotalMilliseconds }
        }
        if ($main.ToInt64() -ne 0) {
            $t = [BowserL2U.Native]::GetTitle($main)
            if ($t -and $t -notmatch '^Bowser( |$)' -and -not $t.StartsWith('about:')) { $outerUsable = $sw.Elapsed.TotalMilliseconds; break }
        }
        if ($p.HasExited) { break }
        Start-Sleep -Milliseconds 4
    }
    [pscustomobject]@{ Process = $p; Main = $main; OuterVisibleMs = $outerVisible; OuterUsableMs = $outerUsable }
}
$script:cpuCounter = New-Object Diagnostics.PerformanceCounter('Processor', '% Processor Time', '_Total')
[void]$script:cpuCounter.NextValue()
# Waits (up to -QuietWaitSeconds) for system CPU to drop below -QuietCpuPct (other builds/agents share this
# machine). Returns the last 1 s CPU reading; a run is flagged quiet only if that reading was under the limit.
function Wait-Quiet {
    $sw = [Diagnostics.Stopwatch]::StartNew(); $cpu = 100.0
    do { Start-Sleep -Milliseconds 700; $cpu = [double]$script:cpuCounter.NextValue(); if ($cpu -lt $QuietCpuPct) { break } } while ($sw.Elapsed.TotalSeconds -lt $QuietWaitSeconds)
    [math]::Round($cpu, 0)
}
function Get-Pct([double[]] $Sorted, [double] $P) {
    $pos = ($Sorted.Length - 1) * $P
    $lo = [int][math]::Floor($pos); $hi = [int][math]::Ceiling($pos)
    $Sorted[$lo] + ($Sorted[$hi] - $Sorted[$lo]) * ($pos - $lo)
}
function Get-Stat($Rows, [string] $Column) {
    $xs = New-Object 'System.Collections.Generic.List[double]'
    foreach ($r in @($Rows)) { $v = $r.$Column; if ($null -ne $v -and "$v" -ne '') { $xs.Add([double]$v) } }
    if ($xs.Count -eq 0) { return $null }
    $s = $xs.ToArray(); [Array]::Sort($s)
    [pscustomobject]@{ n = $s.Length; median = [math]::Round((Get-Pct $s 0.5), 1); p95 = [math]::Round((Get-Pct $s 0.95), 1); min = [math]::Round($s[0], 1); max = [math]::Round($s[$s.Length - 1], 1) }
}

# ---- machine info -------------------------------------------------------------------------------------------
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$os = Get-CimInstance Win32_OperatingSystem
$wv2 = $null
try { $wv2 = (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction Stop).pv } catch { }
$power = (powercfg /getactivescheme 2>$null) -join ' '
$machine = [ordered]@{
    cpu = $cpu.Name.Trim(); logical_cores = $cpu.NumberOfLogicalProcessors
    ram_gib = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1); os = "$($os.Caption) $($os.Version)"
    webview2_runtime = $wv2; power_plan = $power.Trim()
    ac_power = $(try { (Get-CimInstance Win32_Battery -ErrorAction Stop | Select-Object -First 1).BatteryStatus } catch { 'no battery/unknown' })
}
$gitHead = $null; try { $gitHead = (git -C $PSScriptRoot rev-parse --short HEAD 2>$null) } catch { }

# ---- fixture server -----------------------------------------------------------------------------------------
$fixture = New-Object BowserL2U.Fixture (Join-Path $PSScriptRoot 'fixtures')
$fixture.Start()
$SiteUrls = [ordered]@{ fixture = "http://127.0.0.1:$($fixture.Port)/light.html" } + $SiteUrls
foreach ($s in $Sites) { if (-not $SiteUrls.Contains($s)) { $fixture.Stop(); throw "Unknown site '$s'. Known: $($SiteUrls.Keys -join ', ')" } }
try {
    $wc = New-Object Net.WebClient; $wc.Proxy = $null
    if ($wc.DownloadString($SiteUrls['fixture']) -notmatch 'BOWSER_BENCH_READY_LIGHT') { throw 'fixture self-check returned unexpected content' }
    $wc.Dispose()
} catch { $fixture.Stop(); throw "Fixture self-check failed: $($_.Exception.Message)" }

$rows = New-Object 'System.Collections.Generic.List[object]'
$modes = if ($Mode -eq 'both') { @('cold','warm') } else { @($Mode) }
$metricCols = @('outer_visible_ms','outer_title_ms') + $MarkOrder + @($PhaseDefs | ForEach-Object { $_[0] })

function Add-Row($Site, $ModeName, [int] $RunIndex, $Result, [string] $LogPath) {
    $ev = Read-Events $LogPath
    $m = Get-Marks $ev
    $row = [ordered]@{
        site = $Site; mode = $ModeName; run = $RunIndex; ok = [bool]($null -ne $m['usable']); usable_source = $m['usable_source']
        controllers_before_navigate = $m['controller_pairs_before_navigate']
        outer_visible_ms = $(if ($null -ne $Result.OuterVisibleMs) { [math]::Round($Result.OuterVisibleMs, 1) } else { $null })
        outer_title_ms = $(if ($null -ne $Result.OuterUsableMs) { [math]::Round($Result.OuterUsableMs, 1) } else { $null })
    }
    foreach ($n in $MarkOrder) { $row[$n] = $(if ($null -ne $m[$n]) { [math]::Round($m[$n], 1) } else { $null }) }
    foreach ($d in $PhaseDefs) {
        $a = $m[$d[1]]; $b = $m[$d[2]]
        $row[$d[0]] = $(if ($null -ne $a -and $null -ne $b -and $b -ge $a) { [math]::Round($b - $a, 1) } else { $null })
    }
    $rows.Add([pscustomobject]$row)
}

try {
    foreach ($site in $Sites) {
        $url = $SiteUrls[$site]
        $n = if ($site -eq 'fixture') { $Runs } else { [math]::Min($Runs, $RealSiteRuns) }
        foreach ($mo in $modes) {
            Write-Host ("== {0} / {1}: {2} runs ({3})" -f $site, $mo, $n, $url)
            $warmProfile = Join-Path $work "warm-$site"
            if ($mo -eq 'warm') {
                $plog = Join-Path $logRoot "$site-warm-prime.csv"
                $r = Invoke-One $url $warmProfile $plog
                [void](Stop-Bowser $r $warmProfile)
            }
            for ($i = 0; $i -lt $n; $i++) {
                $profile = Join-Path $work "$mo-$site-$i"
                # Warm runs start from an identical copy of the primed profile (cache, cookies, prefs) so a run
                # that leaves a dirty/locked profile cannot affect the next one. The copy is not timed.
                if ($mo -eq 'warm') { Copy-Item -LiteralPath $warmProfile -Destination $profile -Recurse -Force; if (Test-Path -LiteralPath "$warmProfile.appdata") { Copy-Item -LiteralPath "$warmProfile.appdata" -Destination "$profile.appdata" -Recurse -Force } }
                $log = Join-Path $logRoot ("{0}-{1}-{2:00}.csv" -f $site, $mo, $i)
                $cpuBefore = Wait-Quiet
                $r = Invoke-One $url $profile $log
                $clean = $false; $o0 = $script:orphansStopped
                try {
                    Start-Sleep -Milliseconds $(if ($site -eq 'fixture') { $GraceMs } else { $RealSiteGraceMs })   # page_usable is logged shortly after the title appears
                } finally { $clean = Stop-Bowser $r $profile }
                Add-Row $site $mo $i $r $log
                $last = $rows[$rows.Count - 1]
                Add-Member -InputObject $last -NotePropertyName cpu_before_pct -NotePropertyValue $cpuBefore
                Add-Member -InputObject $last -NotePropertyName quiet -NotePropertyValue ($cpuBefore -lt $QuietCpuPct)
                Add-Member -InputObject $last -NotePropertyName orphans_stopped -NotePropertyValue ($script:orphansStopped - $o0)
                Write-Host ("   run {0,2}: ok={1} visible={2} title_outer={3} usable={4} ({5})" -f $i, $last.ok, $last.window_visible, $last.outer_title_ms, $last.usable, $last.usable_source)
                if ($clean) { Remove-Profile $profile; Remove-Profile "$profile.appdata" }
            }
            if ($mo -eq 'warm' -and $script:allClean) { Remove-Profile $warmProfile }
        }
    }
} finally { $fixture.Stop() }

# ---- reports --------------------------------------------------------------------------------------------------
$rows | Export-Csv -LiteralPath (Join-Path $out 'runs.csv') -NoTypeInformation
$summary = [ordered]@{}
foreach ($site in $Sites) {
    $summary[$site] = [ordered]@{}
    foreach ($mo in $modes) {
        $sub = @($rows | Where-Object { $_.site -eq $site -and $_.mode -eq $mo })
        $valid = @($sub | Where-Object { $_.ok })
        $quiet = @($valid | Where-Object { $_.quiet })
        $entry = [ordered]@{ url = $SiteUrls[$site]; runs = $sub.Count; valid_runs = $valid.Count; quiet_valid_runs = $quiet.Count; metrics = [ordered]@{}; metrics_quiet = [ordered]@{} }
        foreach ($c in $metricCols) { $s = Get-Stat $valid $c; if ($null -ne $s) { $entry.metrics[$c] = $s }; $q = Get-Stat $quiet $c; if ($null -ne $q) { $entry.metrics_quiet[$c] = $q } }
        $summary[$site][$mo] = $entry
    }
}
$doc = [ordered]@{
    label = $Label; recorded = (Get-Date -Format 's'); executable = $exe; sha256 = $exeHash; git_head_at_run = $gitHead
    ui_mode = $Ui; machine = $machine; runs_requested = $Runs; webview2_orphans_stopped = $script:orphansStopped; real_site_runs = $RealSiteRuns
    note = 'Times are ms. Mark metrics are cumulative from process_start in Bowser''s event log; p_* are phase durations; outer_* are harness wall clock from just before process creation. Statistics use only runs where page_usable was reached.'
    summary = $summary
}
$doc | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $out 'summary.json') -Encoding utf8

function CellQ($e, [string] $c) { if ($e.metrics_quiet.Contains($c)) { '{0} / {1}' -f $e.metrics_quiet[$c].median, $e.metrics_quiet[$c].p95 } else { 'n/a' } }
function Cell($e, [string] $c) { if ($e.metrics.Contains($c)) { '{0} / {1}' -f $e.metrics[$c].median, $e.metrics[$c].p95 } else { 'n/a' } }
$md = New-Object 'System.Collections.Generic.List[string]'
$md.Add("# Launch-to-usable: $Label"); $md.Add('')
$md.Add("- Executable: $exe"); $md.Add("- SHA-256: $exeHash"); $md.Add("- Recorded: $($doc.recorded) (git HEAD of checkout: $gitHead)")
$md.Add("- UI mode override: $Ui (default = whatever the build selects; html-chrome-default builds open the HTML chrome)")
$md.Add("- Machine: $($machine.cpu), $($machine.logical_cores) logical cores, $($machine.ram_gib) GiB, $($machine.os), WebView2 runtime $($machine.webview2_runtime)")
$md.Add("- Power: $($machine.power_plan); battery status code: $($machine.ac_power)")
$md.Add("- Quiet = system CPU under $QuietCpuPct% in the second before launch (the harness waits up to $QuietWaitSeconds s for that). Other agents/builds share this machine, so non-quiet runs are inflated.")
$md.Add('- Cells are `median / p95` in ms. OS file caches are not flushed; cold = fresh profile directory.'); $md.Add('')
$md.Add('## Top metric: process launch to page usable (ms from process_start; usable = page_usable event on the fixture, first navigation_completed on real sites)'); $md.Add('')
$md.Add('| Site | Mode | valid/runs | quiet runs | window_visible | chrome_painted | usable (Bowser) | usable, quiet runs only | outer visible | outer title shown (wall) |')
$md.Add('| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |')
foreach ($site in $Sites) { foreach ($mo in $modes) { $e = $summary[$site][$mo]
    $md.Add("| $site | $mo | $($e.valid_runs)/$($e.runs) | $($e.quiet_valid_runs) | $(Cell $e 'window_visible') | $(Cell $e 'chrome_painted') | $(Cell $e 'usable') | $(CellQ $e 'usable') | $(Cell $e 'outer_visible_ms') | $(Cell $e 'outer_title_ms') |") } }
$md.Add(''); $md.Add('## Cumulative marks (ms from process_start)'); $md.Add('')
$md.Add('| Site | Mode | engine_started_for_chrome | env_ready | chrome_controller_ready | submitted | content_controller_ready | navigate_called | navigation_started | content_loading | navigation_completed | usable |')
$md.Add('| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |')
foreach ($site in $Sites) { foreach ($mo in $modes) { $e = $summary[$site][$mo]
    $md.Add("| $site | $mo | " + ((@('engine_started_for_chrome','env_ready','chrome_controller_ready','submitted','content_controller_ready','navigate_called','navigation_started','content_loading','navigation_completed','usable') | ForEach-Object { Cell $e $_ }) -join ' | ') + ' |') } }
$md.Add(''); $md.Add('## Phase durations (ms)'); $md.Add('')
$md.Add('| Site | Mode | ' + (($PhaseDefs | ForEach-Object { $_[0] -replace '^p_','' }) -join ' | ') + ' |')
$md.Add('| --- | --- | ' + (($PhaseDefs | ForEach-Object { '---' }) -join ' | ') + ' |')
foreach ($site in $Sites) { foreach ($mo in $modes) { $e = $summary[$site][$mo]
    $md.Add("| $site | $mo | " + (($PhaseDefs | ForEach-Object { Cell $e $_[0] }) -join ' | ') + ' |') } }
$md.Add('')
$md.Add('Phase definitions: ' + (($PhaseDefs | ForEach-Object { "$($_[0] -replace '^p_','') = $($_[1]) to $($_[2])" }) -join '; ') + '.')
$md.Add('env_requested/env_ready = first webview_environment_requested/result. chrome_controller = first webview_controller_requested/result (HTML chrome toolbar controller when present). content_controller = last controller request/result before the first navigate_called.')
$md.Add("Raw rows: runs.csv. Event logs: logs\.")
Set-Content -LiteralPath (Join-Path $out 'summary.md') -Value $md -Encoding utf8
$md | ForEach-Object { Write-Host $_ }

if ($script:allClean) { Remove-Profile $work } else { Write-Warning "Profiles kept under $work because a Bowser process group did not exit." }
