$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$buildRoot = Join-Path $repoRoot 'target\bowser-launcher-build'
$releaseDir = Join-Path $buildRoot 'release'
$exePath = Join-Path $releaseDir 'bowser.exe'
$statePath = Join-Path $buildRoot '.bowser-launcher-source.sha256'
$versionsDir = Join-Path $repoRoot 'target\bowser-versions'

function Get-BowserSourceHash {
    param([Parameter(Mandatory = $true)][string]$Root)

    $requiredFiles = @('Cargo.toml', 'Cargo.lock', 'build.rs', 'bowser.manifest')
    $files = foreach ($relativePath in $requiredFiles) {
        $path = Join-Path $Root $relativePath
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Required Bowser build input is missing: $relativePath"
        }
        Get-Item -LiteralPath $path
    }

    $srcPath = Join-Path $Root 'src'
    if (-not (Test-Path -LiteralPath $srcPath -PathType Container)) {
        throw 'Required Bowser source directory is missing: src'
    }

    $files += Get-ChildItem -LiteralPath $srcPath -File -Recurse |
        Where-Object { $_.Extension -in @('.rs', '.txt') }

    foreach ($uiPath in @('ui\src', 'ui\dist', 'ui\dist.manifest')) {
        $path = Join-Path $Root $uiPath
        if (Test-Path -LiteralPath $path -PathType Container) {
            $files += Get-ChildItem -LiteralPath $path -File -Recurse
        }
        elseif (Test-Path -LiteralPath $path -PathType Leaf) {
            $files += Get-Item -LiteralPath $path
        }
    }

    foreach ($optionalPath in @('rust-toolchain', 'rust-toolchain.toml', '.cargo\config', '.cargo\config.toml')) {
        $path = Join-Path $Root $optionalPath
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $files += Get-Item -LiteralPath $path
        }
    }

    $entries = foreach ($file in ($files | Sort-Object FullName -Unique)) {
        $relativePath = $file.FullName.Substring($Root.Length).TrimStart([char[]]@('\', '/'))
        $fileHash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$relativePath`0$fileHash"
    }

    $buildConfiguration = "BOWSER_UPDATE_URL=$env:BOWSER_UPDATE_URL`nBOWSER_UPDATE_PUBKEY=$env:BOWSER_UPDATE_PUBKEY`n"
    $content = [System.Text.Encoding]::UTF8.GetBytes($buildConfiguration + ($entries -join "`n"))
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($sha.ComputeHash($content))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

try {
    $sourceHash = Get-BowserSourceHash -Root $repoRoot
    $previousHash = if (Test-Path -LiteralPath $statePath -PathType Leaf) {
        (Get-Content -LiteralPath $statePath -Raw).Trim()
    } else {
        ''
    }

    $needsBuild = ($sourceHash -ne $previousHash) -or (-not (Test-Path -LiteralPath $exePath -PathType Leaf))
    if ($needsBuild) {
        Write-Host 'Bowser source changed; building the latest release.'
        Push-Location -LiteralPath $repoRoot
        try {
            $cargo = Get-Command cargo -ErrorAction Stop
            & $cargo.Source build --release --locked --target-dir $buildRoot
            $buildExitCode = $LASTEXITCODE
        }
        finally {
            Pop-Location
        }

        if ($buildExitCode -ne 0) {
            Write-Host "Bowser release build failed with exit code $buildExitCode. The previous executable was not launched." -ForegroundColor Red
            exit $buildExitCode
        }
        if (-not (Test-Path -LiteralPath $exePath -PathType Leaf)) {
            Write-Host "The build reported success but did not produce $exePath. Bowser was not launched." -ForegroundColor Red
            exit 1
        }
        if ((Get-BowserSourceHash -Root $repoRoot) -ne $sourceHash) {
            Write-Host 'Bowser source changed during the build. Nothing was launched; click again to build the latest source.' -ForegroundColor Red
            exit 1
        }

        $null = New-Item -ItemType Directory -Path $buildRoot -Force
        $temporaryStatePath = "$statePath.tmp"
        [System.IO.File]::WriteAllText($temporaryStatePath, $sourceHash, [System.Text.Encoding]::ASCII)
        Move-Item -LiteralPath $temporaryStatePath -Destination $statePath -Force
    }
    else {
        Write-Host 'Bowser source is unchanged; launching the latest successful release.'
    }

    $binaryHash = (Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash.ToLowerInvariant()
    $null = New-Item -ItemType Directory -Path $versionsDir -Force
    $versionedDir = Join-Path $versionsDir "$sourceHash-$binaryHash"
    $versionedExePath = Join-Path $versionedDir 'bowser.exe'

    if (Test-Path -LiteralPath $versionedExePath -PathType Leaf) {
        $existingHash = (Get-FileHash -LiteralPath $versionedExePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($existingHash -ne $binaryHash) {
            $versionedDir = Join-Path $versionsDir "$sourceHash-$binaryHash-$PID-$([guid]::NewGuid().ToString('N'))"
            $versionedExePath = Join-Path $versionedDir 'bowser.exe'
        }
    }

    if (-not (Test-Path -LiteralPath $versionedExePath -PathType Leaf)) {
        $null = New-Item -ItemType Directory -Path $versionedDir -Force
        $temporaryExePath = Join-Path $versionedDir "bowser.$PID.tmp"
        Copy-Item -LiteralPath $exePath -Destination $temporaryExePath
        $copiedHash = (Get-FileHash -LiteralPath $temporaryExePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($copiedHash -ne $binaryHash) {
            Remove-Item -LiteralPath $temporaryExePath -Force -ErrorAction SilentlyContinue
            throw 'The versioned Bowser executable did not match the successful build.'
        }

        try {
            Move-Item -LiteralPath $temporaryExePath -Destination $versionedExePath -ErrorAction Stop
        }
        catch {
            if (-not (Test-Path -LiteralPath $versionedExePath -PathType Leaf) -or
                (Get-FileHash -LiteralPath $versionedExePath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $binaryHash) {
                throw
            }
            Remove-Item -LiteralPath $temporaryExePath -Force -ErrorAction SilentlyContinue
        }
    }

    $previousUiMode = [Environment]::GetEnvironmentVariable('BOWSER_UI', 'Process')
    [Environment]::SetEnvironmentVariable('BOWSER_UI', 'html', 'Process')
    try {
        $launchedProcess = Start-Process -FilePath $versionedExePath -WorkingDirectory $repoRoot -ArgumentList '--dev' -PassThru
    }
    finally {
        [Environment]::SetEnvironmentVariable('BOWSER_UI', $previousUiMode, 'Process')
    }
    Write-Host "Launched Dev Bowser (PID $($launchedProcess.Id)). Its profile is %LOCALAPPDATA%\Bowser-Dev, separate from the installed browser." -ForegroundColor Green
    Write-Host $versionedExePath

    # Old executables remain in use while Windows has them open. A later launch removes
    # versions whose processes have exited; the current version is always retained.
    Get-ChildItem -LiteralPath $versionsDir -Directory |
        Where-Object { $_.FullName -ne $versionedDir } |
        ForEach-Object {
            $oldExePath = Join-Path $_.FullName 'bowser.exe'
            if (Test-Path -LiteralPath $oldExePath -PathType Leaf) {
                try {
                    Remove-Item -LiteralPath $oldExePath -Force -ErrorAction Stop
                    if (@(Get-ChildItem -LiteralPath $_.FullName -Force).Count -eq 0) {
                        Remove-Item -LiteralPath $_.FullName -Force -ErrorAction Stop
                    }
                }
                catch {
                    # Windows still has this version open, so leave it in place.
                }
            }
        }
    Get-ChildItem -LiteralPath $versionsDir -Filter 'bowser-*.exe' -File |
        ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue }
}
catch {
    Write-Host "Unable to prepare or launch Bowser: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
