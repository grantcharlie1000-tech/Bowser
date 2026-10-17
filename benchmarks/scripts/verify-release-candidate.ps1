# Bounded native release smoke, not a performance audit or full human UI QA.
# Run after cargo build --release --locked --bin bowser --features html-chrome-default:
# pwsh -NoProfile -File benchmarks/scripts/verify-release-candidate.ps1
# Requires Windows, WebView2, Node, and an interactive desktop (windows-latest).
# No installer, release publication, updater pin change, or live user profile access.
[CmdletBinding()]
param(
    [string]$Out = "$PSScriptRoot\..\results\release-candidate",
    [ValidateRange(15, 120)][int]$TimeoutSeconds = 60,
    [switch]$SkipKeyboard
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
# Deliberately no -Exe override: smoke the build output, not an installed/stale copy.
$exe = Join-Path $root 'target\release\bowser.exe'
$Out = [IO.Path]::GetFullPath($Out)
New-Item -ItemType Directory -Force $Out | Out-Null
$run = Join-Path ([IO.Path]::GetTempPath()) ('bowser-release-smoke-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $run | Out-Null
$report = [ordered]@{
    schema_version = 1
    scope = 'Windows native release smoke; not full human UI QA or a performance audit'
    started_at_utc = [DateTime]::UtcNow.ToString('o')
    status = 'failed'
    executable = $exe
    source_commit = $null
    source_status = $null
    executable_sha256 = $null
    data_directory = $run
    updater = 'Skipped by --benchmark-log; no pin/install/update state changed'
    checks = [ordered]@{}
    screenshots = @()
    unverified = @('Settings interactions', 'Updater download/install/restart', 'Full human UI QA')
    error = $null
}
$app = $null; $server = $null; $savedEnv = @{}; $failure = $null
$eventLog = Join-Path $Out 'native-events.csv'
$requests = Join-Path $run 'requests.jsonl'
$portFile = Join-Path $run 'port.txt'

function Wait-Until([scriptblock]$Condition, [string]$Description) {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        if ($app) { $app.Refresh(); if ($app.HasExited) { throw "Bowser exited before $Description (exit $($app.ExitCode))" } }
        if ($server) { $server.Refresh(); if ($server.HasExited) { throw "Fixture server exited before $Description" } }
        if (& $Condition) { return }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Timed out waiting for $Description"
}
function Has-Marker([string]$Marker) {
    if (-not (Test-Path -LiteralPath $requests)) { return $false }
    return [bool](@(Get-Content -LiteralPath $requests | ConvertFrom-Json | Where-Object { $_.url -eq $Marker }).Count)
}
function Window-Rect([IntPtr]$Window = $script:hwnd) {
    $rect = [BowserReleaseSmokeNative+RECT]::new()
    if (-not [BowserReleaseSmokeNative]::GetWindowRect($Window, [ref]$rect)) { throw 'GetWindowRect failed' }
    return [ordered]@{ left = $rect.Left; top = $rect.Top; width = $rect.Right - $rect.Left; height = $rect.Bottom - $rect.Top }
}
function Assert-OwnedWindow {
    $ownerId = [uint32]0
    [void][BowserReleaseSmokeNative]::GetWindowThreadProcessId($script:hwnd, [ref]$ownerId)
    if ($ownerId -ne $app.Id -or -not [BowserReleaseSmokeNative]::IsWindowVisible($script:hwnd)) { throw 'Native window is not visible and owned by this run' }
}
function Focus-OwnedWindow {
    Assert-OwnedWindow
    [void][BowserReleaseSmokeNative]::SetForegroundWindow($script:hwnd)
    Start-Sleep -Milliseconds 200
    # Never send global keyboard input unless this exact owned window has focus.
    if ([BowserReleaseSmokeNative]::GetForegroundWindow() -ne $script:hwnd) { throw 'Interactive desktop/foreground unavailable; no keyboard input sent' }
}
function Send-OwnedKeys([string]$Keys) {
    Focus-OwnedWindow
    [Windows.Forms.SendKeys]::SendWait($Keys)
}
function Save-NativeScreenshot([string]$Name, [IntPtr]$Window = $script:hwnd) {
    Assert-OwnedWindow
    $r = Window-Rect $Window
    if ($r.width -lt 100 -or $r.height -lt 60 -or $r.width -gt 10000 -or $r.height -gt 10000) { throw 'Invalid native window dimensions' }
    $bitmap = [Drawing.Bitmap]::new($r.width, $r.height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $dc = $graphics.GetHdc()
    try { $ok = [BowserReleaseSmokeNative]::PrintWindow($Window, $dc, 2) }
    finally { $graphics.ReleaseHdc($dc); $graphics.Dispose() }
    try {
        if (-not $ok) { throw 'PrintWindow failed; no screenshot claimed' }
        # A successful API return can still be an empty GPU surface. Reject a uniform sample.
        $colors = [Collections.Generic.HashSet[int]]::new()
        for ($x = 0; $x -lt $bitmap.Width; $x += 19) {
            for ($y = 0; $y -lt $bitmap.Height; $y += 19) { [void]$colors.Add($bitmap.GetPixel($x, $y).ToArgb()) }
        }
        if ($colors.Count -lt 4) { throw 'PrintWindow returned a blank/uniform image; native capture is inconclusive' }
        $path = Join-Path $Out "$Name.png"
        $bitmap.Save($path, [Drawing.Imaging.ImageFormat]::Png)
        $report.screenshots += [ordered]@{ file = "$Name.png"; method = 'PrintWindow/PW_RENDERFULLCONTENT'; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant(); sampled_colors = $colors.Count; rect = $r }
    } finally { $bitmap.Dispose() }
}

try {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows is required; no native smoke was run' }
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Release executable is missing: $exe" }
    $report.executable_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    $report.executable_bytes = (Get-Item -LiteralPath $exe).Length
    $report.source_commit = (& git -C $root rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify source commit' }
    $report.source_status = @(& git -C $root status --porcelain --untracked-files=no)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect source status' }
    $report.checks.clean_source = ($report.source_status.Count -eq 0)
    if (-not $report.checks.clean_source) { throw 'Tracked source checkout must be clean so the candidate can be tied to a commit' }
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class BowserReleaseSmokeNative {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public delegate bool EnumProc(IntPtr window, IntPtr param);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr param);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr window, uint command);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    public static IntPtr FindFullscreenNotice(uint pid, IntPtr owner) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, param) => {
            uint actualPid; GetWindowThreadProcessId(window, out actualPid);
            if (actualPid != pid || GetWindow(window, 4) != owner || !IsWindowVisible(window)) return true;
            var cls = new StringBuilder(128); GetClassName(window, cls, cls.Capacity);
            var text = new StringBuilder(2048); GetWindowText(window, text, text.Capacity);
            if (String.Equals(cls.ToString(), "Static", StringComparison.OrdinalIgnoreCase) && text.ToString().Contains("Bowser full screen")) found = window;
            return true;
        }, IntPtr.Zero);
        return found;
    }
    public static string WindowText(IntPtr window) {
        var text = new StringBuilder(2048); GetWindowText(window, text, text.Capacity); return text.ToString();
    }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int command);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int height, bool repaint);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
}
'@
    [void][BowserReleaseSmokeNative]::SetProcessDPIAware()
    # A tiny server binds an OS-assigned loopback port. Browser JS sends a distinct marker only
    # after the requested fixture document has loaded. No external origin or local user file is used.
    $serverScript = Join-Path $run 'fixture.cjs'
    @'
const http = require('node:http');
const fs = require('node:fs');
const [portFile, requests] = process.argv.slice(2);
const server = http.createServer((req, res) => {
  fs.appendFileSync(requests, JSON.stringify({url: req.url}) + '\n');
  res.setHeader('Cache-Control', 'no-store');
  res.setHeader('Content-Security-Policy', "default-src 'none'; script-src 'unsafe-inline'; connect-src 'self'; style-src 'unsafe-inline'; img-src 'self'");
  if (['/ready/start', '/ready/navigated', '/fullscreen/entered', '/fullscreen/exited'].includes(req.url)) { res.writeHead(204); res.end(); return; }
  if (req.url !== '/start' && req.url !== '/navigated') { res.writeHead(404); res.end(); return; }
  const name = req.url.slice(1);
  res.setHeader('Content-Type', 'text/html; charset=utf-8');
  res.end(`<!doctype html><html><head><title>Bowser release smoke ${name}</title><style>body{background:#e5efff;color:#142c55;font:24px system-ui;padding:60px}main{border:6px solid #2955aa;padding:36px;background:white}</style></head><body><main><h1>Bowser native release smoke</h1><p>Local offline fixture: ${name}</p><p>This is a bounded smoke check, not full UI QA.</p></main><script>window.addEventListener('load',()=>fetch('/ready/${name}',{cache:'no-store'}));document.addEventListener('keydown',event=>{if(event.key==='f')document.documentElement.requestFullscreen().catch(()=>{});});document.addEventListener('fullscreenchange',()=>fetch('/fullscreen/'+(document.fullscreenElement?'entered':'exited'),{cache:'no-store'}));</script></body></html>`);
});
server.listen(0, '127.0.0.1', () => fs.writeFileSync(portFile, String(server.address().port)));
'@ | Set-Content -LiteralPath $serverScript -Encoding utf8
    $node = (Get-Command node -ErrorAction Stop).Source
    $server = Start-Process -FilePath $node -ArgumentList @("`"$serverScript`"", "`"$portFile`"", "`"$requests`"") -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $Out 'fixture-stderr.txt')
    Wait-Until { Test-Path -LiteralPath $portFile } 'local fixture server'
    $port = [int](Get-Content -LiteralPath $portFile -Raw)
    $base = "http://127.0.0.1:$port"
    foreach ($name in @('LOCALAPPDATA', 'BOWSER_DATA_DIR', 'BOWSER_UI', 'BOWSER_UPDATE_URL', 'BOWSER_UPDATE_PUBKEY')) {
        $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        [Environment]::SetEnvironmentVariable($name, $null, 'Process')
    }
    $env:LOCALAPPDATA = Join-Path $run 'local-app-data'
    $env:BOWSER_DATA_DIR = Join-Path $run 'app-data'
    New-Item -ItemType Directory -Force $env:LOCALAPPDATA, $env:BOWSER_DATA_DIR | Out-Null
    $webviewData = Join-Path $run 'webview2'
    $arguments = @('--data-dir', "`"$env:BOWSER_DATA_DIR`"", '--benchmark-user-data-dir', "`"$webviewData`"", '--benchmark-log', "`"$eventLog`"", '--open-url', "$base/start")
    $app = Start-Process -FilePath $exe -ArgumentList $arguments -WorkingDirectory $root -PassThru
    $report.pid = $app.Id
    $report.arguments = $arguments
    $script:hwnd = [IntPtr]::Zero
    Wait-Until { $app.Refresh(); $script:hwnd = $app.MainWindowHandle; $script:hwnd -ne [IntPtr]::Zero } 'owned native window'
    Assert-OwnedWindow
    [void][BowserReleaseSmokeNative]::ShowWindow($script:hwnd, 9)
    [void][BowserReleaseSmokeNative]::MoveWindow($script:hwnd, 20, 20, 1000, 700, $true)
    Wait-Until { Has-Marker '/ready/start' } 'WebView2 fixture JavaScript'
    $report.checks.native_window = $true
    $report.checks.webview2_local_fixture_javascript = $true
    Start-Sleep -Seconds 2
    Save-NativeScreenshot '01-native-start'
    if (-not $SkipKeyboard) {
        Send-OwnedKeys '^l'
        Send-OwnedKeys "$base/navigated{ENTER}"
        Wait-Until { Has-Marker '/ready/navigated' } 'Ctrl+L local navigation'
        $report.checks.ctrl_l_local_navigation = $true
        Start-Sleep -Seconds 1
        Save-NativeScreenshot '02-native-navigated'
        $before = Window-Rect
        Send-OwnedKeys '{F11}'
        Wait-Until {
            $r = Window-Rect
            $screen = [Windows.Forms.Screen]::FromHandle($script:hwnd).Bounds
            [Math]::Abs($r.left - $screen.Left) -le 2 -and [Math]::Abs($r.top - $screen.Top) -le 2 -and [Math]::Abs($r.width - $screen.Width) -le 2 -and [Math]::Abs($r.height - $screen.Height) -le 2
        } 'F11 monitor-sized window'
        Save-NativeScreenshot '03-native-f11'
        Send-OwnedKeys '{F11}'
        Wait-Until { $r = Window-Rect; $r.left -eq $before.left -and $r.top -eq $before.top -and $r.width -eq $before.width -and $r.height -eq $before.height } 'F11 restored window bounds'
        $report.checks.f11_enter_exit_geometry = $true
        # A real page keyboard gesture, not injected script or a browser-only F11 transition.
        Send-OwnedKeys 'f'
        Wait-Until { Has-Marker '/fullscreen/entered' } 'page Fullscreen API entry'
        $script:notice = [IntPtr]::Zero
        Wait-Until { $script:notice = [BowserReleaseSmokeNative]::FindFullscreenNotice([uint32]$app.Id, $script:hwnd); $script:notice -ne [IntPtr]::Zero } 'owned native fullscreen security notice'
        $noticeText = [BowserReleaseSmokeNative]::WindowText($script:notice)
        $compact = $noticeText -replace '\s', ''
        foreach ($required in @('Bowserfullscreen', ('Top-levelpageat' + $base), 'PressEsctoexitfullscreen')) {
            if (-not $compact.Contains($required)) { throw "Native fullscreen notice is missing trusted identity/exit text: $required" }
        }
        $report.fullscreen_notice_text = $noticeText
        Save-NativeScreenshot '04-native-page-fullscreen'
        Save-NativeScreenshot '05-native-page-fullscreen-notice' $script:notice
        Send-OwnedKeys '{ESC}'
        Wait-Until { Has-Marker '/fullscreen/exited' } 'page Fullscreen API exit'
        Wait-Until { [BowserReleaseSmokeNative]::FindFullscreenNotice([uint32]$app.Id, $script:hwnd) -eq [IntPtr]::Zero } 'native fullscreen notice removal'
        Wait-Until { $r = Window-Rect; $r.left -eq $before.left -and $r.top -eq $before.top -and $r.width -eq $before.width -and $r.height -eq $before.height } 'page fullscreen restored window bounds'
        $report.checks.page_fullscreen_origin_notice_and_escape = $true
    } else {
        $report.unverified += @('Ctrl+L navigation', 'F11 enter/exit', 'Page-requested fullscreen origin/exit notice')
    }
    if (-not $app.CloseMainWindow()) { throw 'Native window did not accept graceful close' }
    if (-not $app.WaitForExit(15000)) { throw 'Bowser did not exit within 15 seconds' }
    if ($app.ExitCode -ne 0) { throw "Bowser exited with code $($app.ExitCode)" }
    $report.checks.clean_exit = $true
    # PerfLog flushes on Drop, so examine instrumentation only after graceful exit.
    $events = @(Import-Csv -LiteralPath $eventLog)
    $names = @($events | ForEach-Object { $_.event })
    foreach ($expected in @('engine_ready', 'html_chrome_shown', 'navigation_started', 'navigation_completed')) {
        if ($names -notcontains $expected) { throw "Missing native event: $expected" }
    }
    if ($names -contains 'navigation_failed') { throw 'Native event log contains a failed navigation' }
    $report.checks.engine_ready_html_chrome_and_navigation_events = $true
    $report.event_names = $names
    $report.checks.executable_unchanged = ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant() -eq $report.executable_sha256)
    if (-not $report.checks.executable_unchanged) { throw 'Executable changed during smoke verification' }
    $report.status = if ($SkipKeyboard) { 'passed_startup_only' } else { 'passed_bounded_native_smoke' }
} catch {
    $failure = $_
    $report.error = $_.Exception.Message
} finally {
    # Only owned process trees and WebView2 instances carrying this GUID-qualified temp path.
    if ($app) {
        $app.Refresh()
        if (-not $app.HasExited) { & taskkill /PID $app.Id /T /F 2>&1 | Out-Null }
    }
    if ($server) {
        $server.Refresh()
        if (-not $server.HasExited) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }
    }
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" -ErrorAction SilentlyContinue |
            Where-Object { $_.CommandLine -and $_.CommandLine.Contains($run) } |
            ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    }
    foreach ($name in $savedEnv.Keys) { [Environment]::SetEnvironmentVariable($name, $savedEnv[$name], 'Process') }
    if (Test-Path -LiteralPath $requests) { Copy-Item -LiteralPath $requests -Destination (Join-Path $Out 'fixture-requests.jsonl') }
    $report.finished_at_utc = [DateTime]::UtcNow.ToString('o')
    $report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $Out 'native-smoke.json') -Encoding utf8
    # Keep only this run's isolated temp data for diagnosis. CI runner disposal removes it.
}
if ($failure) { throw $failure }
Write-Host "Native release smoke: $($report.status). Evidence: $Out"
