[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $BowserExecutable,
    [string] $Label = 'current',
    [ValidateSet('startup','tabs','idle','switch')] [string] $Mode = 'startup',
    [ValidateRange(1,200)] [int] $Switches = 20,
    [ValidateRange(1,100)] [int] $Runs = 9,
    [ValidateRange(1,200)] [int[]] $TabCounts = @(1,5,10,25),
    [ValidateRange(0,65535)] [int] $Port = 0,
    [switch] $MemoryPressure,
    [string] $OutputDirectory = (Join-Path $PSScriptRoot 'results\perf-probe')
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'

if (-not ('BowserPerfProbe.FixtureServer' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
namespace BowserPerfProbe {
public static class Native {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] public static extern IntPtr SendText(IntPtr h, uint m, IntPtr w, string l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] public static extern IntPtr SendGet(IntPtr h, uint m, IntPtr w, StringBuilder l);
  public static string GetEditText(IntPtr h) { var sb = new StringBuilder(2048); SendGet(h, 0x000D, (IntPtr)2048, sb); return sb.ToString(); }
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  public static string GetTitle(IntPtr h) { var sb = new StringBuilder(512); GetWindowText(h, sb, 512); return sb.ToString(); }
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint attach, uint attachTo, bool attachFlag);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  // Windows refuses SetForegroundWindow from a process that is not the foreground one. Sharing the
  // input queue of the current foreground thread for the duration of the call lets it succeed.
  public static bool ForceForeground(IntPtr h) {
    if (GetForegroundWindow() == h) return true;
    // A synthetic Alt tap counts as recent input for this process, which the foreground lock checks.
    keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x12, 0, 2, UIntPtr.Zero);
    uint ignored;
    uint foregroundThread = GetWindowThreadProcessId(GetForegroundWindow(), out ignored);
    uint self = GetCurrentThreadId();
    bool attached = foregroundThread != 0 && foregroundThread != self && AttachThreadInput(self, foregroundThread, true);
    try { SetForegroundWindow(h); }
    finally { if (attached) AttachThreadInput(self, foregroundThread, false); }
    return GetForegroundWindow() == h;
  }
  public static IntPtr FindMain(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) { var sb = new StringBuilder(64); GetClassName(h, sb, 64);
        if (sb.ToString() == "BowserNativeWindow") { found = h; return false; } }
      return true; }, IntPtr.Zero);
    return found;
  }
  public static IntPtr FindEdit(IntPtr main) {
    IntPtr found = IntPtr.Zero;
    EnumChildWindows(main, (h, l) => { var sb = new StringBuilder(64); GetClassName(h, sb, 64);
      if (sb.ToString() == "Edit") { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
}

// Serves GET requests for files directly inside one directory. Anything with a path separator,
// a colon, a percent escape or a parent reference is refused before the file system is touched.
public sealed class FixtureServer {
  private readonly TcpListener listener;
  private readonly string root;
  private volatile bool running;
  public FixtureServer(string root, int port) {
    this.root = Path.GetFullPath(root).TrimEnd('\\') + "\\";
    listener = new TcpListener(IPAddress.Any, port);
  }
  public int Port { get { return ((IPEndPoint)listener.LocalEndpoint).Port; } }
  public void Start() {
    listener.Start();
    running = true;
    var thread = new Thread(AcceptLoop);
    thread.IsBackground = true;
    thread.Start();
  }
  public void Stop() {
    running = false;
    try { listener.Stop(); } catch (Exception) { }
  }
  private void AcceptLoop() {
    while (running) {
      TcpClient client;
      try { client = listener.AcceptTcpClient(); } catch (Exception) { break; }
      // Bound to Any so 127.0.0.N hosts reach it; anything off-machine is refused.
      if (!IPAddress.IsLoopback(((IPEndPoint)client.Client.RemoteEndPoint).Address)) { client.Close(); continue; }
      ThreadPool.QueueUserWorkItem(delegate(object state) { Serve((TcpClient)state); }, client);
    }
  }
  private void Serve(TcpClient client) {
    using (client) {
      try {
        client.ReceiveTimeout = 5000;
        client.SendTimeout = 5000;
        NetworkStream stream = client.GetStream();
        string head = ReadHead(stream);
        if (head == null) return;
        string[] requestLine = head.Split(new[] { "\r\n" }, StringSplitOptions.None)[0].Split(' ');
        if (requestLine.Length != 3 || !requestLine[2].StartsWith("HTTP/1.")) { Send(stream, 400, "Bad Request", "text/plain", new byte[0]); return; }
        if (requestLine[0] != "GET") { Send(stream, 405, "Method Not Allowed", "text/plain", new byte[0]); return; }
        string target = requestLine[1];
        int query = target.IndexOfAny(new[] { '?', '#' });
        if (query >= 0) target = target.Substring(0, query);
        if (!target.StartsWith("/")) { Send(stream, 404, "Not Found", "text/plain", new byte[0]); return; }
        string name = target.Substring(1);
        if (name.Length == 0 || name.IndexOfAny(new[] { '/', '\\', ':', '%' }) >= 0 || name.Contains("..")) { Send(stream, 404, "Not Found", "text/plain", new byte[0]); return; }
        string full = Path.GetFullPath(Path.Combine(root, name));
        if (!full.StartsWith(root, StringComparison.OrdinalIgnoreCase) || !File.Exists(full)) { Send(stream, 404, "Not Found", "text/plain", new byte[0]); return; }
        string type = full.EndsWith(".html", StringComparison.OrdinalIgnoreCase) ? "text/html; charset=utf-8" : "application/octet-stream";
        Send(stream, 200, "OK", type, File.ReadAllBytes(full));
      } catch (Exception) { }
    }
  }
  private static string ReadHead(NetworkStream stream) {
    var buffer = new byte[8192];
    int count = 0;
    while (count < buffer.Length) {
      int read = stream.Read(buffer, count, buffer.Length - count);
      if (read <= 0) break;
      count += read;
      string text = Encoding.ASCII.GetString(buffer, 0, count);
      if (text.Contains("\r\n\r\n")) return text;
    }
    return null;
  }
  private static void Send(NetworkStream stream, int code, string reason, string type, byte[] body) {
    byte[] head = Encoding.ASCII.GetBytes("HTTP/1.1 " + code + " " + reason + "\r\nContent-Type: " + type + "\r\nContent-Length: " + body.Length + "\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n");
    stream.Write(head, 0, head.Length);
    stream.Write(body, 0, body.Length);
  }
}
}
'@
}

$WM_CLOSE = 0x0010
$WM_SETTEXT = 0x000C
$WM_SUBMIT = 0x8000 + 10
$WM_SHORTCUT_KEY = 0x8000 + 15
$exe = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($BowserExecutable)
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Bowser executable not found: $exe" }
$exeHash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
$safeLabel = $Label -replace '[^A-Za-z0-9._-]', '_'
$out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
New-Item -ItemType Directory -Force $out | Out-Null
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$logDir = Join-Path $out "$safeLabel-$Mode-$stamp-logs"
New-Item -ItemType Directory -Force $logDir | Out-Null
$work = Join-Path $env:TEMP ("bowser-perf-probe-{0}-{1}" -f $safeLabel, $stamp)
New-Item -ItemType Directory -Force $work | Out-Null
$script:allClean = $true

function Get-Group([int] $RootPid, [string] $ProfileDir) {
    $all = @(Get-CimInstance Win32_Process)
    $ids = New-Object 'System.Collections.Generic.HashSet[int]'; [void]$ids.Add($RootPid)
    foreach ($p in $all) { if ($p.CommandLine -and $p.CommandLine.IndexOf($ProfileDir, [StringComparison]::OrdinalIgnoreCase) -ge 0) { [void]$ids.Add([int]$p.ProcessId) } }
    $changed = $true
    while ($changed) { $changed = $false; foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId) -and -not $ids.Contains([int]$p.ProcessId)) { [void]$ids.Add([int]$p.ProcessId); $changed = $true } } }
    @($all | Where-Object { $ids.Contains([int]$_.ProcessId) })
}
function SumP($xs, [string] $n) { $t = 0.0; foreach ($x in @($xs)) { if ($null -ne $x) { $t += [double]$x.$n } }; $t }
function Sample([int] $RootPid, [string] $ProfileDir) {
    $g = @(Get-Group $RootPid $ProfileDir)
    $host_ = @($g | Where-Object { $_.ProcessId -eq $RootPid })
    $wv = @($g | Where-Object { $_.ProcessId -ne $RootPid })
    $cpu = { param($x) (SumP $x 'UserModeTime') + (SumP $x 'KernelModeTime') }
    [pscustomobject]@{
        processes = $g.Count
        host_ws_mb = [math]::Round((SumP $host_ 'WorkingSetSize') / 1MB, 2)
        host_priv_mb = [math]::Round((SumP $host_ 'PrivatePageCount') / 1MB, 2)
        wv_ws_mb = [math]::Round((SumP $wv 'WorkingSetSize') / 1MB, 1)
        wv_priv_mb = [math]::Round((SumP $wv 'PrivatePageCount') / 1MB, 1)
        host_cpu = (& $cpu $host_)
        wv_cpu = (& $cpu $wv)
    }
}
function Start-Bowser([string] $ProfileDir, [string] $LogPath) {
    $launchArgs = @('--benchmark-log', "`"$LogPath`"", '--benchmark-user-data-dir', "`"$ProfileDir`"", '--data-dir', "`"$ProfileDir-appdata`"")
    if ($MemoryPressure) { $launchArgs += @('--benchmark-memory-pressure', 'on') }
    # Benchmark flags do not isolate app data (history, workspaces, settings), so give the child a throwaway LOCALAPPDATA.
    $appData = "$ProfileDir.appdata"
    New-Item -ItemType Directory -Force $appData | Out-Null
    $savedLocalAppData = $env:LOCALAPPDATA
    $env:LOCALAPPDATA = $appData
    $sw = [Diagnostics.Stopwatch]::StartNew()
    try { $p = Start-Process -FilePath $exe -ArgumentList $launchArgs -PassThru } finally { $env:LOCALAPPDATA = $savedLocalAppData }
    $main = [IntPtr]::Zero
    while ($main.ToInt64() -eq 0 -and $sw.ElapsedMilliseconds -lt 15000) { $main = [BowserPerfProbe.Native]::FindMain([uint32]$p.Id); if ($main.ToInt64() -eq 0) { Start-Sleep -Milliseconds 2 } }
    $visible = $null
    if ($main.ToInt64() -ne 0) { $visible = $sw.Elapsed.TotalMilliseconds }
    [pscustomobject]@{ Process = $p; Main = $main; OuterVisibleMs = $visible }
}
# Returns $true only when the whole group has exited. Never kills anything; a surviving group is reported.
function Stop-Bowser($Bowser, [string] $ProfileDir) {
    if ($Bowser.Main.ToInt64() -ne 0) { [void][BowserPerfProbe.Native]::PostMessage($Bowser.Main, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) }
    $deadline = (Get-Date).AddSeconds(60)
    while ((Get-Date) -lt $deadline) {
        if (@(Get-Group $Bowser.Process.Id $ProfileDir).Count -eq 0) { return $true }
        Start-Sleep -Milliseconds 200
    }
    $survivors = @(Get-Group $Bowser.Process.Id $ProfileDir | ForEach-Object { $_.ProcessId })
    Write-Warning ("Bowser process group did not exit after WM_CLOSE; surviving PIDs: {0}. Not killed; profile kept." -f ($survivors -join ','))
    $script:allClean = $false
    return $false
}
function Remove-Profile([string] $Dir) {
    for ($attempt = 0; $attempt -lt 5; $attempt++) {
        if (-not (Test-Path -LiteralPath $Dir)) { return }
        try { Remove-Item -LiteralPath $Dir -Recurse -Force -ErrorAction Stop; return } catch { Start-Sleep -Milliseconds 500 }
    }
    Write-Warning "Could not remove profile directory: $Dir"
}
function Read-Events([string] $LogPath) {
    $map = @{}
    if (Test-Path -LiteralPath $LogPath) {
        foreach ($line in @(Get-Content -LiteralPath $LogPath | Select-Object -Skip 1)) {
            $f = $line.Split(',')
            if ($f.Count -lt 2 -or $map.ContainsKey($f[0])) { continue }
            $ms = 0.0
            if ([double]::TryParse($f[1], [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$ms)) { $map[$f[0]] = $ms }
        }
    }
    $map
}
function Count-Event([string] $LogPath, [string] $Name) { if (Test-Path -LiteralPath $LogPath) { @(Get-Content -LiteralPath $LogPath | Where-Object { $_.StartsWith("$Name,") }).Count } else { 0 } }
function Wait-Until([scriptblock] $Condition, [int] $TimeoutMs) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) { if (& $Condition) { return $true }; Start-Sleep -Milliseconds 50 }
    [bool](& $Condition)
}
function Get-Pct([double[]] $Sorted, [double] $P) {
    $pos = ($Sorted.Length - 1) * $P
    $lo = [int][math]::Floor($pos); $hi = [int][math]::Ceiling($pos)
    $Sorted[$lo] + ($Sorted[$hi] - $Sorted[$lo]) * ($pos - $lo)
}
function Get-ColumnStat($Rows, [string] $Column) {
    $xs = New-Object 'System.Collections.Generic.List[double]'
    foreach ($r in @($Rows)) { $v = $r.$Column; if ($null -ne $v) { $xs.Add([double]$v) } }
    if ($xs.Count -eq 0) { return $null }
    $s = $xs.ToArray(); [Array]::Sort($s)
    [pscustomobject]@{ n = $s.Length; median = (Get-Pct $s 0.5); p25 = (Get-Pct $s 0.25); p75 = (Get-Pct $s 0.75) }
}
function Format-Stat($Stat) { if ($null -eq $Stat) { return 'n/a' }; '{0:N1} ({1:N1} to {2:N1}, n={3})' -f $Stat.median, $Stat.p25, $Stat.p75, $Stat.n }
function Start-Fixtures {
    $root = Join-Path $PSScriptRoot 'fixtures'
    if (-not (Test-Path -LiteralPath (Join-Path $root 'light.html'))) { throw "Fixture directory is missing light.html: $root" }
    try { $server = New-Object -TypeName BowserPerfProbe.FixtureServer -ArgumentList $root, $Port; $server.Start() }
    catch { throw "Could not start the fixture server on port $Port (is that port in use?): $($_.Exception.Message)" }
    $client = New-Object System.Net.WebClient
    $client.Proxy = $null
    try { $body = $client.DownloadString("http://127.0.0.1:$($server.Port)/light.html") }
    catch { $server.Stop(); throw "Fixture self-check failed on port $($server.Port): $($_.Exception.Message)" }
    finally { $client.Dispose() }
    if ($body -notmatch 'BOWSER_BENCH_READY_LIGHT') { $server.Stop(); throw "Fixture self-check on port $($server.Port) returned unexpected content" }
    $server
}
function Send-NewTab([IntPtr] $Main) {
    Start-Sleep -Milliseconds 50
    # Posts Bowser's own WM_SHORTCUT_KEY (WM_APP+15; wparam = virtual key, lparam bit 0 = Ctrl) to its main window.
    # This reaches the same command dispatch as the keyboard path without needing foreground or taking over the desktop.
    [void][BowserPerfProbe.Native]::PostMessage($Main, $WM_SHORTCUT_KEY, [IntPtr]0x54, [IntPtr]1)
    Start-Sleep -Milliseconds 150
}
function Submit-Address([IntPtr] $Main, [string] $Url) {
    $edit = [BowserPerfProbe.Native]::FindEdit($Main)
    if ($edit.ToInt64() -eq 0) { throw 'Bowser address EDIT control was not found.' }
    [void][BowserPerfProbe.Native]::SendText($edit, $WM_SETTEXT, [IntPtr]::Zero, $Url)
    [void][BowserPerfProbe.Native]::PostMessage($Main, $WM_SUBMIT, [IntPtr]::Zero, [IntPtr]::Zero)
}
function Write-Report([string] $Path, [string[]] $Lines) {
    Set-Content -LiteralPath $Path -Value $Lines -Encoding utf8
    Get-Content -LiteralPath $Path
}
function Report-Header([string] $Title) {
    @(
        "# Bowser perf-probe: $Title",
        '',
        "- Label: $Label",
        "- Executable: $exe",
        "- SHA-256: $exeHash",
        "- Recorded: $(Get-Date -Format 's')",
        "- Memory pressure flag: $([bool]$MemoryPressure)",
        ''
    )
}

function Invoke-StartupMode {
    $warmProfile = Join-Path $work 'warm'
    # One untimed launch primes the warm profile so every timed warm run reuses existing profile state.
    $prime = Start-Bowser $warmProfile (Join-Path $logDir 'warm-prime.csv')
    try {
        if ($null -eq $prime.OuterVisibleMs) { throw 'Bowser window was not found while priming the warm profile.' }
        [void](Wait-Until { $e = Read-Events (Join-Path $logDir 'warm-prime.csv'); $e.ContainsKey('input_ready') -and $e.ContainsKey('chrome_painted') } 10000)
    } finally { [void](Stop-Bowser $prime $warmProfile) }

    $rows = @()
    $plan = @()
    for ($i = 0; $i -lt $Runs; $i++) { $plan += [pscustomobject]@{ kind = 'fresh'; run = $i }; $plan += [pscustomobject]@{ kind = 'warm'; run = $i } }
    foreach ($item in $plan) {
        $profile = if ($item.kind -eq 'fresh') { Join-Path $work "fresh$($item.run)" } else { $warmProfile }
        $log = Join-Path $logDir "$($item.kind)$($item.run).csv"
        $b = Start-Bowser $profile $log
        $clean = $false
        try {
            if ($null -eq $b.OuterVisibleMs) { throw "Bowser window was not found ($($item.kind) run $($item.run))." }
            [void](Wait-Until { $e = Read-Events $log; $e.ContainsKey('input_ready') -and $e.ContainsKey('chrome_painted') } 10000)
            $e = Read-Events $log
            $s = Sample $b.Process.Id $profile
            $rows += [pscustomobject]@{
                kind = $item.kind; run = $item.run
                outer_visible_ms = [math]::Round($b.OuterVisibleMs, 1)
                window_visible_ms = $e['window_visible']
                input_ready_ms = $e['input_ready']
                chrome_painted_ms = $e['chrome_painted']
                host_ws_mb = $s.host_ws_mb
                host_priv_mb = $s.host_priv_mb
                processes = $s.processes
            }
        } finally {
            $clean = Stop-Bowser $b $profile
            if ($clean -and $item.kind -eq 'fresh') { Remove-Profile $profile }
        }
    }
    $csv = Join-Path $out "$safeLabel-startup-$stamp.csv"
    $rows | Export-Csv -LiteralPath $csv -NoTypeInformation
    $lines = @(Report-Header 'startup')
    $lines += '| Profile | Metric | Median (p25 to p75, n) |'
    $lines += '| --- | --- | --- |'
    foreach ($kind in 'fresh','warm') {
        $sub = @($rows | Where-Object { $_.kind -eq $kind })
        foreach ($m in @(@('outer_visible_ms','Outer launch to visible (ms)'), @('window_visible_ms','window_visible (ms)'), @('input_ready_ms','input_ready (ms)'), @('chrome_painted_ms','chrome_painted (ms)'), @('host_ws_mb','Host working set (MB)'), @('host_priv_mb','Host private (MB)'))) {
            $lines += "| $kind | $($m[1]) | $(Format-Stat (Get-ColumnStat $sub $m[0])) |"
        }
    }
    $lines += ''
    $lines += "Raw rows: $csv. Event logs: $logDir. chrome_painted is n/a on builds without that event."
    Write-Report (Join-Path $out "$safeLabel-startup-$stamp.md") $lines
}

function Invoke-TabsMode {
    $server = Start-Fixtures
    $rows = @()
    try {
        foreach ($count in $TabCounts) {
            $profile = Join-Path $work "tabs$count"
            $log = Join-Path $logDir "tabs$count.csv"
            $b = Start-Bowser $profile $log
            $clean = $false
            try {
                if ($null -eq $b.OuterVisibleMs) { throw "Bowser window was not found (tabs=$count)." }
                Start-Sleep -Milliseconds 800
                for ($t = 1; $t -le $count; $t++) {
                    if ($t -gt 1) { Send-NewTab $b.Main }
                    # Distinct loopback hosts are distinct sites, so each tab gets its own renderer like real browsing.
                    Submit-Address $b.Main "http://127.0.0.$t`:$($server.Port)/light.html"
                    [void](Wait-Until { (Count-Event $log 'page_usable') -ge $t } 30000)
                }
                $loaded = Count-Event $log 'page_usable'
                if ($MemoryPressure) {
                    # One discard per idle tick, and only after a tab has been hidden for 30 s. Wait for
                    # every hidden tab to go, or for the count to stop moving, bounded by 30 s + 3 s per tab.
                    $sw = [Diagnostics.Stopwatch]::StartNew()
                    $capMs = (30 + 3 * $count) * 1000
                    $seen = -1; $seenAt = 0
                    while ($sw.ElapsedMilliseconds -lt $capMs) {
                        $discarded = Count-Event $log 'tab_discarded'
                        if ($discarded -ne $seen) { $seen = $discarded; $seenAt = $sw.ElapsedMilliseconds }
                        if ($discarded -ge ($count - 1)) { break }
                        if ($discarded -gt 0 -and ($sw.ElapsedMilliseconds - $seenAt) -ge 8000) { break }
                        Start-Sleep -Milliseconds 500
                    }
                }
                Start-Sleep -Seconds 8
                $s1 = Sample $b.Process.Id $profile
                Start-Sleep -Seconds 40
                $s2 = Sample $b.Process.Id $profile
                Start-Sleep -Seconds 10
                $s3 = Sample $b.Process.Id $profile
                $rows += [pscustomobject]@{
                    tabs = $count
                    loaded = $loaded
                    tab_discarded = Count-Event $log 'tab_discarded'
                    memory_pressure = [bool]$MemoryPressure
                    processes = $s2.processes
                    host_ws_mb = $s3.host_ws_mb
                    host_priv_mb = $s3.host_priv_mb
                    wv_ws_settle_mb = $s1.wv_ws_mb
                    wv_priv_settle_mb = $s1.wv_priv_mb
                    wv_ws_50s_mb = $s3.wv_ws_mb
                    wv_priv_50s_mb = $s3.wv_priv_mb
                    idle10s_host_cpu_ms = [math]::Round(($s3.host_cpu - $s2.host_cpu) / 10000, 1)
                    idle10s_wv_cpu_ms = [math]::Round(($s3.wv_cpu - $s2.wv_cpu) / 10000, 1)
                }
            } finally {
                $clean = Stop-Bowser $b $profile
                if ($clean) { Remove-Profile $profile }
            }
        }
    } finally { $server.Stop() }
    $csv = Join-Path $out "$safeLabel-tabs-$stamp.csv"
    $rows | Export-Csv -LiteralPath $csv -NoTypeInformation
    $lines = @(Report-Header 'tabs')
    $lines += '| Tabs | Loaded | Processes | Host WS MB | Host private MB | WV WS MB (settle) | WV private MB (settle) | WV WS MB (+50 s) | WV private MB (+50 s) | Idle 10 s host CPU ms | Idle 10 s WV CPU ms | tab_discarded |'
    $lines += '| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |'
    foreach ($r in $rows) {
        $lines += "| $($r.tabs) | $($r.loaded)/$($r.tabs) | $($r.processes) | $($r.host_ws_mb) | $($r.host_priv_mb) | $($r.wv_ws_settle_mb) | $($r.wv_priv_settle_mb) | $($r.wv_ws_50s_mb) | $($r.wv_priv_50s_mb) | $($r.idle10s_host_cpu_ms) | $($r.idle10s_wv_cpu_ms) | $($r.tab_discarded) |"
    }
    $lines += ''
    $lines += 'Host = Bowser process only. WV = WebView2 child processes (renderers, GPU, utility). Samples at settle, +40 s and +50 s; idle CPU is the 10 s between +40 s and +50 s.'
    $lines += "Raw rows: $csv. Event logs: $logDir."
    Write-Report (Join-Path $out "$safeLabel-tabs-$stamp.md") $lines
}

function Invoke-IdleMode {
    $profile = Join-Path $work 'idle'
    $log = Join-Path $logDir 'idle.csv'
    $server = Start-Fixtures
    $rows = @()
    try {
        $b = Start-Bowser $profile $log
        $clean = $false
        try {
            if ($null -eq $b.OuterVisibleMs) { throw 'Bowser window was not found (idle).' }
            Start-Sleep -Seconds 3
            $a = Sample $b.Process.Id $profile
            Start-Sleep -Seconds 20
            $z = Sample $b.Process.Id $profile
            $rows += [pscustomobject]@{ state = 'empty_focused_omnibox'; loaded = 0; host_cpu_20s_ms = [math]::Round(($z.host_cpu - $a.host_cpu) / 10000, 1); wv_cpu_20s_ms = [math]::Round(($z.wv_cpu - $a.wv_cpu) / 10000, 1); host_ws_mb = $z.host_ws_mb; host_priv_mb = $z.host_priv_mb; wv_ws_mb = $z.wv_ws_mb; wv_priv_mb = $z.wv_priv_mb }
            Submit-Address $b.Main "http://127.0.0.1:$($server.Port)/light.html"
            $loaded = [bool](Wait-Until { (Count-Event $log 'page_usable') -ge 1 } 30000)
            Start-Sleep -Seconds 5
            $a = Sample $b.Process.Id $profile
            Start-Sleep -Seconds 20
            $z = Sample $b.Process.Id $profile
            $rows += [pscustomobject]@{ state = 'loaded_page'; loaded = [int]$loaded; host_cpu_20s_ms = [math]::Round(($z.host_cpu - $a.host_cpu) / 10000, 1); wv_cpu_20s_ms = [math]::Round(($z.wv_cpu - $a.wv_cpu) / 10000, 1); host_ws_mb = $z.host_ws_mb; host_priv_mb = $z.host_priv_mb; wv_ws_mb = $z.wv_ws_mb; wv_priv_mb = $z.wv_priv_mb }
        } finally {
            $clean = Stop-Bowser $b $profile
            if ($clean) { Remove-Profile $profile }
        }
    } finally { $server.Stop() }
    $csv = Join-Path $out "$safeLabel-idle-$stamp.csv"
    $rows | Export-Csv -LiteralPath $csv -NoTypeInformation
    $lines = @(Report-Header 'idle')
    $lines += '| State | Page loaded | Host CPU over 20 s (ms) | WV CPU over 20 s (ms) | Host WS MB | Host private MB | WV WS MB | WV private MB |'
    $lines += '| --- | --- | --- | --- | --- | --- | --- | --- |'
    foreach ($r in $rows) { $lines += "| $($r.state) | $($r.loaded) | $($r.host_cpu_20s_ms) | $($r.wv_cpu_20s_ms) | $($r.host_ws_mb) | $($r.host_priv_mb) | $($r.wv_ws_mb) | $($r.wv_priv_mb) |" }
    $lines += ''
    $lines += "Raw rows: $csv. Event logs: $logDir."
    Write-Report (Join-Path $out "$safeLabel-idle-$stamp.md") $lines
}

function Send-CtrlTab([IntPtr] $Main) {
    [void][BowserPerfProbe.Native]::PostMessage($Main, $WM_SHORTCUT_KEY, [IntPtr]0x09, [IntPtr]1)
}

# Tab-switch latency, measured from outside: with two loaded tabs on distinct hosts, press Ctrl+Tab and poll the
# native address EDIT until it shows the other tab's host. This is key-injection to address-bar-updated, an upper
# bound on the time until the new tab's address is shown; it does not prove the new tab's pixels are presented.
function Invoke-SwitchMode {
    $server = Start-Fixtures
    $profile = Join-Path $work 'switch'
    $log = Join-Path $logDir 'switch.csv'
    $fixtureFor = @{ 1 = 'light.html'; 2 = 'demanding.html' }
    $samples = New-Object 'System.Collections.Generic.List[double]'
    $failed = 0
    try {
        $b = Start-Bowser $profile $log
        try {
            if ($null -eq $b.Main -or $b.Main.ToInt64() -eq 0) { throw 'Bowser window was not found (switch).' }
            Start-Sleep -Milliseconds 800
            foreach ($t in 1..2) {
                if ($t -gt 1) { Send-NewTab $b.Main }
                Submit-Address $b.Main "http://127.0.0.$t`:$($server.Port)/$($fixtureFor[$t])"
                [void](Wait-Until { (Count-Event $log 'page_usable') -ge $t } 30000)
            }
            Start-Sleep -Seconds 2
            $markers = @{ 1 = "BOWSER_BENCH_READY_LIGHT"; 2 = "BOWSER_BENCH_READY_DEMANDING" }
            # Two tabs: tab 2 is active, so the first Ctrl+Tab lands on tab 1, then alternates.
            $target = 1
            for ($i = 0; $i -lt $Switches; $i++) {
                $needle = $markers[$target]
                $sw = [Diagnostics.Stopwatch]::StartNew()
                Send-CtrlTab $b.Main
                $seen = $false
                while ($sw.ElapsedMilliseconds -lt 3000) {
                    if ([BowserPerfProbe.Native]::GetTitle($b.Main).Contains($needle)) { $seen = $true; break }
                    Start-Sleep -Milliseconds 1
                }
                if ($seen) { $samples.Add($sw.Elapsed.TotalMilliseconds) } else { $failed++ }
                $target = 3 - $target
                Start-Sleep -Milliseconds 300
            }
        } finally { $clean = Stop-Bowser $b $profile; if ($clean) { Remove-Profile $profile } }
    } finally { $server.Stop() }
    $rows = @($samples | ForEach-Object { [pscustomobject]@{ switch_ms = [math]::Round($_, 1) } })
    $csv = Join-Path $out "$safeLabel-switch-$stamp.csv"
    if ($rows.Count -gt 0) { $rows | Export-Csv -LiteralPath $csv -NoTypeInformation } else { Set-Content -LiteralPath $csv -Value 'switch_ms' }
    $lines = @(Report-Header 'tab switch')
    $lines += '| Metric | Median (p25 to p75, n) | Failed to observe |'
    $lines += '| --- | --- | --- |'
    $lines += "| Ctrl+Tab to window title shows other tab (ms) | $(Format-Stat (Get-ColumnStat $rows 'switch_ms')) | $failed |"
    $lines += ''
    $lines += "Raw rows: $csv. Polling interval is about 1 ms plus PowerShell overhead; the main-window title (active tab title: light vs demanding fixture) is polled, not a frame presentation."
    Write-Report (Join-Path $out "$safeLabel-switch-$stamp.md") $lines
}

switch ($Mode) {
    'switch' { Invoke-SwitchMode }
    'startup' { Invoke-StartupMode }
    'tabs' { Invoke-TabsMode }
    'idle' { Invoke-IdleMode }
}

if ($script:allClean) { Remove-Profile $work } else { Write-Warning "Profiles kept under $work because a Bowser process group did not exit." }
