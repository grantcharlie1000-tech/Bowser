# Native, ordinary-profile baseline. Requires interactive Windows and HTML-default binaries.
# No --benchmark-* arguments, no remote-debugging, no user profile, no disabled security features.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BaselineExe,
    [Parameter(Mandatory)][string]$DiagnosticExe,
    [Parameter(Mandatory)][string]$ReleaseExe,
    [Parameter(Mandatory)][string]$Out,
    [ValidateRange(1,30)][int]$Pairs=5,
    [ValidateSet('light','demanding','redirect')][string[]]$Fixtures=@('light','demanding','redirect'),
    [ValidateRange(1,20)][int]$MaxInvalidSamples=2,
    [string]$DiagnosticSource='8a60dca2ac7de6d369c9b65584fd621a7013f491',
    [ValidateRange(1,30)][int]$IdleSeconds=3,
    [ValidateRange(100,2000)][int]$SampleMilliseconds=250,
    [string]$BaselineSource='80eabd7ae6d421e61c81985c34023bfc824a2e58'
)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/measurements.psm1" -Force
Import-Module "$PSScriptRoot/production-measurements.psm1" -Force
if (-not $IsWindows) { throw 'Native Windows required' }
$Out=[IO.Path]::GetFullPath($Out)
New-Item -ItemType Directory -Force $Out | Out-Null
$run=Join-Path $Out 'isolated'
New-Item -ItemType Directory -Force $run | Out-Null
$arms=[ordered]@{release=(Resolve-BowserExecutable $ReleaseExe);baseline=(Resolve-BowserExecutable $BaselineExe);diagnostic_off=(Resolve-BowserExecutable $DiagnosticExe);diagnostic_on=(Resolve-BowserExecutable $DiagnosticExe)}
$hashes=Get-MeasurementInputHashes @($BaselineExe,$DiagnosticExe,$ReleaseExe)
$expected='341486070d2e74cf9a9e5d5fbeffa90c7dd2ec4fd36bc284a8a3878a7a9bcec1'
if ((Get-FileHash $ReleaseExe -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expected) { throw 'Released executable does not match verified v0.0.19 pin' }
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class BowserProductionNative {
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int command);
}
'@
$saved=@{}
foreach($name in @('LOCALAPPDATA','BOWSER_DATA_DIR','BOWSER_UI','BOWSER_UPDATE_URL','BOWSER_UPDATE_PUBKEY')) { $saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process'); [Environment]::SetEnvironmentVariable($name,$null,'Process') }
$env:LOCALAPPDATA=Join-Path $run 'local-app-data'
New-Item -ItemType Directory -Force $env:LOCALAPPDATA | Out-Null
$portFile=Join-Path $run 'port.txt'; $requestFile=Join-Path $run 'requests.jsonl'
$script:profileTimestamp=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$invalidSamples=0
$server=$null; $app=$null; $rows=[Collections.Generic.List[object]]::new()
function Qpc { [Diagnostics.Stopwatch]::GetTimestamp() }
function Owned-Keys([string]$keys) {
    $app.Refresh(); $h=$app.MainWindowHandle; $owner=[uint32]0
    [void][BowserProductionNative]::GetWindowThreadProcessId($h,[ref]$owner)
    if ($h -eq [IntPtr]::Zero -or $owner -ne $app.Id -or -not [BowserProductionNative]::IsWindowVisible($h)) { throw 'Owned visible window unavailable; no global input sent' }
    [void][BowserProductionNative]::SetForegroundWindow($h)
    Start-Sleep -Milliseconds 80
    if ([BowserProductionNative]::GetForegroundWindow() -ne $h) { throw 'Foreground unavailable; no global input sent' }
    [Windows.Forms.SendKeys]::SendWait($keys)
}
function Disk-Bytes([string]$path) { $files=@(Get-ChildItem -LiteralPath $path -File -Recurse -Force -ErrorAction SilentlyContinue); if (-not $files.Count) { return 0 }; return ($files | Measure-Object Length -Sum).Sum }
function Has-Ready([string]$token) {
    if (-not (Test-Path $requestFile)) { return $false }
    return [bool](@(Get-Content -LiteralPath $requestFile | ConvertFrom-Json | Where-Object {$_.path -eq '/ready' -and $_.token -ceq $token}).Count)
}
function Seed-Profile([string]$path,[int]$bookmarks) {
    New-Item -ItemType Directory -Force $path | Out-Null
    # Fixed, documented synthetic settings. All security, privacy, downloads, blocker defaults remain on.
    "startup=start_page`n" | Set-Content (Join-Path $path 'settings.txt') -Encoding utf8NoBOM
    'answered' | Set-Content (Join-Path $path 'import-offer.txt') -Encoding utf8NoBOM
    $history=[Text.StringBuilder]::new()
    $now=$script:profileTimestamp
    for($i=0;$i -lt 10000;$i++) { [void]$history.AppendLine("$($now-$i)`thttps://history.example.test/item/$i`tSynthetic history item $i") }
    [IO.File]::WriteAllText((Join-Path $path 'history.txt'),$history.ToString())
    $marks=[Text.StringBuilder]::new("bowser-bookmarks-v2`n")
    for($i=1;$i -le $bookmarks;$i++) { [void]$marks.AppendLine("$i`t-`tB`tSynthetic bookmark $i`thttps://bookmark.example.test/$i`tbookmark.example.test`t1700000000") }
    [IO.File]::WriteAllText((Join-Path $path 'bookmarks-v2.txt'),$marks.ToString())
}
try {
    $server=Start-Process node -ArgumentList @("`"$PSScriptRoot/scripts/production-fixture-server.mjs`"","`"$portFile`"","`"$requestFile`"") -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $Out 'server-stderr.txt')
    $deadline=[DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path $portFile)) { if ([DateTime]::UtcNow -gt $deadline) {throw 'Fixture startup timeout'}; Start-Sleep -Milliseconds 50 }
    $base="http://127.0.0.1:$([int](Get-Content $portFile -Raw))"
    $metadata=[ordered]@{
        schema_version=1; baseline_source=$BaselineSource; diagnostic_source=$DiagnosticSource; harness_source=(& git rev-parse HEAD).Trim(); selected_fixtures=$Fixtures; started_at_utc=[DateTime]::UtcNow.ToString('o'); pairs=$Pairs
        inputs=$hashes; qpc_frequency=[Diagnostics.Stopwatch]::Frequency; sample_milliseconds=$SampleMilliseconds
        runner_image=$env:ImageOS; runner_image_version=$env:ImageVersion; runner_name=$env:RUNNER_NAME
        os=(Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber,TotalVisibleMemorySize)
        cpu=(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors)
        gpu=(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,CurrentHorizontalResolution,CurrentVerticalResolution)
        power=(& powercfg /getactivescheme | Out-String).Trim(); resolution=[Windows.Forms.Screen]::PrimaryScreen.Bounds.ToString()
        webview2_runtime=@(Get-ChildItem 'C:/Program Files (x86)/Microsoft/EdgeWebView/Application' -Directory -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
        noise=@('Shared hosted runner, not a laptop/battery claim','Fresh profile is not cold OS cache','Updater remains production-enabled, isolated LOCALAPPDATA','Samples are endpoint process-tree observations, not ETW complete','Working-set sums include shared pages','Fixture double-requestAnimationFrame is acknowledged page readiness, not proven display presentation','Title-based tab switch is only a proxy','Sampling overhead separately recorded; measured values are not overhead-subtracted')
        workload_settings=@{history_records=10000; bookmark_records=818; startup='start_page overridden by ordinary --open-url'; first_run_offer='already answered synthetic profile'; security='unchanged defaults'; ui='HTML-default build, no BOWSER_UI override'}
    }
    $metadata | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $Out 'environment.json') -Encoding utf8
    # Five-pair pilot by default. Alternate whole arm order each pair, same session/runtime/fixture seed.
    foreach($pair in 1..$Pairs) {
        $order=@($arms.Keys); if ($pair % 2 -eq 0) { [array]::Reverse($order) }
        foreach($arm in $order) {
            if ($arm -eq 'release' -and $pair -gt 1) { continue }
            foreach($fixture in @(@($Fixtures)+@('redirect','network-failure','error','hung') | Select-Object -Unique)) {
                if ($fixture -notin $Fixtures -and ($arm -ne 'diagnostic_on' -or $pair -ne 1)) { continue }
                if ($fixture -in @('network-failure','error','hung') -and ($arm -ne 'diagnostic_on' -or $pair -ne 1)) { continue }
                $profile=Join-Path $run "$arm-$pair-$fixture"
                Seed-Profile $profile 818
                foreach($condition in @('fresh','reused')) {
                    if ($fixture -notin $Fixtures -and $condition -eq 'reused') { continue }
                    $token=([guid]::NewGuid().ToString('N')); $url="$base/$($fixture)?token=$token"
                    $id="$arm-$pair-$fixture-$condition"; $eventFile=Join-Path $Out "$id-events.csv"
                    $args=@('--data-dir',"`"$profile`"",'--open-url',"`"$url`"")
                    if($arm -eq 'diagnostic_on') { $args+=@('--diagnostic-log',"`"$eventFile`"") }
                    $snapshots=[Collections.Generic.List[object]]::new(); $overheads=[Collections.Generic.List[double]]::new()
                    $sample=[ordered]@{id=$id;pair=$pair;arm=$arm;fixture=$fixture;condition=$condition;token=$token;valid=$false;expected_failure=($fixture -in @('network-failure','error','hung'));error=$null;launch_qpc=(Qpc);ready_qpc=$null;title_ready_qpc=$null;input_ready_qpc=$null;shutdown_ms=$null;disk_before=(Disk-Bytes $profile);events=$null;input=$null;tab_switch_proxy_ms=$null;idle=$null;minimized=$null}
                    $app=$null
                    try {
                        $app=Start-Process -FilePath $arms[$arm] -ArgumentList $args -PassThru
                        $limit=[Diagnostics.Stopwatch]::StartNew(); $timeout=if($sample.expected_failure){4}else{30}
                        while($limit.Elapsed.TotalSeconds -lt $timeout) {
                            $app.Refresh(); if($app.HasExited){throw "Early exit $($app.ExitCode)"}
                            $t=Qpc; $snapshot=Get-ProcessGroupSnapshot -RootPid $app.Id -Profile $profile
                            $overheads.Add(((Qpc)-$t)*1000.0/[Diagnostics.Stopwatch]::Frequency); $snapshots.Add($snapshot)
                            if($null -eq $sample.input_ready_qpc -and $app.MainWindowHandle -ne [IntPtr]::Zero){$sample.input_ready_qpc=Qpc}
                            if($app.MainWindowTitle.Contains("BOWSER_PERF_READY_$token")){$sample.title_ready_qpc=Qpc}
                            if(Has-Ready $token){
                                if ($null -eq $sample.ready_qpc) { $sample.ready_qpc=Qpc }
                                # Native title propagation can lag the page's loopback acknowledgement.
                                if ($null -ne $sample.title_ready_qpc) { break }
                            }
                            Start-Sleep -Milliseconds $SampleMilliseconds
                        }
                        if($sample.expected_failure) {
                            if($sample.ready_qpc){throw 'Failed fixture counted as ready'}
                            $sample.valid=$true
                        } else {
                            if(-not $sample.ready_qpc -or -not $sample.title_ready_qpc){throw 'Fixture did not acknowledge readiness in the owned window'}
                            # Normal UI address submit, not a benchmark navigation shortcut.
                            $inputToken=[guid]::NewGuid().ToString('N'); $inputUrl="$base/light?token=$inputToken"
                            Owned-Keys '^l'; Owned-Keys $inputUrl
                            $inputStart=Qpc; Owned-Keys '{ENTER}'
                            $wait=[Diagnostics.Stopwatch]::StartNew()
                            while(-not (Has-Ready $inputToken) -and $wait.Elapsed.TotalSeconds -lt 20){Start-Sleep -Milliseconds 50}
                            $sample.input=[ordered]@{token=$inputToken;start_qpc=$inputStart;end_qpc=(Qpc);ready=(Has-Ready $inputToken);method='owned-window SendKeys; endpoint includes foreground verification overhead'}
                            if(-not $sample.input.ready){throw 'UI-enter navigation failed'}
                            # One→five tabs, normal Ctrl+T and address input, title-only switch proxy labelled honestly.
                            for($i=1;$i -le 4;$i++){Owned-Keys '^t';Owned-Keys '^l';Owned-Keys "$base/light?token=$($inputToken)_$i";Owned-Keys '{ENTER}';$w=[Diagnostics.Stopwatch]::StartNew();while(-not (Has-Ready "$($inputToken)_$i") -and $w.Elapsed.TotalSeconds -lt 15){Start-Sleep -Milliseconds 50};if(-not(Has-Ready "$($inputToken)_$i")){throw 'Tab fixture failed'}}
                            $app.Refresh();$oldTitle=$app.MainWindowTitle;$switch=Qpc;Owned-Keys '^{TAB}';$w=[Diagnostics.Stopwatch]::StartNew();do{$app.Refresh();if($app.MainWindowTitle -ne $oldTitle){break};Start-Sleep -Milliseconds 10}while($w.Elapsed.TotalSeconds -lt 5)
                            if($app.MainWindowTitle -eq $oldTitle){throw 'Tab switch title proxy timed out'}
                            $sample.tab_switch_proxy_ms=((Qpc)-$switch)*1000.0/[Diagnostics.Stopwatch]::Frequency
                            # Equal-time foreground and minimized idle windows, resource tree sampled throughout.
                            foreach($state in @('idle','minimized')) {
                                if($state -eq 'minimized'){[void][BowserProductionNative]::ShowWindow($app.MainWindowHandle,6)}
                                $idleRows=[Collections.Generic.List[object]]::new();$w=[Diagnostics.Stopwatch]::StartNew()
                                while($w.Elapsed.TotalSeconds -lt $IdleSeconds){$idleRows.Add((Get-ProcessGroupSnapshot -RootPid $app.Id -Profile $profile));Start-Sleep -Milliseconds $SampleMilliseconds}
                                $sample[$state]=@($idleRows.ToArray());foreach($r in $idleRows){$snapshots.Add($r)}
                            }
                            [void][BowserProductionNative]::ShowWindow($app.MainWindowHandle,9)
                            $sample.valid=$true
                        }
                        $close=Qpc
                        if(-not $app.CloseMainWindow() -or -not $app.WaitForExit(15000)){throw 'Graceful close not confirmed'}
                        $sample.shutdown_ms=((Qpc)-$close)*1000.0/[Diagnostics.Stopwatch]::Frequency
                        if($app.ExitCode -ne 0){throw "Exit $($app.ExitCode)"}
                        if($arm -eq 'diagnostic_on') {
                            $events=@(Import-Csv $eventFile);$sample.events=$events
                            $readiness=Get-ProductionReadiness $events $token
                            # Later UI navigations supersede startup, so validate the initial identity segment only.
                            $initial=@();foreach($e in $events){if($e.event -eq 'diagnostic_navigation_started' -and @($initial|Where-Object event -eq diagnostic_navigation_completed).Count){break};$initial+=$e}
                            $readiness=Get-ProductionReadiness $initial $token
                            if($sample.expected_failure -and $readiness.Valid){throw 'Diagnostic failed fixture incorrectly usable'}
                            if(-not $sample.expected_failure -and -not $readiness.Valid){throw "Identity/success readiness invalid: $($readiness.InvalidReasons -join ',')"}
                            $sample.diagnostic_readiness=$readiness
                        }
                    } catch { $sample.valid=$false; $sample.error=$_.Exception.Message }
                    finally {
                        if($app -and -not $app.HasExited){[void]$app.CloseMainWindow();if(-not $app.WaitForExit(5000)){Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue;$sample.valid=$false;$sample.error='Forced close required'}}
                        $sample.disk_after=Disk-Bytes $profile
                        $sample.snapshots=@($snapshots.ToArray());$sample.sampling_overhead_ms=Get-ProductionDistribution @($overheads.ToArray())
                        $sample.cpu=Get-ProductionCpuDelta @($snapshots.ToArray())
                        Write-Host ("Sample {0}: valid={1}; error={2}" -f $id,$sample.valid,$sample.error)
                        $sample | ConvertTo-Json -Depth 30 | Set-Content (Join-Path $Out "$id.json") -Encoding utf8
                        $rows.Add([pscustomobject]$sample);$app=$null
                        if (-not $sample.valid) { $invalidSamples++ }
                    }
                    if ($invalidSamples -ge $MaxInvalidSamples) { throw "Stopped after $invalidSamples invalid samples; raw failures retained for diagnosis" }
                }
            }
        }
    }
    $summary=foreach($group in @($rows|Group-Object arm,fixture,condition)) {
        $first=$group.Group[0];$valid=@($group.Group|Where-Object {$_.valid -and -not $_.expected_failure})
        [ordered]@{arm=$first.arm;fixture=$first.fixture;condition=$first.condition;valid_count=@($group.Group|Where-Object valid).Count;invalid_count=@($group.Group|Where-Object {-not $_.valid}).Count;expected_failure_count=@($group.Group|Where-Object expected_failure).Count;launch_to_fixture_ack_ms=(Get-ProductionDistribution @($valid|ForEach-Object {($_.ready_qpc-$_.launch_qpc)*1000.0/[Diagnostics.Stopwatch]::Frequency})); errors=@($group.Group|Where-Object error|ForEach-Object error)}
    }
    $summary | ConvertTo-Json -Depth 15 | Set-Content (Join-Path $Out 'summary.json') -Encoding utf8
    Assert-MeasurementInputHashesUnchanged $hashes (Get-MeasurementInputHashes @($BaselineExe,$DiagnosticExe,$ReleaseExe)) | Out-Null
    if(@($rows|Where-Object {-not $_.valid}).Count){throw 'Invalid samples retained; baseline is not yet accepted'}
} finally {
    if($server -and -not $server.HasExited){Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue}
    foreach($name in $saved.Keys){[Environment]::SetEnvironmentVariable($name,$saved[$name],'Process')}
}
