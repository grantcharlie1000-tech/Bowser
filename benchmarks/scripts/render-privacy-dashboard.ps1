# Renders the real Privacy dashboard page (ui/dist plus the Rust-generated fixture snapshot) in
# headless Edge with a stand-in for the WebView2 bridge, and saves a screenshot. The page is served
# over http from a throwaway folder so its real Content-Security-Policy and Trusted Types hold. No
# Bowser process is started and no profile data is touched.
param(
  [string]$Out = "$PSScriptRoot\..\results\surfaces\prefs",
  [string]$Name = "dashboard-harness",
  [string]$Scheme = "dark",
  [int]$Width = 900,
  [int]$Height = 1500,
  [int]$Scroll = 0,
  [string]$Search = "",
  [int]$Port = 18791
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path "$PSScriptRoot\..\.."
$dist = Join-Path $root 'ui\dist'
$fixture = Get-Content (Join-Path $root 'ui\test\fixtures\privacy-snapshot.json') -Raw
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$tmp = Join-Path $env:TEMP ("bowser-harness-" + (Get-Random))
New-Item -ItemType Directory -Force $tmp | Out-Null
Copy-Item "$dist\app.js", "$dist\app.css" $tmp
$dark = $Scheme -eq 'dark'
$darkJs = if ($dark) { 'true' } else { 'false' }
$theme = if ($dark) { '{"bg":"#20242c","fill":"#2b303a","border":"#444b58","focusBorder":"#7aa7ff","text":"#f2f4f8","placeholder":"#a3abba","completion":"#8fb4ff","selection":"#33518f","hover":"rgba(255,255,255,0.08)","progress":"#7aa7ff"}' } else { '{"bg":"#f6f7f9","fill":"#e9ecf1","border":"#c6ccd6","focusBorder":"#2b5fa8","text":"#1b1f27","placeholder":"#5a6372","completion":"#2b5fa8","selection":"#bcd3f5","hover":"rgba(0,0,0,0.06)","progress":"#2b5fa8"}' }
$html = @"
<!doctype html><html lang="en"><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; require-trusted-types-for 'script'; trusted-types 'none'"><title>harness</title><link rel="stylesheet" href="app.css"></head>
<body><div id="app"></div><script src="mock.js"></script><script src="app.js"></script></body></html>
"@
$mock = @"
const listeners = [];
const snap = $fixture;
const state = { t: 'state', scale: 1, solid: true, dark: $darkJs, back: false, forward: false, loading: false, progress: 0, security: 'secure', label: '', theme: $theme };
window.chrome = { webview: {
  addEventListener: (t, f) => { if (t === 'message') listeners.push(f); },
  postMessage: (raw) => {
    const m = JSON.parse(raw);
    if (m.t !== 'ready') return;
    setTimeout(() => {
      listeners.forEach(f => f({ data: state }));
      listeners.forEach(f => f({ data: snap }));
      const s = document.querySelector('.pg-scroll'); if (s) s.scrollTop = $Scroll;
      const q = '$Search'; const input = document.querySelector('.pn-search input');
      if (q && input) { input.value = q; input.dispatchEvent(new Event('input', { bubbles: true })); }
    }, 50);
  } } };
"@
Set-Content (Join-Path $tmp 'harness.html') $html -Encoding UTF8
Set-Content (Join-Path $tmp 'mock.js') $mock -Encoding UTF8
$server = Start-Process node -ArgumentList @("`"$PSScriptRoot\static-server.mjs`"", "`"$tmp`"", "$Port") -PassThru -WindowStyle Hidden
Start-Sleep -Milliseconds 700
$edge = (Get-Command msedge -ErrorAction SilentlyContinue).Source
if (-not $edge) { $edge = "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe" }
$prof = Join-Path $tmp 'edge-profile'
try {
  & $edge --headless=new --disable-gpu --user-data-dir="$prof" --hide-scrollbars --force-device-scale-factor=1 --window-size="$Width,$Height" --virtual-time-budget=4000 --screenshot="$Out\$Name.png" "http://127.0.0.1:$Port/harness.html?view=dialog&p=privacy" | Out-Null
  Start-Sleep -Milliseconds 500
  Write-Host "saved $Name.png"
}
finally {
  Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like "*$prof*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 300
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
