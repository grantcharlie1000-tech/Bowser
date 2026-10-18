# Ordinary native rail actions and whole-tree resource observations, not an input-latency probe.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
# Do not force-reload this shared dependency: the caller also imports its exports.
Import-Module "$PSScriptRoot/measurements.psm1"
function New-ProductionSidebarFixture {
 param([Parameter(Mandatory)][ValidatePattern('^http://127\.0\.0\.1:[0-9]{1,5}$')][string]$BaseUrl,[Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{32}$')][string]$Token)
 # Match SidebarConfig::default: omitted item rows parse Apps as hidden.
 $items=foreach($kind in @('customize','workspaces','apps','bookmarks','history','menu')){"item`t$kind`t$kind`t1"}
 "bowser-sidebar-v1`n$($items -join "`n")`napp`t1`tFixture A`t$BaseUrl/light?token=$($Token)_a`t-`t0`napp`t2`tFixture B`t$BaseUrl/light?token=$($Token)_b`t-`t1`n"
}
function Invoke-ProductionSidebarLifecycle {
 [CmdletBinding()]
 param([uint32]$RootPid,[string]$Profile,[string]$RequestFile,[string]$AToken,[string]$BToken,[ValidateSet('settled','rapid')][string]$Scenario='settled',[Parameter(Mandatory)][AllowEmptyCollection()][Collections.Generic.List[object]]$Trace)
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
 $qpc={ [Diagnostics.Stopwatch]::GetTimestamp() }
 $rootWindow=[BowserProductionNative]::FindHost($RootPid)
 if($rootWindow -eq [IntPtr]::Zero){throw 'Sidebar owned native root unavailable'}
 $root=[Windows.Automation.AutomationElement]::FromHandle($rootWindow)
 function Find-Visible($parent,[string]$name,$type) {
  $condition=[Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,$type))
  @($parent.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {-not $_.Current.IsOffscreen -and $_.Current.BoundingRectangle.Width -gt 10 -and $_.Current.BoundingRectangle.Height -gt 10})
 }
 function Invoke-Rail([string]$name) {
  $owner=[uint32]0;[void][BowserProductionNative]::GetWindowThreadProcessId($rootWindow,[ref]$owner)
  if($owner -ne $RootPid -or [BowserProductionNative]::FindHost($RootPid) -ne $rootWindow){throw 'Sidebar native ownership changed before action'}
  if([BowserProductionNative]::GetForegroundWindow() -ne $rootWindow){throw 'Sidebar owned root not foreground; no rail action'}
  $rails=@(Find-Visible $root 'Left rail' ([Windows.Automation.ControlType]::ToolBar))
  if($rails.Count -ne 1){throw "Sidebar source rail unavailable or ambiguous: $($rails.Count)"}
  $buttons=@(Find-Visible $rails[0] $name ([Windows.Automation.ControlType]::Button))
  if($buttons.Count -ne 1){throw "Sidebar action button unavailable or ambiguous: $name/$($buttons.Count)"}
  $current=$buttons[0].Current;$bounds=$current.BoundingRectangle
  $entry=[ordered]@{kind='rail-invoke';name=$name;qpc=(& $qpc);provider_pid=$current.ProcessId;bounds=@{left=$bounds.Left;top=$bounds.Top;width=$bounds.Width;height=$bounds.Height};invoked=$false;method='UIA Invoke rooted in exact owned native Left rail; action proxy, not keyboard evidence'}
  $Trace.Add($entry)
  $pattern=$buttons[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)
  $pattern.Invoke();$entry.invoked=$true;$entry['return_qpc']=(& $qpc)
 }
 function Wait-App([string]$token,[string]$name) {
  $wait=[Diagnostics.Stopwatch]::StartNew();$docs=@();$acks=@();$panel=[IntPtr]::Zero
  do {
   $panel=[BowserProductionNative]::FindSurface($RootPid,'BowserSidePanelHost',100,100)
   $docs=@()
   if($panel -ne [IntPtr]::Zero){$panelRoot=[Windows.Automation.AutomationElement]::FromHandle($panel);$docs=@(Find-Visible $panelRoot "BOWSER_PERF_READY_$token" ([Windows.Automation.ControlType]::Document))}
   $acks=if(Test-Path -LiteralPath $RequestFile){@(Get-Content -LiteralPath $RequestFile|ConvertFrom-Json|Where-Object {$_.path -ceq '/ready' -and $_.token -ceq $token})}else{@()}
   if($acks.Count -gt 0 -and $docs.Count -gt 0){break};Start-Sleep -Milliseconds 50
  }while($wait.Elapsed.TotalSeconds -lt 20)
  $matched=@($docs|ForEach-Object {$c=$_.Current;$r=$c.BoundingRectangle;[ordered]@{name=$c.Name;provider_pid=$c.ProcessId;offscreen=$c.IsOffscreen;bounds=@{left=$r.Left;top=$r.Top;width=$r.Width;height=$r.Height}}})
  $Trace.Add([ordered]@{kind='visible-app';app_id=if($name -ceq 'Fixture A'){1}else{2};name=$name;token=$token;qpc=(& $qpc);ack_count=$acks.Count;visible_document_count=$docs.Count;matches=$matched;visible_document_unique=($docs.Count -eq 1);unique_ack_and_visible_document=($docs.Count -eq 1 -and $acks.Count -eq 1);scope='Unique synthetic token; duplicate acknowledgements/documents retained, neither multiplicity nor one visible document alone proves controller exclusivity'})
  if(-not $acks.Count -or -not $docs.Count){throw "Sidebar intended fixture acknowledgement/visible panel missing: $name"}
 }
 function Phase([string]$name,[bool]$ExpectWebApps) {
  $start=& $qpc;Start-Sleep -Milliseconds 1000
  $snapshots=[Collections.Generic.List[object]]::new();$overhead=[Collections.Generic.List[double]]::new();$inventoryCost=[Collections.Generic.List[double]]::new()
  $webProfile=Join-Path $Profile 'WebApps';$variant=$webProfile.Replace('\','/')
  function Read-WebRoots {
   $t=& $qpc;$inventory=@(Get-ProcessInventory)
   $identities=@($inventory|Where-Object {$_.CommandLine -and ($_.CommandLine.IndexOf($webProfile,[StringComparison]::OrdinalIgnoreCase) -ge 0 -or $_.CommandLine.IndexOf($variant,[StringComparison]::OrdinalIgnoreCase) -ge 0)}|ForEach-Object {[ordered]@{pid=$_.Pid;created_ticks=$_.CreationTicks;parent=$_.Parent}})
   $inventoryCost.Add(((& $qpc)-$t)*1000.0/[Diagnostics.Stopwatch]::Frequency)
   [ordered]@{available=($inventory.Count -gt 0);identities=$identities}
  }
  $before=Read-WebRoots
  # Do not pass a supplied Inventory: each snapshot retains its per-PID creation-identity recheck.
  for($i=0;$i -lt 3;$i++){$t=& $qpc;$snapshots.Add((Get-ProcessGroupSnapshot -RootPid $RootPid -Profile $Profile));$overhead.Add(((& $qpc)-$t)*1000.0/[Diagnostics.Stopwatch]::Frequency);if($i -lt 2){Start-Sleep -Milliseconds 250}}
  $after=Read-WebRoots
  $coverage=Get-ProductionSidebarPhaseCoverage -Snapshots @($snapshots.ToArray()) -Before $before -After $after -ExpectWebApps $ExpectWebApps
  $Trace.Add([ordered]@{kind='resource-phase';name=$name;start_qpc=$start;end_qpc=(& $qpc);settle_before_samples_ms=1000;sample_count=3;between_samples_ms=250;snapshots=@($snapshots.ToArray());sampling_overhead_ms=@($overhead.ToArray());inventory_overhead_ms=@($inventoryCost.ToArray());webapps_profile=$webProfile;webapps_inventory_before=$before;webapps_inventory_after=$after;webapps_root_identities=$before.identities;coverage=$coverage;eligible=$coverage.eligible;scope='Whole root descendants plus profile path including separate WebApps; stable identity coverage across ALL three snapshots required; native PrivateUsage is private commit, not per-view RAM; working-set sums not PSS; endpoint sampling is not lifetime peak'})
  if(-not $coverage.eligible){throw "Sidebar resource coverage incomplete: $name"}
 }
  $beforeForeground=[BowserProductionNative]::GetForegroundWindow();$activationStart=& $qpc
  $activationRequested=[BowserProductionNative]::SetForegroundWindow($rootWindow)
  # Same fixed activation-confirmation interval as production-baseline.ps1 Owned-Keys.
  # Once per row before resource phases/actions: no per-action pacing or focus repair.
  Start-Sleep -Milliseconds 80
  $afterForeground=[BowserProductionNative]::GetForegroundWindow()
  $Trace.Add([ordered]@{kind='rail-activation';name='initial-sidebar-setup';start_qpc=$activationStart;end_qpc=(& $qpc);requested=$activationRequested;before_hwnd=$beforeForeground.ToInt64();after_hwnd=$afterForeground.ToInt64();root_hwnd=$rootWindow.ToInt64();confirmation_ms=80;scope='Single ordinary foreground request, fixed confirmation interval, then fail-closed guard; no retry'})
  if($afterForeground -ne $rootWindow -or [BowserProductionNative]::FindHost($RootPid) -ne $rootWindow){throw 'Sidebar owned root not foreground; no rail action'}
 Phase 'initial-one-tab' $false
 if($Scenario -eq 'rapid'){foreach($name in @('Fixture A','Fixture B','Fixture A')){Invoke-Rail $name}}else{Invoke-Rail 'Fixture A'}
 Wait-App $AToken 'Fixture A'
 Phase 'A-ready' $true
 Invoke-Rail 'Fixture B';Wait-App $BToken 'Fixture B'
 Phase 'first-B-ready' $true
 for($cycle=1;$cycle -le 3;$cycle++) {
  foreach($pair in @(@('Fixture A',$AToken),@('Fixture B',$BToken),@('Fixture A',$AToken))){Invoke-Rail $pair[0];Wait-App $pair[1] $pair[0]}
  # The ordinary active rail button toggles the panel closed. No testing message or focus repair.
  Invoke-Rail 'Fixture A'
  $wait=[Diagnostics.Stopwatch]::StartNew()
  while([BowserProductionNative]::FindSurface($RootPid,'BowserSidePanelHost',100,100) -ne [IntPtr]::Zero -and $wait.Elapsed.TotalSeconds -lt 5){Start-Sleep -Milliseconds 50}
  if([BowserProductionNative]::FindSurface($RootPid,'BowserSidePanelHost',100,100) -ne [IntPtr]::Zero){throw 'Sidebar ordinary rail toggle did not close panel'}
  Phase "closed-cycle-$cycle" $true
 }
}
Export-ModuleMember -Function New-ProductionSidebarFixture,Invoke-ProductionSidebarLifecycle

function Get-ProductionSidebarLedger {
 param([AllowNull()][object[]]$Rows,[uint32[]]$ExpectedApps=@())
 $Rows=@($Rows|Where-Object {$null -ne $_ -and $_.event -like 'webapp_*'})
 $errors=[Collections.Generic.List[string]]::new();$environments=@{};$requests=@{};$summaries=[Collections.Generic.List[object]]::new()
 if(-not $Rows.Count){$errors.Add('missing_lifecycle_rows')}
 if(@($Rows|Where-Object event -eq webapp_observation_error).Count){$errors.Add('native_observation_error')}
 $end=@($Rows|Where-Object event -eq webapp_observation_end);$allocated=[uint64]0
 if($end.Count -ne 1 -or $end[0].success -cne 'true' -or -not [uint64]::TryParse([string]$end[0].count,[ref]$allocated)){$errors.Add('missing_or_failed_observation_end')}
 if($Rows.Count -and $Rows[-1].event -cne 'webapp_observation_end'){$errors.Add('lifecycle_stream_not_ended')}
 foreach($row in @($Rows|Where-Object {$_.event -like '*_result' -or $_.event -like '*_callback' -or $_.event -eq 'webapp_hresult'})){if($row.success -cnotin @('true','false')){$errors.Add('invalid_result_status')}}
 foreach($row in @($Rows|Where-Object event -eq webapp_environment_requested)) {
  $id=[uint64]0
  if(-not [uint64]::TryParse([string]$row.count,[ref]$id) -or $id -eq 0 -or $environments.ContainsKey([string]$id)){$errors.Add('invalid_or_duplicate_environment_identity');continue}
  $environments[[string]$id]=$row
  $begin=@($Rows|Where-Object {$_.event -ceq 'webapp_environment_begin_result' -and $_.count -ceq [string]$id})
  if($begin.Count -ne 1 -or $begin[0].success -cnotin @('true','false')){$errors.Add("missing_or_invalid_environment_begin:$id")}
  $expected=if($begin.Count -eq 1 -and $begin[0].success -ceq 'false'){0}else{1}
  foreach($event in @('webapp_environment_callback','webapp_environment_configure_result')) {if(@($Rows|Where-Object {$_.event -ceq $event -and $_.count -ceq [string]$id}).Count -ne $expected){$errors.Add("missing_or_duplicate_environment_result:$id/$event")}}
 }
 foreach($row in @($Rows|Where-Object event -eq webapp_controller_requested)) {
  $request=[uint64]0;$appId=[uint32]0
  if(-not [uint64]::TryParse([string]$row.navigation_id,[ref]$request) -or $request -eq 0 -or $requests.ContainsKey([string]$request)){$errors.Add('invalid_or_duplicate_request_identity');continue}
  $requests[[string]$request]=$true
  if(-not [uint32]::TryParse([string]$row.tab_id,[ref]$appId) -or $appId -eq 0 -or -not $environments.ContainsKey([string]$row.count)){$errors.Add("invalid_request_app_or_environment:$request")}
  $events=@($Rows|Where-Object {$_.navigation_id -ceq [string]$request -and $_.event -ne 'webapp_hresult'})
  if(@($events|Where-Object {$_.tab_id -cne $row.tab_id -or $_.count -cne $row.count -or $_.view -cne 'webapps'}).Count){$errors.Add("request_identity_mismatch:$request")}
  $begin=@($events|Where-Object event -eq webapp_controller_begin_result)
  if($begin.Count -ne 1 -or $begin[0].success -cnotin @('true','false')){$errors.Add("missing_or_invalid_request_begin:$request")}
  $callbackExpected=if($begin.Count -eq 1 -and $begin[0].success -ceq 'false'){0}else{1}
  if(@($events|Where-Object event -eq webapp_controller_callback).Count -ne $callbackExpected){$errors.Add("missing_or_duplicate_request_callback:$request")}
  $unavailable=@($events|Where-Object {$_.event -in @('webapp_controller_unavailable','webapp_controller_host_gone')}).Count
  $configured=@($events|Where-Object event -eq webapp_controller_configure_result)
  $configureExpected=if($unavailable -or $callbackExpected -eq 0){0}else{1}
  if($configured.Count -ne $configureExpected){$errors.Add("missing_or_duplicate_configuration_result:$request")}
  $closes=@($events|Where-Object event -eq webapp_controller_close_requested).Count;$closeResults=@($events|Where-Object event -eq webapp_controller_close_result)
  if($closes -ne $closeResults.Count){$errors.Add("unmatched_close_result:$request")}
  $summaries.Add([ordered]@{environment=[uint64]$row.count;request=$request;app=$appId;configured=@($events|Where-Object event -eq webapp_controller_configured).Count;installed=@($events|Where-Object event -eq webapp_controller_installed).Count;replaced=@($events|Where-Object event -eq webapp_controller_replaced).Count;native_navigation_success=@($events|Where-Object event -eq webapp_navigation_success).Count;close_requested=$closes;close_succeeded=@($closeResults|Where-Object success -CEQ 'true').Count;close_failed=@($closeResults|Where-Object success -CEQ 'false').Count})
 }
 foreach($row in @($Rows|Where-Object {$_.event -like 'webapp_controller_*' -and $_.event -ne 'webapp_controller_requested'})){if(-not $requests.ContainsKey([string]$row.navigation_id)){$errors.Add('controller_event_without_request')}}
 if(-not $environments.Count){$errors.Add('no_environment_requests_observed')}
 if(-not $requests.Count){$errors.Add('no_controller_requests_observed')}
 if($allocated -ne ($environments.Count+$requests.Count)){$errors.Add('allocated_identity_count_mismatch')}
 foreach($requestId in $requests.Keys){if($environments.ContainsKey($requestId)){$errors.Add('environment_request_identity_collision')}}
 foreach($row in @($Rows|Where-Object {$_.event -like 'webapp_environment_*'})){if(-not $environments.ContainsKey([string]$row.count)){$errors.Add('environment_event_without_request')}}
 foreach($row in @($Rows|Where-Object {$_.event -like 'webapp_navigation_*' -or $_.event -eq 'webapp_initial_navigate_result'})){if(-not $requests.ContainsKey([string]$row.navigation_id)){$errors.Add('navigation_event_without_request')}}
 foreach($expectedApp in $ExpectedApps){if(-not @($summaries|Where-Object app -eq $expectedApp).Count){$errors.Add("missing_reached_app_participant:$expectedApp")}}
 [ordered]@{complete=($errors.Count -eq 0);expected_observed_apps=$ExpectedApps;allocated_identity_count=$allocated;invalid_reasons=@($errors.ToArray());environment_requests=$environments.Count;controller_requests=$requests.Count;requests=@($summaries.ToArray());scope='Actual COM request/result counts through the declared end marker, not assumed UI invocation count; missing Close is not proof of leaked/freed memory; duplicate requests and acks are retained'}
}
Export-ModuleMember -Function New-ProductionSidebarFixture,Invoke-ProductionSidebarLifecycle,Get-ProductionSidebarLedger

function Get-ProductionSidebarPhaseCoverage {
 param([object[]]$Snapshots,$Before,$After,[bool]$ExpectWebApps)
 $errors=[Collections.Generic.List[string]]::new()
 if(-not $Before.available -or -not $After.available){$errors.Add('inventory_unavailable')}
 $beforeKeys=@($Before.identities|ForEach-Object {"$($_.pid):$($_.created_ticks)"}|Sort-Object)
 $afterKeys=@($After.identities|ForEach-Object {"$($_.pid):$($_.created_ticks)"}|Sort-Object)
 if(($beforeKeys -join '|') -cne ($afterKeys -join '|')){$errors.Add('webapps_identity_churn')}
 if($ExpectWebApps -and -not $beforeKeys.Count){$errors.Add('missing_webapps_roots')}
 if($Snapshots.Count -ne 3){$errors.Add('missing_snapshot')}
 foreach($identity in @($Before.identities)+@($After.identities)) {
  if($null -eq $identity.created_ticks -or $identity.pid -le 0){$errors.Add('invalid_webapps_identity');continue}
  for($i=0;$i -lt $Snapshots.Count;$i++){if(-not @($Snapshots[$i].ProcessDetails|Where-Object {$_.process_id -eq $identity.pid -and $_.created_ticks -eq $identity.created_ticks}).Count){$errors.Add("webapps_identity_missing_snapshot:$i/$($identity.pid)")}}
 }
 foreach($snapshot in $Snapshots){if($snapshot.CoverageErrors.Count -gt 0 -or $snapshot.RacedExitCount -gt 0 -or $snapshot.PrivateSource -cne 'native' -or $null -eq $snapshot.NativePrivateBytes){$errors.Add('native_counter_coverage_gap')}}
 [ordered]@{eligible=($errors.Count -eq 0);invalid_reasons=@($errors.ToArray());scope='Stable WebApps identities before/after window, covered by ALL native snapshots; churn unavailable, never zero'}
}
Export-ModuleMember -Function New-ProductionSidebarFixture,Invoke-ProductionSidebarLifecycle,Get-ProductionSidebarLedger,Get-ProductionSidebarPhaseCoverage
