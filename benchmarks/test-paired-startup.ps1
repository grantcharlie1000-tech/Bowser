# Pure contracts only: no application, native API, CIM query, input or capture.
param([Parameter(Mandatory)][string]$PairManifest,[Parameter(Mandatory)][string]$PairManifestSha256)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/production-paired-startup.psm1" -Force
$script:checks=0
function Check($condition,[string]$name){if(-not $condition){throw "FAILED: $name"};$script:checks++}
function Reject([scriptblock]$call,[string]$name){$rejected=$false;try{& $call|Out-Null}catch{$rejected=$true};Check $rejected $name}
$pair=Get-Content -LiteralPath $PairManifest -Raw|ConvertFrom-Json
$baseline=$pair.arms.baseline.exe;$candidate=$pair.arms.candidate.exe
$identity=Get-PairedInputIdentity $PairManifest $PairManifestSha256 $baseline $candidate
Check ($identity.baseline.sha256 -ieq $pair.arms.baseline.sha256) 'pinned baseline role'
Check ($identity.candidate.sha256 -ieq $pair.arms.candidate.sha256) 'pinned candidate role'
$candidateBuild=Get-Content -LiteralPath $pair.arms.candidate.build_manifest -Raw|ConvertFrom-Json
Check (@($identity.candidate.source.files.PSObject.Properties).Count -eq @($candidateBuild.SourceFiles.PSObject.Properties).Count) 'complete candidate map matched without a fixed count'
if($identity.baseline.source.kind -ceq 'recorded-clean-build-snapshot') {
    Check ($null -eq $identity.baseline.provenance_limit) 'new recorded baseline bound'
} else {
    Check ($identity.baseline.source.build_dirty_state -ceq 'unrecorded' -and $identity.baseline.provenance_limit) 'historical baseline provenance limit retained'
}
Reject {Get-PairedInputIdentity $PairManifest ('0'*64) $baseline $candidate} 'pair mutation rejected'
Reject {Get-PairedInputIdentity $PairManifest $PairManifestSha256 $candidate $baseline} 'swapped roles rejected'
$schedule=@(Get-PairedSchedule 2)
Check (($schedule.role -join ',') -ceq 'baseline,candidate,candidate,baseline') 'AB then BA'
Check (@($schedule|Where-Object condition -ne fresh).Count -eq 0) 'all initial-pilot profiles fresh'
$sample=[pscustomobject]@{id='p1';token='nonce';pid=20;start_ticks=100;exe_sha256='a'*64;exe='C:\test\bowser.exe';begin_cleanup_sha256='b'*64}
$receipt=[pscustomobject]@{sample_id='p1';token='nonce';pid=20;start_ticks=100;exe_sha256='a'*64;exe='C:\test\bowser.exe';begin_cleanup_sha256='b'*64;driver='ComputerUse';closed=$true;outcome='observed'}
Check ((Test-PairedOperatorReceipt $receipt $sample).outcome -ceq 'observed') 'bound CUA receipt returned'
foreach($field in @('sample_id','token','pid','start_ticks','exe','exe_sha256','begin_cleanup_sha256','driver','closed','outcome')) {
    $bad=$receipt|ConvertTo-Json|ConvertFrom-Json
    $bad.$field=switch($field){pid {21};start_ticks {101};closed {$false};default {'wrong'}}
    Reject {Test-PairedOperatorReceipt $bad $sample} "receipt rejects $field"
}

$temp=Join-Path ([IO.Path]::GetTempPath()) ('bowser-paired-contract-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp|Out-Null
# Keep these tiny negative-test inputs; no product/profile data is written.
$source=Get-Content -LiteralPath $pair.arms.candidate.source_manifest -Raw|ConvertFrom-Json
$source.files.PSObject.Properties | Select-Object -First 1 | ForEach-Object {$_.Value='0'*64}
$sourcePath=Join-Path $temp 'source.json';$source|ConvertTo-Json -Depth 10|Set-Content $sourcePath -Encoding UTF8
$badPair=Get-Content -LiteralPath $PairManifest -Raw|ConvertFrom-Json
$badPair.arms.candidate.source_manifest=$sourcePath
$badPair.arms.candidate.source_manifest_sha256=(Get-FileHash $sourcePath).Hash
$badPairPath=Join-Path $temp 'pair.json';$badPair|ConvertTo-Json -Depth 10|Set-Content $badPairPath -Encoding UTF8
Reject {Get-PairedInputIdentity $badPairPath (Get-FileHash $badPairPath).Hash $baseline $candidate} 're-pinned wrong dirty bytes rejected by build manifest'
$source.kind='commit-only';$source|ConvertTo-Json -Depth 10|Set-Content $sourcePath -Encoding UTF8
$badPair.arms.candidate.source_manifest_sha256=(Get-FileHash $sourcePath).Hash
$badPair|ConvertTo-Json -Depth 10|Set-Content $badPairPath -Encoding UTF8
Reject {Get-PairedInputIdentity $badPairPath (Get-FileHash $badPairPath).Hash $baseline $candidate} 'commit-only dirty identity rejected'

# Load the real pure inventory/counter functions without measurements.psm1's
# import-time Add-Type. Supplying Inventory + NativeMemoryReader prevents OS work.
$parseErrors=$null;$tokens=$null
$ast=[Management.Automation.Language.Parser]::ParseFile("$PSScriptRoot/measurements.psm1",[ref]$tokens,[ref]$parseErrors)
Check (@($parseErrors).Count -eq 0) 'counter dependency parses in PS5.1'
$definitions=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -in @('Get-ProcessGroupMembers','Get-ProcessGroupSnapshot')},$true)|ForEach-Object {$_.Extent.Text})
New-Module -Name PurePairCounters -ScriptBlock ([scriptblock]::Create(($definitions -join "`n")+"`nExport-ModuleMember -Function Get-ProcessGroupMembers,Get-ProcessGroupSnapshot")) | Import-Module -Force
function ProcessRow($id,$parent,$created,$command){[pscustomobject]@{Pid=$id;Parent=$parent;CreationTicks=$created;CommandLine=$command;Name='test';WorkingBytes=100;WmiProxyBytes=100;CpuSeconds=1}}
$inventory=@((ProcessRow 0 0 1 ''),(ProcessRow 4 0 2 'System'),(ProcessRow 20 4 100 '--data-dir=C:\owned'),(ProcessRow 21 20 101 '--type=renderer'),(ProcessRow 22 21 102 '--type=utility'),(ProcessRow 30 4 200 'unrelated'))
$known=@{'20:100'=$true}
$remaining=@(Get-PairedRemainingProcesses $inventory $known 'C:\owned')
Check (($remaining.Pid -join ',') -ceq '20,21,22') 'entire tree excludes system/unrelated'
$afterExit=@((ProcessRow 21 999 101 '--type=renderer'),(ProcessRow 22 21 102 '--type=utility'),(ProcessRow 23 22 103 'late child'),(ProcessRow 20 4 300 'recycled'),(ProcessRow 30 20 301 'recycled child'))
$remaining=@(Get-PairedRemainingProcesses $afterExit $known 'C:\owned')
Check (($remaining.Pid -join ',') -ceq '21,22,23') 'remembered reparented helpers seed later children; recycled PID excluded'
Check (@(Get-PairedRemainingProcesses @((ProcessRow 20 4 300 'recycled')) $known 'C:\owned').Count -eq 0) 'recycled PID does not block drain'
$counter=Get-ProcessGroupSnapshot -RootPid 20 -Profile 'C:\owned' -Inventory $inventory -NativeMemoryReader {param($id) [pscustomobject]@{Source='native';WorkingSetBytes=($id*10);PrivateBytes=($id*100);PageFaultCount=1;Error=$null}}
Check ($counter.NativePrivateBytes -eq 6300 -and $counter.ProcessCount -eq 3) 'RAM includes root renderer utility'
Check ($counter.NativeWorkingBytes -eq 630 -and $counter.NativePageFaultCount -eq 3) 'tree working/fault counters'
Assert-PairedCounterIdentity $counter 20 100
Check $true 'snapshot remains bound to launched root'
Reject {Assert-PairedCounterIdentity $counter 20 300} 'snapshot for recycled root rejected'
Reject {Assert-PairedCounterIdentity ([pscustomobject]@{ProcessDetails=@()}) 20 100} 'snapshot missing root rejected'
$partial=Get-ProcessGroupSnapshot -RootPid 20 -Profile 'C:\owned' -Inventory $inventory -NativeMemoryReader {param($id) if($id -eq 22){[pscustomobject]@{Source='unavailable';Error='denied'}}else{[pscustomobject]@{Source='native';WorkingSetBytes=1;PrivateBytes=2;PageFaultCount=1;Error=$null}}}
Check ($null -eq $partial.NativePrivateBytes -and $partial.PrivateSource -ceq 'wmi_pagefileusage_proxy') 'missing native counter stays unavailable; proxy labelled'

Reject {& "$PSScriptRoot/production-baseline.ps1" -ExternalUiDriver -StartupOnly -PositiveOnly -Fixtures light -Out (Join-Path $temp 'never-launch') -DiagnosticAutoUpdateOff} 'mixed legacy control rejected before native import'
Reject {& "$PSScriptRoot/production-baseline.ps1" -Out (Join-Path $temp 'never-launch')} 'missing legacy executables fail without prompt'
foreach($name in @('production-baseline.ps1','production-paired-startup.psm1','production-measurements.psm1')) {
    $errors=$null;$tokens=$null
    $null=[Management.Automation.Language.Parser]::ParseFile("$PSScriptRoot/$name",[ref]$tokens,[ref]$errors)
    Check (@($errors).Count -eq 0) "PS5.1 parser: $name"
}

# A matching synthetic extension is accepted: provenance map sizes are not code constants.
$futureBuild=$candidateBuild|ConvertTo-Json -Depth 20|ConvertFrom-Json
$futureSource=Get-Content -LiteralPath $pair.arms.candidate.source_manifest -Raw|ConvertFrom-Json
$futureBuild.SourceFiles|Add-Member -NotePropertyName 'src/future-contract.rs' -NotePropertyValue ('c'*64)
$futureSource.files|Add-Member -NotePropertyName 'src/future-contract.rs' -NotePropertyValue ('c'*64)
$futureBuildPath=Join-Path $temp 'future-build.json';$futureSourcePath=Join-Path $temp 'future-source.json'
$futureBuild|ConvertTo-Json -Depth 20|Set-Content -LiteralPath $futureBuildPath -Encoding UTF8
$futureSource|ConvertTo-Json -Depth 20|Set-Content -LiteralPath $futureSourcePath -Encoding UTF8
$futurePair=Get-Content -LiteralPath $PairManifest -Raw|ConvertFrom-Json
$futurePair.arms.candidate.build_manifest=$futureBuildPath;$futurePair.arms.candidate.build_manifest_sha256=(Get-FileHash $futureBuildPath).Hash
$futurePair.arms.candidate.source_manifest=$futureSourcePath;$futurePair.arms.candidate.source_manifest_sha256=(Get-FileHash $futureSourcePath).Hash
$futurePairPath=Join-Path $temp 'future-pair.json';$futurePair|ConvertTo-Json -Depth 20|Set-Content -LiteralPath $futurePairPath -Encoding UTF8
$futureIdentity=Get-PairedInputIdentity $futurePairPath (Get-FileHash $futurePairPath).Hash $baseline $candidate
Check (@($futureIdentity.candidate.source.files.PSObject.Properties).Count -eq @($identity.candidate.source.files.PSObject.Properties).Count+1) 'matching synthetic extended source/build maps accepted'
$futureSource.files.PSObject.Properties.Remove('src/future-contract.rs')
$futureSource|ConvertTo-Json -Depth 20|Set-Content -LiteralPath $futureSourcePath -Encoding UTF8
$futurePair.arms.candidate.source_manifest_sha256=(Get-FileHash $futureSourcePath).Hash
$futurePair|ConvertTo-Json -Depth 20|Set-Content -LiteralPath $futurePairPath -Encoding UTF8
Reject {Get-PairedInputIdentity $futurePairPath (Get-FileHash $futurePairPath).Hash $baseline $candidate} 'unmatched full map rejected'

if($identity.baseline.source.kind -ceq 'recorded-clean-build-snapshot') {
    $originalBuild=Get-Content -LiteralPath $pair.arms.baseline.build_manifest -Raw|ConvertFrom-Json
    $cleanBuild=$originalBuild|ConvertTo-Json -Depth 25|ConvertFrom-Json
    $cleanSource=Get-Content -LiteralPath $pair.arms.baseline.source_manifest -Raw|ConvertFrom-Json
    $snapshot=Get-Content -LiteralPath $cleanBuild.SourceBefore -Raw|ConvertFrom-Json
    $snapshot.git_product_status=' M src/main.rs'
    $snapshotPath=Join-Path $temp 'dirty-observation.json';$snapshot|ConvertTo-Json -Depth 25|Set-Content -LiteralPath $snapshotPath -Encoding UTF8
    $cleanBuild.SourceBefore=$snapshotPath;$cleanBuild.SourceBeforeSHA256=(Get-FileHash $snapshotPath).Hash
    $cleanBuildPath=Join-Path $temp 'clean-build.json';$cleanBuild|ConvertTo-Json -Depth 25|Set-Content -LiteralPath $cleanBuildPath -Encoding UTF8
    $cleanSource.source_before_sha256=$cleanBuild.SourceBeforeSHA256;$cleanSource.build_manifest_sha256=(Get-FileHash $cleanBuildPath).Hash
    $cleanSourcePath=Join-Path $temp 'clean-source.json';$cleanSource|ConvertTo-Json -Depth 25|Set-Content -LiteralPath $cleanSourcePath -Encoding UTF8
    $cleanPair=Get-Content -LiteralPath $PairManifest -Raw|ConvertFrom-Json
    $cleanPair.arms.baseline.build_manifest=$cleanBuildPath;$cleanPair.arms.baseline.build_manifest_sha256=(Get-FileHash $cleanBuildPath).Hash
    $cleanPair.arms.baseline.source_manifest=$cleanSourcePath;$cleanPair.arms.baseline.source_manifest_sha256=(Get-FileHash $cleanSourcePath).Hash
    $cleanPairPath=Join-Path $temp 'clean-pair.json';$cleanPair|ConvertTo-Json -Depth 25|Set-Content -LiteralPath $cleanPairPath -Encoding UTF8
    Reject {Get-PairedInputIdentity $cleanPairPath (Get-FileHash $cleanPairPath).Hash $baseline $candidate} 're-pinned dirty build-time observation cannot be relabelled clean'
    $cleanBuild=$originalBuild|ConvertTo-Json -Depth 25|ConvertFrom-Json
    $cleanBuild.ProductUnchanged=$false
    $cleanBuild|ConvertTo-Json -Depth 25|Set-Content -LiteralPath $cleanBuildPath -Encoding UTF8
    $cleanSource.source_before_sha256=$cleanBuild.SourceBeforeSHA256;$cleanSource.build_manifest_sha256=(Get-FileHash $cleanBuildPath).Hash
    $cleanSource|ConvertTo-Json -Depth 25|Set-Content -LiteralPath $cleanSourcePath -Encoding UTF8
    $cleanPair.arms.baseline.build_manifest_sha256=(Get-FileHash $cleanBuildPath).Hash;$cleanPair.arms.baseline.source_manifest_sha256=(Get-FileHash $cleanSourcePath).Hash
    $cleanPair|ConvertTo-Json -Depth 25|Set-Content -LiteralPath $cleanPairPath -Encoding UTF8
    Reject {Get-PairedInputIdentity $cleanPairPath (Get-FileHash $cleanPairPath).Hash $baseline $candidate} 'changed product during build rejected'
}

# Drive the real observation phase with a fake held-process interface. Any heavy
# observer call fails the test. Start-Sleep simulates one poll and a parent receipt.
$phaseDir=Join-Path $temp 'timing-phase';New-Item -ItemType Directory -Path $phaseDir|Out-Null
$start=[DateTime]::UtcNow
$phaseSample=[ordered]@{id='phase';token='phase_nonce';pid=20;start_ticks=$start.Ticks;exe='C:\test\bowser.exe';exe_sha256='a'*64;profile='C:\owned';hwnd=$null}
$fakeApp=[pscustomobject]@{Id=20;HasExited=$false;StartTime=$start;MainModule=[pscustomobject]@{FileName='C:\test\bowser.exe'};MainWindowHandle=[IntPtr]123}
$fakeApp|Add-Member -MemberType ScriptMethod -Name Refresh -Value {}
$module=Get-Module production-paired-startup
$ownershipDir=Join-Path $temp 'ownership';New-Item -ItemType Directory -Path $ownershipDir|Out-Null
$state=@{Known=@{};Tree=@();SeenRoot=$false;RootInventoryTicks=$null}
$ownedInventory=@((ProcessRow 4 0 1 'System'),(ProcessRow 20 4 $start.Ticks '--data-dir=C:\owned'),(ProcessRow 21 20 ($start.Ticks+1) 'renderer'),(ProcessRow 22 21 ($start.Ticks+2) 'utility'),(ProcessRow 30 4 2 'unrelated'))
& $module {
    param($inventory)
    $script:ContractInventory=$inventory;$script:ContractQueryError=$false
    function script:Get-PairedProcessInventory {if($script:ContractQueryError){throw 'simulated inventory denial'};@($script:ContractInventory)}
    function script:Get-CimInstance {throw 'Unexpected host query in cleanup ownership'}
    function script:Get-ProcessGroupSnapshot {throw 'Unexpected counters in cleanup ownership'}
} $ownedInventory
& $module {param($app,$sample,$state,$dir) Write-PairedOwnershipObservation $app $sample $state $dir 'cleanup_ownership' -RequireLiveRoot} $fakeApp $phaseSample $state $ownershipDir
Check ($state.SeenRoot -and ($state.Tree.Pid -join ',') -ceq '20,21,22') 'cleanup captures root renderer utility before close'
$ownershipRows=@(Get-Content -LiteralPath (Join-Path $ownershipDir 'observations.jsonl')|ConvertFrom-Json)
Check ($ownershipRows.Count -eq 1 -and $ownershipRows[0].phase -ceq 'cleanup_ownership' -and $null -eq $ownershipRows[0].counters -and $null -eq $ownershipRows[0].host_cpu_percent) 'cleanup retains ownership observation without resource/host probes'
$survivors=@((ProcessRow 21 999 ($start.Ticks+1) 'renderer'),(ProcessRow 22 21 ($start.Ticks+2) 'utility'),(ProcessRow 23 22 ($start.Ticks+3) 'late child'),(ProcessRow 20 4 ($start.Ticks+30000) 'recycled'),(ProcessRow 30 20 ($start.Ticks+30001) 'unrelated recycled child'))
& $module {param($inventory) $script:ContractInventory=$inventory} $survivors
$fakeApp.HasExited=$true
& $module {param($app,$sample,$state,$dir) Write-PairedOwnershipObservation $app $sample $state $dir 'cleanup_drain'} $fakeApp $phaseSample $state $ownershipDir
Check (($state.Tree.Pid -join ',') -ceq '21,22,23') 'cleanup after root exit follows remembered surviving helpers and new descendants'
$fakeApp.HasExited=$false
Reject {& $module {param($app,$sample,$state,$dir) Write-PairedOwnershipObservation $app $sample $state $dir 'cleanup_ownership' -RequireLiveRoot} $fakeApp $phaseSample $state $ownershipDir} 'recycled inventory root cannot authorize close'
& $module {$script:ContractInventory=@()}
Reject {& $module {param($app,$sample,$state,$dir) Write-PairedOwnershipObservation $app $sample $state $dir 'cleanup_drain'} $fakeApp $phaseSample $state $ownershipDir} 'empty inventory cannot prove drain'
& $module {$script:ContractQueryError=$true}
Reject {& $module {param($app,$sample,$state,$dir) Write-PairedOwnershipObservation $app $sample $state $dir 'cleanup_drain'} $fakeApp $phaseSample $state $ownershipDir} 'failed inventory cannot prove drain'
$ownershipRows=@(Get-Content -LiteralPath (Join-Path $ownershipDir 'observations.jsonl')|ConvertFrom-Json)
Check ($ownershipRows.Count -eq 5 -and @($ownershipRows|Where-Object error).Count -eq 3) 'all failed cleanup observations remain in raw evidence'
$begin=[pscustomobject]@{sample_id='phase';token='phase_nonce';pid=20;start_ticks=$start.Ticks;exe='C:\test\bowser.exe';exe_sha256='a'*64;driver='ComputerUse';closed=$false;outcome='observed';action='begin_cleanup'}
Check ((Test-PairedCleanupReceipt $begin ([pscustomobject]$phaseSample)).action -ceq 'begin_cleanup') 'bound begin-cleanup receipt accepted'
$badBegin=$begin|ConvertTo-Json|ConvertFrom-Json;$badBegin.closed=$true
Reject {Test-PairedCleanupReceipt $badBegin ([pscustomobject]$phaseSample)} 'premature close receipt cannot end timing observation'
$badBegin=$begin|ConvertTo-Json|ConvertFrom-Json;$badBegin.token='stale'
Reject {Test-PairedCleanupReceipt $badBegin ([pscustomobject]$phaseSample)} 'stale begin-cleanup receipt rejected'
$module=Get-Module production-paired-startup
& $module {
    param($beginPath,$payload)
    $script:ContractBeginPath=$beginPath;$script:ContractPayload=$payload
    function script:Start-Sleep {param($Milliseconds) Write-PairedJson $script:ContractBeginPath $script:ContractPayload}
    function script:Write-PairedOwnershipObservation {throw 'Forbidden expensive observer called'}
    function script:Get-CimInstance {throw 'Forbidden CIM called'}
    function script:Get-ProcessInventory {throw 'Forbidden inventory called'}
    function script:Get-ProcessGroupSnapshot {throw 'Forbidden counter called'}
} (Join-Path $phaseDir 'begin-cleanup.json') $begin
Wait-PairedObservationPhase $fakeApp $phaseSample @{} $phaseDir 'timing' 30 250 'http://127.0.0.1/light?token=phase_nonce'
Check ($phaseSample.cleanup.outcome -ceq 'observed' -and $phaseSample.observation_end_received_qpc -gt 0) 'timing observation ends only after matching begin receipt'
Check ($phaseSample.begin_cleanup_sha256 -ieq (Get-FileHash (Join-Path $phaseDir 'begin-cleanup.json')).Hash) 'retained begin receipt bytes bound to close phase'
Check ((Test-Path (Join-Path $phaseDir 'ready-for-operator.json')) -and -not(Test-Path (Join-Path $phaseDir 'ready-for-close.json'))) 'timing stage publishes observation only; close waits for ownership stage'
$metadata=@(Get-Content -LiteralPath (Join-Path $phaseDir 'observations.jsonl')|ConvertFrom-Json)
Check ($metadata.Count -eq 2 -and @($metadata|Where-Object {$_.phase -cne 'observation_metadata' -or $_.mode -cne 'timing'}).Count -eq 0 -and $metadata[1].begin_cleanup_received) 'all timing metadata polls retained without inventory/counter/host rows'
$fakeApp.HasExited=$true
Reject {Wait-PairedObservationPhase $fakeApp $phaseSample @{} $phaseDir 'timing' 30 250 'unused'} 'root exit before cleanup boundary rejects attempt'
$fakeApp.HasExited=$false;$fakeApp.MainModule.FileName='C:\wrong\bowser.exe'
Reject {Wait-PairedObservationPhase $fakeApp $phaseSample @{} $phaseDir 'timing' 30 250 'unused'} 'wrong held executable path rejects attempt'

Write-Output "$script:checks pure paired-startup contracts passed; no app/native/CIM/input/capture work executed. Negative inputs retained at $temp"
