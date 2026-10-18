Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/measurements.psm1" -Force
$beforeResolve=(Get-Command Resolve-BowserExecutable -ErrorAction Stop).ScriptBlock
Import-Module "$PSScriptRoot/bookmark-bar-lifecycle.psm1" -Force
if((Get-Command Resolve-BowserExecutable -ErrorAction Stop).ScriptBlock -ne $beforeResolve){throw 'Bar module replaced caller measurement exports'}
foreach($name in @('Get-ProcessInventory','Get-ProcessGroupMembers','Get-ProcessGroupSnapshot','Assert-MeasurementInputHashesUnchanged')){Get-Command $name -ErrorAction Stop|Out-Null}
function Check($value,$name){if(-not $value){throw "Failed: $name"}}
$token='0123456789abcdef0123456789abcdef';$base='http://127.0.0.1:12345'
$a=New-ProductionBookmarkBarFixture -BaseUrl $base -Token $token
$b=New-ProductionBookmarkBarFixture -BaseUrl $base -Token $token -Updated
$alines=@($a -split "\r?\n"|Where-Object {$_});$blines=@($b -split "\r?\n"|Where-Object {$_})
Check ($alines.Count -eq 819 -and $blines.Count -eq 819 -and $alines[0] -ceq 'bowser-bookmarks-v2') 'ordinary 818-record store fixture'
Check ($alines[1] -ceq "1`t-`tB`tFixture bookmark A`t$base/light?token=$($token)_a`t127.0.0.1`t1700000000") 'fixed first bookmark identity/label/destination A'
Check ($blines[1] -ceq "1`t-`tB`tFixture bookmark updated B`t$base/light?token=$($token)_b`t127.0.0.1`t1700000000") 'same ID changed label AND destination B'
Check (($alines[2..818] -join "`n") -ceq ($blines[2..818] -join "`n")) 'other synthetic bookmarks unchanged'
$directory=Join-Path ([IO.Path]::GetTempPath()) ('bowser-bar-fixture-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory|Out-Null
try {
 $path=Join-Path $directory 'bookmarks-v2.txt';$temp="$path.fixture.tmp"
 [IO.File]::WriteAllText($path,$a);$length=(Get-Item $path).Length
 [IO.File]::WriteAllText($temp,$b);[IO.File]::Replace($temp,$path,$null)
 Check (([IO.File]::ReadAllText($path)) -ceq $b -and (Get-Item $path).Length -ne $length -and -not (Test-Path $temp)) 'atomic disposable same-file update changes native cache stamp/content'
} finally {Remove-Item -LiteralPath $directory -Recurse -Force}
$id=[ordered]@{pid=7;created_ticks=8};$before=[ordered]@{available=$true;identities=@($id)};$after=[ordered]@{available=$true;identities=@($id)}
$snapshot=[pscustomobject]@{CoverageErrors=@();RacedExitCount=0;PrivateSource='native';NativePrivateBytes=100;ProcessDetails=@([pscustomobject]@{process_id=7;created_ticks=8})}
Check (Get-ProductionBookmarkBarCoverage @($snapshot,$snapshot,$snapshot) $before $after).eligible 'stable owned identity native counter coverage'
$missing=[pscustomobject]@{CoverageErrors=@();RacedExitCount=0;PrivateSource='native';NativePrivateBytes=100;ProcessDetails=@([pscustomobject]@{process_id=9;created_ticks=10})}
Check (-not (Get-ProductionBookmarkBarCoverage @($missing,$snapshot,$snapshot) $before $after).eligible) 'early missing owned identity invalidates whole phase'
Check (-not (Get-ProductionBookmarkBarCoverage @($snapshot,$snapshot) $before $after).eligible) 'missing snapshot unavailable'
Check (-not (Get-ProductionBookmarkBarCoverage @($snapshot,$snapshot,$snapshot) $before ([ordered]@{available=$true;identities=@([ordered]@{pid=7;created_ticks=9})})).eligible) 'identity churn unavailable not lower memory'
Check (-not (Get-ProductionBookmarkBarCoverage @($snapshot,$snapshot,$snapshot) ([ordered]@{available=$false;identities=@()}) $after).eligible) 'empty/unavailable inventory not exit/coverage proof'
$module=Get-Content "$PSScriptRoot/bookmark-bar-lifecycle.psm1" -Raw
Check ($module -notmatch 'SetForegroundWindow|AttachThreadInput|SetFocus|MoveFocus|SendKeys|\.Invoke\(|\.Toggle\(|mouse_event|SetCursorPos') 'no action-pattern guessing/focus repair/input alternative'
Check (@([regex]::Matches($module,'\[BowserSidebarInput\]::ClickOwned\(')).Count -eq 1) 'single guarded physical click implementation, no retry'
Check ($module.Contains('[IO.File]::Replace($temporary,$path,$null)') -and $module.Contains('old_label_count') -and $module.Contains('bar-observation-end')) 'atomic update, stale DOM rejection and completeness marker'
$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'production-baseline.ps1'),[ref]$tokens,[ref]$errors)
Check ($errors.Count -eq 0) 'production bar harness parses'
$guards=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.IfStatementAst] -and $node.Clauses.Count -eq 1 -and $node.Clauses[0].Item1.Extent.Text -match '^\$BookmarkBarPilot -and' -and $node.Clauses[0].Item1.Extent.Text.Contains('-not $CandidateExe') -and $node.Clauses[0].Item2.Extent.Text.Contains("throw 'Bookmark bar baseline requires one fixed ordinary updater-off positive light pair, startup-only timing and no mixed workload'")},$true))
Check ($guards.Count -eq 1) 'one actual bar pilot parameter guard'
$guard=[scriptblock]::Create($guards[0].Extent.Text)
function Refused([hashtable]$Changes){
 $BookmarkBarPilot=$true;$CandidateExe='candidate.exe';$StartupOnly=$true;$DiagnosticAutoUpdateOff=$true
 $SidebarLifecyclePilot=$false;$SidebarLifecycleCompare=$false;$SidebarRouteSmoke=$false;$SidebarCapabilityPreflight=$false;$IncludeSurfaceSmoke=$false;$IncludeTabFlow=$false;$DiagnosticInteractionPilot=$false;$DiagnosticDetailPilot=$false
 $Pairs=1;$PositiveOnly=$true;$Fixtures=@('light');$MeasurementMode='timing'
 foreach($key in $Changes.Keys){Set-Variable -Name $key -Value $Changes[$key]}
 try {& $guard;return $false}catch{return $true}
}
Check (-not (Refused @{})) 'one fixed ordinary ON/OFF fresh/reused plan accepted'
foreach($case in @(@{CandidateExe=''},@{StartupOnly=$false},@{DiagnosticAutoUpdateOff=$false},@{SidebarLifecyclePilot=$true},@{SidebarLifecycleCompare=$true},@{IncludeSurfaceSmoke=$true},@{IncludeTabFlow=$true},@{Pairs=2},@{PositiveOnly=$false},@{Fixtures=@('demanding')},@{MeasurementMode='resource'})){Check (Refused $case) 'mixed/unbounded bar workload refused'}
$source=Get-Content "$PSScriptRoot/production-baseline.ps1" -Raw
Check ($source.Contains('bookmarks_bar=newtab') -and $source.Contains('tabs_new_tab_page=blank') -and $source.Contains('toolbar_buttons=back,forward,reload,site-info,split,reader,extensions,downloads,newtab')) 'explicit ordinary opt-in/blank/new-tab-button setup'
Check ($source.Contains('if(-not $BookmarkBarPilot){if(-not $SidebarLifecyclePilot)')) 'bar lane skips inherited activating restore'
Check ($source.Contains('Get-ProductionTokenSegmentReadiness $events $probe.token') -and $source.Contains('OFF: native diagnostic stream unavailable')) 'ON native segments and honest OFF exclusion'
'Bookmark bar baseline contracts passed; no GUI input was injected.'
