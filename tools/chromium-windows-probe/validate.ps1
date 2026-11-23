# Windows-only CEF validation probe; never modifies Bowser's executable.
[CmdletBinding()]
param(
    [string]$WorkingDirectory = (Join-Path $env:TEMP 'BowserChromiumCefProbe'),
    [switch]$InteractiveSmoke
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Run this probe on Windows.'
}
$cefTag = 'cef-v156.3.0+156.0.15'
$expectedCommit = '224a785c7b41758b51821cbb7395ea57ba493a63'
$sourceUrl = 'https://github.com/tauri-apps/cef-rs.git'
$workRoot = [IO.Path]::GetFullPath($WorkingDirectory)
$checkout = Join-Path $workRoot 'cef-rs'
$bundlePath = Join-Path $checkout 'target\bundle'
New-Item -ItemType Directory -Path $workRoot -Force | Out-Null

if (-not (Test-Path (Join-Path $checkout '.git'))) {
    if (Test-Path $checkout) {
        throw "Refusing to overwrite existing non-Git directory: $checkout"
    }
    & git clone --depth 1 --branch $cefTag -- $sourceUrl $checkout
    if ($LASTEXITCODE -ne 0) { throw 'Could not clone the pinned cef-rs release.' }
}
$actualCommit = (& git -C $checkout rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $actualCommit -ne $expectedCommit) {
    throw "Wrong cef-rs revision ($actualCommit); expected $expectedCommit. Use a clean working directory."
}
Write-Host "Using cef-rs $cefTag at $actualCommit"
Push-Location $checkout
try {
    # Official cef-rs sample: tests real Rust bindings, compilation and packaging.
    & cargo run --bin bundle-cef-app -- cefsimple -o target/bundle
    if ($LASTEXITCODE -ne 0) { throw 'CEF compilation or bundling failed.' }
}
finally {
    Pop-Location
}
$requiredFiles = @('cefsimple.exe', 'libcef.dll', 'icudtl.dat')
foreach ($name in $requiredFiles) {
    $fullPath = Join-Path $bundlePath $name
    if (-not (Test-Path -LiteralPath $fullPath -PathType Leaf)) {
        throw "CEF package incomplete: missing $fullPath"
    }
}
$pak = Get-ChildItem -LiteralPath $bundlePath -Filter '*.pak' -Recurse -File | Select-Object -First 1
if ($null -eq $pak) { throw 'CEF package incomplete: no .pak resources found.' }
$files = @(Get-ChildItem -LiteralPath $bundlePath -File -Recurse)
$sumBytes = ($files | Measure-Object -Property Length -Sum).Sum
$report = [ordered]@{
    probe = 'Bowser Windows standalone Chromium feasibility'
    cef_ref = $cefTag
    cef_commit = $actualCommit
    build_and_package = 'pass'
    platform = 'Windows'
    required_files = $requiredFiles
    file_count = $files.Count
    bundle_mebibytes = [Math]::Round($sumBytes / 1MB, 1)
    interactive_render = 'not tested'
    bowser_integration = 'not tested'
    extensions_drm_and_performance = 'not tested'
}
if ($InteractiveSmoke) {
    $smokePage = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot 'smoke.html')).Path
    $smokeUrl = ([uri]$smokePage).AbsoluteUri
    Write-Host 'Inspect HTML, JavaScript, scrolling, input, WebGL and window resizing. Close the sample window when done.'
    $process = Start-Process -FilePath (Join-Path $bundlePath 'cefsimple.exe') -WorkingDirectory $bundlePath -ArgumentList @("--url=$smokeUrl") -PassThru
    Wait-Process -Id $process.Id
    $answer = Read-Host 'Did CEF render the page and accept input properly? (y/N)'
    if ($answer -eq 'y') {
        $report.interactive_render = 'user-confirmed pass'
    } else {
        $report.interactive_render = 'failed or not confirmed'
    }
}
$reportPath = Join-Path $workRoot 'validation.json'
$report | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $reportPath -Encoding UTF8
Write-Host "CEF Windows package validated. Report: $reportPath"
Write-Host "Bundle size: $($report.bundle_mebibytes) MiB"
if ($InteractiveSmoke -and $report.interactive_render -ne 'user-confirmed pass') {
    throw 'Interactive CEF rendering was not confirmed.'
}