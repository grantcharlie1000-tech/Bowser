Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/measurements.psm1" -Force
$beforeResolve=(Get-Command Resolve-BowserExecutable -ErrorAction Stop).ScriptBlock
Import-Module "$PSScriptRoot/sidebar-lifecycle.psm1" -Force
if((Get-Command Resolve-BowserExecutable -ErrorAction Stop).ScriptBlock -ne $beforeResolve){throw 'Sidebar import replaced caller measurement exports'}
foreach($command in @('Get-ProcessInventory','Get-ProcessGroupSnapshot','Get-MeasurementInputHashes','Assert-MeasurementInputHashesUnchanged')){Get-Command $command -ErrorAction Stop | Out-Null}
function Check($value,$name){if(-not $value){throw "Failed: $name"}}
function Row($event,$request=0,$app=0,$success='') {[pscustomobject]@{event=$event;navigation_id=[string]$request;tab_id=[string]$app;success=$success;count='1';view='webapps'}}
$environment=@((Row webapp_environment_requested),(Row webapp_environment_callback 0 0 true),(Row webapp_environment_begin_result 0 0 true),(Row webapp_environment_configure_result 0 0 true))
$controller=@((Row webapp_controller_requested 2 1),(Row webapp_controller_callback 2 1 true),(Row webapp_controller_begin_result 2 1 true),(Row webapp_controller_configure_result 2 1 true),(Row webapp_controller_configured 2 1),(Row webapp_controller_installed 2 1),(Row webapp_navigation_success 2 1),(Row webapp_controller_close_requested 2 1),(Row webapp_controller_close_result 2 1 true))
$end=Row webapp_observation_end 0 0 true;$end.count='2'
$all=$environment+$controller+@($end)
$result=Get-ProductionSidebarLedger $all
Check ($result.complete -and $result.environment_requests -eq 1 -and $result.controller_requests -eq 1 -and $result.requests[0].close_succeeded -eq 1) 'numeric actual lifecycle ledger'
Check (-not (Get-ProductionSidebarLedger @()).complete) 'missing observation is unavailable not zero success'
Check (-not (Get-ProductionSidebarLedger @((Row webapp_navigation_success 2 1))).complete) 'orphan nonempty rows cannot imply zero complete participants'
Check (-not (Get-ProductionSidebarLedger $all -ExpectedApps @(1,2)).complete) 'entirely missing reached app is unavailable'
$invalidStatus=$all|ForEach-Object {$_|Select-Object *};$invalidStatus[-1].success='unknown'
Check (-not (Get-ProductionSidebarLedger $invalidStatus).complete) 'invalid result status rejects attribution'
Check (-not (Get-ProductionSidebarLedger ($all+@((Row webapp_controller_requested 2 1)))).complete) 'duplicate request identity rejected'
Check (-not (Get-ProductionSidebarLedger ($all|Where-Object event -ne webapp_controller_close_result)).complete) 'unmatched close result rejected'
Check (-not (Get-ProductionSidebarLedger ($all|Where-Object event -ne webapp_controller_begin_result)).complete) 'missing synchronous result rejected'
Check (-not (Get-ProductionSidebarLedger ($all+@((Row webapp_observation_error)))).complete) 'counter exhaustion or malformed identity invalidates attribution'
$failed=$all|ForEach-Object {$_|Select-Object *};($failed|Where-Object event -eq webapp_controller_close_result).success='false'
$result=Get-ProductionSidebarLedger $failed
Check ($result.complete -and $result.requests[0].close_failed -eq 1 -and $result.requests[0].close_succeeded -eq 0) 'failed Close retained as observed failure, not freed memory'
$second=$controller|ForEach-Object {$r=$_|Select-Object *;$r.navigation_id='3';$r};$end3=Row webapp_observation_end 0 0 true;$end3.count='3';$result=Get-ProductionSidebarLedger ($environment+$controller+$second+@((Row webapp_controller_replaced 3 1),$end3))
Check ($result.complete -and $result.controller_requests -eq 2 -and $result.requests[1].replaced -eq 1) 'duplicate app requests retained as source behavior, not deduplicated by analyzer'
$syncFailure=$environment+@((Row webapp_controller_requested 2 1),(Row webapp_controller_begin_result 2 1 false),$end)
Check (Get-ProductionSidebarLedger $syncFailure).complete 'known synchronous failure does not invent an expected callback'
Check (-not (Get-ProductionSidebarLedger ($all|Where-Object event -ne webapp_observation_end)).complete) 'parseable truncated stream without final marker is incomplete'
$wrongEnd=$all|ForEach-Object {$_|Select-Object *};$wrongEnd[-1].count='3'
Check (-not (Get-ProductionSidebarLedger $wrongEnd).complete) 'end allocation count detects omitted identity rows'
$identity=[pscustomobject]@{pid=7;created_ticks=8;parent=1}
$before=[ordered]@{available=$true;identities=@($identity)};$after=[ordered]@{available=$true;identities=@($identity)}
$snapshot=[pscustomobject]@{CoverageErrors=@();RacedExitCount=0;PrivateSource='native';NativePrivateBytes=100;ProcessDetails=@([pscustomobject]@{process_id=7;created_ticks=8})}
$snapshots=@($snapshot,$snapshot,$snapshot)
Check (Get-ProductionSidebarPhaseCoverage $snapshots $before $after $true).eligible 'stable WebApps identity covered in all snapshots'
$missing=[pscustomobject]@{CoverageErrors=@();RacedExitCount=0;PrivateSource='native';NativePrivateBytes=100;ProcessDetails=@([pscustomobject]@{process_id=1;created_ticks=2})}
Check (-not (Get-ProductionSidebarPhaseCoverage @($missing,$snapshot,$snapshot) $before $after $true).eligible) 'early missing WebApps root rejects entire phase'
$changed=[ordered]@{available=$true;identities=@([pscustomobject]@{pid=7;created_ticks=9;parent=1})}
Check (-not (Get-ProductionSidebarPhaseCoverage $snapshots $before $changed $true).eligible) 'changed identity is unavailable, not lower memory'

$moduleTokens=$null;$moduleErrors=$null
$moduleAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'sidebar-lifecycle.psm1'),[ref]$moduleTokens,[ref]$moduleErrors)
$railFunctions=@($moduleAst.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Invoke-Rail'},$true))
Check ($railFunctions.Count -eq 1 -and $railFunctions[0].Body.Extent.Text -notmatch 'SetForegroundWindow|Start-Sleep') 'per-action rail path has no foreground repair or pacing'
$fixture=New-ProductionSidebarFixture -BaseUrl 'http://127.0.0.1:12345' -Token '0123456789abcdef0123456789abcdef'
$lines=@($fixture -split "`n" | Where-Object {$_})
Check ($lines.Count -eq 9 -and $lines[0] -ceq 'bowser-sidebar-v1') 'ordinary sidebar fixture header and record count'
foreach($kind in @('customize','workspaces','apps','bookmarks','history','menu')){Check (@($lines|Where-Object {$_ -ceq "item`t$kind`t$kind`t1"}).Count -eq 1) "ordinary default-visible sidebar item: $kind"}
Check ($lines[7] -ceq "app`t1`tFixture A`thttp://127.0.0.1:12345/light?token=0123456789abcdef0123456789abcdef_a`t-`t0") 'exact synthetic A identity and token'
Check ($lines[8] -ceq "app`t2`tFixture B`thttp://127.0.0.1:12345/light?token=0123456789abcdef0123456789abcdef_b`t-`t1") 'exact synthetic B identity and token'

$ackFile=Join-Path ([IO.Path]::GetTempPath()) ("bowser-sidebar-acks-"+[guid]::NewGuid().ToString('N')+".jsonl")
try {
 Check (@(Get-ProductionSidebarReadyAcknowledgements -RequestFile $ackFile -Token 'fixture_a').Count -eq 0) 'missing acknowledgement file is an empty collection'
 '{"path":"/light","token":"fixture_a"}' | Set-Content -LiteralPath $ackFile -Encoding utf8
 Check (@(Get-ProductionSidebarReadyAcknowledgements -RequestFile $ackFile -Token 'fixture_a').Count -eq 0) 'zero ready matches remain an empty collection'
 '{"path":"/ready","token":"fixture_a"}' | Add-Content -LiteralPath $ackFile -Encoding utf8
 Check (@(Get-ProductionSidebarReadyAcknowledgements -RequestFile $ackFile -Token 'fixture_a').Count -eq 1) 'one ready match remains a one-element collection'
 '{"path":"/ready","token":"fixture_a"}' | Add-Content -LiteralPath $ackFile -Encoding utf8
 '{"path":"/ready","token":"fixture_b"}' | Add-Content -LiteralPath $ackFile -Encoding utf8
 Check (@(Get-ProductionSidebarReadyAcknowledgements -RequestFile $ackFile -Token 'fixture_a').Count -eq 2) 'duplicate ready acknowledgements retained without collapsing or accepting another token'
} finally {if(Test-Path -LiteralPath $ackFile){Remove-Item -LiteralPath $ackFile}}

'Sidebar lifecycle observation contracts passed.'
