$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$exePath = Join-Path $repoRoot 'target\debug\bowser.exe'

Set-Location -LiteralPath $repoRoot

Write-Host "Building Bowser from $repoRoot..."
& cargo build --locked
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build --locked failed with exit code $LASTEXITCODE. Bowser was not launched."
    exit $LASTEXITCODE
}

if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
    Write-Error "Build succeeded, but the expected executable was not produced: $exePath"
    exit 1
}

Start-Process -FilePath $exePath -WorkingDirectory $repoRoot
exit 0
