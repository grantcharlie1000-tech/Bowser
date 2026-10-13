[CmdletBinding()]
param(
    [string]$InstallDirectory,
    [string]$DataDirectory,
    [string]$StartMenuDirectory,
    [string]$DesktopShortcutPath,
    [string]$CargoPath,
    [switch]$DesktopShortcut,
    [switch]$NoPrompt
)

$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSCommandPath))
$localAppData = [Environment]::GetFolderPath('LocalApplicationData')
if ([string]::IsNullOrWhiteSpace($localAppData)) {
    throw 'Windows did not provide LocalAppData; Bowser cannot be installed for this user.'
}

$programsDirectory = Join-Path $localAppData 'Programs'
if ([string]::IsNullOrWhiteSpace($InstallDirectory)) {
    $InstallDirectory = Join-Path $programsDirectory 'Bowser'
}
if ([string]::IsNullOrWhiteSpace($DataDirectory)) {
    $DataDirectory = Join-Path $localAppData 'Bowser'
}
$InstallDirectory = [IO.Path]::GetFullPath($InstallDirectory)
$DataDirectory = [IO.Path]::GetFullPath($DataDirectory)
$isIsolatedRun = $PSBoundParameters.ContainsKey('InstallDirectory') -or
    $PSBoundParameters.ContainsKey('DataDirectory') -or
    $PSBoundParameters.ContainsKey('StartMenuDirectory')

$programName = 'Bowser'
$installedExe = Join-Path $InstallDirectory 'Bowser.exe'
$startMenuRoot = [Environment]::GetFolderPath('StartMenu')
if ([string]::IsNullOrWhiteSpace($StartMenuDirectory)) {
    $StartMenuDirectory = Join-Path (Join-Path $startMenuRoot 'Programs') 'Bowser'
}
$StartMenuDirectory = [IO.Path]::GetFullPath($StartMenuDirectory)
$startMenuShortcut = Join-Path $StartMenuDirectory 'Bowser.lnk'
$desktopRoot = [Environment]::GetFolderPath('DesktopDirectory')
$legacyDesktopShortcut = Join-Path $desktopRoot 'Launch Bowser.lnk'
$canonicalDesktopShortcut = Join-Path $desktopRoot 'Bowser.lnk'
$legacyDebugShortcut = Join-Path (Join-Path $startMenuRoot 'Programs') 'Bowser Debug Launcher.lnk'
$uninstallScript = Join-Path (Join-Path $DataDirectory 'Tools') 'Uninstall-Bowser.ps1'
$uninstallKeyName = 'Bowser'
if ($isIsolatedRun) {
    $installPathHash = [Security.Cryptography.SHA256]::Create()
    try {
        $installTag = ([BitConverter]::ToString($installPathHash.ComputeHash([Text.Encoding]::UTF8.GetBytes($InstallDirectory))).Replace('-', '').Substring(0, 12).ToLowerInvariant())
    }
    finally {
        $installPathHash.Dispose()
    }
    $uninstallKeyName = "Bowser-Test-$installTag"
}
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$uninstallKeyName"
$uninstallRegKey = "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\$uninstallKeyName"

$guid = [guid]::NewGuid().ToString('N')
$stagingDirectory = Join-Path $programsDirectory ".Bowser-stage-$guid"
$stagedExe = Join-Path $stagingDirectory 'Bowser.exe'
$stagedUninstall = Join-Path $stagingDirectory 'Uninstall-Bowser.ps1'
$rollbackExe = Join-Path $InstallDirectory "Bowser.exe.rollback-$guid"
$logDirectory = Join-Path $DataDirectory 'Logs'
$logPath = Join-Path $logDirectory ("Update-Bowser-{0:yyyyMMdd-HHmmss}.log" -f (Get-Date))
$shortcutTransactions = [System.Collections.Generic.List[object]]::new()
$temporaryShortcuts = [System.Collections.Generic.List[string]]::new()
$oldExeMoved = $false
$newExeInstalled = $false
$registryWasBackedUp = $false
$registryExisted = Test-Path -LiteralPath $uninstallKey
$registryBackupPath = Join-Path $stagingDirectory 'uninstall-registry-backup.reg'
$transcriptStarted = $false

function Get-BowserBuildInputHash {
    param([Parameter(Mandatory = $true)][string]$Root)

    $files = @(
        (Join-Path $Root 'Cargo.toml'),
        (Join-Path $Root 'Cargo.lock'),
        (Join-Path $Root 'build.rs'),
        (Join-Path $Root 'bowser.manifest')
    )
    $files += @(Get-ChildItem -LiteralPath (Join-Path $Root 'src') -File -Recurse | ForEach-Object FullName)
    foreach ($relativePath in @('rust-toolchain', 'rust-toolchain.toml', '.cargo\config', '.cargo\config.toml')) {
        $path = Join-Path $Root $relativePath
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $files += $path
        }
    }

    $entries = foreach ($path in ($files | Sort-Object -Unique)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Required build input is missing: $path"
        }
        $relative = $path.Substring($Root.Length).TrimStart([char[]]@('\', '/'))
        $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        "$relative`0$hash"
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes(($entries -join "`n"))
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

function Test-BowserPeImage {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Staged executable is missing: $Path"
    }
    $stream = [IO.File]::OpenRead($Path)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($stream.Length -lt 512 -or $reader.ReadUInt16() -ne 0x5A4D) {
            throw 'The staged Bowser file is not a valid Windows executable.'
        }
        $stream.Position = 0x3C
        $peOffset = $reader.ReadInt32()
        if ($peOffset -lt 64 -or $peOffset -gt ($stream.Length - 26)) {
            throw 'The staged Bowser executable has an invalid PE header offset.'
        }
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550 -or $reader.ReadUInt16() -ne 0x8664) {
            throw 'The staged Bowser executable is not a valid x64 Windows PE image.'
        }
        $stream.Position = $peOffset + 24
        if ($reader.ReadUInt16() -ne 0x020B) {
            throw 'The staged Bowser executable does not have a 64-bit PE optional header.'
        }
        if ($stream.Length -lt 512KB) {
            throw 'The staged Bowser executable is unexpectedly small.'
        }
    }
    finally {
        $reader.Dispose()
        $stream.Dispose()
    }
}

function Get-InstalledBowserProcesses {
    $expected = [IO.Path]::GetFullPath($installedExe)
    @(Get-CimInstance Win32_Process -Filter "Name='Bowser.exe'" -ErrorAction Stop |
        Where-Object {
            -not [string]::IsNullOrWhiteSpace($_.ExecutablePath) -and
            [string]::Equals([IO.Path]::GetFullPath($_.ExecutablePath), $expected, [StringComparison]::OrdinalIgnoreCase)
        })
}

function Close-InstalledBowserGracefully {
    $processes = Get-InstalledBowserProcesses
    if ($processes.Count -eq 0) {
        Write-Host 'No Bowser process is using the installed executable.'
        return
    }

    Write-Host "Asking $($processes.Count) installed Bowser process(es) to close and save browser state..."
    foreach ($record in $processes) {
        $process = Get-Process -Id $record.ProcessId -ErrorAction Stop
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
            if (-not $process.CloseMainWindow()) {
                Write-Host "Bowser process $($record.ProcessId) did not accept a graceful close request."
            }
        }
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(25)
    do {
        Start-Sleep -Milliseconds 250
        $remaining = Get-InstalledBowserProcesses
    } while ($remaining.Count -gt 0 -and [DateTime]::UtcNow -lt $deadline)
    if ($remaining.Count -gt 0) {
        $ids = ($remaining | ForEach-Object ProcessId) -join ', '
        throw "Bowser did not close gracefully (process IDs: $ids). The installed files were left unchanged. Close Bowser and retry; no process was terminated."
    }
}

function New-BowserShortcutStage {
    param(
        [Parameter(Mandatory = $true)][string]$Destination,
        [Parameter(Mandatory = $true)][string]$Description
    )

    $parent = Split-Path -Parent $Destination
    $null = New-Item -ItemType Directory -Path $parent -Force
    $temporaryPath = "$Destination.stage-$guid.lnk"
    $temporaryShortcuts.Add($temporaryPath)
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($temporaryPath)
    $shortcut.TargetPath = $installedExe
    $shortcut.WorkingDirectory = $InstallDirectory
    $shortcut.Description = $Description
    $shortcut.IconLocation = "$installedExe,0"
    $shortcut.Save()

    $check = $shell.CreateShortcut($temporaryPath)
    if (-not [string]::Equals([IO.Path]::GetFullPath($check.TargetPath), $installedExe, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Shortcut validation failed: $temporaryPath"
    }
    return $temporaryPath
}

function Install-BowserShortcut {
    param([Parameter(Mandatory = $true)][string]$Destination, [Parameter(Mandatory = $true)][string]$TemporaryPath)

    $backupPath = $null
    if (Test-Path -LiteralPath $Destination) {
        $backupPath = "$Destination.rollback-$guid"
        Move-Item -LiteralPath $Destination -Destination $backupPath -ErrorAction Stop
    }
    $transaction = [pscustomobject]@{ Path = $Destination; Backup = $backupPath; Created = $false }
    $shortcutTransactions.Add($transaction)
    Move-Item -LiteralPath $TemporaryPath -Destination $Destination -ErrorAction Stop
    $transaction.Created = $true
}

function Back-Up-ShortcutForRemoval {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return }
    $backupPath = "$Path.rollback-$guid"
    Move-Item -LiteralPath $Path -Destination $backupPath -ErrorAction Stop
    $shortcutTransactions.Add([pscustomobject]@{ Path = $Path; Backup = $backupPath; Created = $false })
}

function Restore-DeploymentChanges {
    $transactionsToRestore = @($shortcutTransactions.ToArray())
    [array]::Reverse($transactionsToRestore)
    foreach ($transaction in $transactionsToRestore) {
        if ($transaction.Created -and (Test-Path -LiteralPath $transaction.Path -PathType Leaf)) {
            Remove-Item -LiteralPath $transaction.Path -Force -ErrorAction SilentlyContinue
        }
        if ($transaction.Backup -and (Test-Path -LiteralPath $transaction.Backup -PathType Leaf)) {
            Move-Item -LiteralPath $transaction.Backup -Destination $transaction.Path -Force -ErrorAction SilentlyContinue
        }
    }

    if ($registryWasBackedUp) {
        if ($registryExisted) {
            & "$env:SystemRoot\System32\reg.exe" import $registryBackupPath | Out-Null
        }
        else {
            & "$env:SystemRoot\System32\reg.exe" delete $uninstallRegKey /f | Out-Null
        }
    }
    if (Test-Path -LiteralPath $stagedUninstall -PathType Leaf) {
        Remove-Item -LiteralPath $stagedUninstall -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $uninstallScript -PathType Leaf) {
        if (Test-Path -LiteralPath (Join-Path $stagingDirectory 'Uninstall-Bowser.ps1.rollback')) {
            Remove-Item -LiteralPath $uninstallScript -Force -ErrorAction SilentlyContinue
            Move-Item -LiteralPath (Join-Path $stagingDirectory 'Uninstall-Bowser.ps1.rollback') -Destination $uninstallScript -Force -ErrorAction SilentlyContinue
        }
        elseif ($script:uninstallWasNew) {
            Remove-Item -LiteralPath $uninstallScript -Force -ErrorAction SilentlyContinue
        }
    }

    if ($newExeInstalled -and (Test-Path -LiteralPath $installedExe -PathType Leaf)) {
        Remove-Item -LiteralPath $installedExe -Force -ErrorAction SilentlyContinue
    }
    if ($oldExeMoved -and (Test-Path -LiteralPath $rollbackExe -PathType Leaf)) {
        Move-Item -LiteralPath $rollbackExe -Destination $installedExe -Force -ErrorAction SilentlyContinue
    }
}

try {
    $null = New-Item -ItemType Directory -Path $logDirectory -Force
    Start-Transcript -Path $logPath -Force | Out-Null
    $transcriptStarted = $true
    Write-Host "Bowser deployment started. Project: $repoRoot"
    Write-Host "Install directory: $InstallDirectory"
    Write-Host "User data directory is preserved at: $DataDirectory"

    $requiredInputs = @('Cargo.toml', 'Cargo.lock', 'build.rs', 'bowser.manifest', 'src')
    foreach ($input in $requiredInputs) {
        if (-not (Test-Path -LiteralPath (Join-Path $repoRoot $input))) {
            throw "The project is incomplete; required build input is missing: $input"
        }
    }
    $sourceHashBeforeBuild = Get-BowserBuildInputHash -Root $repoRoot
    $releaseExe = Join-Path $repoRoot 'target\release\bowser.exe'
    if ([string]::IsNullOrWhiteSpace($CargoPath)) {
        $cargo = Get-Command cargo -ErrorAction Stop
        $CargoPath = $cargo.Source
    }
    elseif (-not (Test-Path -LiteralPath $CargoPath -PathType Leaf)) {
        throw "Cargo executable was not found: $CargoPath"
    }

    $savedUpdateUrl = [Environment]::GetEnvironmentVariable('BOWSER_UPDATE_URL', 'Process')
    $savedUpdateKey = [Environment]::GetEnvironmentVariable('BOWSER_UPDATE_PUBKEY', 'Process')
    try {
        [Environment]::SetEnvironmentVariable('BOWSER_UPDATE_URL', $null, 'Process')
        [Environment]::SetEnvironmentVariable('BOWSER_UPDATE_PUBKEY', $null, 'Process')
        Write-Host 'Building the locked x64 release. The existing installation is not touched during this step.'
        Push-Location -LiteralPath $repoRoot
        try {
            & $CargoPath build --release --locked --bin bowser --features html-chrome-default
            $buildExitCode = $LASTEXITCODE
        }
        finally {
            Pop-Location
        }
        if ($buildExitCode -ne 0) {
            throw "Cargo release build failed with exit code $buildExitCode. The installed Bowser files were left untouched."
        }
    }
    finally {
        [Environment]::SetEnvironmentVariable('BOWSER_UPDATE_URL', $savedUpdateUrl, 'Process')
        [Environment]::SetEnvironmentVariable('BOWSER_UPDATE_PUBKEY', $savedUpdateKey, 'Process')
    }

    if ((Get-BowserBuildInputHash -Root $repoRoot) -ne $sourceHashBeforeBuild) {
        throw 'Bowser source changed during the release build. Nothing was staged or installed; run Update-Bowser.cmd again.'
    }
    Test-BowserPeImage -Path $releaseExe

    $null = New-Item -ItemType Directory -Path $programsDirectory -Force
    $null = New-Item -ItemType Directory -Path $stagingDirectory -ErrorAction Stop
    Copy-Item -LiteralPath $releaseExe -Destination $stagedExe -ErrorAction Stop
    Copy-Item -LiteralPath (Join-Path $repoRoot 'Uninstall-Bowser.ps1') -Destination $stagedUninstall -ErrorAction Stop
    Test-BowserPeImage -Path $stagedExe
    $stagedHash = (Get-FileHash -LiteralPath $stagedExe -Algorithm SHA256).Hash

    $startShortcutStage = New-BowserShortcutStage -Destination $startMenuShortcut -Description 'Launch Bowser browser'
    $desktopDestination = $null
    if (-not $isIsolatedRun) {
        if (-not [string]::IsNullOrWhiteSpace($DesktopShortcutPath)) {
            $desktopDestination = [IO.Path]::GetFullPath($DesktopShortcutPath)
        }
        elseif (Test-Path -LiteralPath $legacyDesktopShortcut -PathType Leaf) {
            $desktopDestination = $legacyDesktopShortcut
        }
        elseif ($DesktopShortcut) {
            $desktopDestination = $canonicalDesktopShortcut
        }
    }
    elseif (-not [string]::IsNullOrWhiteSpace($DesktopShortcutPath)) {
        $desktopDestination = [IO.Path]::GetFullPath($DesktopShortcutPath)
    }
    $desktopShortcutStage = $null
    if ($desktopDestination) {
        $desktopShortcutStage = New-BowserShortcutStage -Destination $desktopDestination -Description 'Launch Bowser browser'
    }

    Close-InstalledBowserGracefully

    $null = New-Item -ItemType Directory -Path $InstallDirectory -Force
    if (Test-Path -LiteralPath $installedExe -PathType Leaf) {
        Move-Item -LiteralPath $installedExe -Destination $rollbackExe -ErrorAction Stop
        $oldExeMoved = $true
    }
    Move-Item -LiteralPath $stagedExe -Destination $installedExe -ErrorAction Stop
    $newExeInstalled = $true
    Test-BowserPeImage -Path $installedExe
    if ((Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash -ne $stagedHash) {
        throw 'The installed executable does not match the validated staged build.'
    }

    Install-BowserShortcut -Destination $startMenuShortcut -TemporaryPath $startShortcutStage
    if ($desktopShortcutStage) {
        Install-BowserShortcut -Destination $desktopDestination -TemporaryPath $desktopShortcutStage
    }
    if (-not $isIsolatedRun -and (Test-Path -LiteralPath $legacyDebugShortcut -PathType Leaf)) {
        $legacyTarget = (New-Object -ComObject WScript.Shell).CreateShortcut($legacyDebugShortcut).TargetPath
        if ([string]::Equals([IO.Path]::GetFullPath($legacyTarget), (Join-Path $repoRoot 'Launch-Bowser-Debug.cmd'), [StringComparison]::OrdinalIgnoreCase)) {
            Back-Up-ShortcutForRemoval -Path $legacyDebugShortcut
        }
    }

    $null = New-Item -ItemType Directory -Path (Split-Path -Parent $uninstallScript) -Force
    if (Test-Path -LiteralPath $uninstallScript -PathType Leaf) {
        Copy-Item -LiteralPath $uninstallScript -Destination (Join-Path $stagingDirectory 'Uninstall-Bowser.ps1.rollback') -ErrorAction Stop
        $script:uninstallWasNew = $false
    }
    else {
        $script:uninstallWasNew = $true
    }
    Copy-Item -LiteralPath $stagedUninstall -Destination $uninstallScript -Force -ErrorAction Stop

    if ($registryExisted) {
        & "$env:SystemRoot\System32\reg.exe" export $uninstallRegKey $registryBackupPath /y | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Could not save the previous Bowser uninstall registration.' }
    }
    $registryWasBackedUp = $true
    $null = New-Item -Path $uninstallKey -Force
    $cargoToml = Get-Content -LiteralPath (Join-Path $repoRoot 'Cargo.toml') -Raw
    $versionMatch = [regex]::Match($cargoToml, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $versionMatch.Success) { throw 'Could not read Bowser version from Cargo.toml.' }
    $uninstallCommand = 'powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "{0}" -RegistryKeyName "{1}"' -f $uninstallScript, $uninstallKeyName
    New-ItemProperty -Path $uninstallKey -Name DisplayName -Value 'Bowser' -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $uninstallKey -Name DisplayVersion -Value $versionMatch.Groups[1].Value -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $uninstallKey -Name Publisher -Value 'Bowser' -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $uninstallKey -Name InstallLocation -Value $InstallDirectory -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $uninstallKey -Name DisplayIcon -Value $installedExe -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $uninstallKey -Name UninstallString -Value $uninstallCommand -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $uninstallKey -Name NoModify -Value 1 -PropertyType DWord -Force | Out-Null
    New-ItemProperty -Path $uninstallKey -Name NoRepair -Value 1 -PropertyType DWord -Force | Out-Null

    # Commit only after the executable, shortcuts and uninstall registration are valid.
    $oldExeMoved = $false
    $newExeInstalled = $false
    foreach ($transaction in $shortcutTransactions) {
        if ($transaction.Backup -and (Test-Path -LiteralPath $transaction.Backup -PathType Leaf)) {
            Remove-Item -LiteralPath $transaction.Backup -Force -ErrorAction SilentlyContinue
        }
    }
    if (Test-Path -LiteralPath $rollbackExe -PathType Leaf) {
        Remove-Item -LiteralPath $rollbackExe -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $registryBackupPath -PathType Leaf) {
        Remove-Item -LiteralPath $registryBackupPath -Force -ErrorAction SilentlyContinue
    }

    Write-Host "Bowser $($versionMatch.Groups[1].Value) installed successfully: $installedExe" -ForegroundColor Green
    Write-Host "Start Menu shortcut: $startMenuShortcut"
    if ($desktopDestination) { Write-Host "Desktop shortcut: $desktopDestination" }
    Write-Host "Browser data was preserved at: $DataDirectory"
    Write-Host "Diagnostic log: $logPath"

    $launchNow = $false
    if (-not $NoPrompt) {
        $answer = Read-Host 'Launch Bowser now? [Y/n]'
        $launchNow = [string]::IsNullOrWhiteSpace($answer) -or $answer -match '^(y|yes)$'
    }
    if ($launchNow) {
        $null = Start-Process -FilePath $installedExe -WorkingDirectory $InstallDirectory
    }
}
catch {
    Write-Host "Bowser deployment failed: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "Diagnostic log: $logPath" -ForegroundColor Yellow
    Restore-DeploymentChanges
    exit 1
}
finally {
    foreach ($temporaryPath in $temporaryShortcuts) {
        if (Test-Path -LiteralPath $temporaryPath -PathType Leaf) {
            Remove-Item -LiteralPath $temporaryPath -Force -ErrorAction SilentlyContinue
        }
    }
    if (Test-Path -LiteralPath $stagingDirectory -PathType Container) {
        Get-ChildItem -LiteralPath $stagingDirectory -Force | Remove-Item -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $stagingDirectory -Force -ErrorAction SilentlyContinue
    }
    if ($transcriptStarted) {
        Stop-Transcript | Out-Null
    }
}
