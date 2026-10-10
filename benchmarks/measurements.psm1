Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'

if (-not ('BowserMeasurements.Native' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace BowserMeasurements {
  public static class Native {
    [StructLayout(LayoutKind.Sequential)] public struct PROCESS_MEMORY_COUNTERS_EX {
      public uint cb;
      public uint PageFaultCount;
      public UIntPtr PeakWorkingSetSize;
      public UIntPtr WorkingSetSize;
      public UIntPtr QuotaPeakPagedPoolUsage;
      public UIntPtr QuotaPagedPoolUsage;
      public UIntPtr QuotaPeakNonPagedPoolUsage;
      public UIntPtr QuotaNonPagedPoolUsage;
      public UIntPtr PagefileUsage;
      public UIntPtr PeakPagefileUsage;
      public UIntPtr PrivateUsage;
    }
    [DllImport("kernel32.dll", SetLastError=true)] public static extern IntPtr OpenProcess(uint access, bool inherit, uint processId);
    [DllImport("kernel32.dll", SetLastError=true)] public static extern bool CloseHandle(IntPtr handle);
    [DllImport("psapi.dll", SetLastError=true)] public static extern bool GetProcessMemoryInfo(IntPtr process, out PROCESS_MEMORY_COUNTERS_EX counters, uint size);
  }
}
'@
}

function Resolve-BowserExecutable([string] $Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) { throw 'Bowser executable path must be explicit.' }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Bowser executable does not exist: $Path" }
    $resolved = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).Path
    $stream = $null
    try {
        $stream = [IO.File]::Open($resolved, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        if ($stream.Length -lt 0x40) { throw "Bowser executable is not a valid PE file: $resolved" }
        $reader = New-Object IO.BinaryReader($stream)
        if ($reader.ReadUInt16() -ne 0x5a4d) { throw "Bowser executable is missing the PE DOS signature: $resolved" }
        $stream.Position = 0x3c
        $peOffset = [long]$reader.ReadUInt32()
        if ($peOffset -lt 0x40 -or $peOffset + 6 -gt $stream.Length) { throw "Bowser executable has an invalid PE header offset: $resolved" }
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) { throw "Bowser executable is missing the PE signature: $resolved" }
        $machine = $reader.ReadUInt16()
        if ($machine -ne 0x8664) { throw ('Bowser executable must be x64 (PE machine 0x8664); found 0x{0:x4}: {1}' -f $machine, $resolved) }
    } finally {
        if ($null -ne $stream) { $stream.Dispose() }
    }
    return [IO.Path]::GetFullPath($resolved)
}

function New-IntervalResult([string] $Name, [string] $StartEvent, [string] $EndEvent, [object[]] $Rows, [long] $QpcFrequency, [int] $StartIndex = -1, [int] $EndIndex = -1) {
    $reason = $null
    $duration = $null
    $startTick = $null
    $endTick = $null
    $startElapsed = $null
    $endElapsed = $null
    if ($QpcFrequency -le 0) {
        $reason = 'invalid_qpc_frequency'
    } elseif ($StartIndex -lt 0 -or $EndIndex -lt 0) {
        $reason = "missing_endpoint:$StartEvent->$EndEvent"
    } else {
        try {
            $startTick = [long]$Rows[$StartIndex].qpc_ticks
            $endTick = [long]$Rows[$EndIndex].qpc_ticks
            $startElapsed = [double]$Rows[$StartIndex].elapsed_ms
            $endElapsed = [double]$Rows[$EndIndex].elapsed_ms
            if ($endTick -le $startTick) { $reason = "out_of_order_qpc:$StartEvent->$EndEvent" }
            elseif ($EndIndex -le $StartIndex) { $reason = "out_of_order_rows:$StartEvent->$EndEvent" }
            else { $duration = [Math]::Round((($endTick - $startTick) * 1000.0) / $QpcFrequency, 6) }
        } catch { $reason = "invalid_event_value:$StartEvent->$EndEvent" }
    }
    $crossCheck = $null
    if ($null -ne $duration -and $null -ne $startElapsed -and $null -ne $endElapsed) {
        $crossCheck = [Math]::Round(($endElapsed - $startElapsed) - $duration, 6)
    }
    return [pscustomobject]@{
        Name=$Name; StartEvent=$StartEvent; EndEvent=$EndEvent; StartQpc=$startTick; EndQpc=$endTick
        DurationMs=$duration; AppElapsedCrossCheckDeltaMs=$crossCheck; Valid=($null -eq $reason); InvalidReason=$reason
    }
}

function Get-BowserEventIntervals {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory=$true)] [object[]] $Rows,
        [Parameter(Mandatory=$true)] [long] $QpcFrequency,
        [ValidateSet('direct','blank-idle','blank-submit')] [string] $Workload = 'direct',
        [ValidateSet('Balanced','Strict')] [string] $TrackingPrevention
    )
    $rowsArray = @($Rows)
    $reasons = New-Object 'System.Collections.Generic.List[string]'
    $intervals = [ordered]@{}
    $allTicksValid = $true
    $previousTick = $null
    for ($i = 0; $i -lt $rowsArray.Count; $i++) {
        try { $tick = [long]$rowsArray[$i].qpc_ticks } catch { $tick = $null }
        if ($null -eq $tick) {
            $allTicksValid = $false
            [void]$reasons.Add("invalid_qpc_value_at_row:$i")
        } elseif ($null -ne $previousTick -and $tick -lt $previousTick) {
            $allTicksValid = $false
            [void]$reasons.Add("out_of_order_qpc_at_row:$i")
        }
        if ($null -ne $tick) { $previousTick = $tick }
    }
    $firstBoundaryIndex = $rowsArray.Count
    for ($i = 0; $i -lt $rowsArray.Count; $i++) {
        if ([string]$rowsArray[$i].event -eq 'repeat_navigation_boundary') { $firstBoundaryIndex = $i; break }
    }
    $findUnique = {
        param([string] $EventName, [int] $EndExclusive)
        $hits = @()
        for ($j = 0; $j -lt $EndExclusive; $j++) { if ([string]$rowsArray[$j].event -eq $EventName) { $hits += $j } }
        if ($hits.Count -eq 0) { return -1 }
        if ($hits.Count -gt 1) { return -2 }
        return [int]$hits[0]
    }
    $make = {
        param([string] $Name, [string] $Start, [string] $End, [int] $Limit = $firstBoundaryIndex)
        $startIndex = & $findUnique $Start $Limit
        $endIndex = & $findUnique $End $Limit
        if ($startIndex -eq -2 -or $endIndex -eq -2) {
            $r = New-IntervalResult $Name $Start $End $rowsArray $QpcFrequency -1 -1
            $r.InvalidReason = "duplicate_endpoint:$Start->$End"
            return $r
        }
        return New-IntervalResult $Name $Start $End $rowsArray $QpcFrequency $startIndex $endIndex
    }
    $intervals.shell_ready_to_visible = & $make 'shell_ready_to_visible' 'process_start' 'window_visible' $firstBoundaryIndex
    $intervals.shell_ready_to_input = & $make 'shell_ready_to_input' 'process_start' 'input_ready' $firstBoundaryIndex
    if ($Workload -eq 'direct') {
        $intervals.profile_setup = & $make 'profile_setup' 'webview_profile_started' 'webview_profile_ready' $firstBoundaryIndex
        $intervals.environment_creation = & $make 'environment_creation' 'webview_environment_requested' 'webview_environment_result' $firstBoundaryIndex
        $intervals.controller_creation = & $make 'controller_creation' 'webview_controller_requested' 'webview_controller_result' $firstBoundaryIndex
        $intervals.controller_configuration = & $make 'controller_configuration' 'webview_configure_started' 'webview_configure_finished' $firstBoundaryIndex
        $intervals.engine_creation = & $make 'engine_creation' 'webview_profile_started' 'webview_create_completed' $firstBoundaryIndex
        $navigationStart = & $findUnique 'navigation_started' $firstBoundaryIndex
        $navigationDone = & $findUnique 'navigation_completed' $firstBoundaryIndex
        $pageUsable = & $findUnique 'page_usable' $firstBoundaryIndex
        if ($navigationStart -eq -2 -or $navigationDone -eq -2 -or $pageUsable -eq -2) {
            $intervals.first_navigation = New-IntervalResult 'first_navigation' 'navigation_started' 'navigation_completed' $rowsArray $QpcFrequency -1 -1
            $intervals.first_navigation.InvalidReason = 'duplicate_endpoint:first_navigation'
            $intervals.navigation_to_page_usable = New-IntervalResult 'navigation_to_page_usable' 'navigation_started' 'page_usable' $rowsArray $QpcFrequency -1 -1
            $intervals.navigation_to_page_usable.InvalidReason = 'duplicate_endpoint:navigation_to_page_usable'
            $intervals.first_page_elapsed = New-IntervalResult 'first_page_elapsed' 'process_start' 'page_usable' $rowsArray $QpcFrequency -1 -1
            $intervals.first_page_elapsed.InvalidReason = 'duplicate_endpoint:first_page_elapsed'
            $intervals.submit_to_page_usable = New-IntervalResult 'submit_to_page_usable' 'submitted' 'page_usable' $rowsArray $QpcFrequency -1 -1
            $intervals.submit_to_page_usable.InvalidReason = 'duplicate_endpoint:submit_to_page_usable'
        } else {
            $intervals.first_navigation = New-IntervalResult 'first_navigation' 'navigation_started' 'navigation_completed' $rowsArray $QpcFrequency $navigationStart $navigationDone
            $intervals.navigation_to_page_usable = New-IntervalResult 'navigation_to_page_usable' 'navigation_started' 'page_usable' $rowsArray $QpcFrequency $navigationStart $pageUsable
            $processStart = & $findUnique 'process_start' $firstBoundaryIndex
            $intervals.first_page_elapsed = New-IntervalResult 'first_page_elapsed' 'process_start' 'page_usable' $rowsArray $QpcFrequency $processStart $pageUsable
            $submitIndex = & $findUnique 'submitted' $firstBoundaryIndex
            $intervals.submit_to_page_usable = New-IntervalResult 'submit_to_page_usable' 'submitted' 'page_usable' $rowsArray $QpcFrequency $submitIndex $pageUsable
        }
    } elseif ($Workload -eq 'blank-submit') {
        $intervals.submission = & $make 'submission' 'process_start' 'submitted' $firstBoundaryIndex
    }
    foreach ($item in $intervals.Values) {
        if (-not $item.Valid) { [void]$reasons.Add($item.InvalidReason) }
    }
    if (-not $allTicksValid) {
        foreach ($item in $intervals.Values) {
            if ($null -ne $item.DurationMs) {
                $item.Valid = $false
                $item.InvalidReason = 'sample_contains_out_of_order_qpc'
                $item.DurationMs = $null
            }
        }
    }
    $repeatNavigations = New-Object 'System.Collections.Generic.List[object]'
    for ($boundaryIndex = 0; $boundaryIndex -lt $rowsArray.Count; $boundaryIndex++) {
        if ([string]$rowsArray[$boundaryIndex].event -ne 'repeat_navigation_boundary') { continue }
        $nextBoundary = $rowsArray.Count
        for ($k = $boundaryIndex + 1; $k -lt $rowsArray.Count; $k++) { if ([string]$rowsArray[$k].event -eq 'repeat_navigation_boundary') { $nextBoundary = $k; break } }
        $startIndex = -1; $doneIndex = -1; $usableIndex = -1
        for ($k = $boundaryIndex + 1; $k -lt $nextBoundary; $k++) {
            if ($startIndex -lt 0 -and [string]$rowsArray[$k].event -eq 'navigation_started') { $startIndex = $k }
            elseif ($startIndex -ge 0 -and $doneIndex -lt 0 -and [string]$rowsArray[$k].event -eq 'navigation_completed') { $doneIndex = $k }
            elseif ($startIndex -ge 0 -and [string]$rowsArray[$k].event -eq 'page_usable') { $usableIndex = $k; break }
        }
        $nav = New-IntervalResult 'repeat_navigation' 'navigation_started' 'navigation_completed' $rowsArray $QpcFrequency $startIndex $doneIndex
        $usable = New-IntervalResult 'repeat_navigation_to_page_usable' 'navigation_started' 'page_usable' $rowsArray $QpcFrequency $startIndex $usableIndex
        if (-not $nav.Valid -or -not $usable.Valid) { [void]$reasons.Add("invalid_repeat_navigation:$boundaryIndex") }
        [void]$repeatNavigations.Add([pscustomobject]@{
            BoundaryQpc=[long]$rowsArray[$boundaryIndex].qpc_ticks
            NavigationMs=$nav.DurationMs
            NavigationToUsableMs=$usable.DurationMs
            Valid=($nav.Valid -and $usable.Valid)
            InvalidReason=(@($nav.InvalidReason,$usable.InvalidReason | Where-Object { $_ }) -join ';')
        })
    }
    $eventNames = @($rowsArray | ForEach-Object { [string]$_.event })
    $profileTainted = $eventNames -contains 'tracking_prevention_profile_unknown'
    $policy = [pscustomobject]@{ Requested=$TrackingPrevention; Valid=$true; InvalidReasons=@(); ProfileTainted=$profileTainted }
    if ($TrackingPrevention) {
        $expected = $TrackingPrevention.ToLowerInvariant()
        $policyReasons = New-Object 'System.Collections.Generic.List[string]'
        foreach ($requiredEvent in @("tracking_prevention_requested_$expected", 'tracking_prevention_enabled_true', 'tracking_prevention_applied', "tracking_prevention_readback_$expected")) {
            $count = @($eventNames | Where-Object { $_ -eq $requiredEvent }).Count
            if ($count -ne 1) { [void]$policyReasons.Add("policy_event_count:$requiredEvent=$count") }
        }
        foreach ($failureEvent in @('tracking_prevention_enabled_false','tracking_prevention_enabled_unavailable','tracking_prevention_readback_other','tracking_prevention_unsupported','tracking_prevention_error','tracking_prevention_mismatch','tracking_prevention_profile_unknown','tracking_prevention_restored')) {
            if ($eventNames -contains $failureEvent) { [void]$policyReasons.Add("policy_failure_event:$failureEvent") }
        }
        $policy.Valid = ($policyReasons.Count -eq 0)
        $policy.InvalidReasons = @($policyReasons.ToArray())
        foreach ($reason in $policyReasons) { [void]$reasons.Add($reason) }
    }
    return [pscustomobject]@{
        Valid=($reasons.Count -eq 0)
        Workload=$Workload
        QpcFrequency=$QpcFrequency
        Intervals=[pscustomobject]$intervals
        RepeatNavigations=@($repeatNavigations.ToArray())
        Policy=$policy
        InvalidReasons=@($reasons.ToArray())
        RawRows=$rowsArray
    }
}

function Test-BowserProfileReuseAllowed([bool] $ProfileTainted, [bool] $ShutdownConfirmed) {
    return (-not $ProfileTainted -and $ShutdownConfirmed)
}

function Get-BowserPhaseSummary {
    [CmdletBinding()]
    param([Parameter(Mandatory=$true)] [AllowEmptyCollection()] [object[]] $Rows)
    $groups = @($Rows | Group-Object browser, workload, launch_condition, phase)
    foreach ($group in $groups) {
        $first = $group.Group[0]
        $eligible = New-Object 'System.Collections.Generic.List[double]'
        $perRun = New-Object 'System.Collections.Generic.List[object]'
        $invalidReasons = New-Object 'System.Collections.Generic.List[string]'
        foreach ($row in $group.Group) {
            $policyInvalid = ($row.PSObject.Properties['tracking_policy_valid'] -and $row.tracking_policy_valid -eq $false)
            $usable = ([bool]$row.valid -and -not $policyInvalid -and $null -ne $row.duration_ms -and "$($row.duration_ms)" -ne '')
            if ($usable) {
                $duration = [double]$row.duration_ms
                [void]$eligible.Add($duration)
                [void]$perRun.Add([pscustomobject]@{ run=$row.run; valid=$true; duration_ms=$duration; invalid_reason=$null })
            } else {
                $reason = [string]$row.invalid_reason
                if ($policyInvalid -and $reason -notmatch 'tracking_policy_invalid') {
                    if ($reason) { $reason += ';tracking_policy_invalid' } else { $reason = 'tracking_policy_invalid' }
                }
                if (-not $reason) { $reason = 'invalid_or_missing_duration' }
                [void]$invalidReasons.Add($reason)
                [void]$perRun.Add([pscustomobject]@{ run=$row.run; valid=$false; duration_ms=$null; invalid_reason=$reason })
            }
        }
        $sorted = @($eligible.ToArray() | Sort-Object)
        $median = $null
        if ($sorted.Count -gt 0) {
            if (($sorted.Count % 2) -eq 1) { $median=$sorted[[int][Math]::Floor($sorted.Count/2)] }
            else { $median=($sorted[($sorted.Count/2)-1]+$sorted[$sorted.Count/2])/2.0 }
        }
        [pscustomobject]@{
            browser=$first.browser; workload=$first.workload; launch_condition=$first.launch_condition; phase=$first.phase
            ValidCount=$sorted.Count; InvalidCount=($group.Group.Count-$sorted.Count); MedianMs=$median
            MinMs=if($sorted.Count -gt 0){$sorted[0]}else{$null}; MaxMs=if($sorted.Count -gt 0){$sorted[-1]}else{$null}
            InvalidReasons=@($invalidReasons.ToArray() | Sort-Object -Unique); PerRun=@($perRun.ToArray())
        }
    }
}

function Get-NativeProcessMemory([int] $ProcessId) {
    $handle = [IntPtr]::Zero
    try {
        $handle = [BowserMeasurements.Native]::OpenProcess(0x0410, $false, [uint32]$ProcessId)
        if ($handle -eq [IntPtr]::Zero) {
            $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            return [pscustomobject]@{ WorkingSetBytes=$null; PrivateBytes=$null; PageFaultCount=$null; Source='unavailable'; Error=(New-Object ComponentModel.Win32Exception($code)).Message }
        }
        $counters = New-Object BowserMeasurements.Native+PROCESS_MEMORY_COUNTERS_EX
        $counters.cb = [uint32][Runtime.InteropServices.Marshal]::SizeOf([type][BowserMeasurements.Native+PROCESS_MEMORY_COUNTERS_EX])
        $success = [BowserMeasurements.Native]::GetProcessMemoryInfo($handle, [ref]$counters, $counters.cb)
        if (-not $success) {
            $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            return [pscustomobject]@{ WorkingSetBytes=$null; PrivateBytes=$null; PageFaultCount=$null; Source='unavailable'; Error=(New-Object ComponentModel.Win32Exception($code)).Message }
        }
        return [pscustomobject]@{
            WorkingSetBytes=[long]$counters.WorkingSetSize.ToUInt64()
            PrivateBytes=[long]$counters.PrivateUsage.ToUInt64()
            PageFaultCount=[long]$counters.PageFaultCount
            Source='native'
            Error=$null
        }
    } catch {
        return [pscustomobject]@{ WorkingSetBytes=$null; PrivateBytes=$null; PageFaultCount=$null; Source='unavailable'; Error=$_.Exception.Message }
    } finally {
        if ($handle -ne [IntPtr]::Zero) { [void][BowserMeasurements.Native]::CloseHandle($handle) }
    }
}

function Get-ProcessInventory {
    return @(Get-CimInstance -ClassName Win32_Process -ErrorAction SilentlyContinue | ForEach-Object {
        $creationTicks = if ($_.CreationDate) { ([datetime]$_.CreationDate).ToUniversalTime().Ticks } else { $null }
        $workingBytes = if ($null -ne $_.WorkingSetSize) { [long]$_.WorkingSetSize } else { $null }
        $wmiProxyBytes = if ($null -ne $_.PageFileUsage) { [long]$_.PageFileUsage * 1KB } else { $null }
        $cpuSeconds = if ($null -ne $_.UserModeTime -and $null -ne $_.KernelModeTime) { ([double]$_.UserModeTime + [double]$_.KernelModeTime) / 10000000.0 } else { $null }
        [pscustomobject]@{
            Pid=[int]$_.ProcessId; Parent=[int]$_.ParentProcessId; Name=[string]$_.Name; CommandLine=[string]$_.CommandLine
            CreationTicks=$creationTicks; WorkingBytes=$workingBytes; WmiProxyBytes=$wmiProxyBytes; CpuSeconds=$cpuSeconds
        }
    })
}

function Get-ProcessGroupMembers {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory=$true)] [int] $RootPid,
        [Parameter(Mandatory=$true)] [string] $Profile,
        [Parameter(Mandatory=$true)] [object[]] $Inventory
    )
    $ids = New-Object 'System.Collections.Generic.HashSet[int]'
    $byPid = @{}
    foreach ($item in $Inventory) { if ($null -ne $item.CreationTicks) { $byPid[[int]$item.Pid] = $item } }
    [void]$ids.Add($RootPid)
    $profileVariant = $Profile.Replace('\','/')
    foreach ($item in $Inventory) {
        if ($item.CommandLine -and ($item.CommandLine.IndexOf($Profile,[StringComparison]::OrdinalIgnoreCase) -ge 0 -or $item.CommandLine.IndexOf($profileVariant,[StringComparison]::OrdinalIgnoreCase) -ge 0)) { [void]$ids.Add([int]$item.Pid) }
    }
    $changed = $true
    while ($changed) {
        $changed = $false
        foreach ($item in $Inventory) {
            $parent = $byPid[[int]$item.Parent]
            if ($parent -and $null -ne $item.CreationTicks -and $null -ne $parent.CreationTicks -and [long]$item.CreationTicks -ge [long]$parent.CreationTicks -and $ids.Contains([int]$item.Parent) -and -not $ids.Contains([int]$item.Pid)) {
                [void]$ids.Add([int]$item.Pid); $changed=$true
            }
        }
    }
    return @($Inventory | Where-Object { $ids.Contains([int]$_.Pid) })
}

function Get-ProcessGroupSnapshot {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory=$true)] [int] $RootPid,
        [Parameter(Mandatory=$true)] [string] $Profile,
        [object[]] $Inventory,
        [scriptblock] $NativeMemoryReader
    )
    $inventoryWasProvided = $PSBoundParameters.ContainsKey('Inventory')
    if (-not $inventoryWasProvided) { $Inventory = @(Get-ProcessInventory) }
    if (-not $NativeMemoryReader) { $NativeMemoryReader = { param($id) Get-NativeProcessMemory -ProcessId $id } }
    $processes = @(Get-ProcessGroupMembers -RootPid $RootPid -Profile $Profile -Inventory $Inventory)
    if (@($processes | Where-Object { [int]$_.Pid -eq $RootPid }).Count -eq 0) {
        return [pscustomobject]@{ CpuSeconds=$null; CpuByPid=@{}; WorkingBytes=$null; WorkingSource='unavailable'; PrivateBytes=$null; NativeWorkingBytes=$null; NativePrivateBytes=$null; NativePageFaultCount=$null; WmiWorkingBytes=$null; WmiProxyPrivateBytes=$null; PrivateSource='unavailable'; ProcessCount=0; RacedExitCount=0; ProcessDetails=@(); SampledAtUtc=[DateTime]::UtcNow.ToString('o'); Approximate=$true; CoverageErrors=@("root_pid_missing:$RootPid"); HardFaults=$null; PageInBytes=$null }
    }
    [double]$cpu=0; [long]$wmiWorking=0; [long]$wmiPrivate=0; [long]$nativeWorking=0; [long]$nativePrivate=0; [long]$faults=0
    $nativeCount=0; $cpuByPid=@{}; $details=New-Object 'System.Collections.Generic.List[object]'; $errors=New-Object 'System.Collections.Generic.List[string]'; $raced=0
    foreach ($item in $processes) {
        $pidValue=[int]$item.Pid
        $native=& $NativeMemoryReader $pidValue
        $sameIdentity=$true
        if (-not $inventoryWasProvided) {
            $fresh=Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $pidValue" -ErrorAction SilentlyContinue | Select-Object -First 1
            $freshTicks=if($fresh -and $fresh.CreationDate){([datetime]$fresh.CreationDate).ToUniversalTime().Ticks}else{$null}
            if($null -eq $freshTicks -or [long]$freshTicks -ne [long]$item.CreationTicks){$sameIdentity=$false}
        }
        if(-not $sameIdentity){
            if($pidValue -eq $RootPid){[void]$errors.Add("root_identity_changed_during_snapshot:$pidValue")}else{$raced+=1}
            continue
        }
        $nativeSource=if($native.Source -eq 'native'){'native'}else{'unavailable'}
        $nws=if($nativeSource -eq 'native'){$native.WorkingSetBytes}else{$null}
        $nprivate=if($nativeSource -eq 'native'){$native.PrivateBytes}else{$null}
        $pageFaults=if($nativeSource -eq 'native'){$native.PageFaultCount}else{$null}
        if($nativeSource -eq 'native'){$nativeCount+=1;$nativeWorking += [long]$nws;$nativePrivate += [long]$nprivate;$faults += [long]$pageFaults}else{[void]$errors.Add("native_memory_unavailable:${pidValue}:$($native.Error)")}
        $wmiWs=if($null -ne $item.WorkingBytes){[long]$item.WorkingBytes}else{$null}
        $wmiProxy=$null
        if($item.PSObject.Properties['WmiProxyBytes'] -and $null -ne $item.WmiProxyBytes){$wmiProxy=[long]$item.WmiProxyBytes}
        elseif($item.PSObject.Properties['PrivateBytes'] -and $null -ne $item.PrivateBytes){$wmiProxy=[long]$item.PrivateBytes}
        if($null -ne $wmiWs){$wmiWorking += $wmiWs};if($null -ne $wmiProxy){$wmiPrivate += $wmiProxy}
        $processCpu=if($null -ne $item.CpuSeconds){[double]$item.CpuSeconds}else{$null}
        if($null -ne $processCpu){$cpu += $processCpu;$cpuByPid[('{0}:{1}' -f $pidValue,$item.CreationTicks)]=$processCpu}else{[void]$errors.Add("wmi_cpu_unavailable:$pidValue")}
        $role='browser'
        if($pidValue -eq $RootPid){$role='root'}elseif($item.CommandLine -match '(?:^|\s)--type=([^\s]+)'){$role=$Matches[1]}
        [void]$details.Add([pscustomobject]@{
            process_id=$pidValue; created_ticks=[long]$item.CreationTicks; name=[string]$item.Name; role=$role
            command_line_type=if($item.CommandLine -match '(?:^|\s)--type=([^\s]+)'){$Matches[1]}else{$null}
            working_set_bytes=$nws; private_bytes=$nprivate; page_fault_count=$pageFaults; native_counter_source=$nativeSource; native_error=$native.Error
            wmi_working_set_bytes=$wmiWs; wmi_pagefileusage_proxy_bytes=$wmiProxy; wmi_proxy_source='wmi_pagefileusage_proxy'; cpu_seconds=$processCpu
        })
    }
    $completeNative=($nativeCount -eq $details.Count -and $details.Count -gt 0)
    $privateSource=if($completeNative){'native'}elseif($wmiPrivate -gt 0){'wmi_pagefileusage_proxy'}else{'unavailable'}
    $chosenWorking=if($completeNative){[long]$nativeWorking}else{[long]$wmiWorking}
    $workingSource=if($completeNative){'native'}elseif($wmiWorking -gt 0){'wmi_workingsetsize'}else{'unavailable'}
    $chosenPrivate=if($completeNative){[long]$nativePrivate}elseif($wmiPrivate -gt 0){[long]$wmiPrivate}else{$null}
    if($details.Count -eq 0){[void]$errors.Add('no_process_counters_collected')}
    return [pscustomobject]@{
        CpuSeconds=$cpu; CpuByPid=$cpuByPid; WorkingBytes=$chosenWorking; WorkingSource=$workingSource; PrivateBytes=$chosenPrivate; PrivateSource=$privateSource
        NativeWorkingBytes=if($completeNative){[long]$nativeWorking}else{$null}; NativePrivateBytes=if($completeNative){[long]$nativePrivate}else{$null}
        NativePageFaultCount=if($completeNative){[long]$faults}else{$null}; WmiWorkingBytes=$wmiWorking; WmiProxyPrivateBytes=if($wmiPrivate -gt 0){[long]$wmiPrivate}else{$null}
        ProcessCount=$details.Count; RacedExitCount=$raced; ProcessDetails=@($details.ToArray()); SampledAtUtc=[DateTime]::UtcNow.ToString('o')
        Approximate=$true; CoverageErrors=@($errors.ToArray()); HardFaults=$null; PageInBytes=$null
    }
}

function Get-MeasurementInputHashes([string[]] $Paths) {
    $hashes=[ordered]@{}
    foreach($path in $Paths){
        if(-not (Test-Path -LiteralPath $path -PathType Leaf)){throw "Measurement input is missing: $path"}
        $resolved=(Resolve-Path -LiteralPath $path).Path
        $hashes[$resolved]=(Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    return $hashes
}

function Assert-MeasurementInputHashesUnchanged($Before, $After) {
    $changed=New-Object 'System.Collections.Generic.List[string]'
    foreach($path in $Before.Keys){
        if(-not $After.Contains($path) -or $Before[$path] -ne $After[$path]){[void]$changed.Add([string]$path)}
    }
    foreach($path in $After.Keys){if(-not $Before.Contains($path)){[void]$changed.Add([string]$path)}}
    if($changed.Count -gt 0){throw ('Measurement input hashes changed: ' + ($changed -join ', '))}
    return $true
}

function Resolve-BowserWorkloadSelection {
    [CmdletBinding()]
    param(
        [AllowNull()] [string[]] $SelectedWorkloads,
        [AllowNull()] [AllowEmptyString()] [string] $TrackingPrevention
    )
    $TrackingPrevention = Resolve-BowserTrackingPrevention $TrackingPrevention
    $available = @('blank-idle','blank-submit','light','demanding')
    if ($null -eq $SelectedWorkloads) {
        if ($TrackingPrevention) { return @('light','demanding') }
        return $available
    }
    if ($SelectedWorkloads.Count -eq 0) { throw 'Select at least one benchmark workload.' }
    $result = New-Object 'System.Collections.Generic.List[string]'
    foreach ($selection in $SelectedWorkloads) {
        $canonical = @($available | Where-Object { $_ -ieq $selection }) | Select-Object -First 1
        if (-not $canonical) { throw "Unsupported benchmark workload '$selection'. Choose: $($available -join ', ')." }
        if ($TrackingPrevention -and $canonical -notin @('light','demanding')) {
            throw 'Tracking-prevention comparisons support only light and demanding workloads.'
        }
        if (-not ($result -contains [string]$canonical)) { [void]$result.Add([string]$canonical) }
    }
    return @($result.ToArray())
}

function Resolve-BowserTrackingPrevention {
    [CmdletBinding()]
    param([AllowNull()] [AllowEmptyString()] [string] $TrackingPrevention)
    if ([string]::IsNullOrEmpty($TrackingPrevention)) { return $null }
    foreach ($supported in @('Balanced','Strict')) {
        if ($TrackingPrevention -ieq $supported) { return $supported }
    }
    throw "Unsupported tracking-prevention policy '$TrackingPrevention'. Choose Balanced or Strict, or omit the option for runtime-default."
}

function Resolve-BowserRunSelection {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory=$true)] [string] $OutputDirectory,
        [AllowNull()] [string[]] $SelectedWorkloads,
        [AllowNull()] [AllowEmptyString()] [string] $TrackingPrevention
    )
    $policy = Resolve-BowserTrackingPrevention $TrackingPrevention
    $workloads = @(Resolve-BowserWorkloadSelection -SelectedWorkloads $SelectedWorkloads -TrackingPrevention $policy)
    return [pscustomobject]@{
        OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
        Workloads=$workloads
        TrackingPrevention=$policy
    }
}

function Add-BowserSelectionMetadata {
    [CmdletBinding()]
    param([Parameter(Mandatory=$true)] $Metadata, [Parameter(Mandatory=$true)] $Selection)
    $result = [ordered]@{}
    if ($Metadata -is [System.Collections.IDictionary]) {
        foreach ($key in $Metadata.Keys) { $result[[string]$key] = $Metadata[$key] }
    } else {
        foreach ($property in $Metadata.PSObject.Properties) { $result[$property.Name] = $property.Value }
    }
    $result['selected_workloads'] = @($Selection.Workloads)
    $result['tracking_prevention'] = $Selection.TrackingPrevention
    return $result
}


function Get-BowserMedian([double[]] $Values) {
    $s = @($Values | Sort-Object)
    if ($s.Count -eq 0) { return $null }
    $mid = [int][math]::Floor($s.Count / 2)
    if ($s.Count % 2 -eq 1) { return [double]$s[$mid] }
    return ([double]$s[$mid - 1] + [double]$s[$mid]) / 2.0
}

function Get-BowserProbeMetrics {
    # Reads the newest perf-probe CSV of each mode in $Directory and reduces it to named medians.
    # Modes that were not run simply contribute no metrics.
    param([Parameter(Mandatory)] [string] $Directory)
    $metrics = [ordered]@{}
    $latest = { param($mode) Get-ChildItem -LiteralPath $Directory -Filter "*-$mode-*.csv" -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1 }
    $num = { param($rows, $col) @($rows | Where-Object { $null -ne $_.$col -and "$($_.$col)" -ne '' } | ForEach-Object { [double]::Parse("$($_.$col)", [Globalization.CultureInfo]::InvariantCulture) }) }
    $startup = & $latest 'startup'
    if ($startup) {
        $rows = @(Import-Csv -LiteralPath $startup.FullName)
        foreach ($kind in 'fresh','warm') {
            $sub = @($rows | Where-Object { $_.kind -eq $kind })
            $m = Get-BowserMedian (& $num $sub 'window_visible_ms'); if ($null -ne $m) { $metrics["startup_${kind}_window_visible_ms"] = [math]::Round($m, 1) }
            $m = Get-BowserMedian (& $num $sub 'outer_visible_ms'); if ($null -ne $m) { $metrics["startup_${kind}_outer_visible_ms"] = [math]::Round($m, 1) }
        }
    }
    $idle = & $latest 'idle'
    if ($idle) {
        foreach ($r in @(Import-Csv -LiteralPath $idle.FullName)) {
            $tag = if ($r.state -eq 'loaded_page') { 'idle_loaded' } else { 'idle_empty' }
            $metrics["${tag}_private_mb"] = [math]::Round([double]::Parse($r.host_priv_mb, [Globalization.CultureInfo]::InvariantCulture) + [double]::Parse($r.wv_priv_mb, [Globalization.CultureInfo]::InvariantCulture), 1)
            $metrics["${tag}_cpu_ms_20s"] = [math]::Round([double]::Parse($r.host_cpu_20s_ms, [Globalization.CultureInfo]::InvariantCulture) + [double]::Parse($r.wv_cpu_20s_ms, [Globalization.CultureInfo]::InvariantCulture), 1)
        }
    }
    $tabs = & $latest 'tabs'
    if ($tabs) {
        foreach ($r in @(Import-Csv -LiteralPath $tabs.FullName)) {
            $metrics["tabs$($r.tabs)_private_mb"] = [math]::Round([double]::Parse($r.host_priv_mb, [Globalization.CultureInfo]::InvariantCulture) + [double]::Parse($r.wv_priv_50s_mb, [Globalization.CultureInfo]::InvariantCulture), 1)
            # A negative sum means a WebView2 child exited during the window and its CPU left the snapshot; that
            # run has no valid idle CPU figure, so the metric is omitted (reported as missing) rather than recorded.
            $idleCpu = [double]::Parse($r.idle10s_host_cpu_ms, [Globalization.CultureInfo]::InvariantCulture) + [double]::Parse($r.idle10s_wv_cpu_ms, [Globalization.CultureInfo]::InvariantCulture)
            if ($idleCpu -ge 0) { $metrics["tabs$($r.tabs)_idle10s_cpu_ms"] = [math]::Round($idleCpu, 1) }
        }
    }
    $switch = & $latest 'switch'
    if ($switch) {
        $m = Get-BowserMedian (& $num @(Import-Csv -LiteralPath $switch.FullName) 'switch_ms')
        if ($null -ne $m) { $metrics['tab_switch_ms'] = [math]::Round($m, 1) }
    }
    $metrics
}

function Compare-BowserBaseline {
    # $Baseline: object with .metrics (name -> value) and .tolerances (wildcard -> {pct, floor}).
    # A metric fails when current > baseline * (1 + pct/100) + floor. Lower is better for every metric.
    # A baseline metric absent from $Current fails as 'missing'; extra current metrics are ignored.
    param([Parameter(Mandatory)] $Baseline, [Parameter(Mandatory)] [System.Collections.IDictionary] $Current)
    $tolerances = @($Baseline.tolerances.PSObject.Properties)
    foreach ($p in $Baseline.metrics.PSObject.Properties) {
        $tol = $tolerances | Where-Object { $p.Name -like $_.Name } | Select-Object -First 1
        if (-not $tol) { throw "No tolerance configured for baseline metric '$($p.Name)'." }
        $base = [double]$p.Value
        # Computed as base*(100+pct)/100 so a value exactly at the limit is not lost to binary rounding; 9 decimals keeps float noise out of the comparison.
        $limit = [math]::Round($base * (100.0 + [double]$tol.Value.pct) / 100.0 + [double]$tol.Value.floor, 9)
        if (-not $Current.Contains($p.Name)) {
            [pscustomobject]@{ metric = $p.Name; baseline = $base; current = $null; limit = [math]::Round($limit, 1); status = 'missing' }
            continue
        }
        $cur = [double]$Current[$p.Name]
        [pscustomobject]@{ metric = $p.Name; baseline = $base; current = $cur; limit = [math]::Round($limit, 1); status = $(if ($cur -gt $limit) { 'FAIL' } else { 'ok' }) }
    }
}

Export-ModuleMember -Function Resolve-BowserExecutable,Get-BowserEventIntervals,Get-NativeProcessMemory,Get-ProcessInventory,Get-ProcessGroupMembers,Get-ProcessGroupSnapshot,Get-MeasurementInputHashes,Assert-MeasurementInputHashesUnchanged,Get-BowserPhaseSummary,Test-BowserProfileReuseAllowed,Resolve-BowserWorkloadSelection,Resolve-BowserTrackingPrevention,Resolve-BowserRunSelection,Add-BowserSelectionMetadata,Get-BowserMedian,Get-BowserProbeMetrics,Compare-BowserBaseline
