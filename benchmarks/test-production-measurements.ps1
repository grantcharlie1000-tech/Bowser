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
