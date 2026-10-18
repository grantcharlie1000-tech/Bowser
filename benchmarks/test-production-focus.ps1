Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/production-focus.psm1" -Force
function Check($condition,$name){if(-not $condition){throw "Failed: $name"}}
Check ([BowserFocusObservation]::StructureSize() -eq $(if([IntPtr]::Size -eq 8){72}else{48})) 'GUITHREADINFO native layout size'
Check ([BowserFocusObservation]::Relation($false,$false,$false,$false,$false) -ceq 'none') 'null focus is none, never toolbar'
Check ([BowserFocusObservation]::Relation($true,$false,$true,$true,$true) -ceq 'unknown') 'missing owner cannot become owned'
Check ([BowserFocusObservation]::Relation($true,$true,$true,$false,$false) -ceq 'owned-root') 'root identity'
Check ([BowserFocusObservation]::Relation($true,$true,$false,$true,$false) -ceq 'owned-descendant') 'child ancestry, not specific controller'
Check ([BowserFocusObservation]::Relation($true,$true,$false,$false,$true) -ceq 'owned-popup') 'owned popup root'
Check ([BowserFocusObservation]::Relation($true,$true,$false,$false,$false) -ceq 'outside-owned-root') 'external root retained'
$invalid=Get-ProductionNativeFocusSnapshot -RootPid 1 -RootWindow ([IntPtr]::Zero) -Boundary contract-invalid-root -Attempt 0
Check (-not $invalid.native.query_ok -and $invalid.native.error -eq -1) 'invalid owned root is an explicit failed observation'
Check ($invalid.end_qpc -ge $invalid.start_qpc) 'observer interval retained'
$module=Get-Content "$PSScriptRoot/production-focus.psm1" -Raw
$boundary=$module.IndexOf('# Pure coverage accounting.')
Check ($boundary -gt 0) 'native observation implementation boundary exists'
$native=$module.Substring(0,$boundary)
Check (-not ($native -match 'GetWindowText|SetFocus|SetForegroundWindow|SendInput|SendWait|MoveFocus|Thread.Sleep|Start-Sleep')) 'observer cannot read text or change focus/input/timing policy'
Check (-not ($module.Substring($boundary) -match 'DllImport|Add-Type|\[BowserFocusObservation\]::')) 'coverage accounting cannot add native calls'
'Native focus observation contracts passed.'
function FocusRow($event,$tick,$success=''){[pscustomobject]@{event=$event;qpc_ticks=[string]$tick;success=$success;navigation_id='1';tab_id='0';view='toolbar'}}
$complete=@((FocusRow interaction_focus_controller_created 1),(FocusRow interaction_focus_registration_started 2),(FocusRow interaction_focus_got_registration 3 true),(FocusRow interaction_focus_lost_registration 4 true),(FocusRow interaction_chrome_move_request 5),(FocusRow interaction_chrome_move_result 6 true))
Check (Get-ProductionControllerFocusCoverage $complete -ExpectedViews @('toolbar')).complete 'registered unique controller and single move result'
Check (-not (Get-ProductionControllerFocusCoverage ($complete|Where-Object event -ne interaction_focus_got_registration) -ExpectedViews @('toolbar')).complete) 'missing hook rejects attribution'
Check (-not (Get-ProductionControllerFocusCoverage ($complete+@((FocusRow interaction_focus_controller_created 7))) -ExpectedViews @('toolbar')).complete) 'duplicate controller rejects attribution'
Check (-not (Get-ProductionControllerFocusCoverage ($complete|Where-Object event -ne interaction_chrome_move_result) -ExpectedViews @('toolbar')).complete) 'missing original-call result rejects attribution'
$bad=$complete|ForEach-Object {$_|Select-Object *};$bad[2].success='false'
Check (-not (Get-ProductionControllerFocusCoverage $bad -ExpectedViews @('toolbar')).complete) 'registration failure rejects attribution'
$early=@($complete[0],$complete[1],(FocusRow interaction_controller_got_focus 2),$complete[2],$complete[3],$complete[4],$complete[5])
$coverage=Get-ProductionControllerFocusCoverage $early -ExpectedViews @('toolbar')
Check ($coverage.complete -and $coverage.controllers[0].callback_coverage_start_qpc -eq 4 -and $coverage.controllers[0].callbacks_before_coverage -eq 1) 'earlier callback retained outside complete coverage window'

Check (-not (Get-ProductionControllerFocusCoverage $complete -ExpectedViews @('toolbar','content')).complete) 'entirely missing known participant rejects coverage'
