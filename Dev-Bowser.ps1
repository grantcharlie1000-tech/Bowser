# Builds and opens Dev Bowser, then rebuilds when the source changes.
# The dev profile is %LOCALAPPDATA%\Bowser-Dev. The installed Bowser is never replaced.
[CmdletBinding()]
param(
    [switch]$Once
)

$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSCommandPath))
$localAppData = [Environment]::GetFolderPath('LocalApplicationData')
if ([string]::IsNullOrWhiteSpace($localAppData)) {
    throw 'Windows did not provide LocalAppData.'
}
$dataRoot = Join-Path $localAppData 'Bowser-Dev'
$appDirectory = Join-Path (Join-Path $localAppData 'Programs') 'Bowser Dev'
$launchExe = Join-Path $appDirectory 'Bowser.exe'
$buildRoot = Join-Path $repoRoot 'target\bowser-dev'
$builtExe = Join-Path $buildRoot 'debug\bowser.exe'
$lockPath = Join-Path $dataRoot 'watcher.lock'

function Get-DevSourceHash {
    $files = @('Cargo.toml', 'Cargo.lock', 'build.rs', 'bowser.manifest', 'bowser.rc')
    foreach ($relative in $files) {
        $path = Join-Path $repoRoot $relative
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Required Bowser build input is missing: $relative"
        }
    }
    $paths = foreach ($relative in $files) { Join-Path $repoRoot $relative }
    foreach ($relative in @('src', 'ui\src', 'ui\dist', 'icons')) {
        $path = Join-Path $repoRoot $relative
        if (Test-Path -LiteralPath $path -PathType Container) {
            $paths += Get-ChildItem -LiteralPath $path -File -Recurse | ForEach-Object FullName
        }
    }
    $manifest = Join-Path $repoRoot 'ui\dist.manifest'
    if (Test-Path -LiteralPath $manifest -PathType Leaf) { $paths += $manifest }
    $entries = foreach ($path in ($paths | Sort-Object -Unique)) {
        $relative = $path.Substring($repoRoot.Length).TrimStart([char[]]@('\', '/'))
        $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        "$relative`0$hash"
    }
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes(($entries -join "`n"))
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

function Get-TreeHash {
    param([Parameter(Mandatory = $true)][string]$Relative)
    $path = Join-Path $repoRoot $Relative
    if (-not (Test-Path -LiteralPath $path -PathType Container)) { return '' }
    $entries = foreach ($file in (Get-ChildItem -LiteralPath $path -File -Recurse | Sort-Object FullName)) {
        $name = $file.FullName.Substring($path.Length)
        $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$name`0$hash"
    }
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes(($entries -join "`n"))
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

function Test-WatcherLock {
    if (-not (Test-Path -LiteralPath $lockPath -PathType Leaf)) { return }
    $owner = (Get-Content -LiteralPath $lockPath -Raw).Trim()
    if ($owner -match '^\d+$') {
        $running = Get-Process -Id ([int]$owner) -ErrorAction SilentlyContinue
        if ($running -and $running.ProcessName -match 'powershell|pwsh') {
            throw "Dev Bowser is already watching in process $owner. Close that window before starting another."
        }
    }
}

function Get-DevProcesses {
    @(Get-Process -Name 'Bowser' -ErrorAction SilentlyContinue | Where-Object {
        try {
            [string]::Equals([IO.Path]::GetFullPath($_.Path), $launchExe, [StringComparison]::OrdinalIgnoreCase)
        }
        catch { $false }
    })
}

function Stop-DevBowser {
    $processes = @(Get-DevProcesses)
    if ($processes.Count -eq 0) { return $true }
    Write-Host 'Closing Dev Bowser so the new build can open...'
    foreach ($process in $processes) {
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
            [void]$process.CloseMainWindow()
        }
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(8)
    do {
        Start-Sleep -Milliseconds 250
        $processes = @(Get-DevProcesses)
    } while ($processes.Count -gt 0 -and [DateTime]::UtcNow -lt $deadline)
    if ($processes.Count -gt 0) {
        Write-Host 'Dev Bowser is still open. The new build will open after that window is closed. No process was terminated.'
        return $false
    }
    return $true
}

function Install-DevShortcut {
    $startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Bowser Dev.lnk'
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($startMenu)
    $shortcut.TargetPath = Join-Path $repoRoot 'Dev-Bowser.cmd'
    $shortcut.WorkingDirectory = $repoRoot
    $shortcut.Description = 'Build and watch Dev Bowser'
    $shortcut.WindowStyle = 1
    if (Test-Path -LiteralPath $launchExe -PathType Leaf) {
        $shortcut.IconLocation = "$launchExe,0"
    }
    $shortcut.Save()
    Write-Host "Start Menu shortcut: $startMenu"
}

function Build-DevBowser {
    $uiSource = Join-Path $repoRoot 'ui\src'
    $uiScript = Join-Path $repoRoot 'ui\package.json'
    if ((Test-Path -LiteralPath $uiSource -PathType Container) -and (Test-Path -LiteralPath $uiScript -PathType Leaf)) {
        $sourceHash = Get-TreeHash -Relative 'ui\src'
        $stampPath = Join-Path $buildRoot 'ui-src.sha256'
        $previous = if (Test-Path -LiteralPath $stampPath -PathType Leaf) { (Get-Content -LiteralPath $stampPath -Raw).Trim() } else { '' }
        if ($sourceHash -ne $previous) {
            Write-Host 'UI source changed. Building the HTML chrome.'
            $npm = Get-Command npm.cmd -ErrorAction SilentlyContinue
            if (-not $npm) { $npm = Get-Command npm -ErrorAction SilentlyContinue }
            if (-not $npm) { throw 'npm was not found, so the changed UI could not be built.' }
            Push-Location (Join-Path $repoRoot 'ui')
            try {
                & $npm.Source run build
                if ($LASTEXITCODE -ne 0) { throw "The UI build failed with exit code $LASTEXITCODE." }
            }
            finally {
                Pop-Location
            }
            New-Item -ItemType Directory -Path $buildRoot -Force | Out-Null
            [IO.File]::WriteAllText($stampPath, $sourceHash)
        }
    }

    Write-Host 'Building Dev Bowser.'
    $cargo = Get-Command cargo -ErrorAction Stop
    Push-Location $repoRoot
    try {
        & $cargo.Source build --bin bowser --locked --features html-chrome-default --target-dir $buildRoot
        if ($LASTEXITCODE -ne 0) { throw "The dev build failed with exit code $LASTEXITCODE. The running Dev Bowser was left as it is." }
    }
    finally {
        Pop-Location
    }
    if (-not (Test-Path -LiteralPath $builtExe -PathType Leaf)) {
        throw "The build did not produce $builtExe."
    }
}

function Publish-DevBowser {
    if (-not (Stop-DevBowser)) { return $false }
    New-Item -ItemType Directory -Path $appDirectory -Force | Out-Null
    $temporary = Join-Path $appDirectory 'Bowser.exe.download'
    Copy-Item -LiteralPath $builtExe -Destination $temporary -Force
    Move-Item -LiteralPath $temporary -Destination $launchExe -Force
    $process = Start-Process -FilePath $launchExe -ArgumentList '--dev' -WorkingDirectory $appDirectory -PassThru
    Write-Host "Opened Dev Bowser (PID $($process.Id)). Profile: $dataRoot" -ForegroundColor Green
    return $true
}

New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
if (-not $Once) { Test-WatcherLock }
[IO.File]::WriteAllText($lockPath, "$PID")

try {
    $builtHash = $null
    $stableHash = $null
    while ($true) {
        $sourceHash = Get-DevSourceHash
        if ($sourceHash -eq $stableHash -and (Test-Path -LiteralPath $launchExe -PathType Leaf)) {
            if ($Once) { break }
            Start-Sleep -Seconds 3
            continue
        }
        Start-Sleep -Seconds 1
        if ((Get-DevSourceHash) -ne $sourceHash) { continue }
        try {
            Build-DevBowser
            $exeHash = (Get-FileHash -LiteralPath $builtExe -Algorithm SHA256).Hash.ToLowerInvariant()
            $published = $true
            if ($exeHash -ne $builtHash -or -not (Test-Path -LiteralPath $launchExe -PathType Leaf)) {
                $published = Publish-DevBowser
                if ($published) {
                    $builtHash = $exeHash
                    Install-DevShortcut
                }
            }
            if ($published) { $stableHash = $sourceHash }
        }
        catch {
            Write-Host $_.Exception.Message -ForegroundColor Red
            if ($Once -and -not (Test-Path -LiteralPath $launchExe -PathType Leaf)) { throw }
            $stableHash = $null
            Start-Sleep -Seconds 8
        }
        if ($Once) { break }
        Start-Sleep -Seconds 2
    }
}
finally {
    if ((Test-Path -LiteralPath $lockPath -PathType Leaf) -and ((Get-Content -LiteralPath $lockPath -Raw).Trim() -eq "$PID")) {
        Remove-Item -LiteralPath $lockPath -Force -ErrorAction SilentlyContinue
    }
}
