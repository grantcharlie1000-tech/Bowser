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

# True when $Candidate names the file $Expected. Never throws: Windows PowerShell's GetFullPath
# rejects shortcut targets such as 'https://...' or 'shell:AppsFolder\...', and one odd pinned
# shortcut must not abort an uninstall that has already removed files.
function Test-SameFilePath {
    param([string]$Candidate, [string]$Expected)
    if ([string]::IsNullOrWhiteSpace($Candidate) -or [string]::IsNullOrWhiteSpace($Expected)) { return $false }
    try {
        return [string]::Equals([IO.Path]::GetFullPath($Candidate), [IO.Path]::GetFullPath($Expected), [StringComparison]::OrdinalIgnoreCase)
    }
    catch {
        return $false
    }
}

# The program a shell\open\command value runs: the quoted first token, or the first word.
function Get-CommandProgram {
    param([string]$Command)
    if ([string]::IsNullOrWhiteSpace($Command)) { return $null }
    $text = $Command.Trim()
    if ($text.StartsWith('"')) {
        $end = $text.IndexOf('"', 1)
        if ($end -lt 1) { return $null }
        return $text.Substring(1, $end - 1)
    }
    return ($text -split '\s+', 2)[0]
}

# Removes the default-browser registration Bowser writes under HKCU (src/default_browser.rs),
# but only when it launches this installation's executable: a Dev build that registered itself
# keeps its own registration.
function Remove-BowserBrowserRegistration {
    param([string]$Executable)
    $client = 'HKCU:\Software\Clients\StartMenuInternet\Bowser'
    $progId = 'HKCU:\Software\Classes\Bowser.HTML'
    foreach ($key in @($client, $progId)) {
        $commandKey = Join-Path $key 'shell\open\command'
        if (-not (Test-Path -LiteralPath $commandKey)) { continue }
        $command = (Get-Item -LiteralPath $commandKey).GetValue('')
        if (-not (Test-SameFilePath -Candidate (Get-CommandProgram -Command $command) -Expected $Executable)) { continue }
        Remove-Item -LiteralPath $key -Recurse -Force
        if ($key -eq $client) {
            Remove-ItemProperty -LiteralPath 'HKCU:\Software\RegisteredApplications' -Name 'Bowser' -ErrorAction SilentlyContinue
        }
    }
}

function Get-InstalledBowserProcesses {
    $expected = [IO.Path]::GetFullPath($installedExe)
    @(Get-CimInstance Win32_Process -Filter "Name='Bowser.exe'" -ErrorAction Stop |
        Where-Object {
            Test-SameFilePath -Candidate $_.ExecutablePath -Expected $expected
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
    Remove-BowserBrowserRegistration -Executable $installedExe

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
        try {
            $target = $shell.CreateShortcut($shortcutPath).TargetPath
        }
        catch {
            Write-Host "Skipping a shortcut that could not be read: $shortcutPath"
            continue
        }
        if (Test-SameFilePath -Candidate $target -Expected $installedExe) {
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
