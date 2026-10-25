# Helper-only Windows PowerShell 5.1 regression suite. Never dot-source or run installer top-level
# statements: no network request, browser launch, shortcut, profile or installed executable changes.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$installer = Join-Path $root 'Open-Bowser.ps1'
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($installer, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { $parseErrors | Format-List; throw 'Open-Bowser.ps1 does not parse.' }
$definitions = $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $false)
foreach ($definition in $definitions) { . ([scriptblock]::Create($definition.Extent.Text)) }

$count = 0
function Assert-Equal { param($Actual, $Expected, [string]$Message)
    if ($Actual -ne $Expected) { throw "$Message (expected $Expected, got $Actual)" }
    $script:count++
}
function Assert-Throws { param([scriptblock]$Work, [string]$Message)
    $threw = $false
    try { & $Work } catch { $threw = $true }
    if (-not $threw) { throw "$Message (expected refusal)" }
    $script:count++
}
function Pin-Bytes { param([hashtable]$Changes = @{})
    $pin = @{ version = '0.0.24'; revision = 25; tag = 'v0.0.24'; repository = 'grantcharlie1000-tech/Bowser'; asset = 'Bowser.exe'; sha256 = ('a' * 64); bytes = 512 }
    foreach ($name in $Changes.Keys) { $pin[$name] = $Changes[$name] }
    return ,[Text.Encoding]::UTF8.GetBytes(($pin | ConvertTo-Json -Compress))
}

$pin = ConvertFrom-BowserPin -Bytes (Pin-Bytes)
Assert-BowserPinMinimum -Pin $pin -Version '0.0.24' -Revision 25
Assert-Equal $pin.bytes 512 'A matching current pin is usable'
Assert-Throws { Assert-BowserPinMinimum -Pin $pin -Version '0.0.25' -Revision 25 } 'A lower pinned version is refused'
Assert-Throws { Assert-BowserPinMinimum -Pin $pin -Version '0.0.24' -Revision 26 } 'A lower pin revision is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ repository = 'attacker/Bowser' }) } 'Another repository is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ asset = 'other.exe' }) } 'Another asset is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ tag = '../evil' }) } 'A path in a tag is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ version = '0.0.24.1' }) } 'A four-part version is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ version = '0.0.-1' }) } 'A negative version part is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ bytes = 0 }) } 'A missing size is never inferred'
Assert-Throws { ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ bytes = 268435457 }) } 'An oversized pin is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ bytes = -1 }) } 'A negative size is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ revision = 4294967296 }) } 'An out-of-range revision is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ sha256 = ('g' * 64) }) } 'A non-hex digest is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes (New-Object byte[] 16385) } 'A pin over 16 KiB is refused'
Assert-Throws { ConvertFrom-BowserPin -Bytes ([byte[]]@(255, 255)) } 'Invalid UTF-8 is refused'

$https = [Uri]'https://github.com/owner/repo'
Assert-PlainHttpsUri -Uri $https
Assert-Equal (Resolve-BowserRedirect -Current $https -Location '/asset' -Hop 0).AbsoluteUri 'https://github.com/asset' 'A relative HTTPS redirect is resolved'
foreach ($url in @('http://example.com/file', 'file:///C:/secret', 'https://user:password@example.com/file', 'https://example.com/file#fragment')) {
    Assert-Throws { Assert-PlainHttpsUri -Uri ([Uri]$url) } "Unsafe URL $url is refused"
    Assert-Throws { Resolve-BowserRedirect -Current $https -Location $url -Hop 0 } "Unsafe redirect $url is refused"
}
Assert-Throws { Resolve-BowserRedirect -Current $https -Location '/asset' -Hop 5 } 'A sixth redirect is refused'
Assert-Throws { Resolve-BowserRedirect -Current $https -Location ('x' * 8193) -Hop 0 } 'An excessive Location is refused'

$source = [IO.MemoryStream]::new([byte[]]@(1, 2, 3, 4))
$destination = New-Object IO.MemoryStream
try {
    Assert-Equal (Copy-BoundedStream -Source $source -Destination $destination -Limit 4) 4 'An exact-size stream is copied'
    Assert-Equal $destination.Length 4 'All exact-size bytes are present'
    $source.Position = 0
    $destination.SetLength(0)
    Assert-Throws { Copy-BoundedStream -Source $source -Destination $destination -Limit 3 } 'A chunk exceeding the cap is refused'
    Assert-Equal $destination.Length 0 'An overflowing chunk is not written'
}
finally { $source.Dispose(); $destination.Dispose() }

# Authenticode is isolated with a test-only function, not by trusting arbitrary fake PE bytes.
function Get-AuthenticodeSignature { param([string]$LiteralPath) return [pscustomobject]@{ Status = $script:signatureStatus } }
foreach ($state in @('Valid', 'NotSigned')) { $script:signatureStatus = $state; Test-BowserSignature -Path 'unused' }
foreach ($state in @('HashMismatch', 'NotTrusted', 'UnknownError', 'NotSupportedFileFormat')) {
    $script:signatureStatus = $state
    Assert-Throws { Test-BowserSignature -Path 'unused' } "Signature status $state is refused"
}
Remove-Item Function:\Get-AuthenticodeSignature

$shortcut = [pscustomobject]@{ TargetPath = ''; Arguments = '--dev --benchmark-idle --user-data-dir C:\wrong'; WorkingDirectory = ''; WindowStyle = 0; Description = ''; IconLocation = ''; Saved = 0 }
$shortcut | Add-Member -MemberType ScriptMethod -Name Save -Value { $this.Saved++ }
Set-BowserShortcut -Shortcut $shortcut -Target 'C:\safe\Bowser.exe' -WorkingDirectory 'C:\safe' -Description 'Open Bowser'
Assert-Equal $shortcut.Arguments '' 'Recovery clears old dev, benchmark and profile arguments'
Assert-Equal $shortcut.Saved 1 'The corrected shortcut is saved once'
Assert-BowserShortcut -Shortcut $shortcut -Target 'C:\safe\Bowser.exe'
$shortcut.Arguments = '--dev'
Assert-Throws { Assert-BowserShortcut -Shortcut $shortcut -Target 'C:\safe\Bowser.exe' } 'Argument retention after Save is refused'

# Inert process-list mocks only: no Get-Process call and no process is started or terminated.
$downloaded = [pscustomobject]@{ ProcessName = 'Bowser'; Path = 'C:\Users\me\Downloads\Bowser.exe' }
$duplicate = [pscustomobject]@{ ProcessName = 'Bowser (1)'; Path = 'C:\Users\me\Downloads\Bowser (1).exe' }
$installed = [pscustomobject]@{ ProcessName = 'Bowser'; Path = 'C:\Users\me\AppData\Local\Programs\Bowser\Bowser.exe' }
$unknown = [pscustomobject]@{ ProcessName = 'Bowser' }
$unknown | Add-Member -MemberType ScriptProperty -Name Path -Value { throw 'A blocker path must never be inspected.' }
Assert-Equal (Test-BowserLaunchBlocked -Processes @($downloaded)) $true 'An old Downloads copy prevents a second profile owner'
Assert-Equal (Test-BowserLaunchBlocked -Processes @($duplicate)) $true 'A duplicate-download executable is still a blocker'
Assert-Equal (Test-BowserLaunchBlocked -Processes @($installed)) $true 'An already-running installed target also prevents auto-launch'
Assert-Equal (Test-BowserLaunchBlocked -Processes @($unknown)) $true 'An unreadable or unknown path remains a blocker without path inspection'
Assert-Equal (Test-BowserLaunchBlocked -Processes @()) $false 'An empty Bowser process list allows standalone launch'
Assert-Equal (Test-BowserLaunchBlocked -Processes $null) $false 'No process list has no blockers'
Assert-Equal (Test-BowserLaunchBlocked -Processes @($null)) $false 'Null process entries are not blockers'

# Exercise ownership on a synthetic GUID name only; never acquire the production user lock.
# C# owns the competing thread so it needs no PowerShell runspace or subprocess.
Add-Type -TypeDefinition @'
using System;
using System.Threading;
public sealed class BowserInstallerMutexFixture : IDisposable {
    private readonly Mutex mutex;
    private readonly ManualResetEvent ready = new ManualResetEvent(false);
    private readonly ManualResetEvent release = new ManualResetEvent(false);
    private readonly Thread owner;
    public BowserInstallerMutexFixture(string name, bool abandon) {
        mutex = new Mutex(false, name);
        owner = new Thread(() => {
            mutex.WaitOne();
            ready.Set();
            if (!abandon) {
                release.WaitOne();
                mutex.ReleaseMutex();
            }
        });
        owner.IsBackground = true;
        owner.Start();
        if (!ready.WaitOne(5000)) throw new Exception("Synthetic mutex owner did not start.");
        if (abandon && !owner.Join(5000)) throw new Exception("Synthetic mutex owner did not finish.");
    }
    public void Dispose() {
        release.Set();
        if (!owner.Join(5000)) throw new Exception("Synthetic mutex owner did not stop.");
        mutex.Dispose();
        ready.Dispose();
        release.Dispose();
    }
}
'@
$mutexName = 'Local\BowserStableInstall-unit-' + [guid]::NewGuid().ToString('n')
$holder = [BowserInstallerMutexFixture]::new($mutexName, $false)
try { Assert-Throws { Enter-BowserInstallMutex -Name $mutexName } 'A concurrent synthetic installer fails immediately while busy' }
finally { $holder.Dispose() }
$testMutex = Enter-BowserInstallMutex -Name $mutexName
try { Assert-Equal ($null -ne $testMutex) $true 'A released synthetic install mutex can be acquired' }
finally { Exit-BowserInstallMutex -Mutex $testMutex }
$holder = [BowserInstallerMutexFixture]::new($mutexName, $true)
try {
    $testMutex = Enter-BowserInstallMutex -Name $mutexName
    try { Assert-Equal ($null -ne $testMutex) $true 'An abandoned synthetic mutex is acquired rather than refused' }
    finally { Exit-BowserInstallMutex -Mutex $testMutex }
}
finally { $holder.Dispose() }
$testMutex = Enter-BowserInstallMutex -Name $mutexName
try { Assert-Equal ($null -ne $testMutex) $true 'Cleanup leaves the synthetic mutex available for another install' }
finally { Exit-BowserInstallMutex -Mutex $testMutex }

# Only synthetic fixtures under this suite's unique temp directory are written below.
$temp = Join-Path ([IO.Path]::GetTempPath()) ('bowser-installer-unit-' + [guid]::NewGuid().ToString('n'))
New-Item -ItemType Directory -Path $temp | Out-Null
$junction = Join-Path $temp 'junction'
try {
    $managed = Join-Path $temp 'managed'
    New-Item -ItemType Directory -Path $managed | Out-Null
    $old = Join-Path $managed 'Bowser.exe'
    [IO.File]::WriteAllText($old, 'old synthetic fixture, not an executable')
    Assert-Equal (Get-ManagedInstallHash -Directory $managed) $null 'An unrecorded old executable is not replaceable'
    $oldHash = Get-LowerSha256 -Path $old
    [IO.File]::WriteAllText((Join-Path $managed 'STABLE.txt'), "version=0.0.25`r`nrevision=26`r`nsha256=$oldHash`r`n")
    Assert-Equal (Get-ManagedInstallHash -Directory $managed) $oldHash 'A matching installed record identifies only those bytes'
    Assert-Throws { Assert-PinNotOlderThanManagedInstalls -Pin $pin -Directories @($managed) } 'A newer installed managed build prevents rollback and shortcut repointing'
    $newPin = ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ version = '0.0.25'; revision = 26; tag = 'v0.0.25' })
    Assert-PinNotOlderThanManagedInstalls -Pin $newPin -Directories @($managed)
    [IO.File]::AppendAllText($old, 'changed')
    Assert-Equal (Get-ManagedInstallHash -Directory $managed) $null 'A stale marker grants no replacement permission'
    Assert-Throws { Assert-PinNotOlderThanManagedInstalls -Pin $pin -Directories @($managed) } 'A hash-stale marker blocks an ambiguous fallback before shortcut writes'
    $matching = ConvertFrom-BowserPin -Bytes (Pin-Bytes @{ sha256 = (Get-LowerSha256 -Path $old); bytes = (Get-Item -LiteralPath $old).Length })
    Assert-PinNotOlderThanManagedInstalls -Pin $matching -Directories @($managed)
    $stage = Join-Path $managed 'stage'
    [IO.File]::WriteAllText($stage, 'new synthetic fixture')
    Assert-Throws { Install-StagedFile -Staged $stage -Destination $old -PreviousHash $oldHash } 'A changed destination is never overwritten'
    Assert-Equal ([IO.File]::ReadAllText($stage)) 'new synthetic fixture' 'A refused replacement preserves its staged fixture'
    $receiptBefore = [IO.File]::ReadAllText((Join-Path $managed 'STABLE.txt'))
    New-Item -ItemType Junction -Path $junction -Target $managed | Out-Null
    Assert-Throws { Assert-NoReparsePath -Path (Join-Path $junction 'Bowser.exe') } 'A junction ancestor is refused'
    # WinPS 5.1's FileSystem provider can throw while removing a junction. Nonrecursive .NET
    # deletion unlinks only this synthetic junction, never the directory it points at.
    [IO.Directory]::Delete($junction, $false)
    Assert-Equal ([IO.Directory]::Exists($junction)) $false 'The synthetic junction is removed'
    Assert-Equal ([IO.Directory]::Exists($managed)) $true 'Removing the fixture junction preserves its target directory'
    Assert-Equal ([IO.File]::ReadAllText($old)) 'old synthetic fixture, not an executablechanged' 'Removing the fixture junction preserves its target bytes'
    Assert-Equal ([IO.File]::ReadAllText($stage)) 'new synthetic fixture' 'Removing the fixture junction preserves staged fixture bytes'
    Assert-Equal ([IO.File]::ReadAllText((Join-Path $managed 'STABLE.txt'))) $receiptBefore 'Removing the fixture junction preserves its target receipt'
}
finally {
    # An earlier failed assertion must not leave a junction for WinPS's recursive provider cleanup.
    if ([IO.Directory]::Exists($junction)) { [IO.Directory]::Delete($junction, $false) }
    Remove-Item -LiteralPath $temp -Recurse -Force
}

# Stale-pin exit is part of the native wrapper protocol and must occur before installation work.
$text = [IO.File]::ReadAllText($installer)
Assert-Equal ($text.IndexOf('exit 23') -gt $text.IndexOf('try { Assert-BowserPinMinimum')) $true 'Stale-pin failure is given distinct exit code 23'
Assert-Equal ($text.IndexOf('exit 23') -lt $text.IndexOf('$localAppData =')) $true 'Stale-pin failure precedes install directory writes'
Write-Host "installer-safety: $count assertions passed; top-level installer was never executed."
