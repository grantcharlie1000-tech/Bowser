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

$localPin = Get-Content -LiteralPath $pinPath -Raw | ConvertFrom-Json
$pin = $localPin
$remotePin = 'https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/stable/release.json'
try {
    $remote = Invoke-WebRequest -Uri $remotePin -UseBasicParsing
    $remoteJson = $remote.Content | ConvertFrom-Json
    $localRevision = 0
    if ($localPin.revision) { $localRevision = [int]$localPin.revision }
    $remoteRevision = 0
    if ($remoteJson.revision) { $remoteRevision = [int]$remoteJson.revision }
    if ($remoteRevision -ge $localRevision) {
        $pin = $remoteJson
        Write-Host 'Using the stable release published on GitHub.'
    }
    else {
        Write-Host 'The GitHub pin is older than this folder. Using the local pin.'
    }
}
catch {
    Write-Host 'GitHub could not be reached. Using the release pin in this folder.'
}
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
if ($pin.repository -ne 'grantcharlie1000-tech/Bowser' -or $pin.asset -ne 'Bowser.exe' -or $pin.tag -notmatch '^v') {
    throw 'The stable release pin does not name this Bowser.'
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

function Get-StableInstallCandidates {
    param([Parameter(Mandatory = $true)][string]$Preferred)
    $preferredFull = [IO.Path]::GetFullPath($Preferred)
    $parent = Split-Path -Parent $preferredFull
    $names = New-Object System.Collections.Generic.List[string]
    [void]$names.Add($preferredFull)
    $stable = [IO.Path]::GetFullPath((Join-Path $parent 'Bowser Stable'))
    if (-not $names.Contains($stable)) { [void]$names.Add($stable) }
    for ($n = 2; $n -le 8; $n++) {
        [void]$names.Add([IO.Path]::GetFullPath((Join-Path $parent "Bowser Stable $n")))
    }
    return $names
}

function Test-BowserExeLocked {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $false }
    $full = [IO.Path]::GetFullPath($Path)
    foreach ($process in @(Get-Process -Name 'Bowser' -ErrorAction SilentlyContinue)) {
        $procPath = $null
        try { $procPath = $process.Path } catch { continue }
        if ([string]::IsNullOrWhiteSpace($procPath)) { continue }
        if ([string]::Equals([IO.Path]::GetFullPath($procPath), $full, [StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    try {
        $stream = [IO.File]::Open($full, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        $stream.Dispose()
        return $false
    }
    catch {
        return $true
    }
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

$candidates = @(Get-StableInstallCandidates -Preferred $InstallDirectory)
$ready = $false
foreach ($dir in $candidates) {
    $candidateExe = Join-Path $dir 'Bowser.exe'
    if (-not (Test-Path -LiteralPath $candidateExe -PathType Leaf)) { continue }
    if ((Get-LowerSha256 -Path $candidateExe) -eq $expectedHash) {
        $InstallDirectory = $dir
        $installedExe = $candidateExe
        Write-Host "Stable Bowser $($pin.version) is already installed."
        $ready = $true
        break
    }
}

if (-not $ready) {
    Write-Host 'The installed Bowser is not the pinned stable release. Replacing it.'
    $download = Join-Path ([IO.Path]::GetTempPath()) ('Bowser-' + [guid]::NewGuid().ToString('n') + '.download')
    try {
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
            Invoke-WebRequest -Uri $url -OutFile $download -UseBasicParsing
        }

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

        $destDir = $null
        foreach ($dir in $candidates) {
            $candidateExe = Join-Path $dir 'Bowser.exe'
            if (Test-BowserExeLocked -Path $candidateExe) {
                Write-Host "Leaving the open Bowser in place: $candidateExe"
                continue
            }
            $destDir = $dir
            break
        }
        if (-not $destDir) {
            throw 'Every stable Bowser folder is open. Close one Bowser window, then run Open-Bowser again. No process was terminated.'
        }
        $InstallDirectory = $destDir
        $installedExe = Join-Path $InstallDirectory 'Bowser.exe'
        New-Item -ItemType Directory -Path $InstallDirectory -Force | Out-Null
        $staged = Join-Path $InstallDirectory 'Bowser.exe.download'
        Copy-Item -LiteralPath $download -Destination $staged -Force
        if (Test-Path -LiteralPath $installedExe -PathType Leaf) {
            Remove-Item -LiteralPath $installedExe -Force
        }
        Move-Item -LiteralPath $staged -Destination $installedExe
        if ((Get-LowerSha256 -Path $installedExe) -ne $expectedHash) {
            throw 'The installed file did not match the pinned release after copying.'
        }
    }
    catch {
        throw
    }
    finally {
        Remove-Item -LiteralPath $download -Force -ErrorAction SilentlyContinue
        if ($installedExe) {
            Remove-Item -LiteralPath (Join-Path (Split-Path -Parent $installedExe) 'Bowser.exe.download') -Force -ErrorAction SilentlyContinue
        }
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
