Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/measurements.psm1" -Force
Import-Module "$PSScriptRoot/sidebar-preflight.psm1" -Force
$source=Get-Content -LiteralPath "$PSScriptRoot/sidebar-preflight.psm1" -Raw
if($source -match 'SetForegroundWindow|SetFocus|MoveFocus|SendKeys|SendInput|mouse_event|GetWindowText|Start-Sleep|\.Invoke\(|\.Toggle\('){throw 'Passive sidebar preflight contains an action/text/delay API'}
$imports=@([regex]::Matches($source,'public static extern [^;]+? (\w+)\(')|ForEach-Object {$_.Groups[1].Value})
$expected=@('GetForegroundWindow','GetWindowThreadProcessId','GetClassName','GetAncestor','GetConsoleWindow')
if(@(Compare-Object $imports $expected).Count){throw 'Unexpected native import in passive preflight'}
$unknown=[BowserSidebarPreflight]::Read([IntPtr]::Zero)
if($unknown.ownership_stable -or $unknown.status -cne 'null_handle' -or $unknown.pid -ne 0){throw 'Null ownership must remain unknown'}
$observation=Get-ProductionSidebarPreflightSnapshot -Boundary 'contract-no-app'
if($observation.expected_root_pid -ne 0 -or $observation.owned_root.ownership_stable -or $observation.scope -notmatch 'non-atomic'){throw 'Absent app is not owned focus proof'}
$baseline=Get-Content -LiteralPath "$PSScriptRoot/production-baseline.ps1" -Raw
if(-not $baseline.Contains('if(-not $SidebarCapabilityPreflight){[void][BowserProductionNative]::ShowWindow(')){throw 'Passive preflight must skip inherited activating restore'}
'Passive sidebar capability/foreground contracts passed.'
