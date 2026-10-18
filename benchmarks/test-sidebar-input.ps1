Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/sidebar-input.psm1" -Force
if([BowserSidebarInput]::InputSize() -ne $(if([Environment]::Is64BitProcess){40}else{28})){throw 'Unexpected SendInput structure ABI'}
if([BowserSidebarInput]::Absolute(-1920,-1920,3840) -ne 0 -or [BowserSidebarInput]::Absolute(1919,-1920,3840) -ne 65535){throw 'Physical virtual-desktop normalization failed'}
$rejected=$false;try{[void][BowserSidebarInput]::Absolute(1920,-1920,3840)}catch{$rejected=$true};if(-not $rejected){throw 'Out-of-desktop coordinates must fail closed'}
$noRoot=[BowserSidebarInput]::ClickOwned([IntPtr]::Zero,0,100,100)
if($noRoot.guard_ok -or $noRoot.sent_count -ne 0 -or $noRoot.refusal -cne 'missing_owned_root'){throw 'Missing root must never inject input'}
$source=Get-Content "$PSScriptRoot/sidebar-input.psm1" -Raw
if($source -match 'SetForegroundWindow|AttachThreadInput|SetFocus|MoveFocus|SendKeys|GetWindowText|mouse_event|SetCursorPos'){throw 'Unexpected activation workaround or text API'}
if(@([regex]::Matches($source,'r\.sent_count=SendInput\(')).Count -ne 1){throw 'Owned click must have one injection call, no fallback/retry'}
$baseline=Get-Content "$PSScriptRoot/production-baseline.ps1" -Raw
if(-not $baseline.Contains('if(-not $SidebarRouteSmoke){if(-not $SidebarCapabilityPreflight){[void][BowserProductionNative]::ShowWindow(')){throw 'Route smoke must skip inherited activating restore'}
'Owned physical sidebar click contracts passed; no input was injected by these tests.'
