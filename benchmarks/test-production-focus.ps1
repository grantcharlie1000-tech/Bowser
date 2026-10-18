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
Check (-not ($module -match 'GetWindowText|SetFocus|SetForegroundWindow|SendInput|SendWait|MoveFocus|Thread.Sleep|Start-Sleep')) 'observer cannot read text or change focus/input/timing policy'
'Native focus observation contracts passed.'
