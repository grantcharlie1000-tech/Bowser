# Public-package + isolated launcher contract verification, not background updater/restart QA.
# Run in Windows CI against the FINAL checked-out stable/release.json, after publication.
# Does not launch Bowser, build code, publish releases, change pins, or create live shortcuts.
[CmdletBinding()]
param([string]$Out = "$PSScriptRoot\..\results\stable-package")
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$Out = [IO.Path]::GetFullPath($Out)
New-Item -ItemType Directory -Force $Out | Out-Null
$run = Join-Path ([IO.Path]::GetTempPath()) ('bowser-stable-package-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $run | Out-Null
$pinPath = Join-Path $root 'stable\release.json'
$launcher = Join-Path $root 'Open-Bowser.ps1'
$repository = 'grantcharlie1000-tech/Bowser'
$remotePinUrl = 'https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/stable/release.json'
$report = [ordered]@{
    schema_version = 1
    scope = 'Public stable package integrity and isolated no-launch installer idempotence'
    started_at_utc = [DateTime]::UtcNow.ToString('o')
    status = 'failed'
    checkout_commit = $null
    checked_out_pin = $null
    published_metadata = $null
    tag_commit = $null
    install_directory = (Join-Path $run 'install')
    attempts = @()
    checks = [ordered]@{}
    unverified = @('Background updater network/staging/restart', 'Interactive installed browser launch', 'Full human UI QA')
    error = $null
}
$failure = $null
$pinHash = $null; $launcherHash = $null

function Hash([string]$Path) { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Require-Field($Object, [string]$Name) {
    $field = $Object.PSObject.Properties[$Name]
    if (-not $field -or $null -eq $field.Value -or [string]::IsNullOrWhiteSpace([string]$field.Value)) { throw "Missing required field: $Name" }
    return $field.Value
}
function Assert-SamePin($Selected, $Expected) {
    foreach ($field in @('repository', 'version', 'revision', 'tag', 'commit', 'asset', 'sha256', 'bytes')) {
        if ([string](Require-Field $Selected $field) -cne [string](Require-Field $Expected $field)) { throw "Launcher selected a different pin field: $field" }
    }
}
function Assert-Package([string]$Path, $Pin) {
    if ((Get-Item -LiteralPath $Path).Length -ne [long]$Pin.bytes -or (Hash $Path) -cne [string]$Pin.sha256) { throw "Package hash/size mismatch: $Path" }
}
function Read-PublicApi([string]$Path) {
    return Invoke-RestMethod -Uri "https://api.github.com/repos/$repository/$Path" -Headers @{ Accept = 'application/vnd.github+json'; 'X-GitHub-Api-Version' = '2022-11-28'; 'User-Agent' = 'Bowser-stable-package-verification' } -TimeoutSec 60
}
function Installed-Snapshot {
    return @(Get-ChildItem -LiteralPath $report.install_directory -Recurse -File | Sort-Object FullName | ForEach-Object {
        [ordered]@{ name = $_.FullName.Substring($report.install_directory.Length); bytes = $_.Length; sha256 = (Hash $_.FullName); modified_utc_ticks = $_.LastWriteTimeUtc.Ticks }
    }) | ConvertTo-Json -Depth 5 -Compress
}

try {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows is required; no installation verification ran' }
    $report.checkout_commit = (& git -C $root rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify checked-out source' }
    if (@(& git -C $root status --porcelain --untracked-files=no).Count -ne 0 -or $LASTEXITCODE -ne 0) { throw 'Tracked checkout must be clean' }
    $pinHash = Hash $pinPath
    $launcherHash = Hash $launcher
    $pin = Get-Content -LiteralPath $pinPath -Raw | ConvertFrom-Json
    $report.checked_out_pin = $pin
    foreach ($field in @('repository', 'version', 'revision', 'tag', 'commit', 'asset', 'sha256', 'bytes')) { [void](Require-Field $pin $field) }
    if ($pin.repository -cne $repository -or $pin.asset -cne 'Bowser.exe') { throw 'Pin must name the verified Bowser repository and executable' }
    if ([string]$pin.version -cnotmatch '^\d+\.\d+\.\d+$' -or $pin.tag -cne "v$($pin.version)" -or [string]$pin.commit -cnotmatch '^[0-9a-f]{40}$') { throw 'Invalid pinned version/tag/source commit' }
    if ([string]$pin.sha256 -cnotmatch '^[0-9a-f]{64}$' -or [long]$pin.bytes -lt 512 -or [long]$pin.bytes -gt 536870912 -or [long]$pin.revision -lt 1 -or [long]$pin.revision -gt [int]::MaxValue) { throw 'Invalid pinned digest, size or revision' }
    $reference = Read-PublicApi "git/ref/tags/$($pin.tag)"
    $object = $reference.object
    # Bounded dereference supports annotated as well as lightweight release tags.
    for ($depth = 0; $object.type -eq 'tag'; $depth++) {
        if ($depth -ge 8 -or [string]$object.sha -cnotmatch '^[0-9a-f]{40}$') { throw 'Invalid/deep annotated release tag' }
        $object = (Read-PublicApi "git/tags/$($object.sha)").object
    }
    if ($object.type -cne 'commit' -or $object.sha -cne $pin.commit) { throw 'Published tag does not resolve to the pinned build commit' }
    $report.tag_commit = $object.sha
    $release = Read-PublicApi "releases/tags/$($pin.tag)"
    if ($release.draft -or $release.prerelease -or $release.tag_name -cne $pin.tag) { throw 'Expected a public, non-prerelease stable release' }
    $base = "https://github.com/$repository/releases/download/$($pin.tag)"
    foreach ($assetName in @('Bowser.exe', 'build-metadata.json')) {
        $asset = @($release.assets | Where-Object { $_.name -ceq $assetName })
        if ($asset.Count -ne 1 -or $asset[0].browser_download_url -cne "$base/$assetName") { throw "Missing or unexpected public release asset: $assetName" }
    }
    $metadataPath = Join-Path $Out 'published-build-metadata.json'
    Invoke-WebRequest -Uri "$base/build-metadata.json" -OutFile $metadataPath -UseBasicParsing -TimeoutSec 120
    $metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
    $report.published_metadata = $metadata
    Assert-SamePin $metadata $pin
    if ((Require-Field $metadata 'features') -cne 'html-chrome-default' -or (Require-Field $metadata 'smoke_passed') -isnot [bool] -or $metadata.smoke_passed -ne $true) { throw 'Published build lacks verified default HTML chrome smoke provenance' }
    $download = Join-Path $run 'Bowser.exe'
    Invoke-WebRequest -Uri "$base/Bowser.exe" -OutFile $download -UseBasicParsing -TimeoutSec 180
    Assert-Package $download $pin
    $report.checks.public_tag_metadata_and_package_match_pin = $true
    # Capture the ACTUAL remote-pin response seen by unchanged Open-Bowser.ps1. Its logs omit
    # revision, and pre-fetching independently would not prove which pin its own request selected.
    # This transparent wrapper delegates the request unchanged to the built-in cmdlet; it neither
    # substitutes a response nor edits the pin/launcher. All candidate installation remains temp-only.
    $wrapper = Join-Path $run 'invoke-isolated-launcher.ps1'
    @'
param([string]$Launcher, [string]$SourceExe, [string]$InstallDirectory, [string]$CapturePath)
$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($CapturePath)) { throw 'Missing owned capture path' }
# This variable belongs only to the disposable wrapper process, not a user/machine environment.
# Process scope stays unambiguous across invocation of a different launcher script scope.
$env:BOWSER_STABLE_VERIFIER_CAPTURE_PATH = $CapturePath
function Invoke-WebRequest {
    param([Parameter(Mandatory = $true)][string]$Uri, [switch]$UseBasicParsing)
    $captureFile = [Environment]::GetEnvironmentVariable('BOWSER_STABLE_VERIFIER_CAPTURE_PATH', 'Process')
    if ([string]::IsNullOrWhiteSpace($captureFile)) { throw 'Owned wrapper process lost its capture path' }
    try {
        if ($Uri -cne 'https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/stable/release.json') { throw 'Unexpected request in isolated no-launch installer' }
        [IO.File]::WriteAllText($captureFile + '.invoked.txt', "Actual unchanged launcher requested $Uri")
        $response = Microsoft.PowerShell.Utility\Invoke-WebRequest @PSBoundParameters -TimeoutSec 60
        [IO.File]::WriteAllText($captureFile, [string]$response.Content)
        return $response
    } catch {
        $detail = ($_ | Format-List * -Force | Out-String) + "`n" + $_.ScriptStackTrace
        [Console]::Error.WriteLine($detail)
        try { [IO.File]::WriteAllText($captureFile + '.error.txt', $detail) }
        catch { [Console]::Error.WriteLine("Capture diagnostic write also failed: $($_.Exception)") }
        throw
    }
}
$global:LASTEXITCODE = 0
& $Launcher -SourceExe $SourceExe -InstallDirectory $InstallDirectory -NoLaunch -NoShortcuts
exit $LASTEXITCODE
'@ | Set-Content -LiteralPath $wrapper -Encoding utf8
    $pwsh = (Get-Command pwsh -ErrorAction Stop).Source
    $beforeSnapshot = $null
    foreach ($attempt in 1..2) {
        $capture = Join-Path $Out "attempt-$attempt-remote-pin.json"
        # Do not misattribute a previous run's response when the current remote request fails.
        foreach ($old in @($capture, ($capture + '.error.txt'), ($capture + '.invoked.txt'))) {
            if (Test-Path -LiteralPath $old) { Remove-Item -LiteralPath $old -Force }
        }
        $stdout = Join-Path $Out "attempt-$attempt-stdout.txt"
        $stderr = Join-Path $Out "attempt-$attempt-stderr.txt"
        $args = @('-NoProfile', '-NonInteractive', '-File', "`"$wrapper`"", '-Launcher', "`"$launcher`"", '-SourceExe', "`"$download`"", '-InstallDirectory', "`"$($report.install_directory)`"", '-CapturePath', "`"$capture`"")
        $process = Start-Process -FilePath $pwsh -ArgumentList $args -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        if (-not $process.WaitForExit(120000)) {
            # This is only the owned PowerShell wrapper, never a Bowser/browser process.
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
            $process.Dispose()
            throw "Isolated launcher attempt $attempt exceeded 120 seconds"
        }
        $process.WaitForExit()
        $process.Refresh()
        $exitCode = $process.ExitCode
        $process.Dispose()
        if ($null -eq $exitCode) { throw "Isolated launcher attempt $attempt returned no exit code" }
        $selected = $pin
        $selection = 'checked-out pin (actual remote response older)'
        $entry = [ordered]@{ attempt = $attempt; exit_code = $exitCode; selection = $null; selected_pin = $null; remote_response_observed = $false }
        $report.attempts += $entry
        if (Test-Path -LiteralPath ($capture + '.error.txt')) {
            $detail = Get-Content -LiteralPath ($capture + '.error.txt') -Raw
            throw "Actual launcher remote request/capture failed: $detail"
        }
        if (-not (Test-Path -LiteralPath $capture -PathType Leaf) -or -not (Test-Path -LiteralPath ($capture + '.invoked.txt') -PathType Leaf)) {
            throw "Actual launcher remote response was not captured; see attempt-$attempt stdout/stderr. Fallback cannot satisfy this release gate."
        }
        try {
            $remote = Get-Content -LiteralPath $capture -Raw | ConvertFrom-Json
            foreach ($field in @('repository', 'version', 'revision', 'tag', 'commit', 'asset', 'sha256', 'bytes')) { [void](Require-Field $remote $field) }
            # Match the unchanged launcher's Int32 revision-selection rule, using its actual response.
            $localRevision = [int]$pin.revision
            $remoteRevision = [int]$remote.revision
            $expectedLine = 'The GitHub pin is older than this folder. Using the local pin.'
            if ($remoteRevision -ge $localRevision) {
                $selected = $remote
                $selection = 'actual launcher remote response'
                $expectedLine = 'Using the stable release published on GitHub.'
            }
        } catch { throw "Captured launcher remote response is malformed: $($_.Exception.Message)" }
        $launcherOutput = Get-Content -LiteralPath $stdout -Raw
        if (-not $launcherOutput.Contains($expectedLine) -or $launcherOutput.Contains('GitHub could not be reached.')) { throw 'Launcher output does not confirm the observed remote-response selection.' }
        $entry.selection = $selection
        $entry.selected_pin = $selected
        $entry.remote_response_observed = $true
        Assert-SamePin $selected $pin
        if ($exitCode -ne 0) { throw "Isolated launcher attempt $attempt failed (exit $exitCode); see captured logs" }
        $installed = Join-Path $report.install_directory 'Bowser.exe'
        Assert-Package $installed $pin
        $note = Get-Content -LiteralPath (Join-Path $report.install_directory 'STABLE.txt')
        $expectedNote = @("version=$($pin.version)", "tag=$($pin.tag)", "commit=$($pin.commit)", "sha256=$($pin.sha256)")
        if (($note -join "`n") -cne ($expectedNote -join "`n")) { throw 'Installed STABLE.txt differs from checked-out pin' }
        if (@(Get-ChildItem -LiteralPath $run -Recurse -Filter '*.lnk').Count -ne 0) { throw 'NoShortcuts contract violated in isolated root' }
        if (@(Get-Process -Name Bowser -ErrorAction SilentlyContinue | Where-Object { try { $_.Path -and $_.Path.StartsWith($run, [StringComparison]::OrdinalIgnoreCase) } catch { $false } }).Count -ne 0) { throw 'NoLaunch contract violated; a temporary Bowser process exists (not terminated by verifier)' }
        $snapshot = Installed-Snapshot
        if ($attempt -eq 1) { $beforeSnapshot = $snapshot }
        elseif ($snapshot -cne $beforeSnapshot) { throw 'Second no-launch installation changed installed files or timestamps' }
        if ((Hash $pinPath) -cne $pinHash -or (Hash $launcher) -cne $launcherHash) { throw 'Checked-out pin or launcher changed during verification' }
        $entry.installed_sha256 = Hash $installed
        $entry.stable_note_verified = $true
    }
    $report.checks.actual_launcher_remote_response_observed_both_attempts = $true
    $report.checks.isolated_no_launch_no_shortcuts_install = $true
    $report.checks.second_run_idempotent = $true
    $report.checks.checked_out_pin_and_launcher_unchanged = $true
    $report.status = 'passed_isolated_stable_package_verification'
} catch {
    $failure = $_
    $report.error = $_.Exception.Message
} finally {
    $report.finished_at_utc = [DateTime]::UtcNow.ToString('o')
    $report | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath (Join-Path $Out 'stable-package.json') -Encoding utf8
    # GUID-qualified temporary downloads/install are retained for diagnosis on the disposable runner.
}
if ($failure) { throw $failure }
Write-Host "Stable package verification: $($report.status). Evidence: $Out"
