Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/production-measurements.psm1" -Force
function Row($event,$tab,$nav,$success='',$token='',$tick=1) { [pscustomobject]@{event=$event;tab_id=$tab;navigation_id=$nav;success=$success;fixture_token=$token;qpc_ticks=$tick} }
function Check($condition,$name) { if (-not $condition) { throw "Failed: $name" } }
$start=Row diagnostic_navigation_started 1 10
$ready=Row diagnostic_fixture_ready 1 10 '' fixture 3
$done=Row diagnostic_navigation_completed 1 10 true '' 4
Check (Get-ProductionReadiness @($start,$ready,$done) fixture).Valid 'current successful document'
Check (Get-ProductionReadiness @($start,$done,$ready) fixture).Valid 'marker after completion'
Check (-not (Get-ProductionReadiness @($start,$ready,(Row diagnostic_navigation_completed 1 10 false)) fixture).Valid) 'failed navigation rejected'
Check (-not (Get-ProductionReadiness @($start,$ready) fixture).Valid) 'hung navigation rejected'
Check (-not (Get-ProductionReadiness @($start,$done) fixture).Valid) 'error/nonfixture page rejected'
Check (-not (Get-ProductionReadiness @($start,$ready,(Row diagnostic_navigation_started 1 11),$done) fixture).Valid) 'stale completion rejected'
Check (-not (Get-ProductionReadiness @($start,$ready,(Row diagnostic_navigation_completed 2 10 true)) fixture).Valid) 'wrong tab rejected'
Check (-not (Get-ProductionReadiness @($start,$done,$ready,(Row diagnostic_navigation_started 1 11)) fixture).Valid) 'superseded ready document rejected'
Check (Get-ProductionReadiness @($start,$start,$ready,$done) fixture).Valid 'redirect identity retained'
Check (-not (Get-ProductionReadiness @($start,$ready,$done) other).Valid) 'wrong fixture rejected'
Check (-not (Get-ProductionReadiness @($start,$ready,$done,(Row diagnostic_navigation_completed 1 10 false)) fixture).Valid) 'late failure cannot retain readiness'
$cpu=Get-ProductionCpuDelta @([pscustomobject]@{CpuByPid=@{'1:100'=1.0}},[pscustomobject]@{CpuByPid=@{'1:100'=2.0;'2:200'=3.0}},[pscustomobject]@{CpuByPid=@{'1:300'=0.5}})
Check ($cpu.CoreSeconds -eq 5.5) 'new identity CPU counted and PID reuse separated'
Check ($cpu.ObservedIdentityCount -eq 3) 'all identities retained'
$d=Get-ProductionDistribution @(1,2,3,4)
Check ($d.Median -eq 2.5 -and $d.P95 -eq 4) 'distribution'
'Production measurement contract tests passed.'

# A negative drain sentinel cannot seed the PID0/System ancestry.
$harness=Get-Content "$PSScriptRoot/production-baseline.ps1" -Raw
Check ($harness.Contains('Get-ProcessGroupMembers -RootPid -1')) 'negative drain sentinel excludes system ancestry'
Check ($harness.IndexOf('$sample.launch_qpc=Qpc') -lt $harness.IndexOf('$app=Start-Process -FilePath')) 'launch clock immediately precedes process launch'
Import-Module "$PSScriptRoot/measurements.psm1" -Force
$inventory=@(
    [pscustomobject]@{Pid=0;Parent=0;CreationTicks=1;CommandLine=''},
    [pscustomobject]@{Pid=4;Parent=0;CreationTicks=2;CommandLine='System'},
    [pscustomobject]@{Pid=10;Parent=4;CreationTicks=3;CommandLine='unrelated'},
    [pscustomobject]@{Pid=20;Parent=4;CreationTicks=4;CommandLine='--user-data-dir=C:\isolated'},
    [pscustomobject]@{Pid=21;Parent=20;CreationTicks=5;CommandLine='--type=renderer'}
)
$drain=@(Get-ProcessGroupMembers -RootPid -1 -Profile 'C:\isolated' -Inventory $inventory)
Check (($drain.Pid -join ',') -eq '20,21') 'drain selects only profile group, not Windows system ancestry'
