[CmdletBinding()]
param()

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'measurements.psm1') -Force


function Assert-True([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw "Assertion failed: $Message" }
}

function Assert-Throws([scriptblock] $Action, [string] $Message) {
    $threw = $false
    try { & $Action } catch { $threw = $true }
    Assert-True $threw $Message
}

$defaultWorkloads = @(Resolve-BowserWorkloadSelection)
Assert-True (($defaultWorkloads -join ',') -eq 'blank-idle,blank-submit,light,demanding') 'ordinary default workload set remains unchanged'
$policyDefaultWorkloads = @(Resolve-BowserWorkloadSelection -TrackingPrevention Strict)
Assert-True (($policyDefaultWorkloads -join ',') -eq 'light,demanding') 'tracking-policy default remains limited to comparable workloads'
$selectedWorkloads = @(Resolve-BowserWorkloadSelection -SelectedWorkloads @('demanding','light','demanding'))
Assert-True (($selectedWorkloads -join ',') -eq 'demanding,light') 'workload selection preserves order and removes duplicates'
Assert-Throws { Resolve-BowserWorkloadSelection -SelectedWorkloads @('unknown') } 'workload selection rejects names outside the existing workload set'
Assert-Throws { Resolve-BowserWorkloadSelection -SelectedWorkloads @('blank-idle') -TrackingPrevention Balanced } 'policy selection rejects non-comparable blank workloads'

function New-EventRow([string] $Name, [long] $Tick) {
    return [pscustomobject]@{ event=$Name; elapsed_ms=($Tick / 10000.0); qpc_ticks=$Tick }
}

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('bowser-measurement-tests-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
try {
    $preflightOutput = Join-Path $tempRoot 'invalid-run-output'
    $preflight = Resolve-BowserRunSelection -OutputDirectory $preflightOutput -SelectedWorkloads @('demanding','light','demanding') -TrackingPrevention ''
    Assert-True ($null -eq $preflight.TrackingPrevention) 'omitted or empty policy is normalized to runtime-default'
    Assert-True (($preflight.Workloads -join ',') -eq 'demanding,light') 'preflight returns canonical selected workload order'
    Assert-True (-not (Test-Path -LiteralPath $preflightOutput)) 'preflight does not create output files or directories'
    $preflightMetadata = Add-BowserSelectionMetadata -Metadata ([ordered]@{ marker='test' }) -Selection $preflight
    $metadataRoundTrip = ConvertFrom-Json ($preflightMetadata | ConvertTo-Json -Depth 5)
    Assert-True (($metadataRoundTrip.selected_workloads -join ',') -eq 'demanding,light') 'JSON metadata contains the canonical selected_workloads array'
    Assert-True ($null -eq $metadataRoundTrip.tracking_prevention) 'JSON metadata records omitted policy as runtime-default'
    $balancedPreflight = Resolve-BowserRunSelection -OutputDirectory $preflightOutput -TrackingPrevention Balanced
    $strictPreflight = Resolve-BowserRunSelection -OutputDirectory $preflightOutput -TrackingPrevention Strict
    Assert-True ($balancedPreflight.TrackingPrevention -eq 'Balanced' -and $strictPreflight.TrackingPrevention -eq 'Strict') 'preflight accepts only explicit supported policy arms'
    Assert-Throws { Resolve-BowserRunSelection -OutputDirectory $preflightOutput -SelectedWorkloads @('not-a-workload') } 'invalid workload is rejected during preflight'
    Assert-True (-not (Test-Path -LiteralPath $preflightOutput)) 'rejected preflight creates no output directory or partial JSON'
    $runnerOutput = Join-Path $tempRoot 'empty-policy-runner-output'
    $runnerError = $null
    try {
        & (Join-Path $PSScriptRoot 'run.ps1') -TrackingPrevention '' -SelectedWorkloads light -Browser Bowser -BowserExecutable (Join-Path $tempRoot 'missing.exe') -OutputDirectory $runnerOutput
    } catch { $runnerError = $_.Exception.Message }
    Assert-True ($runnerError -match 'executable does not exist') 'runner accepts an empty optional policy value as runtime-default before reaching the deliberate missing-executable stop'
    Assert-True (-not (Test-Path -LiteralPath $runnerOutput)) 'runner preflight failures do not create partial output files'

    # Executable resolution is explicit and cannot silently select a fallback.
    $exe = Join-Path $tempRoot 'bowser.exe'
    $peBytes = New-Object byte[] 0x100
    $peBytes[0]=0x4d; $peBytes[1]=0x5a; $peBytes[0x3c]=0x80
    $peBytes[0x80]=0x50; $peBytes[0x81]=0x45; $peBytes[0x82]=0; $peBytes[0x83]=0
    $peBytes[0x84]=0x64; $peBytes[0x85]=0x86
    [IO.File]::WriteAllBytes($exe, $peBytes)
    Assert-True ((Resolve-BowserExecutable $exe) -eq [IO.Path]::GetFullPath($exe)) 'explicit executable resolves to its absolute path'
    Assert-Throws { Resolve-BowserExecutable (Join-Path $tempRoot 'missing.exe') } 'missing executable is rejected'
    $wrongArch = Join-Path $tempRoot 'wrong-arch.exe'
    $peBytes[0x84]=0x4c; $peBytes[0x85]=0x01
    [IO.File]::WriteAllBytes($wrongArch, $peBytes)
    Assert-Throws { Resolve-BowserExecutable $wrongArch } 'wrong-architecture executable is rejected'

    $valid = @(
        (New-EventRow process_start 100),
        (New-EventRow window_visible 200),
        (New-EventRow input_ready 300),
        (New-EventRow submitted 400),
        (New-EventRow webview_profile_started 500),
        (New-EventRow webview_profile_ready 600),
        (New-EventRow webview_environment_requested 700),
        (New-EventRow webview_environment_result 800),
        (New-EventRow webview_controller_requested 900),
        (New-EventRow webview_controller_result 1000),
        (New-EventRow webview_configure_started 1100),
        (New-EventRow webview_configure_finished 1200),
        (New-EventRow webview_create_completed 1300),
        (New-EventRow navigation_started 1400),
        (New-EventRow navigation_completed 1500),
        (New-EventRow page_usable 1600)
    )
    $intervals = Get-BowserEventIntervals $valid 1000000
    Assert-True $intervals.Valid 'valid first-use event stream is valid'
    Assert-True ($intervals.Intervals.profile_setup.DurationMs -eq 0.1) 'profile setup duration uses QPC ticks'
    Assert-True ($intervals.Intervals.submit_to_page_usable.DurationMs -eq 1.2) 'submit to page usable interval is calculated'
    Assert-True ($intervals.Intervals.first_page_elapsed.DurationMs -eq 1.5) 'first-page elapsed starts at process start'

    $repeat = @($valid[0..12]) + @(
        (New-EventRow navigation_started 1400),
        (New-EventRow navigation_completed 1500),
        (New-EventRow page_usable 1600),
        (New-EventRow repeat_navigation_boundary 1700),
        (New-EventRow navigation_started 1800),
        (New-EventRow navigation_completed 1950),
        (New-EventRow page_usable 2300)
    )
    $repeatIntervals = Get-BowserEventIntervals $repeat 1000000
    Assert-True $repeatIntervals.Valid 'a well-formed repeat navigation remains valid'
    Assert-True ($repeatIntervals.RepeatNavigations.Count -eq 1) 'repeat boundary produces one repeat interval'
    Assert-True ($repeatIntervals.RepeatNavigations[0].NavigationToUsableMs -eq 0.5) 'repeat interval pairs only with events after its boundary'
    $badRepeat = @($valid) + @((New-EventRow repeat_navigation_boundary 1700),(New-EventRow navigation_started 1800))
    $badRepeatResult = Get-BowserEventIntervals $badRepeat 1000000
    Assert-True (-not $badRepeatResult.Valid) 'repeat boundary without endpoints invalidates the sample'
    Assert-True ($null -eq $badRepeatResult.RepeatNavigations[0].NavigationToUsableMs) 'incomplete repeat never invents a duration'

    $blankIdleRows = @((New-EventRow process_start 100),(New-EventRow window_visible 200),(New-EventRow input_ready 300))
    $blankIdleResult = Get-BowserEventIntervals $blankIdleRows 1000000 -Workload blank-idle
    Assert-True $blankIdleResult.Valid 'blank-idle requires shell-ready events only'
    $blankSubmitRows = @((New-EventRow process_start 100),(New-EventRow window_visible 200),(New-EventRow input_ready 300),(New-EventRow submitted 500))
    $blankSubmitResult = Get-BowserEventIntervals $blankSubmitRows 1000000 -Workload blank-submit
    Assert-True $blankSubmitResult.Valid 'blank-submit ends at Enter without requiring page-ready events'
    Assert-True (-not $blankSubmitResult.Intervals.PSObject.Properties['first_page_elapsed']) 'blank-submit exposes no fabricated page-ready interval'

    $policyRows = @($valid) + @(
        (New-EventRow tracking_prevention_requested_strict 1700),
        (New-EventRow tracking_prevention_enabled_true 1800),
        (New-EventRow tracking_prevention_applied 1900),
        (New-EventRow tracking_prevention_readback_strict 2000)
    )
    $policyResult = Get-BowserEventIntervals $policyRows 1000000 -TrackingPrevention Strict
    Assert-True $policyResult.Policy.Valid 'matching requested, enabled, applied, and readback events validate policy'
    $unknownPolicyResult = Get-BowserEventIntervals (@($policyRows) + @(New-EventRow tracking_prevention_profile_unknown 2100)) 1000000 -TrackingPrevention Strict
    Assert-True (-not $unknownPolicyResult.Policy.Valid) 'unknown profile after a policy request invalidates the sample'
    Assert-True $unknownPolicyResult.Policy.ProfileTainted 'unknown profile taints later profile reuse'
    $restoredPolicyResult = Get-BowserEventIntervals (@($policyRows) + @(New-EventRow tracking_prevention_restored 2100)) 1000000 -TrackingPrevention Strict
    Assert-True (-not $restoredPolicyResult.Policy.Valid) 'engine contract restored event invalidates an otherwise successful requested-policy sample'
    Assert-True (-not $restoredPolicyResult.Policy.ProfileTainted) 'verified profile restoration does not taint later reuse'
    $badPolicyResult = Get-BowserEventIntervals (@($valid) + @(New-EventRow tracking_prevention_mismatch 1700)) 1000000 -TrackingPrevention Strict
    Assert-True (-not $badPolicyResult.Policy.Valid) 'policy mismatch event invalidates the policy sample'

    $aggregateInput = @(
        [pscustomobject]@{ browser='Bowser'; workload='light'; launch_condition='cold-profile'; run=1; phase='submit_to_page_usable'; duration_ms=10.0; valid=$true; tracking_policy_valid=$true; invalid_reason=$null },
        [pscustomobject]@{ browser='Bowser'; workload='light'; launch_condition='cold-profile'; run=2; phase='submit_to_page_usable'; duration_ms=5.0; valid=$true; tracking_policy_valid=$false; invalid_reason='tracking_policy_invalid:profile_unknown' },
        [pscustomobject]@{ browser='Bowser'; workload='light'; launch_condition='cold-profile'; run=3; phase='submit_to_page_usable'; duration_ms=$null; valid=$false; tracking_policy_valid=$true; invalid_reason='missing_endpoint' }
    )
    $aggregate = @(Get-BowserPhaseSummary -Rows $aggregateInput)
    Assert-True ($aggregate.Count -eq 1 -and $aggregate[0].ValidCount -eq 1) 'phase aggregate counts only valid policy samples'
    Assert-True ($aggregate[0].InvalidCount -eq 2 -and $aggregate[0].MedianMs -eq 10.0) 'phase aggregate excludes policy-invalid durations and retains invalid count'
    Assert-True (@(Get-BowserPhaseSummary -Rows @()).Count -eq 0) 'empty phase collection produces no fabricated summary'

    Assert-True (Test-BowserProfileReuseAllowed -ProfileTainted $false -ShutdownConfirmed $true) 'known profile may be reused after confirmed shutdown'
    Assert-True (-not (Test-BowserProfileReuseAllowed -ProfileTainted $true -ShutdownConfirmed $true)) 'unknown profile is not reused even after shutdown'
    Assert-True (-not (Test-BowserProfileReuseAllowed -ProfileTainted $false -ShutdownConfirmed $false)) 'profile is not reused when shutdown is unconfirmed'

    $missing = @($valid | Where-Object { $_.event -ne 'webview_controller_result' })
    $missingResult = Get-BowserEventIntervals $missing 1000000
    Assert-True (-not $missingResult.Valid) 'missing endpoint invalidates the sample'
    Assert-True ($null -eq $missingResult.Intervals.controller_creation.DurationMs) 'missing endpoint never produces a duration'
    Assert-True (@($missingResult.InvalidReasons | Where-Object { $_ -match 'webview_controller_result' }).Count -gt 0) 'missing endpoint reason is specific'

    $duplicate = @($valid) + @(New-EventRow page_usable 1700)
    $duplicateResult = Get-BowserEventIntervals $duplicate 1000000
    Assert-True (-not $duplicateResult.Valid) 'duplicate endpoint invalidates the sample'
    Assert-True ($null -eq $duplicateResult.Intervals.navigation_to_page_usable.DurationMs) 'duplicate endpoint never produces a duration'

    $reversed = @($valid | ForEach-Object { $_ })
    $reversed[5].qpc_ticks = 450
    $reversedResult = Get-BowserEventIntervals $reversed 1000000
    Assert-True (-not $reversedResult.Valid) 'reversed QPC order invalidates the sample'
    Assert-True ($null -eq $reversedResult.Intervals.profile_setup.DurationMs) 'reversed endpoint never produces a duration'

    # A stale parent PID cannot pull in a child that predates the current parent.
    $inventory = @(
        [pscustomobject]@{ Pid=10; Parent=1; CreationTicks=1000L; Name='bowser.exe'; CommandLine='bowser.exe --profile C:\measurement-profile-owned'; WorkingBytes=1000L; PrivateBytes=1000L; CpuSeconds=0.1 },
        [pscustomobject]@{ Pid=11; Parent=10; CreationTicks=900L; Name='old-child.exe'; CommandLine='old-child.exe'; WorkingBytes=1000L; PrivateBytes=1000L; CpuSeconds=0.1 },
        [pscustomobject]@{ Pid=12; Parent=10; CreationTicks=1100L; Name='new-child.exe'; CommandLine='new-child.exe'; WorkingBytes=1000L; PrivateBytes=1000L; CpuSeconds=0.1 }
    )
    $members = @(Get-ProcessGroupMembers -RootPid 10 -Profile 'C:\measurement-profile-owned' -Inventory $inventory)
    Assert-True ($members.Pid -contains 12) 'child newer than current parent is included'
    Assert-True (-not ($members.Pid -contains 11)) 'child older than current parent is excluded after PID reuse'

    # Hash comparison rejects mutation of a copied input, without touching fixtures.
    $copiedInput = Join-Path $tempRoot 'hash-input.txt'
    Set-Content -LiteralPath $copiedInput -Value 'before' -NoNewline
    $beforeHash = Get-MeasurementInputHashes -Paths @($copiedInput)
    Set-Content -LiteralPath $copiedInput -Value 'after' -NoNewline
    $afterHash = Get-MeasurementInputHashes -Paths @($copiedInput)
    Assert-Throws { Assert-MeasurementInputHashesUnchanged $beforeHash $afterHash } 'input mutation is rejected by hash comparison'

    # Use real native counters for this PowerShell process and a disposable child.
    $self = Get-NativeProcessMemory -ProcessId $PID
    Assert-True ($self.Source -eq 'native') 'current PowerShell PID uses native counters'
    Assert-True ($self.WorkingSetBytes -gt 0 -and $self.PrivateBytes -gt 0 -and $null -ne $self.PageFaultCount) 'available native counters are non-null and positive where applicable'
    $selfSnapshot = Get-ProcessGroupSnapshot -RootPid $PID -Profile $tempRoot
    $rootDetail = @($selfSnapshot.ProcessDetails | Where-Object { $_.process_id -eq $PID }) | Select-Object -First 1
    Assert-True ($null -ne $rootDetail -and $rootDetail.created_ticks -gt 0) 'process detail records PID plus creation-time identity'
    Assert-True ($rootDetail.native_counter_source -eq 'native') 'native counter source is explicit'
    Assert-True ($rootDetail.working_set_bytes -gt 0 -and $rootDetail.private_bytes -gt 0 -and $null -ne $rootDetail.page_fault_count) 'native process detail counters are available'
    Assert-True ($rootDetail.wmi_proxy_source -eq 'wmi_pagefileusage_proxy') 'PageFileUsage remains explicitly labeled as a proxy'

    $childExe = Join-Path $PSHOME 'powershell.exe'
    $child = Start-Process -FilePath $childExe -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 8') -PassThru
    try {
        Start-Sleep -Milliseconds 300
        $childMemory = Get-NativeProcessMemory -ProcessId $child.Id
        Assert-True ($childMemory.Source -eq 'native') 'disposable child process uses native counters'
        Assert-True ($childMemory.WorkingSetBytes -gt 0 -and $childMemory.PrivateBytes -gt 0) 'disposable child native counters are available'
    } finally {
        if (-not $child.HasExited) { Stop-Process -Id $child.Id -Force -ErrorAction SilentlyContinue }
    }

    # Failure of native collection leaves the WMI counter labeled as a proxy.
    $fallbackInventory = @([pscustomobject]@{ Pid=77; Parent=1; CreationTicks=1000L; Name='bowser.exe'; CommandLine='bowser.exe --profile TEST'; WorkingBytes=1024L; PrivateBytes=2048L; CpuSeconds=0.1 })
    $unavailableReader = { param($ProcessId) [pscustomobject]@{ WorkingSetBytes=$null; PrivateBytes=$null; PageFaultCount=$null; Source='unavailable'; Error='access denied' } }
    $fallback = Get-ProcessGroupSnapshot -RootPid 77 -Profile 'TEST' -Inventory $fallbackInventory -NativeMemoryReader $unavailableReader
    $fallbackRoot = $fallback.ProcessDetails | Select-Object -First 1
    Assert-True ($fallbackRoot.native_counter_source -eq 'unavailable') 'native access failure is reported as unavailable'
    Assert-True ($fallbackRoot.wmi_proxy_source -eq 'wmi_pagefileusage_proxy') 'native access failure does not relabel WMI PageFileUsage'
    Assert-True ($fallbackRoot.wmi_pagefileusage_proxy_bytes -eq 2048) 'proxy value stays separately available'
    Assert-True ($fallback.PrivateBytes -eq 2048 -and $fallback.PrivateSource -eq 'wmi_pagefileusage_proxy') 'proxy aggregate retains its explicit source label'

    # run.ps1 collects phase rows in a generic List. Windows PowerShell 5.1 throws "Argument types do not match"
    # when such a List is wrapped in @(), which broke the summary after the CSV had been written.
    $phaseList = New-Object 'System.Collections.Generic.List[object]'
    [void]$phaseList.Add([pscustomobject]@{ browser='Bowser'; workload='blank-idle'; launch_condition='cold-profile'; run=1; phase='shell_ready_to_visible'; duration_ms=10.0; valid=$true; invalid_reason=$null; tracking_prevention_requested=$null; tracking_policy_valid=$null })
    [void]$phaseList.Add([pscustomobject]@{ browser='Bowser'; workload='blank-idle'; launch_condition='cold-profile'; run=2; phase='shell_ready_to_visible'; duration_ms=20.0; valid=$true; invalid_reason=$null; tracking_prevention_requested=$null; tracking_policy_valid=$null })
    $listSummary = @(Get-BowserPhaseSummary -Rows $phaseList.ToArray())
    Assert-True ($listSummary.Count -eq 1 -and $listSummary[0].ValidCount -eq 2 -and $listSummary[0].MedianMs -eq 15.0) 'generic phase List summarized through ToArray gives the expected median'
    $emptyPhaseList = New-Object 'System.Collections.Generic.List[object]'
    Assert-True (@(Get-BowserPhaseSummary -Rows $emptyPhaseList.ToArray()).Count -eq 0) 'empty generic phase List summarizes without error'
    $runnerSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'run.ps1') -Raw
    Assert-True ($runnerSource -notmatch '@\(\$phaseRows\)') 'runner does not wrap the generic phase List in @()'

    # Baseline comparison: tolerance arithmetic, regressions, improvements, missing metrics.
    $baseline = [pscustomobject]@{
        metrics = [pscustomobject]@{ startup_warm_window_visible_ms = 100.0; tabs5_private_mb = 400.0; tab_switch_ms = 30.0; idle_loaded_cpu_ms_20s = 300.0 }
        tolerances = [pscustomobject]@{
            '*_ms' = [pscustomobject]@{ pct = 50; floor = 20 }
            '*_mb' = [pscustomobject]@{ pct = 15; floor = 5 }
            '*cpu*' = [pscustomobject]@{ pct = 100; floor = 50 }
        }
    }
    $same = @(Compare-BowserBaseline -Baseline $baseline -Current ([ordered]@{ startup_warm_window_visible_ms = 100.0; tabs5_private_mb = 400.0; tab_switch_ms = 30.0; idle_loaded_cpu_ms_20s = 300.0 }))
    Assert-True (@($same | Where-Object { $_.status -ne 'ok' }).Count -eq 0) 'identical metrics pass'
    $edge = @(Compare-BowserBaseline -Baseline $baseline -Current ([ordered]@{ startup_warm_window_visible_ms = 170.0; tabs5_private_mb = 465.0; tab_switch_ms = 65.0; idle_loaded_cpu_ms_20s = 650.0 }))
    Assert-True (@($edge | Where-Object { $_.status -ne 'ok' }).Count -eq 0) 'values exactly at the tolerance limit pass'
    $bad = @(Compare-BowserBaseline -Baseline $baseline -Current ([ordered]@{ startup_warm_window_visible_ms = 171.0; tabs5_private_mb = 466.0; tab_switch_ms = 66.0; idle_loaded_cpu_ms_20s = 651.0 }))
    Assert-True (@($bad | Where-Object { $_.status -eq 'FAIL' }).Count -eq 4) 'values just over the limit all fail'
    $better = @(Compare-BowserBaseline -Baseline $baseline -Current ([ordered]@{ startup_warm_window_visible_ms = 10.0; tabs5_private_mb = 100.0; tab_switch_ms = 1.0; idle_loaded_cpu_ms_20s = 0.0 }))
    Assert-True (@($better | Where-Object { $_.status -ne 'ok' }).Count -eq 0) 'improvements pass'
    $missing = @(Compare-BowserBaseline -Baseline $baseline -Current ([ordered]@{ tab_switch_ms = 30.0 }))
    Assert-True (@($missing | Where-Object { $_.status -eq 'missing' }).Count -eq 3) 'baseline metrics absent from the run are reported missing'
    Assert-Throws { Compare-BowserBaseline -Baseline ([pscustomobject]@{ metrics = [pscustomobject]@{ unknown_thing = 1 }; tolerances = $baseline.tolerances }) -Current ([ordered]@{}) | Out-Null } 'metric without a tolerance rule is a configuration error'
    Assert-True ((Get-BowserMedian @(3.0, 1.0, 2.0)) -eq 2.0 -and (Get-BowserMedian @(4.0, 1.0)) -eq 2.5 -and $null -eq (Get-BowserMedian @())) 'median helper'

    # Probe CSV reduction.
    $probeDir = Join-Path $tempRoot 'probe'
    New-Item -ItemType Directory -Force $probeDir | Out-Null
    @([pscustomobject]@{ kind='fresh'; run=0; outer_visible_ms=300; window_visible_ms=150 }, [pscustomobject]@{ kind='fresh'; run=1; outer_visible_ms=320; window_visible_ms=170 }, [pscustomobject]@{ kind='warm'; run=0; outer_visible_ms=310; window_visible_ms=80 }) | Export-Csv -LiteralPath (Join-Path $probeDir 'x-startup-1.csv') -NoTypeInformation
    @([pscustomobject]@{ state='empty_focused_omnibox'; host_cpu_20s_ms=0; wv_cpu_20s_ms=0; host_priv_mb=2.5; wv_priv_mb=0 }, [pscustomobject]@{ state='loaded_page'; host_cpu_20s_ms=10; wv_cpu_20s_ms=90; host_priv_mb=3.5; wv_priv_mb=100 }) | Export-Csv -LiteralPath (Join-Path $probeDir 'x-idle-1.csv') -NoTypeInformation
    @([pscustomobject]@{ tabs=5; host_priv_mb=4; wv_priv_50s_mb=300; idle10s_host_cpu_ms=1; idle10s_wv_cpu_ms=2 }) | Export-Csv -LiteralPath (Join-Path $probeDir 'x-tabs-1.csv') -NoTypeInformation
    @([pscustomobject]@{ switch_ms=20 }, [pscustomobject]@{ switch_ms=40 }, [pscustomobject]@{ switch_ms=30 }) | Export-Csv -LiteralPath (Join-Path $probeDir 'x-switch-1.csv') -NoTypeInformation
    $reduced = Get-BowserProbeMetrics -Directory $probeDir
    Assert-True ($reduced['startup_fresh_window_visible_ms'] -eq 160.0 -and $reduced['startup_warm_window_visible_ms'] -eq 80.0) 'startup medians per profile kind'
    Assert-True ($reduced['idle_loaded_private_mb'] -eq 103.5 -and $reduced['idle_empty_private_mb'] -eq 2.5 -and $reduced['idle_loaded_cpu_ms_20s'] -eq 100.0) 'idle RAM and CPU reduce host plus WebView2'
    Assert-True ($reduced['tabs5_private_mb'] -eq 304.0 -and $reduced['tabs5_idle10s_cpu_ms'] -eq 3.0) 'multi-tab metrics reduce host plus WebView2'
    Assert-True ($reduced['tab_switch_ms'] -eq 30.0) 'tab switch median'

    Write-Host 'All benchmark measurement tests passed.'
} finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
