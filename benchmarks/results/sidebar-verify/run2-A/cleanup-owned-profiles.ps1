$ErrorActionPreference = 'Stop'
$out = $PSScriptRoot
$exe = (Resolve-Path (Join-Path $out 'test-bin\bowser.exe')).Path
$profiles = @('vA-1','vA-2','vA-3','vA-4')
$profileBase = Join-Path $env:LOCALAPPDATA 'Bowser\Profiles'
$owned = @(Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -eq $exe -and $_.CommandLine -match '--profile\s+vA-[1-4]'
})
$starts = @()
foreach ($w in $owned) {
    $gp = Get-Process -Id $w.ProcessId -ErrorAction SilentlyContinue
    if ($gp) {
        $profile = $profiles | Where-Object { $w.CommandLine -match [regex]::Escape($_) } | Select-Object -First 1
        $starts += [pscustomobject]@{ pid=$gp.Id; start=$gp.StartTime; profile=$profile }
        [void]$gp.CloseMainWindow()
        if (!$gp.WaitForExit(6000)) { Stop-Process -Id $gp.Id -Force -ErrorAction SilentlyContinue }
    }
}
$eventLines = @()
foreach ($s in $starts) {
    $ev = Get-WinEvent -FilterHashtable @{ LogName='Application'; Id=1000,1001; StartTime=$s.start.AddSeconds(-2) } -ErrorAction SilentlyContinue |
        Where-Object { $_.Message -match [regex]::Escape($exe) -or $_.Message -match ('Faulting process ID: 0x{0:x}' -f $s.pid) }
    if ($ev) { $eventLines += ($ev | Format-List TimeCreated,Id,ProviderName,Message | Out-String -Width 300) }
    else { $eventLines += ("No Application Error/WER 1000/1001 event matched owned test process PID {0} ({1}) since {2}" -f $s.pid,$s.profile,$s.start) }
}
$eventLines | Set-Content (Join-Path $out 'owned-test-events.txt') -Encoding UTF8
$manifest = [System.Collections.Generic.List[string]]::new()
foreach ($name in $profiles) {
    $path = Join-Path $profileBase $name
    if (Test-Path -LiteralPath $path) {
        $item = Get-Item -LiteralPath $path
        $manifest.Add("PROFILE $name $($item.FullName)")
        Get-ChildItem -LiteralPath $path -Force -File -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
            $manifest.Add("FILE $($_.FullName) length=$($_.Length)")
            if ($_.Name -in @('workspaces.txt','sidebar.txt','bookmarks-v2.txt','settings.txt')) {
                $manifest.Add((Get-Content -Raw -LiteralPath $_.FullName))
            }
        }
        $manifest.Add('---')
    } else { $manifest.Add("PROFILE $name ABSENT") }
}
$manifest | Set-Content (Join-Path $out 'profile-files-before-cleanup.txt') -Encoding UTF8
foreach ($name in $profiles) {
    $path = Join-Path $profileBase $name
    if (Test-Path -LiteralPath $path) {
        $resolved = (Resolve-Path -LiteralPath $path).Path
        $expected = (Resolve-Path -LiteralPath $profileBase).Path
        if ((Split-Path $resolved -Parent) -ne $expected) { throw "Refusing cleanup outside Profiles: $resolved" }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
$remaining = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $exe -and $_.CommandLine -match '--profile\s+vA-[1-4]' })
[pscustomobject]@{
    ownedProcessIdsBefore=@($owned | ForEach-Object ProcessId)
    ownedProcessesRemaining=$remaining.Count
    profileRootsRemoved=@($profiles | Where-Object { !(Test-Path (Join-Path $profileBase $_)) })
    crashLogPresent=(Test-Path (Join-Path $env:LOCALAPPDATA 'Bowser\crash.log'))
    eventsFile=(Join-Path $out 'owned-test-events.txt')
    profilesFile=(Join-Path $out 'profile-files-before-cleanup.txt')
} | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $out 'cleanup-summary.json') -Encoding UTF8
Get-Content (Join-Path $out 'cleanup-summary.json')
Get-Content (Join-Path $out 'owned-test-events.txt')
Get-Content (Join-Path $out 'profile-files-before-cleanup.txt')
