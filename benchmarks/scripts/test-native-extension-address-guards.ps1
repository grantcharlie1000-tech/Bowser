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
foreach($name in @('Address-IsToolbarSource','Address-ObservedSourceKind','Address-ObservedSourcePair','Record-AddressObservedRuntime','Assert-AddressGeometry','Address-RuntimeKey','Assert-AddressInput')){
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
