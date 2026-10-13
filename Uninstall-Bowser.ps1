[CmdletBinding()]
param(
    [string]$InstallDirectory,
    [string]$DataDirectory,
    [string]$StartMenuDirectory,
    [string]$DesktopShortcutPath,
    [string]$RegistryKeyName = 'Bowser'
)

$ErrorActionPreference = 'Stop'
$localAppData = [Environment]::GetFolderPath('LocalApplicationData')
if ([string]::IsNullOrWhiteSpace($localAppData)) {
    throw 'Windows did not provide LocalAppData; Bowser cannot be uninstalled safely.'
}
$programsDirectory = Join-Path $localAppData 'Programs'
$expectedInstall = [IO.Path]::GetFullPath((Join-Path $programsDirectory 'Bowser'))
if ([string]::IsNullOrWhiteSpace($InstallDirectory)) { $InstallDirectory = $expectedInstall }
$InstallDirectory = [IO.Path]::GetFullPath($InstallDirectory)
if (-not $PSBoundParameters.ContainsKey('InstallDirectory') -and
    -not [string]::Equals($InstallDirectory, $expectedInstall, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Refusing to remove a path outside the per-user Bowser installation directory.'
}
if ([string]::IsNullOrWhiteSpace($DataDirectory)) { $DataDirectory = Join-Path $localAppData 'Bowser' }
$DataDirectory = [IO.Path]::GetFullPath($DataDirectory)
$startMenuRoot = [Environment]::GetFolderPath('StartMenu')
if ([string]::IsNullOrWhiteSpace($StartMenuDirectory)) {
    $StartMenuDirectory = Join-Path (Join-Path $startMenuRoot 'Programs') 'Bowser'
}
$StartMenuDirectory = [IO.Path]::GetFullPath($StartMenuDirectory)
$installedExe = Join-Path $InstallDirectory 'Bowser.exe'
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$RegistryKeyName"
$userDataKept = $true

function Get-InstalledBowserProcesses {
    $expected = [IO.Path]::GetFullPath($installedExe)
    @(Get-CimInstance Win32_Process -Filter "Name='Bowser.exe'" -ErrorAction Stop |
        Where-Object {
            -not [string]::IsNullOrWhiteSpace($_.ExecutablePath) -and
            [string]::Equals([IO.Path]::GetFullPath($_.ExecutablePath), $expected, [StringComparison]::OrdinalIgnoreCase)
        })
}

try {
    Write-Host 'Uninstalling Bowser. Browser profiles, sessions, settings, and passwords will be kept.'
    $processes = Get-InstalledBowserProcesses
    foreach ($record in $processes) {
        $process = Get-Process -Id $record.ProcessId -ErrorAction Stop
        if ($process.MainWindowHandle -ne [IntPtr]::Zero -and -not $process.CloseMainWindow()) {
            Write-Host "Bowser process $($record.ProcessId) did not accept a close request."
        }
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(25)
    do {
        Start-Sleep -Milliseconds 250
        $remaining = Get-InstalledBowserProcesses
    } while ($remaining.Count -gt 0 -and [DateTime]::UtcNow -lt $deadline)
    if ($remaining.Count -gt 0) {
        throw 'Bowser did not close gracefully. Close the application and run the uninstaller again; no process was terminated.'
    }

    if (Test-Path -LiteralPath $InstallDirectory -PathType Container) {
        $item = Get-Item -LiteralPath $InstallDirectory -Force
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'Refusing to remove an installation directory that is a link or reparse point.'
        }
        Remove-Item -LiteralPath $InstallDirectory -Recurse -Force
    }

    $shell = New-Object -ComObject WScript.Shell
    $shortcutCandidates = @(
        (Join-Path $StartMenuDirectory 'Bowser.lnk'),
        (Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) 'Launch Bowser.lnk'),
        (Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) 'Bowser.lnk')
    )
    if (-not [string]::IsNullOrWhiteSpace($DesktopShortcutPath)) {
        $shortcutCandidates += [IO.Path]::GetFullPath($DesktopShortcutPath)
    }
    $shortcutCandidates += Get-ChildItem -LiteralPath (Join-Path $env:APPDATA 'Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar') -Filter '*.lnk' -ErrorAction SilentlyContinue |
        ForEach-Object FullName
    foreach ($shortcutPath in ($shortcutCandidates | Sort-Object -Unique)) {
        if (-not (Test-Path -LiteralPath $shortcutPath -PathType Leaf)) { continue }
        $target = $shell.CreateShortcut($shortcutPath).TargetPath
        if ([string]::IsNullOrWhiteSpace($target)) { continue }
        if ([string]::Equals([IO.Path]::GetFullPath($target), $installedExe, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $shortcutPath -Force
        }
    }
    if ((Test-Path -LiteralPath $StartMenuDirectory -PathType Container) -and
        @(Get-ChildItem -LiteralPath $StartMenuDirectory -Force).Count -eq 0) {
        Remove-Item -LiteralPath $StartMenuDirectory -Force
    }
    if (Test-Path -LiteralPath $uninstallKey) {
        Remove-Item -LiteralPath $uninstallKey -Recurse -Force
    }

    Write-Host "Bowser was removed. Your browser data remains at: $DataDirectory" -ForegroundColor Green
    Write-Host 'To remove profiles, sessions, settings, and saved passwords, delete that folder yourself after confirming you no longer need it.'
}
catch {
    Write-Host "Bowser uninstall failed: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
