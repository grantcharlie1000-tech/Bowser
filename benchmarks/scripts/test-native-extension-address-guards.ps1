# Generated-data guard contract only. No browser, UIA provider, profile, key or network is used.
# This parses the real script, compiles its read-only COM prefix/PInvoke declarations, and
# imports only its source/runtime/input policy functions. It is NOT native runtime evidence.
[CmdletBinding()]
param()
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$path=Join-Path $PSScriptRoot 'measure-native-extensions.ps1'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)
if($errors.Count){throw ('Resource address script does not parse: '+$errors[0].Message)}
$native=$ast.Find({param($node) $node -is [Management.Automation.Language.StringConstantExpressionAst] -and $node.Value.Contains('public static class BowserExtensionMeasure')},$true)
if(!$native){throw 'Real read-only native declarations missing.'}
Add-Type -TypeDefinition $native.Value
foreach($name in @('Address-IsToolbarSource','Address-ObservedSourceKind','Address-ObservedSourcePair','Record-AddressObservedRuntime','Assert-AddressReadinessTime','Advance-AddressReadiness','Bind-OwnedAddressAfterReadiness','Assert-AddressGeometry','Address-RuntimeKey','Assert-AddressInput')){
 $definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $name},$true)
 if(!$definition){throw "Real address policy function missing: $name"}
 . ([scriptblock]::Create($definition.Extent.Text))
}
$checks=0
function Check([bool]$Condition,[string]$Description){if(!$Condition){throw $Description};$script:checks++}
function Refuses([scriptblock]$Action,[string]$Expected){
 $refused=$false
 try{& $Action}catch{if($_.Exception.GetBaseException().Message -cne $Expected -and $_.Exception.Message -cne $Expected){throw};$refused=$true}
 Check $refused 'Expected exact fail-closed policy refusal was not observed.'
}
Check (Address-IsToolbarSource 'bowser-ui://app/index.html?view=toolbar') 'Exact embedded toolbar source refused.'
foreach($source in @('', 'Bowser toolbar','Address and search bar','http://127.0.0.1/page?view=toolbar','https://bowser-ui/app/index.html?view=toolbar','bowser-ui://evil/index.html?view=toolbar','bowser-ui://app/index.html?view=settings','bowser-ui://app/index.html?view=toolbar&x=1','bowser-ui://app/index.html?view=toolbar#x','bowser-ui://app/index.html?view=toolbar/','Bowser-ui://app/index.html?view=toolbar',' bowser-ui://app/index.html?view=toolbar')){
 Check (!(Address-IsToolbarSource $source)) 'Nonexact/spoofed toolbar source was accepted.'
}
# Sanitized classifications use supplied values only and never retain URLs or discover UIA.
$exact='bowser-ui://app/index.html?view=toolbar';$other='https://example.invalid/private-observation-marker'
foreach($case in @(
 @{managed=$null;native=$null;want='missing,missing,missing'},
 @{managed=$exact;native=$null;want='exact-toolbar,missing,missing'},
 @{managed=$null;native=$exact;want='missing,exact-toolbar,exact-toolbar'},
 @{managed=$other;native=$other;want='other,other,other'},
 @{managed=$exact;native=$other;want='exact-toolbar,other,conflict'},
 @{managed=$other;native=$exact;want='other,exact-toolbar,conflict'})){
 $observed=Address-ObservedSourcePair $case.managed $case.native
 Check ((@($observed.managed_source,$observed.native_source,$observed.source_result) -join ',') -ceq $case.want) 'Supplied source classifications changed.'
 Check (!(($observed|ConvertTo-Json).Contains('private-observation-marker'))) 'Raw source content leaked into observation.'
}
$observed=@{runtime_id=$null;runtime_id_status='not-read'}
Record-AddressObservedRuntime $observed 'runtime_id' ([int[]]@(1..128))
Check ($observed.runtime_id_status -ceq 'observed' -and $observed.runtime_id.Count -eq 128 -and $observed.runtime_id[127] -eq 128) 'Full already-read bounded runtime ID not retained.'
foreach($invalid in @(@{ids=[int[]]@(1..129)},@{ids=@('raw-not-an-id')},@{ids=$null})){
 $observed=@{runtime_id=$null;runtime_id_status='not-read'}
 Record-AddressObservedRuntime $observed 'runtime_id' $invalid.ids
 Check ($observed.runtime_id_status -ceq 'unavailable-or-over-bound' -and $null -eq $observed.runtime_id) 'Unbounded/invalid observed ID was retained.'
}
# An unusable metadata destination must not throw or alter the original selector result.
Record-AddressObservedRuntime ([pscustomobject]@{}) 'runtime_id' ([int[]]@(1))
Check $true 'Metadata destination failure escaped its non-authority helper.'
# Execute protocol4 transitions from the real script on supplied snapshots only.
function Readiness-Doc([string]$Key,[string]$Source){return @{key=$Key;managed=$Source;native=$Source;eligible=$true}}
$otherDoc=Readiness-Doc '42,10' $other;$toolbarDoc=Readiness-Doc '42,20' $exact
$receipt=@{state='Observing'}
$initialReady=Advance-AddressReadiness $null @($toolbarDoc) 1 0 $receipt
Check ($initialReady.kind -ceq 'Ready' -and $initialReady.document_key -ceq '42,20' -and $receipt.state -ceq 'Ready') 'Initial exact toolbar did not become Ready.'
$receipt=@{state='Observing'}
$pending=Advance-AddressReadiness $null @($otherDoc) 1 0 $receipt
Check ($pending.kind -ceq 'PendingComplete' -and $receipt.state -ceq 'PendingComplete') 'Complete zero-exact snapshot was not PendingComplete.'
$receipt=@{state='Observing'}
$newReady=Advance-AddressReadiness $pending @($otherDoc,$toolbarDoc) 1001 1 $receipt
Check ($newReady.kind -ceq 'Ready' -and $newReady.document_key -ceq '42,20') 'Only-new-ID exact toolbar did not become Ready.'
Check (!(($receipt|ConvertTo-Json).Contains('private-observation-marker'))) 'Private exact source leaked into readiness receipt.'
Refuses {Advance-AddressReadiness $pending @((Readiness-Doc '42,10' $exact)) 1001 1 @{state='Observing'}} 'Before-binding prior document source changed; no input sent.'
Refuses {Advance-AddressReadiness $pending @((Readiness-Doc '42,11' $other)) 1001 1 @{state='Observing'}} 'Before-binding prior document identity disappeared; no input sent.'
Refuses {Advance-AddressReadiness $null @($otherDoc,$otherDoc) 1 0 @{state='Observing'}} 'Before-binding duplicate document runtime identity; no input sent.'
Refuses {Advance-AddressReadiness $null @($toolbarDoc,(Readiness-Doc '42,21' $exact)) 1 0 @{state='Observing'}} 'Before-binding exact toolbar source ambiguous; no input sent.'
$missing=Readiness-Doc '42,10' $other;$missing.native=$null
Refuses {Advance-AddressReadiness $null @($missing) 1 0 @{state='Observing'}} 'Before-binding document eligibility/source missing or over bound; no input sent.'
$conflict=Readiness-Doc '42,10' $other;$conflict.native=$exact
Refuses {Advance-AddressReadiness $null @($conflict) 1 0 @{state='Observing'}} 'Before-binding managed/native source disagreement; no input sent.'
$ineligible=Readiness-Doc '42,10' $other;$ineligible.eligible=$false
Refuses {Advance-AddressReadiness $null @($ineligible) 1 0 @{state='Observing'}} 'Before-binding document eligibility/source missing or over bound; no input sent.'
Refuses {Advance-AddressReadiness $null @((Readiness-Doc '' $other)) 1 0 @{state='Observing'}} 'Before-binding document runtime identity missing or over bound; no input sent.'
Refuses {Advance-AddressReadiness $null @() 1 0 @{state='Observing'}} 'Before-binding document inventory missing or over bound; no input sent.'
Refuses {Advance-AddressReadiness $null @((1..65|ForEach-Object {Readiness-Doc ([string]$_) $other})) 1 0 @{state='Observing'}} 'Before-binding document inventory missing or over bound; no input sent.'
foreach($late in @(10000.0,10001.0,[double]::NaN,[double]::PositiveInfinity,-1.0)){
 Refuses {Advance-AddressReadiness $null @($toolbarDoc) $late 0 @{state='Observing'}} 'Before-binding readiness absolute budget expired or clock unavailable; no input sent.'
}
Refuses {Advance-AddressReadiness $initialReady @($toolbarDoc) 1001 1 @{state='Observing'}} 'Before-binding readiness observation sequence invalid; no input sent.'
Refuses {Advance-AddressReadiness $pending @($otherDoc) 2001 2 @{state='Observing'}} 'Before-binding readiness observation sequence invalid; no input sent.'
$p1=Advance-AddressReadiness $pending @($otherDoc) 1001 1 @{state='Observing'}
$p2=Advance-AddressReadiness $p1 @($otherDoc) 2001 2 @{state='Observing'}
$lastReceipt=@{state='Observing'}
Refuses {Advance-AddressReadiness $p2 @($otherDoc) 3001 3 $lastReceipt} 'Before-binding readiness schedule exhausted without exact toolbar; no input sent.'
Check ($lastReceipt.state -ceq 'PendingComplete') 'Final complete zero-exact receipt was erased by exhaustion.'

# Execute the actual final binder with stubbed strict read and clock only. No UIA call.
& {
 $script:bindingReads=0;$script:bindingMode='ok'
 $clock=[pscustomobject]@{Elapsed=[pscustomobject]@{TotalMilliseconds=500.0};Stopped=$false}
 $clock|Add-Member ScriptMethod Stop {$this.Stopped=$true}
 $ready=@{owner=@{window=[IntPtr]7;thread=[uint32]8;root_key='42,7'};document_key='42,9';clock=$clock}
 function Read-OwnedAddress($Expected){
  $script:bindingReads++
  if($null -ne $Expected){throw 'Initial binder supplied a substitute expected control.'}
  if($script:bindingMode -ceq 'getter-failure'){throw 'synthetic-strict-getter-failure'}
  if($script:bindingMode -ceq 'late'){$clock.Elapsed.TotalMilliseconds=10000.0}
  $doc=if($script:bindingMode -ceq 'changed-document'){'42,99'}else{'42,9'}
  return @{bound=@{window=[IntPtr]7;thread=[uint32]8;root_key='42,7';document_key=$doc;edit_key='42,10'}}
 }
 foreach($scenario in @(
  @{mode='ok';error=$null},
  @{mode='getter-failure';error='synthetic-strict-getter-failure'},
  @{mode='changed-document';error='Ready toolbar/root identity changed before strict initial edit binding; no input sent.'},
  @{mode='late';error='Before-binding readiness absolute budget expired or clock unavailable; no input sent.'})){
  $script:bindingMode=$scenario.mode;$script:bindingReads=0;$clock.Elapsed.TotalMilliseconds=500.0;$clock.Stopped=$false
  $trace=@{before_binding_readiness=@{status='Ready';strict_binding_status='not-started';strict_binding_completed_ms=$null;elapsed_ms=$null}}
  if($scenario.error){Refuses {Bind-OwnedAddressAfterReadiness $ready $trace} $scenario.error}else{$bound=Bind-OwnedAddressAfterReadiness $ready $trace;Check ($trace.before_binding_readiness.status -ceq 'Bound' -and $bound.bound.edit_key -ceq '42,10') 'One strict initial binding did not retain exact edit identity.'}
  Check ($script:bindingReads -eq 1 -and $clock.Stopped) 'Strict initial binding retried or did not close its clock.'
 }
}

# All rectangle components must be finite; only extents must be positive.
$bounds=[pscustomobject]@{X=-10.0;Y=-20.0;Width=100.0;Height=25.0}
Assert-AddressGeometry $bounds
Check $true 'Finite negative coordinates with positive extents were refused.'
foreach($field in @('X','Y','Width','Height')){
 foreach($nonfinite in @([double]::NaN,[double]::PositiveInfinity,[double]::NegativeInfinity)){
  $bad=[pscustomobject]@{X=0.0;Y=0.0;Width=100.0;Height=25.0};$bad.$field=$nonfinite
  Refuses {Assert-AddressGeometry $bad} 'Source-bound address geometry unavailable or nonfinite; no input sent.'
 }
}
foreach($field in @('Width','Height')){
 foreach($nonpositive in @(0.0,-1.0)){
  $bad=[pscustomobject]@{X=0.0;Y=0.0;Width=100.0;Height=25.0};$bad.$field=$nonpositive
  Refuses {Assert-AddressGeometry $bad} 'Source-bound address geometry has nonpositive extents; no input sent.'
 }
}
Refuses {Assert-AddressGeometry $null} 'Source-bound address geometry unavailable or nonfinite; no input sent.'
function Node($Ids){$node=[pscustomobject]@{ids=$Ids};$node|Add-Member ScriptMethod GetRuntimeId {return $this.ids};return $node}
Check ((Address-RuntimeKey (Node @(42,-7,0,3))) -ceq '42,-7,0,3') 'Opaque integer runtime ID was not compared without reinterpretation.'
Refuses {Address-RuntimeKey $null} 'Current UIA element unavailable; no input sent.'
Refuses {Address-RuntimeKey (Node @())} 'Current UIA runtime identity unavailable or over bound; no input sent.'
Refuses {Address-RuntimeKey (Node @($null))} 'Current UIA runtime identity unavailable or over bound; no input sent.'
Refuses {Address-RuntimeKey (Node @('1','2'))} 'Current UIA runtime identity unavailable or over bound; no input sent.'
Refuses {Address-RuntimeKey (Node @(1..129))} 'Current UIA runtime identity unavailable or over bound; no input sent.'
Check ((Address-RuntimeKey (Node @(1..128))).Split(',').Count -eq 128) 'Allowed runtime ID cap changed.'
Check ((Address-RuntimeKey (Node @(1,2))) -cne (Address-RuntimeKey (Node @(1,3)))) 'Runtime mutation was not observable.'
Refuses {[BowserResourceDocument]::LegacyValue([IntPtr]::Zero,0,0,$null,$null)} 'Invalid native document identity.'
Refuses {[BowserResourceDocument]::LegacyValue([IntPtr]::Zero,0,0,[int[]]@(1),[int[]]@(1..129))} 'Invalid native document identity.'
# Stub only the I/O edges of Assert-AddressInput. Execute its actual extracted body.
$script:bound=@{window=[IntPtr]77;thread=[uint32]91;root_key='1';document_key='2';edit_key='3'}
$script:state=@{window=[IntPtr]77;bound=$script:bound}
$script:fresh=@{focused=$true;value='generated';bound=$script:bound}
$script:mode='ok';$script:calls=@()
function Read-OwnedAddress($Expected){
 $script:calls+=,'read'
 if(![Object]::ReferenceEquals($Expected,$script:bound)){throw 'Original binding was not forwarded.'}
 if($script:mode -ceq 'read-failure'){throw 'synthetic-source-or-identity-failure'}
 return $script:fresh
}
function Assert-NeutralModifiers {$script:calls+=,'modifiers';if($script:mode -ceq 'modifier-failure'){throw 'synthetic-modifier-failure'}}
function Assert-AddressWindow([IntPtr]$Window,[uint32]$Thread){
 $script:calls+=,'window'
 if($Window -ne [IntPtr]77 -or $Thread -ne 91){throw 'Original native window/thread not forwarded.'}
 if($script:mode -ceq 'window-failure'){throw 'synthetic-window-foreground-failure'}
 return $Thread
}
$actual=Assert-AddressInput $state
Check ([Object]::ReferenceEquals($actual,$fresh)) 'Input guard did not return the current observation.'
Check (($calls -join ',') -ceq 'read,modifiers,window') 'Fresh source/identity, modifier and final window guards changed order.'
$calls=@();$fresh.focused=$false
Refuses {Assert-AddressInput $state} 'Exact source-bound address focus changed before input; no repair or retry.'
Check (($calls -join ',') -ceq 'read') 'Lost focus did not stop before later input guards.'
$fresh.focused=$true
foreach($scenario in @(
 @{mode='read-failure';error='synthetic-source-or-identity-failure';calls='read'},
 @{mode='modifier-failure';error='synthetic-modifier-failure';calls='read,modifiers'},
 @{mode='window-failure';error='synthetic-window-foreground-failure';calls='read,modifiers,window'})){
 $mode=$scenario.mode;$calls=@()
 Refuses {Assert-AddressInput $state} $scenario.error
 Check (($calls -join ',') -ceq $scenario.calls) 'A failed input guard retried or continued.'
}
Write-Output "RESOURCE_ADDRESS_GUARD_CONTRACT passed=$checks source=generated-data-only native_provider=not-run input=not-sent"
