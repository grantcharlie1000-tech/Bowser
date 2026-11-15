# Windows PowerShell 5.1 compatible. Importing this module performs no native work.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'

function Read-PairedPinnedJson([string]$Path,[string]$Sha256) {
    if($Sha256 -notmatch '^[0-9a-fA-F]{64}$'){throw 'Explicit SHA256 required'}
    if((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ine $Sha256){throw "Pinned input changed: $Path"}
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function Get-PairedInputIdentity {
    param([string]$PairManifest,[string]$PairManifestSha256,[string]$BaselineExe,[string]$CandidateExe)
    $pair=Read-PairedPinnedJson $PairManifest $PairManifestSha256
    if($pair.schema_version -ne 1){throw 'Unsupported pair manifest schema'}
    $result=[ordered]@{}
    foreach($role in @('baseline','candidate')) {
        $arm=$pair.arms.$role
        $exe=if($role -eq 'baseline'){$BaselineExe}else{$CandidateExe}
        if(-not $exe -or [IO.Path]::GetFullPath($exe) -ine [IO.Path]::GetFullPath($arm.exe)){throw "Executable role mismatch: $role"}
        if((Get-FileHash -LiteralPath $exe).Hash -ine $arm.sha256 -or (Get-Item -LiteralPath $exe).Length -ne $arm.bytes){throw "Executable bytes mismatch: $role"}
        $build=Read-PairedPinnedJson $arm.build_manifest $arm.build_manifest_sha256
        $source=Read-PairedPinnedJson $arm.source_manifest $arm.source_manifest_sha256
        if($build.SHA256 -ine $arm.sha256 -or $build.Bytes -ne $arm.bytes -or [IO.Path]::GetFullPath($build.Executable) -ine [IO.Path]::GetFullPath($exe)){throw "Build manifest executable mismatch: $role"}
        if($build.Features -cne 'html-chrome-default'){throw 'Ordinary HTML-default build required'}
        $commit=if($build.PSObject.Properties['BaseCommit']){$build.BaseCommit}else{$build.Source}
        if($commit -notmatch '^[0-9a-f]{40}$' -or $source.base_commit -cne $commit){throw "Source base mismatch: $role"}
        $files=@($source.files.PSObject.Properties)
        if(-not $files.Count){throw "Incomplete source inputs: $role"}
        foreach($file in $files){if($file.Value -notmatch '^[0-9a-fA-F]{64}$' -or $file.Name -match '(^/|:|\\|(^|/)\.\.(/|$))'){throw 'Invalid source manifest entry'}}
        $provenanceLimit=$null
        if($role -eq 'baseline' -and $source.kind -ceq 'git-committed-inputs') {
            if($source.build_dirty_state -cne 'unrecorded' -or $build.PSObject.Properties['SourceFiles']){throw 'Historical baseline must retain its unrecorded build-time dirty-state limitation'}
            $provenanceLimit='Historical pilot only: committed bytes do not prove the original F0203D9F build-time dirty state.'
        } else {
            Assert-PairedSourceMap $source.files $build.SourceFiles
            if($source.kind -ceq 'recorded-clean-build-snapshot') {
                if($build.Provenance -cne 'observed-clean-build-time-snapshot' -or $build.ExitCode -ne 0 -or $build.ProductUnchanged -ne $true -or $source.build_manifest_sha256 -ine $arm.build_manifest_sha256){throw 'Clean baseline needs the successful contemporaneous build manifest'}
                foreach($phase in @('Before','After')) {
                    $snapshot=Read-PairedPinnedJson $build.('Source'+$phase) $build.('Source'+$phase+'SHA256')
                    if($source.('source_'+$phase.ToLowerInvariant()+'_sha256') -ine $build.('Source'+$phase+'SHA256')){throw 'Source observation pin mismatch'}
                    if($snapshot.head -cne $commit -or $snapshot.git_status -or $snapshot.git_product_status -or @($snapshot.ignored_product).Count -or @($snapshot.untracked_product).Count){throw 'Build-time product inputs were not observed clean'}
                    Assert-PairedSourceMap $source.files $snapshot.SourceFiles
                    if(@($snapshot.product_paths).Count -ne $files.Count -or @(Compare-Object @($snapshot.product_paths) @($files.Name)).Count){throw 'Additional or missing build-time product inputs'}
                }
            } elseif($role -ne 'candidate' -or $source.kind -cne 'frozen-dirty-snapshot') {throw 'Full recorded source provenance required'}
        }
        $result[$role]=[pscustomobject]@{exe=[IO.Path]::GetFullPath($exe);sha256=$arm.sha256;bytes=$arm.bytes;build_manifest_sha256=$arm.build_manifest_sha256;source_manifest_sha256=$arm.source_manifest_sha256;source=$source;provenance_limit=$provenanceLimit}
    }
    if($result.baseline.source.base_commit -cne $result.candidate.source.base_commit){throw 'Mismatched base commits'}
    [pscustomobject]$result
}


function Assert-PairedSourceMap($Expected,$Actual) {
    if($null -eq $Actual){throw 'Full build SourceFiles map required'}
    $entries=@($Expected.PSObject.Properties)
    if(@($Actual.PSObject.Properties).Count -ne $entries.Count){throw 'Source map count mismatch'}
    foreach($entry in $entries){$match=$Actual.PSObject.Properties[$entry.Name];if(-not $match -or $match.Value -ine $entry.Value){throw "Source map mismatch: $($entry.Name)"}}
    foreach($required in @('Cargo.toml','Cargo.lock','build.rs','bowser.manifest','bowser.rc','src/main.rs','ui/dist.manifest')){if(-not $Expected.PSObject.Properties[$required]){throw "Missing product source input: $required"}}
}

function Get-PairedSchedule([ValidateRange(1,30)][int]$Pairs) {
    foreach($pair in 1..$Pairs){
        $roles=if($pair % 2){@('baseline','candidate')}else{@('candidate','baseline')}
        foreach($role in $roles){[pscustomobject]@{pair=$pair;role=$role;condition='fresh'}}
    }
}

function Get-PairedRemainingProcesses {
    param([object[]]$Inventory,[hashtable]$Known,[string]$Profile)
    # PID/creation identity survives root exit and cannot capture a recycled PID.
    $members=@(Get-ProcessGroupMembers -RootPid -1 -Profile $Profile -Inventory $Inventory)
    foreach($live in $Inventory){if($Known.ContainsKey("$($live.Pid):$($live.CreationTicks)")){$members+=@(Get-ProcessGroupMembers -RootPid $live.Pid -Profile $Profile -Inventory $Inventory)}}
    foreach($member in $members){if($null -eq $member.CreationTicks){throw 'Owned process creation identity missing'};$Known["$($member.Pid):$($member.CreationTicks)"]=$true}
    @($Inventory | Where-Object {$Known.ContainsKey("$($_.Pid):$($_.CreationTicks)")})
}

function Assert-PairedReceiptBinding($Receipt,$Sample) {
    if($Receipt.sample_id -cne $Sample.id -or $Receipt.token -cne $Sample.token -or $Receipt.pid -ne $Sample.pid -or $Receipt.start_ticks -ne $Sample.start_ticks -or $Receipt.exe_sha256 -ine $Sample.exe_sha256 -or [IO.Path]::GetFullPath($Receipt.exe) -ine [IO.Path]::GetFullPath($Sample.exe)){throw 'Stale or wrong operator receipt'}
    if($Receipt.driver -cne 'ComputerUse' -or $Receipt.outcome -notin @('observed','failed')){throw 'Computer Use observation receipt required'}
}

function Test-PairedCleanupReceipt($Receipt,$Sample) {
    Assert-PairedReceiptBinding $Receipt $Sample
    if($Receipt.action -cne 'begin_cleanup' -or $Receipt.closed -ne $false){throw 'Bound begin-cleanup receipt required before close'}
    $Receipt
}

function Test-PairedOperatorReceipt($Receipt,$Sample) {
    Assert-PairedReceiptBinding $Receipt $Sample
    if($Receipt.closed -ne $true -or $Receipt.begin_cleanup_sha256 -ine $Sample.begin_cleanup_sha256){throw 'Computer Use close receipt must bind the begin-cleanup receipt'}
    # Observations remain unaccepted. The cleanup boundary is never task latency.
    $Receipt
}

function Assert-PairedLiveRoot($App,$Sample) {
    $App.Refresh()
    if($App.HasExited -or $App.Id -ne $Sample.pid -or $App.StartTime.ToUniversalTime().Ticks -ne $Sample.start_ticks -or [IO.Path]::GetFullPath($App.MainModule.FileName) -ine [IO.Path]::GetFullPath($Sample.exe)){throw 'Held root PID/start/path identity lost'}
}

function Get-PairedProcessInventory {
    # Unlike the legacy reader, an error cannot silently become a partial drain.
    @(Get-CimInstance -ClassName Win32_Process -ErrorAction Stop | ForEach-Object {
        [pscustomobject]@{Pid=[int]$_.ProcessId;Parent=[int]$_.ParentProcessId;Name=[string]$_.Name;CommandLine=[string]$_.CommandLine;CreationTicks=if($_.CreationDate){([datetime]$_.CreationDate).ToUniversalTime().Ticks}else{$null}}
    })
}

function Write-PairedOwnershipObservation($App,$Sample,$State,[string]$SampleDir,[string]$Phase,[switch]$RequireLiveRoot,[switch]$Resource) {
    $row=[ordered]@{phase=$Phase;before_qpc=[Diagnostics.Stopwatch]::GetTimestamp();after_qpc=$null;overhead_ms=$null;inventory_count=$null;tree=@();counters=$null;host_available_mb=$null;host_cpu_percent=$null;error=$null;serialization_and_sleep_in_overhead=$false}
    try {
        if($RequireLiveRoot){Assert-PairedLiveRoot $App $Sample}
        $inventory=@(Get-PairedProcessInventory)
        $row.inventory_count=$inventory.Count
        if(-not $inventory.Count){throw 'Empty inventory is not ownership or exit proof'}
        if($RequireLiveRoot) {
            $root=@($inventory|Where-Object {$_.Pid -eq $Sample.pid -and $null -ne $_.CreationTicks})
            if($root.Count -ne 1){throw 'Launched root inventory identity missing/recycled'}
            if($null -eq $State.RootInventoryTicks){
                if([Math]::Abs([long]$root[0].CreationTicks-$Sample.start_ticks) -gt 10000){throw 'Root creation timestamp mismatch'}
                $State.RootInventoryTicks=$root[0].CreationTicks
            } elseif($root[0].CreationTicks -ne $State.RootInventoryTicks){throw 'Root inventory identity changed'}
            $State.Known["$($Sample.pid):$($State.RootInventoryTicks)"]=$true
            $State.SeenRoot=$true
        }
        $State.Tree=@(Get-PairedRemainingProcesses -Inventory $inventory -Known $State.Known -Profile $Sample.profile)
        $row.tree=@($State.Tree|Select-Object Pid,Parent,Name,CreationTicks)
        if($Resource) {
            $row.counters=Get-ProcessGroupSnapshot -RootPid $Sample.pid -Profile $Sample.profile
            Assert-PairedCounterIdentity $row.counters $Sample.pid $State.RootInventoryTicks
            $row.host_available_mb=(Get-CimInstance Win32_OperatingSystem -ErrorAction Stop).FreePhysicalMemory/1024
            $row.host_cpu_percent=@(Get-CimInstance Win32_Processor -ErrorAction Stop|Select-Object -ExpandProperty LoadPercentage)
        }
        if($RequireLiveRoot){Assert-PairedLiveRoot $App $Sample}
    } catch {$row.error=$_.Exception.Message;throw}
    finally {
        $row.after_qpc=[Diagnostics.Stopwatch]::GetTimestamp()
        $row.overhead_ms=($row.after_qpc-$row.before_qpc)*1000.0/[Diagnostics.Stopwatch]::Frequency
        $row|ConvertTo-Json -Depth 20 -Compress|Add-Content -LiteralPath (Join-Path $SampleDir 'observations.jsonl') -Encoding UTF8
    }
}

function Wait-PairedObservationPhase($App,$Sample,$State,[string]$SampleDir,[string]$MeasurementMode,[int]$OperatorTimeoutSeconds,[int]$SampleMilliseconds,[string]$Url) {
    $clock=[Diagnostics.Stopwatch]::StartNew();$announced=$false
    $beginPath=Join-Path $SampleDir 'begin-cleanup.json'
    while($clock.Elapsed.TotalSeconds -lt $OperatorTimeoutSeconds) {
        $row=[ordered]@{phase='observation_metadata';mode=$MeasurementMode;before_qpc=[Diagnostics.Stopwatch]::GetTimestamp();after_qpc=$null;hwnd=$null;begin_cleanup_received=$false;error=$null}
        try {
        Assert-PairedLiveRoot $App $Sample
        $Sample.hwnd=$App.MainWindowHandle.ToInt64()
        $row.hwnd=$Sample.hwnd
        if(-not $announced -and $Sample.hwnd -ne 0) {
            Write-PairedJson (Join-Path $SampleDir 'ready-for-operator.json') ([ordered]@{state='ready_for_observation_then_begin_cleanup';sample_id=$Sample.id;token=$Sample.token;pid=$Sample.pid;start_ticks=$Sample.start_ticks;exe=$Sample.exe;exe_sha256=$Sample.exe_sha256;hwnd=$Sample.hwnd;profile=$Sample.profile;url=$Url;begin_cleanup=$beginPath;ready_for_close=(Join-Path $SampleDir 'ready-for-close.json');operator_result=(Join-Path $SampleDir 'operator-result.json');deadline_utc=[DateTime]::UtcNow.AddSeconds([Math]::Max(0,$OperatorTimeoutSeconds-$clock.Elapsed.TotalSeconds)).ToString('o');instructions='Computer Use observes/captures usable page and fresh input, then writes bound begin_cleanup receipt while leaving the root alive. Wait for ready-for-close before Computer Use closes it. No task-time acceptance implied.'})
            Write-Host "Ready for Computer Use observation: $SampleDir"
            $announced=$true
        }
        if($announced -and (Test-Path -LiteralPath $beginPath)) {
            # Read bytes once: the retained receipt and its hash identify the same payload.
            $bytes=[IO.File]::ReadAllBytes($beginPath)
            $cleanup=Test-PairedCleanupReceipt ([Text.Encoding]::UTF8.GetString($bytes).TrimStart([char]0xfeff)|ConvertFrom-Json) ([pscustomobject]$Sample)
            $Sample['cleanup']=$cleanup
            $digest=[Security.Cryptography.SHA256]::Create()
            try {$Sample['begin_cleanup_sha256']=([BitConverter]::ToString($digest.ComputeHash($bytes))).Replace('-','').ToLowerInvariant()}finally{$digest.Dispose()}
            $Sample['observation_end_received_qpc']=[Diagnostics.Stopwatch]::GetTimestamp()
            $row.begin_cleanup_received=$true
            return
        }
        } catch {$row.error=$_.Exception.Message;throw}
        finally {
            $row.after_qpc=[Diagnostics.Stopwatch]::GetTimestamp()
            $row|ConvertTo-Json -Depth 8 -Compress|Add-Content -LiteralPath (Join-Path $SampleDir 'observations.jsonl') -Encoding UTF8
        }
        # Timing has no inventories, counters or host queries before begin_cleanup.
        if($MeasurementMode -eq 'resource'){Write-PairedOwnershipObservation $App $Sample $State $SampleDir 'resource_observation' -RequireLiveRoot -Resource}
        Start-Sleep -Milliseconds $(if($MeasurementMode -eq 'resource'){$SampleMilliseconds}else{500})
    }
    throw 'Observation deadline: app left untouched; no subsequent launch'
}

function Assert-PairedCounterIdentity($Snapshot,[int]$RootPid,[long]$CreationTicks) {
    $roots=@($Snapshot.ProcessDetails|Where-Object {$_.role -ceq 'root' -and $_.process_id -eq $RootPid})
    if($roots.Count -ne 1 -or $roots[0].created_ticks -ne $CreationTicks){throw 'Counter snapshot lost the bound root identity'}
}

function Write-PairedJson([string]$Path,$Value) {
    $json=$Value | ConvertTo-Json -Depth 35
    $temp=$Path+'.tmp'
    [IO.File]::WriteAllText($temp,$json,(New-Object Text.UTF8Encoding($false)))
    Move-Item -LiteralPath $temp -Destination $Path -Force
}

function Invoke-PairedStartup {
    [CmdletBinding()]
    param([string]$BaselineExe,[string]$CandidateExe,[string]$PairManifest,[string]$PairManifestSha256,
        [Parameter(Mandatory)][string]$Out,[ValidateRange(1,30)][int]$Pairs=1,
        [ValidateSet('timing','resource')][string]$MeasurementMode='timing',
        [ValidateRange(100,2000)][int]$SampleMilliseconds=250,[ValidateRange(30,600)][int]$OperatorTimeoutSeconds=180)
    if([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT){throw 'Native Windows required'}
    $identity=Get-PairedInputIdentity $PairManifest $PairManifestSha256 $BaselineExe $CandidateExe
    $Out=[IO.Path]::GetFullPath($Out)
    if(Test-Path -LiteralPath $Out){throw 'Output must be new: never overwrite attempts or reuse partial profiles'}
    $block=Join-Path (Split-Path $Out) 'paired-isolation-blocked.json'
    if(Test-Path -LiteralPath $block){throw "Prior tree exit unproven; Computer Use cleanup/review required: $block"}
    foreach($entry in Get-ChildItem Env:){if($entry.Name -like 'BOWSER_*' -or $entry.Name -like 'WEBVIEW2_*'){throw "Inherited browser override forbidden: $($entry.Name)"}}
    Import-Module "$PSScriptRoot/measurements.psm1" -Force
    Import-Module "$PSScriptRoot/production-measurements.psm1" -Force
    New-Item -ItemType Directory -Path $Out | Out-Null
    $runStatus=[ordered]@{state='starting';started_utc=[DateTime]::UtcNow.ToString('o');sample=$null;error=$null;accepted=$false}
    Write-PairedJson (Join-Path $Out 'run-status.json') $runStatus
    $locks=New-Object 'Collections.Generic.List[object]';$server=$null
    try {
        foreach($role in @('baseline','candidate')){$locks.Add([IO.File]::Open($identity.$role.exe,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read))}
        $portFile=Join-Path $Out 'port.txt';$requests=Join-Path $Out 'requests.jsonl'
        $serverStart=$null
        $server=Start-Process -FilePath node -ArgumentList @("`"$PSScriptRoot/scripts/production-fixture-server.mjs`"","`"$portFile`"","`"$requests`"") -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $Out 'server-stderr.txt')
        $serverStart=$server.StartTime.ToUniversalTime().Ticks
        $wait=[Diagnostics.Stopwatch]::StartNew()
        while(-not(Test-Path -LiteralPath $portFile)){if($server.HasExited -or $wait.Elapsed.TotalSeconds -ge 10){throw 'Fixture server failed'};Start-Sleep -Milliseconds 100}
        $port=[int](Get-Content -LiteralPath $portFile -Raw)
        $harness=@{};foreach($name in @('production-baseline.ps1','production-paired-startup.psm1','production-measurements.psm1','measurements.psm1','scripts/production-fixture-server.mjs','scripts/filter-parity-fixture.mjs','scripts/conversation-fixture.mjs','scripts/conversation-fixture.html','scripts/conversation-client.mjs','scripts/conversation-model.mjs')){$harness[$name]=(Get-FileHash -LiteralPath (Join-Path $PSScriptRoot $name)).Hash}
        Write-PairedJson (Join-Path $Out 'environment.json') ([ordered]@{schema_version=1;mode=$MeasurementMode;arms=$identity;pair_manifest_sha256=$PairManifestSha256;harness=$harness;qpc_frequency=[Diagnostics.Stopwatch]::Frequency;schedule=@(Get-PairedSchedule $Pairs);powershell=$PSVersionTable.PSVersion.ToString();os=(Get-CimInstance Win32_OperatingSystem|Select-Object Caption,Version,BuildNumber);cpu=(Get-CimInstance Win32_Processor|Select-Object Name,NumberOfLogicalProcessors);gpu=(Get-CimInstance Win32_VideoController|Select-Object Name,DriverVersion,CurrentHorizontalResolution,CurrentVerticalResolution);power=(& powercfg /getactivescheme|Out-String);webview2=@(Get-ChildItem 'C:/Program Files (x86)/Microsoft/EdgeWebView/Application' -Directory -ErrorAction SilentlyContinue|Select-Object -ExpandProperty Name);profile='fresh empty isolated roots; no seed; ordinary updater/security defaults';cache='fresh profile is not cold OS cache';window='no runner geometry/foreground changes; parent records actual DPI/bounds/power/display conditions';acceptance='unmeasured; <=0.50 per area unchanged';baseline_provenance=$identity.baseline.provenance_limit;resource_scope='sampled process tree; shared working pages double-count; short-lived processes may be missed; no WMI proxy substitution as native RAM';overhead='Timing observation uses held-root metadata and receipt polling only. Inventory/counters/host queries are excluded until bound begin_cleanup. Resource collection is separate. No overhead subtraction; observation/cleanup/operator delays are never task latency'})
        foreach($job in @(Get-PairedSchedule $Pairs)) {
            $arm=$identity.($job.role);$token=[guid]::NewGuid().ToString('N');$id="p$($job.pair)-$($job.role)-$token"
            $sampleDir=Join-Path $Out $id;New-Item -ItemType Directory -Path $sampleDir|Out-Null
            $runStatus.state='sampling';$runStatus.sample=$sampleDir
            Write-PairedJson (Join-Path $Out 'run-status.json') $runStatus
            $profile=Join-Path $sampleDir 'data';$local=Join-Path $sampleDir 'local-app-data'
            New-Item -ItemType Directory -Path $profile,$local|Out-Null
            $events=Join-Path $sampleDir 'events.csv';$receipt=Join-Path $sampleDir 'operator-result.json'
            $sample=[ordered]@{id=$id;pair=$job.pair;role=$job.role;token=$token;mode=$MeasurementMode;condition='fresh';exe=$arm.exe;exe_sha256=$arm.sha256;source_manifest_sha256=$arm.source_manifest_sha256;pid=$null;start_ticks=$null;hwnd=$null;profile=$profile;local_app_data=$local;launch_before_qpc=$null;launch_after_qpc=$null;raw=@();proxy=$null;presentation=$null;accepted_task_ms=$null;accepted=$false;operator=$null;tree_exit_confirmed=$false;exit_code=$null;error=$null}
            $sampleFile=Join-Path $sampleDir 'sample.json';$app=$null;$state=@{Known=@{};Tree=@();SeenRoot=$false;RootInventoryTicks=$null}
            Write-PairedJson $sampleFile $sample
            try {
                $null=Get-PairedInputIdentity $PairManifest $PairManifestSha256 $BaselineExe $CandidateExe
                $info=New-Object Diagnostics.ProcessStartInfo
                $info.FileName=$arm.exe;$info.UseShellExecute=$false;$info.WorkingDirectory=Split-Path $arm.exe
                $info.EnvironmentVariables['LOCALAPPDATA']=$local
                $info.Arguments="--data-dir `"$profile`" --open-url `"http://127.0.0.1:$port/light?token=$token`" --diagnostic-log `"$events`""
                $sample['arguments']=$info.Arguments
                $sample.launch_before_qpc=[Diagnostics.Stopwatch]::GetTimestamp()
                $app=[Diagnostics.Process]::Start($info)
                $sample.launch_after_qpc=[Diagnostics.Stopwatch]::GetTimestamp()
                $sample.raw=[ordered]@{launch_before_qpc=$sample.launch_before_qpc;launch_after_qpc=$sample.launch_after_qpc;observations_file=(Join-Path $sampleDir 'observations.jsonl');diagnostic_events_file=$events;server_requests_file=$requests}
                $sample.pid=$app.Id;$sample.start_ticks=$app.StartTime.ToUniversalTime().Ticks
                $null=$app.Handle # Hold the launched process object/handle across root exit.
                Assert-PairedLiveRoot $app $sample
                $state.Known["$($app.Id):$($sample.start_ticks)"]=$true
                Write-PairedJson $sampleFile $sample
                Wait-PairedObservationPhase $app $sample $state $sampleDir $MeasurementMode $OperatorTimeoutSeconds $SampleMilliseconds "http://127.0.0.1:$port/light?token=$token"
                Write-PairedJson $sampleFile $sample
                # Capture ownership while the held root is alive, after the timing boundary.
                Write-PairedOwnershipObservation $app $sample $state $sampleDir 'cleanup_ownership' -RequireLiveRoot
                Write-PairedJson (Join-Path $sampleDir 'ready-for-close.json') ([ordered]@{state='ready_for_computer_use_close';sample_id=$id;token=$token;pid=$sample.pid;start_ticks=$sample.start_ticks;exe=$arm.exe;exe_sha256=$arm.sha256;begin_cleanup_sha256=$sample.begin_cleanup_sha256;ownership_captured_qpc=[Diagnostics.Stopwatch]::GetTimestamp();operator_result=$receipt;instructions='Ownership captured outside observation. Verify bound window again, close via Computer Use, then write bound close receipt. Cleanup delay is not task latency.'})
                Write-Host "Ready for Computer Use close: $sampleDir"
                $closeWait=[Diagnostics.Stopwatch]::StartNew()
                while(-not $app.HasExited -and $closeWait.Elapsed.TotalSeconds -lt $OperatorTimeoutSeconds) {
                    Write-PairedOwnershipObservation $app $sample $state $sampleDir 'cleanup_awaiting_close'
                    Start-Sleep -Milliseconds 250
                    $app.Refresh()
                }
                if(-not $app.HasExited){throw 'Close deadline: process left untouched; stop all subsequent launches'}
                $sample.exit_code=$app.ExitCode
                $drain=[Diagnostics.Stopwatch]::StartNew();$emptyPasses=0
                do {
                    Write-PairedOwnershipObservation $app $sample $state $sampleDir 'cleanup_drain'
                    if($state.Tree.Count -eq 0){$emptyPasses++}else{$emptyPasses=0}
                    if($emptyPasses -ge 2){break};Start-Sleep -Milliseconds 250
                } while($drain.Elapsed.TotalSeconds -lt 20)
                $sample.tree_exit_confirmed=($state.SeenRoot -and $emptyPasses -ge 2)
                $sample['drain_remaining']=@($state.Tree|Select-Object Pid,Parent,Name,CreationTicks)
                if(-not $sample.tree_exit_confirmed){throw 'Whole owned/profile process tree exit unproven'}
                $receiptWait=[Diagnostics.Stopwatch]::StartNew()
                while(-not(Test-Path -LiteralPath $receipt) -and $receiptWait.Elapsed.TotalSeconds -lt 30){Start-Sleep -Milliseconds 250}
                $sample.operator=Test-PairedOperatorReceipt (Get-Content -LiteralPath $receipt -Raw|ConvertFrom-Json) ([pscustomobject]$sample)
                if(Test-Path -LiteralPath $events){$sample.proxy=Get-ProductionReadiness @(Import-Csv -LiteralPath $events) $token}
                $null=Get-PairedInputIdentity $PairManifest $PairManifestSha256 $BaselineExe $CandidateExe
                if($sample.exit_code -ne 0 -or $sample.operator.outcome -ne 'observed' -or $sample.cleanup.outcome -ne 'observed'){throw 'Failed process/operator outcome retained'}
            } catch {$sample.error=$_.Exception.Message;$sample['error_stack']=$_.ScriptStackTrace}
            finally {
                Write-PairedJson $sampleFile $sample
                if(-not $sample.tree_exit_confirmed){Write-PairedJson $block ([ordered]@{sample=$sampleFile;pid=$sample.pid;start_ticks=$sample.start_ticks;profile=$profile;reason=$sample.error;action='Computer Use cleanup only; no automatic close/kill; verify full tree before removing block'})}
            }
            if($sample.error){throw "Attempt retained at $sampleFile : $($sample.error)"}
        }
        $runStatus.state='completed_pilot_unaccepted'
    } catch {
        $runStatus.state='failed';$runStatus.error=$_.Exception.Message
        throw
    } finally {
        Write-PairedJson (Join-Path $Out 'run-status.json') $runStatus
        if($server -and -not $server.HasExited -and $server.StartTime.ToUniversalTime().Ticks -eq $serverStart){Stop-Process -Id $server.Id -Force}
        foreach($lock in $locks){$lock.Dispose()}
    }
}
Export-ModuleMember -Function Get-PairedInputIdentity,Get-PairedSchedule,Get-PairedRemainingProcesses,Test-PairedCleanupReceipt,Test-PairedOperatorReceipt,Assert-PairedCounterIdentity,Wait-PairedObservationPhase,Invoke-PairedStartup
