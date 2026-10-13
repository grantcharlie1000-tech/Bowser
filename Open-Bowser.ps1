# Installs and opens the pinned Bowser release. This script never builds the source tree.
# The pin is stable/release.json. A matching copy in %LOCALAPPDATA%\Programs\Bowser is launched as-is.
[CmdletBinding()]
param(
    [string]$SourceExe,
    [string]$InstallDirectory,
    [switch]$NoLaunch,
    [switch]$NoShortcuts
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
}
catch {
}

$repoRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSCommandPath))
$pinPath = Join-Path $repoRoot 'stable\release.json'
if (-not (Test-Path -LiteralPath $pinPath -PathType Leaf)) {
    throw "Stable release pin is missing: $pinPath"
}

$pin = Get-Content -LiteralPath $pinPath -Raw | ConvertFrom-Json
foreach ($name in @('version', 'tag', 'repository', 'asset', 'sha256')) {
    if ([string]::IsNullOrWhiteSpace([string]$pin.$name)) {
        throw "stable/release.json is missing $name."
    }
}
$expectedHash = ([string]$pin.sha256).Trim().ToLowerInvariant()
if ($expectedHash -notmatch '^[0-9a-f]{64}$') {
    throw 'stable/release.json sha256 must be 64 hex characters.'
}
if ([string]$pin.repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') {
    throw 'stable/release.json repository must be owner/name.'
}
if ([string]$pin.tag -notmatch '^[A-Za-z0-9._-]+$' -or [string]$pin.asset -notmatch '^[A-Za-z0-9._-]+$') {
    throw 'stable/release.json tag and asset must be plain file names.'
}

$localAppData = [Environment]::GetFolderPath('LocalApplicationData')
if ([string]::IsNullOrWhiteSpace($localAppData)) {
    throw 'Windows did not provide LocalAppData.'
}
if ([string]::IsNullOrWhiteSpace($InstallDirectory)) {
    $InstallDirectory = Join-Path (Join-Path $localAppData 'Programs') 'Bowser'
}
$InstallDirectory = [IO.Path]::GetFullPath($InstallDirectory)
$installedExe = Join-Path $InstallDirectory 'Bowser.exe'

function Get-LowerSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Test-BowserExecutable {
    param([Parameter(Mandatory = $true)][string]$Path)
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    try {
        if ($stream.Length -lt 512) { throw "Bowser executable is too small: $Path" }
        $bytes = New-Object byte[] 2
        [void]$stream.Read($bytes, 0, 2)
        if ($bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) {
            throw "Bowser file is not a Windows executable: $Path"
        }
    }
    finally {
        $stream.Dispose()
    }
}

function Get-InstalledBowserProcesses {
    $found = @()
    foreach ($process in @(Get-Process -Name 'Bowser' -ErrorAction SilentlyContinue)) {
        $path = $null
        try { $path = $process.Path } catch { continue }
        if ([string]::IsNullOrWhiteSpace($path)) { continue }
        if ([string]::Equals([IO.Path]::GetFullPath($path), $installedExe, [StringComparison]::OrdinalIgnoreCase)) {
            $found += $process
        }
    }
    return $found
}

function Close-InstalledBowser {
    $processes = @(Get-InstalledBowserProcesses)
    if ($processes.Count -eq 0) { return $true }
    Write-Host "The previous Bowser is open. Asking it to close so the stable release can take its place..."
    foreach ($process in $processes) {
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
            [void]$process.CloseMainWindow()
        }
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(8)
    do {
        Start-Sleep -Milliseconds 250
        $processes = @(Get-InstalledBowserProcesses)
    } while ($processes.Count -gt 0 -and [DateTime]::UtcNow -lt $deadline)
    return ($processes.Count -eq 0)
}

function Install-StableShortcuts {
    if ($NoShortcuts) { return }
    $desktop = [Environment]::GetFolderPath('Desktop')
    $startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Bowser'
    $links = @(
        @{ Path = (Join-Path $desktop 'Bowser.lnk'); Description = 'Open Bowser' },
        @{ Path = (Join-Path $startMenu 'Bowser.lnk'); Description = 'Open Bowser' }
    )
    $shell = New-Object -ComObject WScript.Shell
    foreach ($link in $links) {
        $parent = Split-Path -Parent $link.Path
        if (-not (Test-Path -LiteralPath $parent)) {
            New-Item -ItemType Directory -Path $parent -Force | Out-Null
        }
        $shortcut = $shell.CreateShortcut($link.Path)
        $shortcut.TargetPath = $installedExe
        $shortcut.WorkingDirectory = $InstallDirectory
        $shortcut.WindowStyle = 1
        $shortcut.Description = $link.Description
        $shortcut.IconLocation = "$installedExe,0"
        $shortcut.Save()
        $check = $shell.CreateShortcut($link.Path)
        if (-not [string]::Equals([IO.Path]::GetFullPath($check.TargetPath), $installedExe, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Could not point the shortcut at the stable Bowser: $($link.Path)"
        }
        Write-Host "Shortcut: $($link.Path)"
    }
}

$ready = $false
if (Test-Path -LiteralPath $installedExe -PathType Leaf) {
    $installedHash = Get-LowerSha256 -Path $installedExe
    if ($installedHash -eq $expectedHash) {
        Write-Host "Stable Bowser $($pin.version) is already installed."
        $ready = $true
    }
    else {
        Write-Host 'The installed Bowser is not the pinned stable release. Replacing it.'
    }
}

if (-not $ready) {
    $download = Join-Path $InstallDirectory 'Bowser.exe.download'
    New-Item -ItemType Directory -Path $InstallDirectory -Force | Out-Null
    if (-not [string]::IsNullOrWhiteSpace($SourceExe)) {
        if (-not (Test-Path -LiteralPath $SourceExe -PathType Leaf)) {
            throw "Source executable was not found: $SourceExe"
        }
        Copy-Item -LiteralPath $SourceExe -Destination $download -Force
        Write-Host "Using local build: $SourceExe"
    }
    else {
        $url = "https://github.com/$($pin.repository)/releases/download/$($pin.tag)/$($pin.asset)"
        Write-Host "Downloading stable Bowser $($pin.version)"
        Write-Host $url
        try {
            Invoke-WebRequest -Uri $url -OutFile $download -UseBasicParsing
        }
        catch {
            Remove-Item -LiteralPath $download -Force -ErrorAction SilentlyContinue
            throw "Could not download the stable Bowser release. $_"
        }
    }

    try {
        Test-BowserExecutable -Path $download
        $downloadHash = Get-LowerSha256 -Path $download
        if ($downloadHash -ne $expectedHash) {
            throw "The downloaded Bowser does not match the pinned release. Expected $expectedHash, got $downloadHash."
        }
        if ($pin.bytes) {
            $size = (Get-Item -LiteralPath $download).Length
            if ([int64]$pin.bytes -ne $size) {
                throw "The downloaded Bowser size is $size bytes; the pin expects $($pin.bytes)."
            }
        }
        $closed = Close-InstalledBowser
        if ($closed) {
            Move-Item -LiteralPath $download -Destination $installedExe -Force
        }
        else {
            # The open Bowser still has the usual path loaded. Keep that process, and
            # put the pinned release in a sibling folder the shortcut can open later.
            $fallbackDirectory = Join-Path (Split-Path -Parent $InstallDirectory) 'Bowser Stable'
            $InstallDirectory = [IO.Path]::GetFullPath($fallbackDirectory)
            $installedExe = Join-Path $InstallDirectory 'Bowser.exe'
            New-Item -ItemType Directory -Path $InstallDirectory -Force | Out-Null
            $fallbackDownload = Join-Path $InstallDirectory 'Bowser.exe.download'
            Move-Item -LiteralPath $download -Destination $fallbackDownload -Force
            $download = $fallbackDownload
            Move-Item -LiteralPath $download -Destination $installedExe -Force
            Write-Host 'The previous Bowser is still open, so its file was left in place. No process was terminated.'
        }
        if ((Get-LowerSha256 -Path $installedExe) -ne $expectedHash) {
            throw 'The installed file did not match the pinned release after copying.'
        }
    }
    catch {
        Remove-Item -LiteralPath $download -Force -ErrorAction SilentlyContinue
        throw
    }

    $note = @(
        "version=$($pin.version)"
        "tag=$($pin.tag)"
        "commit=$($pin.commit)"
        "sha256=$expectedHash"
    ) -join "`r`n"
    [IO.File]::WriteAllText((Join-Path $InstallDirectory 'STABLE.txt'), $note + "`r`n")
    Write-Host "Installed stable Bowser $($pin.version): $installedExe"
}

Install-StableShortcuts

if ($NoLaunch) {
    Write-Host "Stable Bowser is ready: $installedExe"
    exit 0
}

$previous = @(Get-Process -Name 'Bowser' -ErrorAction SilentlyContinue | Where-Object {
    try {
        -not [string]::Equals([IO.Path]::GetFullPath($_.Path), $installedExe, [StringComparison]::OrdinalIgnoreCase)
    }
    catch { $false }
})
if ($previous.Count -gt 0) {
    Write-Host "Stable Bowser $($pin.version) is installed: $installedExe"
    Write-Host 'Close the Bowser window that is already open, then use the Bowser shortcut. That shortcut opens this release.'
    exit 0
}

$process = Start-Process -FilePath $installedExe -WorkingDirectory $InstallDirectory -PassThru
Write-Host "Opened Bowser $($pin.version) (PID $($process.Id))."
