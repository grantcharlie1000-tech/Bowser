# Opt-in ordinary NewTab bookmarks-bar baseline, not default startup or typing latency.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/measurements.psm1"
Import-Module "$PSScriptRoot/sidebar-input.psm1"
function New-ProductionBookmarkBarFixture {
 param([ValidatePattern('^http://127\.0\.0\.1:[0-9]{1,5}$')][string]$BaseUrl,[ValidatePattern('^[0-9a-f]{32}$')][string]$Token,[switch]$Updated)
 $text=[Text.StringBuilder]::new("bowser-bookmarks-v2`n")
 $label=if($Updated){'Fixture bookmark updated B'}else{'Fixture bookmark A'}
 $suffix=if($Updated){'b'}else{'a'}
 [void]$text.AppendLine("1`t-`tB`t$label`t$BaseUrl/light?token=$($Token)_$suffix`t127.0.0.1`t1700000000")
 for($i=2;$i -le 818;$i++){[void]$text.AppendLine("$i`t-`tB`tSynthetic bookmark $i`thttps://bookmark.example.test/$i`tbookmark.example.test`t1700000000")}
 $text.ToString()
}
function Get-ProductionBookmarkBarCoverage {
 param([object[]]$Snapshots,$Before,$After)
 $errors=[Collections.Generic.List[string]]::new()
 if(-not $Before.available -or -not $After.available){$errors.Add('owned_inventory_unavailable')}
 $beforeKeys=@($Before.identities|ForEach-Object {"$($_.pid):$($_.created_ticks)"}|Sort-Object)
 $afterKeys=@($After.identities|ForEach-Object {"$($_.pid):$($_.created_ticks)"}|Sort-Object)
 if(-not $beforeKeys.Count -or ($beforeKeys -join '|') -cne ($afterKeys -join '|')){$errors.Add('owned_identity_missing_or_churn')}
 if($Snapshots.Count -ne 3){$errors.Add('missing_snapshot')}
 foreach($identity in @($Before.identities)+@($After.identities)){
  if($identity.pid -le 0 -or $null -eq $identity.created_ticks){$errors.Add('invalid_owned_identity');continue}
  foreach($snapshot in $Snapshots){if(-not @($snapshot.ProcessDetails|Where-Object {$_.process_id -eq $identity.pid -and $_.created_ticks -eq $identity.created_ticks}).Count){$errors.Add('owned_identity_missing_snapshot')}}
 }
 foreach($snapshot in $Snapshots){if($snapshot.CoverageErrors.Count -or $snapshot.RacedExitCount -or $snapshot.PrivateSource -cne 'native' -or $null -eq $snapshot.NativePrivateBytes){$errors.Add('native_counter_coverage_gap')}}
 [ordered]@{eligible=($errors.Count -eq 0);invalid_reasons=@($errors.ToArray());scope='All owned/profile process identities stable before/after and covered in all three native snapshots; churn unavailable, not lower memory'}
}
# Detail-only correctness ledger; setter success is not freed-memory evidence.
function Get-ProductionBookmarkBarHintCoverage {
 param([object[]]$Events)
 $Events=@($Events|Where-Object {$null -ne $_})
 $errors=[Collections.Generic.List[string]]::new()
 $bar=@($Events|Where-Object {$_.view -ceq 'bookmark_bar'})
 $hints=@($Events|Where-Object {$_.event -like 'chrome_bar_memory_*'})
 $expected='chrome_bar_memory_low_requested,chrome_bar_memory_low_succeeded,chrome_bar_memory_normal_requested,chrome_bar_memory_normal_succeeded,chrome_bar_memory_low_requested,chrome_bar_memory_low_succeeded'
 if((@($hints|ForEach-Object {$_.event}) -join ',') -cne $expected -or @($hints|Where-Object {$_.view -cne 'bookmark_bar'}).Count){$errors.Add('missing_failed_unsupported_or_wrong_hint_sequence')}
 $ids=@($hints|ForEach-Object {$_.count}|Sort-Object -Unique)
 $ready=@($bar|Where-Object {$_.event -ceq 'chrome_host_ready'})
 if($ids.Count -ne 1 -or $ids[0] -notmatch '^[1-9][0-9]*$' -or $ready.Count -ne 1 -or $ready[0].count -cne $ids[0]){$errors.Add('unknown_or_changed_bar_controller_identity')}
 if(-not $errors.Count -and @($bar|Where-Object {$_.event -in @('chrome_host_shown','chrome_host_hidden','chrome_host_close_requested','chrome_host_close_succeeded') -and $_.count -cne $ids[0]}).Count){$errors.Add('host_lifecycle_identity_mismatch')}
 foreach($event in @('chrome_host_requested','chrome_host_close_requested','chrome_host_close_succeeded')){if(@($bar|Where-Object {$_.event -ceq $event}).Count -ne 1){$errors.Add('missing_or_recreated_host_lifecycle')}}
 if(@($bar|Where-Object {$_.event -ceq 'chrome_host_close_failed'}).Count){$errors.Add('failed_host_close')}
 foreach($event in $bar){$clock=[long]0;if(-not [long]::TryParse([string]$event.qpc_ticks,[ref]$clock) -or $clock -le 0){$errors.Add('unknown_native_event_clock')}}
 if(-not $errors.Count){
  for($i=1;$i -lt $hints.Count;$i++){if([long]$hints[$i].qpc_ticks -le [long]$hints[$i-1].qpc_ticks){$errors.Add('nonincreasing_hint_request_result_clock')}}
  $requested=@($bar|Where-Object {$_.event -ceq 'chrome_host_requested'})[0]
  $close=@($bar|Where-Object {$_.event -ceq 'chrome_host_close_requested'})[0]
  $closed=@($bar|Where-Object {$_.event -ceq 'chrome_host_close_succeeded'})[0]
  if([long]$requested.qpc_ticks -ge [long]$ready[0].qpc_ticks -or [long]$ready[0].qpc_ticks -ge [long]$hints[0].qpc_ticks -or [long]$close.qpc_ticks -le [long]$hints[-1].qpc_ticks -or [long]$closed.qpc_ticks -le [long]$close.qpc_ticks){$errors.Add('ready_or_paired_close_lifecycle_order_unproven')}
 }
 if(-not $errors.Count){
  $hidden=@($bar|Where-Object {$_.event -ceq 'chrome_host_hidden'})
  $shown=@($bar|Where-Object {$_.event -ceq 'chrome_host_shown'})
  if($hidden.Count -ne 2 -or -not @($shown|Where-Object {[long]$_.qpc_ticks -lt [long]$hidden[0].qpc_ticks}).Count -or [long]$hidden[0].qpc_ticks -ge [long]$hints[0].qpc_ticks -or [long]$hints[1].qpc_ticks -ge [long]$hints[2].qpc_ticks -or -not @($shown|Where-Object {[long]$_.qpc_ticks -gt [long]$hints[3].qpc_ticks -and [long]$_.qpc_ticks -lt [long]$hidden[1].qpc_ticks}).Count -or [long]$hidden[1].qpc_ticks -ge [long]$hints[4].qpc_ticks){$errors.Add('hide_low_restore_before_reveal_order_unproven')}
 }
 [ordered]@{complete=($errors.Count -eq 0);errors=@($errors.ToArray());scope='Detail-only API request/result and retained-host ordering, not reclaimed private bytes or physical presentation'}
}
function Invoke-ProductionBookmarkBarLifecycle {
 [CmdletBinding()]
 param([uint32]$RootPid,[string]$InitialToken,[string]$Profile,[string]$BaseUrl,[string]$RequestFile,[string]$ContaminationMarker,[Parameter(Mandatory)][AllowEmptyCollection()][Collections.Generic.List[object]]$Trace)
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
 $rootWindow=[BowserProductionNative]::FindHost($RootPid)
 $state=@{bar_window=[IntPtr]::Zero;bar_tid=[uint32]0}
 function Qpc {[Diagnostics.Stopwatch]::GetTimestamp()}
 function Root {
  $pidNow=[uint32]0;$tid=[BowserProductionNative]::GetWindowThreadProcessId($rootWindow,[ref]$pidNow)
  if($rootWindow -eq [IntPtr]::Zero -or $tid -eq 0 -or $pidNow -ne $RootPid -or [BowserProductionNative]::FindHost($RootPid) -ne $rootWindow){throw 'Bookmark bar owned native root changed'}
  [Windows.Automation.AutomationElement]::FromHandle($rootWindow)
 }
 function Visible($parent,[string]$name,$type){
  $filter=[Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,$type))
  @($parent.FindAll([Windows.Automation.TreeScope]::Descendants,$filter)|Where-Object {-not $_.Current.IsOffscreen -and $_.Current.IsEnabled -and $_.Current.BoundingRectangle.Width -gt 5 -and $_.Current.BoundingRectangle.Height -gt 5})
 }
 function Toolbar-Document($element){
  $walker=[Windows.Automation.TreeWalker]::ControlViewWalker
  for($depth=0;$depth -lt 32 -and $null -ne $element;$depth++){
   if($element.Current.ControlType -eq [Windows.Automation.ControlType]::Document){return $element}
   $element=$walker.GetParent($element)
  }
  throw 'Chrome document ancestry unavailable; no button action'
 }
 function Click-Target($element,[string]$label){
  $entry=[ordered]@{kind='bar-owned-point-click';label=$label;lookup_finished_qpc=(Qpc);dispatch_qpc=$null;return_qpc=$null;click=$null;point_target_matched=$false;scope='Single guarded ordinary physical click, exact source-bound UIA button; no action pattern, focus repair or retry'}
  $Trace.Add($entry)
  $current=$element.Current;$rect=$current.BoundingRectangle
  $entry['target_bounds']=[ordered]@{left=$rect.Left;top=$rect.Top;width=$rect.Width;height=$rect.Height;provider_pid=$current.ProcessId};$entry['name_matches']=($current.Name -ceq $label)
  if(-not $entry.name_matches -or -not $current.IsEnabled -or $current.IsOffscreen -or $current.ControlType -ne [Windows.Automation.ControlType]::Button -or $rect.Width -le 5 -or $rect.Height -le 5){throw 'Bookmark bar click target is not a current enabled visible button'}
  $x=[int][Math]::Floor($rect.Left+$rect.Width/2);$y=[int][Math]::Floor($rect.Top+$rect.Height/2)
  $hit=[Windows.Automation.AutomationElement]::FromPoint([Windows.Point]::new($x,$y));$key=$element.GetRuntimeId() -join ':'
  for($depth=0;$depth -lt 16 -and $null -ne $hit;$depth++){
   if(($hit.GetRuntimeId() -join ':') -ceq $key){$entry.point_target_matched=$true;break}
   $hit=[Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($hit)
  }
  [void](Root)
  if(-not $entry.point_target_matched -or [BowserProductionNative]::GetForegroundWindow() -ne $rootWindow){throw 'Source-bound button is occluded or owned root not foreground; no click'}
  $entry.dispatch_qpc=Qpc;$entry.click=[BowserSidebarInput]::ClickOwned($rootWindow,$RootPid,$x,$y);$entry.return_qpc=Qpc
  if(-not $entry.click.guard_ok -or $entry.click.sent_count -ne 3){throw 'One bookmark-bar physical input batch refused/incomplete; no retry'}
  $entry
 }
 function New-Tab {
  $root=Root
  $inputs=@(Visible $root 'Address and search bar' ([Windows.Automation.ControlType]::Edit))
  if($inputs.Count -ne 1){throw 'Unique HTML toolbar address source binding unavailable'}
  $toolbar=Toolbar-Document $inputs[0]
  $buttons=@(Visible $toolbar 'New tab' ([Windows.Automation.ControlType]::Button))
  if($buttons.Count -ne 1){throw 'Unique source-bound HTML toolbar New tab button unavailable'}
  Click-Target $buttons[0] 'New tab'
 }
 function Read-Bar([string]$name){
  $window=[BowserProductionNative]::FindSurface($RootPid,'BowserBookmarkBar',100,10)
  if($window -eq [IntPtr]::Zero){return $null}
  $nativePid=[uint32]0;$tid=[BowserProductionNative]::GetWindowThreadProcessId($window,[ref]$nativePid)
  if($nativePid -ne $RootPid -or $tid -eq 0 -or [BowserSidebarInput]::GetAncestor($window,2) -ne $rootWindow){throw 'Bookmark bar native container ownership unknown'}
  $bars=@(Visible ([Windows.Automation.AutomationElement]::FromHandle($window)) 'Bookmarks bar' ([Windows.Automation.ControlType]::ToolBar))
  if($bars.Count -ne 1){return $null}
  $items=@(Visible $bars[0] $name ([Windows.Automation.ControlType]::Button))
  if($items.Count -ne 1){return $null}
  $oldLabelCount=@($bars[0].FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Fixture bookmark A'))).Count
  if($name -ceq 'Fixture bookmark updated B' -and $oldLabelCount -ne 0){return $null}
  [pscustomobject]@{window=$window;tid=$tid;element=$items[0];old_label_count=$oldLabelCount}
 }
 function Wait-Bar([string]$name,[string]$stage,[long]$start){
  $wait=[Diagnostics.Stopwatch]::StartNew();$bar=$null
  do{$bar=Read-Bar $name;if($null -ne $bar){break};Start-Sleep -Milliseconds 20}while($wait.Elapsed.TotalSeconds -lt 20)
  if($null -eq $bar){throw "Source-bound bookmark bar/expected snapshot unavailable: $stage"}
  if($state.bar_window -ne [IntPtr]::Zero -and ($state.bar_window -ne $bar.window -or $state.bar_tid -ne $bar.tid)){throw 'Retained bookmark bar HWND/TID changed during workload'}
  $state.bar_window=$bar.window;$state.bar_tid=$bar.tid
  $Trace.Add([ordered]@{kind='bar-visible';stage=$stage;name=$name;qpc=(Qpc);action_start_qpc=$start;container_hwnd=$bar.window.ToInt64();container_tid=$bar.tid;old_label_count=$bar.old_label_count;scope='Unique owned native container + visible HTML toolbar + expected synthetic bookmark; HWND retention is not exclusive renderer/controller attribution or physical presentation'})
 }
 function Click-Bookmark([string]$name){$bar=Read-Bar $name;if($null -eq $bar){throw 'Expected bookmark snapshot/button missing before action'};Click-Target $bar.element $name}
 function Wait-FixtureHidden([string]$token){
  $wait=[Diagnostics.Stopwatch]::StartNew();$acks=@();$docs=@();$hidden=$false;$titleMatched=$false
  do{
   $root=Root;$docs=@(Visible $root "BOWSER_PERF_READY_$token" ([Windows.Automation.ControlType]::Document))
   $acks=@(Get-Content -LiteralPath $RequestFile | ConvertFrom-Json | Where-Object {$_.path -ceq '/ready' -and $_.token -ceq $token})
   $titleMatched=[BowserProductionNative]::Title($rootWindow).Contains("BOWSER_PERF_READY_$token")
   $hidden=([BowserProductionNative]::FindSurface($RootPid,'BowserBookmarkBar',100,10) -eq [IntPtr]::Zero -and @(Visible $root 'Bookmarks bar' ([Windows.Automation.ControlType]::ToolBar)).Count -eq 0)
   if(($acks.Count -eq 1 -and $docs.Count -eq 1 -and $titleMatched -and $hidden) -or $acks.Count -gt 1){break};Start-Sleep -Milliseconds 20
  }while($wait.Elapsed.TotalSeconds -lt 20)
  $nativePid=[uint32]0;$tid=[BowserProductionNative]::GetWindowThreadProcessId($state.bar_window,[ref]$nativePid)
  $class=[Text.StringBuilder]::new(128);$classRead=[BowserSidebarInput]::GetClassName($state.bar_window,$class,$class.Capacity)
  $retained=($state.bar_window -ne [IntPtr]::Zero -and $tid -eq $state.bar_tid -and $nativePid -eq $RootPid -and $classRead -gt 0 -and $class.ToString() -ceq 'BowserBookmarkBar' -and [BowserSidebarInput]::GetAncestor($state.bar_window,2) -eq $rootWindow -and -not [BowserProductionNative]::IsWindowVisible($state.bar_window))
  $Trace.Add([ordered]@{kind='bar-fixture-ready';token=$token;qpc=(Qpc);ack_count=$acks.Count;visible_document_count=$docs.Count;owned_title_matched=$titleMatched;bar_hidden=$hidden;bar_hwnd_retained=$retained;native_navigation_success=$null;scope='Independent unique-token HTTP/owned title/visible page and hidden retained bar; native segment checked after log flush in ON arm'})
  if($acks.Count -ne 1 -or $docs.Count -ne 1 -or -not $titleMatched -or -not $hidden -or -not $retained){throw 'Bookmark action/readiness/hide/retention guard failed'}
 }
 function Read-Counts {
  $root=Root;$strips=@(Visible $root 'Tabs' ([Windows.Automation.ControlType]::Tab))
  $tabCount=$null
  if($strips.Count -eq 1){$tabCount=@($strips[0].FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::TabItem))).Count}
  $documents=@($root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Document))|Where-Object {-not $_.Current.IsOffscreen -and $_.Current.BoundingRectangle.Width -gt 5 -and $_.Current.BoundingRectangle.Height -gt 5})
  [ordered]@{owned_tablist_count=$strips.Count;tab_count=$tabCount;visible_document_count=$documents.Count;scope='Source-owned Tabs accessibility count; visible chrome/content documents, not renderer/controller counts'}
 }
 function Phase([string]$name,[int]$ExpectedTabs){
  $start=Qpc;$countsBefore=Read-Counts;Start-Sleep -Milliseconds 1000
  $snapshots=[Collections.Generic.List[object]]::new();$sampleCost=[Collections.Generic.List[double]]::new();$inventoryCost=[Collections.Generic.List[double]]::new()
  function Inventory {
   $t=Qpc;$inventory=@(Get-ProcessInventory);$members=@(Get-ProcessGroupMembers -RootPid $RootPid -Profile $Profile -Inventory $inventory)
   $ids=@($members|ForEach-Object {[ordered]@{pid=$_.Pid;created_ticks=$_.CreationTicks}})
   $inventoryCost.Add(((Qpc)-$t)*1000.0/[Diagnostics.Stopwatch]::Frequency)
   [ordered]@{available=($inventory.Count -gt 0 -and @($members|Where-Object {$_.Pid -eq $RootPid -and $null -ne $_.CreationTicks}).Count -eq 1);identities=$ids}
  }
  $before=Inventory
  for($i=0;$i -lt 3;$i++){$t=Qpc;$snapshots.Add((Get-ProcessGroupSnapshot -RootPid $RootPid -Profile $Profile));$sampleCost.Add(((Qpc)-$t)*1000.0/[Diagnostics.Stopwatch]::Frequency);if($i -lt 2){Start-Sleep -Milliseconds 250}}
  $after=Inventory;$countsAfter=Read-Counts;$coverage=Get-ProductionBookmarkBarCoverage -Snapshots @($snapshots.ToArray()) -Before $before -After $after
  $logicalCounts=($countsBefore.owned_tablist_count -eq 1 -and $countsAfter.owned_tablist_count -eq 1 -and $countsBefore.tab_count -eq $ExpectedTabs -and $countsAfter.tab_count -eq $ExpectedTabs)
  $Trace.Add([ordered]@{kind='bar-resource-phase';name=$name;start_qpc=$start;end_qpc=(Qpc);settle_ms=1000;between_samples_ms=250;snapshots=@($snapshots.ToArray());owned_inventory_before=$before;owned_inventory_after=$after;sampling_overhead_ms=@($sampleCost.ToArray());inventory_overhead_ms=@($inventoryCost.ToArray());expected_tab_count=$ExpectedTabs;counts_before=$countsBefore;counts_after=$countsAfter;eligible=($coverage.eligible -and $logicalCounts);coverage=$coverage;scope='Whole-tree native private commit at this opt-in multi-tab UIA-conditioned workload; no per-view RAM, default-startup, lifetime peak or gain claim'})
  if(-not $coverage.eligible -or -not $logicalCounts){throw "Bookmark bar phase counter/identity coverage unavailable: $name"}
 }
 try {
  Invoke-ProductionSidebarActivation -RootPid $RootPid -InitialToken $InitialToken -Trace $Trace
  $first=New-Tab;Wait-Bar 'Fixture bookmark A' 'first' $first.dispatch_qpc;Phase 'bar-first-shown' 2
  [void](Click-Bookmark 'Fixture bookmark A');Wait-FixtureHidden "$($InitialToken)_a";Phase 'bar-first-hidden' 2
  $updated=New-ProductionBookmarkBarFixture -BaseUrl $BaseUrl -Token $InitialToken -Updated
  $path=Join-Path $Profile 'bookmarks-v2.txt';$temporary="$path.fixture.tmp"
  $old=Get-Item -LiteralPath $path;$oldLength=$old.Length;$oldTicks=$old.LastWriteTimeUtc.Ticks;$oldHash=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
  [IO.File]::WriteAllText($temporary,$updated);[IO.File]::Replace($temporary,$path,[NullString]::Value)
  $new=Get-Item -LiteralPath $path;$newHash=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
  if(($new.Length -eq $oldLength -and $new.LastWriteTimeUtc.Ticks -eq $oldTicks) -or $newHash -ceq $oldHash){throw 'Synthetic bookmark cache stamp/content did not change; no freshness claim'}
  $Trace.Add([ordered]@{kind='bar-hidden-fixture-update';qpc=(Qpc);same_bookmark_id=1;before_length=$oldLength;after_length=$new.Length;before_mtime_ticks=$oldTicks;after_mtime_ticks=$new.LastWriteTimeUtc.Ticks;content_changed=($newHash -cne $oldHash);scope='Only disposable synthetic bookmark label/URL changed while hidden, before reveal timer; no real user data'})
  $warm=New-Tab;Wait-Bar 'Fixture bookmark updated B' 'warm' $warm.dispatch_qpc;Phase 'bar-reshown' 3
  [void](Click-Bookmark 'Fixture bookmark updated B');Wait-FixtureHidden "$($InitialToken)_b";Phase 'bar-warm-hidden' 3
  $Trace.Add([ordered]@{kind='bar-observation-end';qpc=(Qpc);complete=$true;click_count=4;resource_phase_count=4;scope='Complete fixed external workload; ON native segments still require post-flush validation, OFF unavailable'})
 } finally {
  $partial=@($Trace|Where-Object {$_.Contains('click') -and $null -ne $_.click -and $_.click.guard_ok -and $_.click.sent_count -ne $_.click.planned_count})
  if($partial.Count){[ordered]@{reason='Partial physical input batch; all later native lanes blocked';clicks=$partial}|ConvertTo-Json -Depth 15|Set-Content -LiteralPath $ContaminationMarker -Encoding utf8}
 }
}
Export-ModuleMember -Function New-ProductionBookmarkBarFixture,Get-ProductionBookmarkBarCoverage,Get-ProductionBookmarkBarHintCoverage,Invoke-ProductionBookmarkBarLifecycle
