# Installs and opens the pinned Bowser release. This script never builds the source tree.
# The pin is stable/release.json. A matching copy in %LOCALAPPDATA%\Programs\Bowser is launched as-is.
[CmdletBinding()]
param(
    [string]$SourceExe,
    [string]$InstallDirectory,
    [switch]$NoLaunch,
    [switch]$NoShortcuts,
    [string]$MinimumVersion = '0.0.0',
    [uint32]$MinimumRevision = 0
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
}
catch {
}

# Every HTTP response is bounded while streaming. Automatic redirects are disabled so an HTTPS
# origin cannot redirect the installer to HTTP, a local file, credentials, or an unbounded body.
function Assert-PlainHttpsUri {
    param([Parameter(Mandatory = $true)][Uri]$Uri)
    if (-not $Uri.IsAbsoluteUri -or $Uri.Scheme -ne 'https' -or
        [string]::IsNullOrWhiteSpace($Uri.Host) -or $Uri.UserInfo -or $Uri.Fragment) {
        throw 'The download URL must be a plain HTTPS URL.'
    }
}

function Resolve-BowserRedirect {
    param([Parameter(Mandatory = $true)][Uri]$Current, [Parameter(Mandatory = $true)][string]$Location, [ValidateRange(0, 5)][int]$Hop)
    if ($Hop -eq 5) { throw 'The download exceeded five redirects.' }
    if ([string]::IsNullOrWhiteSpace($Location) -or $Location.Length -gt 8192) { throw 'The download redirect has no usable Location.' }
    $next = [Uri]::new($Current, $Location)
    Assert-PlainHttpsUri -Uri $next
    return $next
}

function Copy-BoundedStream {
    param([Parameter(Mandatory = $true)][IO.Stream]$Source, [Parameter(Mandatory = $true)][IO.Stream]$Destination,
        [Parameter(Mandatory = $true)][ValidateRange(1, 268435456)][long]$Limit)
    $buffer = New-Object byte[] 32768
    [long]$total = 0
    while (($count = $Source.Read($buffer, 0, $buffer.Length)) -gt 0) {
        $total += $count
        if ($total -gt $Limit) { throw 'The download exceeds its size limit.' }
        $Destination.Write($buffer, 0, $count)
    }
    return $total
}

function Invoke-BoundedHttpsGet {
    param(
        [Parameter(Mandatory = $true)][Uri]$Uri,
        [Parameter(Mandatory = $true)][ValidateRange(1, 268435456)][long]$Limit,
        [string]$Destination
    )
    $current = $Uri
    for ($hop = 0; $hop -le 5; $hop++) {
        Assert-PlainHttpsUri -Uri $current
        $request = [Net.HttpWebRequest]::Create($current)
        $request.AllowAutoRedirect = $false
        $request.Timeout = 15000
        $request.ReadWriteTimeout = 30000
        $request.UserAgent = 'Bowser-Installer/1'
        $response = $null
        try {
            try { $response = [Net.HttpWebResponse]$request.GetResponse() }
            catch [Net.WebException] {
                if (-not $_.Exception.Response) { throw }
                $response = [Net.HttpWebResponse]$_.Exception.Response
            }
            $status = [int]$response.StatusCode
            if ($status -in @(301, 302, 303, 307, 308)) {
                $current = Resolve-BowserRedirect -Current $current -Location ([string]$response.Headers['Location']) -Hop $hop
                continue
            }
            if ($status -ne 200) { throw "The download returned HTTP $status." }
            if ($response.ContentLength -gt $Limit) { throw 'The download exceeds its size limit.' }
            $responseStream = $response.GetResponseStream()
            $output = $null
            try {
                if ([string]::IsNullOrWhiteSpace($Destination)) {
                    $output = New-Object IO.MemoryStream
                }
                else {
                    # Never truncate an existing file, including an attacker-created link.
                    $output = [IO.File]::Open($Destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                }
                $total = Copy-BoundedStream -Source $responseStream -Destination $output -Limit $Limit
                $output.Flush()
                if ([string]::IsNullOrWhiteSpace($Destination)) { return ,$output.ToArray() }
                return $total
            }
            finally {
                if ($output) { $output.Dispose() }
                if ($responseStream) { $responseStream.Dispose() }
            }
        }
        finally {
            if ($response) { $response.Close() }
            $request.Abort()
        }
    }
    throw 'The download exceeded five redirects.'
}

function ConvertTo-BowserVersion {
    param([Parameter(Mandatory = $true)][string]$Text)
    if ($Text -cnotmatch '^[0-9]{1,9}\.[0-9]{1,9}\.[0-9]{1,9}$') {
        throw 'The Bowser version must be major.minor.patch.'
    }
    return [version]$Text
}

function ConvertFrom-BowserPin {
    param([Parameter(Mandatory = $true)][byte[]]$Bytes)
    if ($Bytes.Length -eq 0 -or $Bytes.Length -gt 16384) { throw 'The stable release pin exceeds 16 KiB or is empty.' }
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    $text = $utf8.GetString($Bytes)
    if ($text.Contains([string][char]0)) { throw 'The stable release pin contains a NUL.' }
    $value = $text | ConvertFrom-Json
    foreach ($name in @('version', 'tag', 'repository', 'asset', 'sha256', 'bytes', 'revision')) {
        if ([string]::IsNullOrWhiteSpace([string]$value.$name)) { throw "stable/release.json is missing $name." }
    }
    [void](ConvertTo-BowserVersion -Text ([string]$value.version))
    if ([string]$value.repository -cne 'grantcharlie1000-tech/Bowser' -or [string]$value.asset -cne 'Bowser.exe' -or
        [string]$value.tag -cnotmatch '^v[A-Za-z0-9._-]{0,63}$') {
        throw 'The stable release pin does not name this Bowser.'
    }
    if (([string]$value.sha256).Trim() -notmatch '^[0-9a-fA-F]{64}$') { throw 'The stable release pin sha256 must be 64 hex characters.' }
    if ([string]$value.bytes -cnotmatch '^[0-9]{1,12}$' -or [long]$value.bytes -lt 1 -or [long]$value.bytes -gt 268435456) {
        throw 'The stable release pin bytes must be between 1 and 256 MiB.'
    }
    if ([string]$value.revision -cnotmatch '^[0-9]{1,10}$' -or [long]$value.revision -gt [uint32]::MaxValue) {
        throw 'The stable release pin revision must be an unsigned 32-bit integer.'
    }
    return $value
}

function Assert-BowserPinMinimum {
    param([Parameter(Mandatory = $true)]$Pin, [string]$Version = '0.0.0', [uint32]$Revision = 0)
    $minimum = ConvertTo-BowserVersion -Text $Version
    if ((ConvertTo-BowserVersion -Text ([string]$Pin.version)) -lt $minimum -or [uint32]$Pin.revision -lt $Revision) {
        throw "The published stable release is older than this Bowser. Installation was not performed; try again after a newer stable release is published (requires version $Version, pin revision $Revision)."
    }
}

# Lexical allowlisting is not enough: junctions and symbolic links may redirect writes elsewhere.
# Recheck before each install write. This is not a privilege boundary against same-user races.
function Assert-NoReparsePath {
    param([Parameter(Mandatory = $true)][string]$Path)
    $cursor = [IO.Path]::GetFullPath($Path)
    while (-not [string]::IsNullOrWhiteSpace($cursor)) {
        if (Test-Path -LiteralPath $cursor) {
            $entry = Get-Item -LiteralPath $cursor -Force
            if (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "The install path contains a reparse point: $cursor"
            }
        }
        else {
            # Get-Item sees a dangling link on Windows even when Test-Path reports false.
            $entry = Get-Item -LiteralPath $cursor -Force -ErrorAction SilentlyContinue
            if ($entry -and ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "The install path contains a reparse point: $cursor"
            }
        }
        $parent = Split-Path -Parent $cursor
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
}

function Test-BowserSignature {
    param([Parameter(Mandatory = $true)][string]$Path)
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -notin @('Valid', 'NotSigned')) {
        throw "The Bowser executable has an invalid or untrusted signature: $($signature.Status)."
    }
}

# This record identifies installer-owned bytes, not an authentication credential. Official new
# bytes are trusted only after the fixed GitHub pin's SHA/size/signature checks below.
function Get-ManagedInstallInfo {
    param([Parameter(Mandatory = $true)][string]$Directory)
    $exe = Join-Path $Directory 'Bowser.exe'
    $note = Join-Path $Directory 'STABLE.txt'
    Assert-NoReparsePath -Path $exe
    Assert-NoReparsePath -Path $note
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf) -or -not (Test-Path -LiteralPath $note -PathType Leaf)) { return $null }
    if ((Get-Item -LiteralPath $exe).Length -gt 268435456 -or (Get-Item -LiteralPath $note).Length -gt 4096) { return $null }
    $record = [IO.File]::ReadAllText($note)
    $sha = [regex]::Match($record, '(?m)^sha256=([0-9a-fA-F]{64})\r?$')
    $versionText = [regex]::Match($record, '(?m)^version=([0-9]{1,9}\.[0-9]{1,9}\.[0-9]{1,9})\r?$')
    $revisionText = [regex]::Match($record, '(?m)^revision=([0-9]{1,10})\r?$')
    if (-not $sha.Success -or -not $versionText.Success) { return $null }
    if ($record -match '(?m)^revision=' -and -not $revisionText.Success) { return $null }
    [uint32]$revision = 0 # Legacy installer notes did not record revision.
    if ($revisionText.Success) {
        if ([long]$revisionText.Groups[1].Value -gt [uint32]::MaxValue) { return $null }
        $revision = [uint32]$revisionText.Groups[1].Value
    }
    $hash = Get-LowerSha256 -Path $exe
    if ($hash -ne $sha.Groups[1].Value.ToLowerInvariant()) { return $null }
    return [pscustomobject]@{ Version = (ConvertTo-BowserVersion -Text $versionText.Groups[1].Value); Revision = $revision; Sha256 = $hash }
}

function Get-ManagedInstallHash {
    param([Parameter(Mandatory = $true)][string]$Directory)
    $info = Get-ManagedInstallInfo -Directory $Directory
    if ($info) { return $info.Sha256 }
    return $null
}

function Assert-PinNotOlderThanManagedInstalls {
    param([Parameter(Mandatory = $true)]$Pin, [Parameter(Mandatory = $true)][string[]]$Directories)
    $version = ConvertTo-BowserVersion -Text ([string]$Pin.version)
    foreach ($directory in $Directories) {
        $note = Join-Path $directory 'STABLE.txt'
        Assert-NoReparsePath -Path $note
        if (-not (Test-Path -LiteralPath $note)) { continue }
        $info = Get-ManagedInstallInfo -Directory $directory
        if (-not $info) {
            # The native updater may legitimately leave an older STABLE.txt behind. Only a
            # match to the freshly verified selected pin resolves that ambiguity; otherwise
            # do not redirect shortcuts to an older fallback or overwrite unknown bytes.
            $exe = Join-Path $directory 'Bowser.exe'
            Assert-NoReparsePath -Path $exe
            if ((Test-Path -LiteralPath $exe -PathType Leaf) -and
                (Get-Item -LiteralPath $exe).Length -eq [long]$Pin.bytes -and
                (Get-LowerSha256 -Path $exe) -eq ([string]$Pin.sha256).Trim().ToLowerInvariant()) { continue }
            throw 'An existing managed installation changed since its record. No file or shortcut was changed. Open its existing shortcut and use its native updater, or choose an empty install directory; do not delete the existing files.'
        }
        if ($version -lt $info.Version -or [uint32]$Pin.revision -lt $info.Revision) {
            throw [IO.InvalidDataException]::new('An already installed managed Bowser is newer than the published pin. No file or shortcut was changed.')
        }
    }
}

# Serialize setup across Bowser processes and sessions, not just one Rust worker. Global\ plus
# the current SID names one per-user mutex; no caller-provided text controls its name.
function Get-BowserInstallMutexName {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        if (-not $identity.User) { throw 'Windows did not provide a user SID for the install lock.' }
        return 'Global\BowserStableInstall-' + $identity.User.Value
    }
    finally { $identity.Dispose() }
}

function Enter-BowserInstallMutex {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Name)
    $mutex = [Threading.Mutex]::new($false, $Name)
    try {
        $acquired = $false
        try { $acquired = $mutex.WaitOne(0) }
        catch [Threading.AbandonedMutexException] { $acquired = $true }
        if (-not $acquired) {
            throw 'Another Bowser installation is already running. Wait for it to finish, then try Install Bowser again.'
        }
        return $mutex
    }
    catch {
        $mutex.Dispose()
        throw
    }
}

function Exit-BowserInstallMutex {
    param([Parameter(Mandatory = $true)][Threading.Mutex]$Mutex)
    try { $Mutex.ReleaseMutex() }
    finally { $Mutex.Dispose() }
}

$repoRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSCommandPath))
$pinPath = Join-Path $repoRoot 'stable\release.json'
if (-not (Test-Path -LiteralPath $pinPath -PathType Leaf)) { throw "Stable release pin is missing: $pinPath" }
if ((Get-Item -LiteralPath $pinPath).Length -gt 16384) { throw 'The local stable release pin exceeds 16 KiB.' }
$localPin = ConvertFrom-BowserPin -Bytes ([IO.File]::ReadAllBytes($pinPath))
$pin = $localPin
$remotePin = 'https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/stable/release.json'
try {
    $remoteBytes = Invoke-BoundedHttpsGet -Uri ([Uri]$remotePin) -Limit 16384
    $remote = ConvertFrom-BowserPin -Bytes $remoteBytes
    if ([uint32]$remote.revision -ge [uint32]$localPin.revision -and
        (ConvertTo-BowserVersion -Text ([string]$remote.version)) -ge (ConvertTo-BowserVersion -Text ([string]$localPin.version))) {
        $pin = $remote
        Write-Host 'Using the stable release published on GitHub.'
    }
    else { Write-Host 'The GitHub pin is older than this folder. Using the local pin.' }
}
catch { Write-Host 'GitHub could not provide a usable pin. Using the release pin in this folder.' }
# Enforce after selection, including offline fallback. The native Install action supplies its
# compiled-in version/revision here; a stale local/remote pin can never silently downgrade it.
try { Assert-BowserPinMinimum -Pin $pin -Version $MinimumVersion -Revision $MinimumRevision }
catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 23 # The native Install action reports a stale release separately from an install failure.
}
$expectedHash = ([string]$pin.sha256).Trim().ToLowerInvariant()
$expectedBytes = [long]$pin.bytes

$localAppData = [Environment]::GetFolderPath('LocalApplicationData')
if ([string]::IsNullOrWhiteSpace($localAppData)) {
    throw 'Windows did not provide LocalAppData.'
}
if ([string]::IsNullOrWhiteSpace($InstallDirectory)) {
    $InstallDirectory = Join-Path (Join-Path $localAppData 'Programs') 'Bowser'
}
Assert-NoReparsePath -Path $localAppData
Assert-NoReparsePath -Path (Join-Path $localAppData 'Programs')
$InstallDirectory = [IO.Path]::GetFullPath($InstallDirectory)
Assert-NoReparsePath -Path $InstallDirectory
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

# True when $Candidate names the file $Expected. Never throws: Windows PowerShell's GetFullPath
# rejects an empty string and targets such as 'https://...' or 'shell:AppsFolder\...'.
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

# Puts a staged file in place of $Destination without a moment where neither exists: the old
# file is swapped out by one rename (ReplaceFile), and survives as a backup if the swap fails.
function Install-StagedFile {
    param([Parameter(Mandatory = $true)][string]$Staged, [Parameter(Mandatory = $true)][string]$Destination, [string]$PreviousHash)
    Assert-NoReparsePath -Path $Staged
    Assert-NoReparsePath -Path $Destination
    if (-not (Test-Path -LiteralPath $Destination -PathType Leaf)) {
        # No -Force: a concurrently created destination is never overwritten.
        [IO.File]::Move($Staged, $Destination)
        return
    }
    if ([string]::IsNullOrWhiteSpace($PreviousHash) -or (Get-LowerSha256 -Path $Destination) -ne $PreviousHash) {
        throw 'The previous installed file changed or is not a managed Bowser. It was not replaced.'
    }
    $backup = "$Destination.previous-" + [guid]::NewGuid().ToString('n')
    [IO.File]::Replace($Staged, $Destination, $backup)
    Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue
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

# The caller supplies only Get-Process -Name 'Bowser*' results. Do not read executable paths:
# Downloads copies share the real profile, and access-denied/unknown paths are still blockers.
function Test-BowserLaunchBlocked {
    param([AllowNull()][object[]]$Processes = @())
    foreach ($process in $Processes) {
        if ($null -ne $process) { return $true }
    }
    return $false
}

function Set-BowserShortcut {
    param([Parameter(Mandatory = $true)]$Shortcut, [string]$Target, [string]$WorkingDirectory, [string]$Description)
    $Shortcut.TargetPath = $Target
    $Shortcut.Arguments = '' # An old --dev/benchmark/profile argument must not survive recovery.
    $Shortcut.WorkingDirectory = $WorkingDirectory
    $Shortcut.WindowStyle = 1
    $Shortcut.Description = $Description
    $Shortcut.IconLocation = "$Target,0"
    $Shortcut.Save()
}

function Assert-BowserShortcut {
    param([Parameter(Mandatory = $true)]$Shortcut, [string]$Target)
    if (-not (Test-SameFilePath -Candidate $Shortcut.TargetPath -Expected $Target) -or
        -not [string]::IsNullOrEmpty([string]$Shortcut.Arguments)) {
        throw 'The shortcut does not open the installed Bowser with empty arguments.'
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
        Assert-NoReparsePath -Path $link.Path
        Assert-NoReparsePath -Path $parent
        if (-not (Test-Path -LiteralPath $parent)) {
            New-Item -ItemType Directory -Path $parent -Force | Out-Null
        }
        Assert-NoReparsePath -Path $link.Path
        $shortcut = $shell.CreateShortcut($link.Path)
        Set-BowserShortcut -Shortcut $shortcut -Target $installedExe -WorkingDirectory $InstallDirectory -Description $link.Description
        $check = $shell.CreateShortcut($link.Path)
        Assert-BowserShortcut -Shortcut $check -Target $installedExe
        Write-Host "Shortcut: $($link.Path)"
    }
}

# Fetching the pin may happen concurrently; every destination preflight MUST happen after this
# lock, so an older process observes a newer install receipt instead of replacing its build.
$installMutex = Enter-BowserInstallMutex -Name (Get-BowserInstallMutexName)
try {
    $candidates = @(Get-StableInstallCandidates -Preferred $InstallDirectory)
    try { Assert-PinNotOlderThanManagedInstalls -Pin $pin -Directories $candidates }
    catch [IO.InvalidDataException] {
        [Console]::Error.WriteLine($_.Exception.Message)
        exit 23 # Do not replace a newer installed copy or repoint its shortcuts to an older release.
    }
    $ready = $false
    foreach ($dir in $candidates) {
        Assert-NoReparsePath -Path $dir
        $candidateExe = Join-Path $dir 'Bowser.exe'
        Assert-NoReparsePath -Path $candidateExe
        if (-not (Test-Path -LiteralPath $candidateExe -PathType Leaf)) { continue }
        if ((Get-Item -LiteralPath $candidateExe).Length -eq $expectedBytes -and (Get-LowerSha256 -Path $candidateExe) -eq $expectedHash) {
            Test-BowserExecutable -Path $candidateExe
            Test-BowserSignature -Path $candidateExe
            $InstallDirectory = $dir
            $installedExe = $candidateExe
            Write-Host "Stable Bowser $($pin.version) is already installed."
            $ready = $true
            break
        }
    }

    if (-not $ready) {
        Write-Host 'Installing the pinned stable Bowser without replacing unknown files.'
        $download = Join-Path ([IO.Path]::GetTempPath()) ('Bowser-' + [guid]::NewGuid().ToString('n') + '.download.exe')
        $staged = $null
        try {
            if (-not [string]::IsNullOrWhiteSpace($SourceExe)) {
                if (-not (Test-Path -LiteralPath $SourceExe -PathType Leaf)) {
                    throw "Source executable was not found: $SourceExe"
                }
                Assert-NoReparsePath -Path $SourceExe
                if ((Get-Item -LiteralPath $SourceExe).Length -ne $expectedBytes) { throw 'The source executable has the wrong pinned size.' }
                [IO.File]::Copy([IO.Path]::GetFullPath($SourceExe), $download, $false)
                Write-Host "Using local build: $SourceExe"
            }
            else {
                $url = "https://github.com/$($pin.repository)/releases/download/$($pin.tag)/$($pin.asset)"
                Write-Host "Downloading stable Bowser $($pin.version)"
                Write-Host $url
                [void](Invoke-BoundedHttpsGet -Uri ([Uri]$url) -Limit $expectedBytes -Destination $download)
            }

            Test-BowserExecutable -Path $download
            $downloadHash = Get-LowerSha256 -Path $download
            if ($downloadHash -ne $expectedHash) {
                throw "The downloaded Bowser does not match the pinned release. Expected $expectedHash, got $downloadHash."
            }
            $size = (Get-Item -LiteralPath $download).Length
            if ($size -ne $expectedBytes) { throw "The downloaded Bowser size is $size bytes; the pin expects $expectedBytes." }
            Test-BowserSignature -Path $download

            $destDir = $null
            $previousHash = $null
            foreach ($dir in $candidates) {
                Assert-NoReparsePath -Path $dir
                $candidateExe = Join-Path $dir 'Bowser.exe'
                Assert-NoReparsePath -Path $candidateExe
                $managedHash = $null
                if (Test-Path -LiteralPath $candidateExe) {
                    $managedHash = Get-ManagedInstallHash -Directory $dir
                    if (-not $managedHash) {
                        Write-Host "Leaving an unrecognised existing file in place: $candidateExe"
                        continue
                    }
                }
                elseif ((Test-Path -LiteralPath $dir) -and @(Get-ChildItem -LiteralPath $dir -Force).Count -gt 0) {
                    Write-Host "Leaving a non-empty unrelated folder in place: $dir"
                    continue
                }
                if (Test-BowserExeLocked -Path $candidateExe) {
                    Write-Host "Leaving the open Bowser in place: $candidateExe"
                    continue
                }
                $destDir = $dir
                $previousHash = $managedHash
                break
            }
            if (-not $destDir) {
                throw 'No safe stable Bowser folder is available. Close an open stable copy or choose an empty install folder, then run Open-Bowser again. No process was terminated and no unrelated file was replaced.'
            }
            $InstallDirectory = $destDir
            $installedExe = Join-Path $InstallDirectory 'Bowser.exe'
            Assert-NoReparsePath -Path $InstallDirectory
            New-Item -ItemType Directory -Path $InstallDirectory -Force | Out-Null
            Assert-NoReparsePath -Path $InstallDirectory
            $staged = Join-Path $InstallDirectory ('Bowser.download-' + [guid]::NewGuid().ToString('n') + '.exe')
            [IO.File]::Copy($download, $staged, $false)
            if ((Get-Item -LiteralPath $staged).Length -ne $expectedBytes -or (Get-LowerSha256 -Path $staged) -ne $expectedHash) { throw 'The staged file did not match the pinned release.' }
            Test-BowserSignature -Path $staged
            Install-StagedFile -Staged $staged -Destination $installedExe -PreviousHash $previousHash
            if ((Get-LowerSha256 -Path $installedExe) -ne $expectedHash) {
                throw 'The installed file did not match the pinned release after copying.'
            }
        }
        catch {
            throw
        }
        finally {
            Remove-Item -LiteralPath $download -Force -ErrorAction SilentlyContinue
            if ($staged) { Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue }
        }

        $note = @(
            "version=$($pin.version)"
            "revision=$($pin.revision)"
            "tag=$($pin.tag)"
            "commit=$($pin.commit)"
            "sha256=$expectedHash"
        ) -join "`r`n"
        $notePath = Join-Path $InstallDirectory 'STABLE.txt'
        Assert-NoReparsePath -Path $notePath
        [IO.File]::WriteAllText($notePath, $note + "`r`n")
        Write-Host "Installed stable Bowser $($pin.version): $installedExe"
    }

    Install-StableShortcuts

    if ($NoLaunch) {
        Write-Host "Stable Bowser is ready: $installedExe"
        Write-Host 'Close all Bowser windows, then open Bowser using its Desktop or Start Menu shortcut. Browser profiles and tabs were not changed.'
        exit 0
    }

    # A downloaded old build (including Bowser (1).exe) can share the normal profile too.
    # If ANY Bowser-named process exists, leave it alone and do not auto-launch another copy.
    $openBowsers = @(Get-Process -Name 'Bowser*' -ErrorAction SilentlyContinue)
    if (Test-BowserLaunchBlocked -Processes $openBowsers) {
        Write-Host "Stable Bowser $($pin.version) is installed and ready: $installedExe"
        Write-Host 'Close ALL Bowser windows, then use the Bowser Desktop or Start Menu shortcut. No browser process was terminated and no second copy was opened.'
        exit 0
    }

    $process = Start-Process -FilePath $installedExe -WorkingDirectory $InstallDirectory -PassThru
    Write-Host "Opened Bowser $($pin.version) (PID $($process.Id))."
}
finally {
    # PowerShell executes finally for exit 0/23 as well as errors; never leave setup serialized
    # only until a process crash when an ordinary launch or -NoLaunch installation finishes.
    Exit-BowserInstallMutex -Mutex $installMutex
}
