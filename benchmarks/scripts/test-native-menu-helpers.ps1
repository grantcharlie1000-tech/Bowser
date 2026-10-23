# Syntax/managed compilation and pure serializer checks only. Never execute a probe body,
# load a browser/profile, send native input, touch clipboard data, or establish native acceptance.
[CmdletBinding()]
param()
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,WindowsBase
$references=@([System.Windows.Automation.AutomationElement].Assembly.Location,[System.Windows.Automation.ControlType].Assembly.Location,[System.Windows.Rect].Assembly.Location)
$helpers=@('probe-default-context-cancellation.ps1','probe-native-context-observation.ps1','probe-app-context-menu.ps1','probe-app-context-menu-reload.ps1','probe-app-host-context-menu.ps1')
$compiled=@{};$typeNames=[ordered]@{}
$definitions=[Collections.Generic.List[string]]::new()
$imports=[Collections.Generic.SortedSet[string]]::new([StringComparer]::Ordinal)
foreach($name in $helpers){
 $tokens=$null;$errors=$null
 $ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $name),[ref]$tokens,[ref]$errors)
 if($errors.Count -ne 0){throw "$name has PowerShell syntax errors: $($errors[0].Message)"}
 $sources=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.StringConstantExpressionAst] -and $node.Value -match 'public static class Bowser'},$true))
 if($name -ceq 'probe-native-context-observation.ps1'){
  if($sources.Count -ne 0){throw 'Observation wrapper must reuse the strict Chromium helper, not duplicate native authority.'}
  continue
 }
 if($sources.Count -ne 1){throw "$name must have one complete managed helper source."}
 $source=$sources[0].Value
 $match=[regex]::Match($source,'public static class (Bowser[A-Za-z0-9]+)')
 if(!$match.Success){throw "$name managed helper class unavailable."}
 # Two genuinely independent scripts use BowserAppContext. Rename the compile-only copy,
 # never the actual source or its runtime class, so both can be checked in this process.
 $original=$match.Groups[1].Value;$unique=$original+($definitions.Count.ToString())+'CompileOnly'
 $source=[regex]::Replace($source,'\b'+[regex]::Escape($original)+'\b',$unique)
 $prefix=[regex]::Match($source,'\A(?:\s*using\s+[A-Za-z_][A-Za-z0-9_.]*\s*;)+')
 if(!$prefix.Success){throw "$name must have only ordinary namespace imports before its helper class."}
 foreach($import in [regex]::Matches($prefix.Value,'using\s+([A-Za-z_][A-Za-z0-9_.]*)\s*;')){
  $null=$imports.Add('using '+$import.Groups[1].Value+';')
 }
 $definitions.Add($source.Substring($prefix.Length));$typeNames[$name]=$unique
}
# Compile once: Windows PowerShell 5.1 registers compiler-generated anonymous types by
# name too. Separate Add-Type calls can collide even after the public classes are renamed.
# One source unit generates its anonymous types only once; no probe body is invoked.
$combined=($imports -join "`n")+"`n"+($definitions -join "`n")
Add-Type -TypeDefinition $combined -ReferencedAssemblies $references
foreach($name in $typeNames.Keys){
 $compiled[$name]=($typeNames[$name] -as [type])
 if(!$compiled[$name]){throw "$name managed helper did not compile."}
}
# Pure methods only. This verifies exact diagnostic JSON escaping and the no-HWND branch
# without making even one Win32/UIA query. It does not simulate a successful native probe.
$type=$compiled['probe-default-context-cancellation.ps1']
$flags=[Reflection.BindingFlags]::NonPublic -bor [Reflection.BindingFlags]::Static
$escape=$type.GetMethod('JsonText',$flags)
$inputText='quote" slash\'+[char]0+[char]9+[char]10+[char]13+[char]31
$escaped=$escape.Invoke($null,@($inputText))
$roundTrip=('{"value":"'+$escaped+'"}'|ConvertFrom-Json).value
if($roundTrip -cne $inputText){throw 'Bounded native class diagnostic JSON did not round-trip control characters.'}
$boolean=$type.GetMethod('BooleanValue',$flags)
foreach($value in @($true,$false)){
 $actual=$boolean.Invoke($null,@($value))|ConvertFrom-Json
 if($actual.kind -cne 'boolean' -or $actual.value -isnot [bool] -or $actual.value -ne $value){throw 'Actual UIA Boolean diagnostic value was not preserved.'}
}
$unsupported=$boolean.Invoke($null,@([System.Windows.Automation.AutomationElement]::NotSupported))|ConvertFrom-Json
if($unsupported.kind -cne 'not-supported' -or $null -ne $unsupported.value){throw 'Unsupported UIA property must not become false.'}
foreach($value in @($null,[object]::new(),0)){
 $actual=$boolean.Invoke($null,[object[]]@($value))|ConvertFrom-Json
 if($actual.kind -cne 'other-kind' -or $null -ne $actual.value){throw 'Unknown UIA property kind must not become false.'}
}
# Pure bounds metadata must distinguish supported Empty from NotSupported and malformed kinds.
$boundsValue=$type.GetMethod('BoundsValue',[Reflection.BindingFlags]::Public -bor [Reflection.BindingFlags]::Static)
$emptyBounds=$boundsValue.Invoke($null,[object[]]@([System.Windows.Rect]::Empty))|ConvertFrom-Json
if($emptyBounds.kind -cne 'rectangle-value' -or $emptyBounds.rectangle.is_empty -ne $true -or $emptyBounds.rectangle.canonical_empty -ne $true -or $emptyBounds.rectangle.all_finite -ne $false -or $emptyBounds.rectangle.x -cne 'Infinity' -or $emptyBounds.rectangle.width -cne '-Infinity'){throw 'Supported Rect.Empty diagnostic must retain the real nonfinite sentinel, never coerce it.'}
$finiteBounds=$boundsValue.Invoke($null,[object[]]@([System.Windows.Rect]::new(1,2,3,4)))|ConvertFrom-Json
if($finiteBounds.kind -cne 'rectangle-value' -or $finiteBounds.rectangle.all_finite -ne $true -or $finiteBounds.rectangle.is_empty -ne $false){throw 'Finite diagnostic bounds were not preserved.'}
$nanBounds=$boundsValue.Invoke($null,[object[]]@([System.Windows.Rect]::new([double]::NaN,2,3,4)))|ConvertFrom-Json
if($nanBounds.rectangle.canonical_empty -ne $false -or $nanBounds.rectangle.all_finite -ne $false -or $nanBounds.rectangle.x -cne 'NaN'){throw 'Arbitrary NaN must not be represented as canonical Empty or numeric zero.'}
$infiniteBounds=$boundsValue.Invoke($null,[object[]]@([System.Windows.Rect]::new([double]::PositiveInfinity,2,3,4)))|ConvertFrom-Json
if($infiniteBounds.rectangle.canonical_empty -ne $false -or $infiniteBounds.rectangle.all_finite -ne $false){throw 'Arbitrary infinity must not be represented as canonical Empty.'}
$unsupportedBounds=$boundsValue.Invoke($null,[object[]]@([System.Windows.Automation.AutomationElement]::NotSupported))|ConvertFrom-Json
if($unsupportedBounds.kind -cne 'not-supported' -or $null -ne $unsupportedBounds.rectangle){throw 'Unsupported diagnostic bounds must remain unknown.'}
foreach($value in @($null,[object]::new(),0)){
 $unknownBounds=$boundsValue.Invoke($null,[object[]]@($value))|ConvertFrom-Json
 if($unknownBounds.kind -cne 'other-kind' -or $null -ne $unknownBounds.rectangle){throw 'Malformed diagnostic bounds must remain unknown.'}
}
$policy=$type.GetMethod('CheckMembershipPolicy',[Reflection.BindingFlags]::Public -bor [Reflection.BindingFlags]::Static)
$null=$policy.Invoke($null,@())
$inventoryPolicy=$type.GetMethod('CheckInventoryPolicy',[Reflection.BindingFlags]::Public -bor [Reflection.BindingFlags]::Static)
$null=$inventoryPolicy.Invoke($null,@())
$leafPolicy=$type.GetMethod('CheckOwnedLeafPolicy',[Reflection.BindingFlags]::Public -bor [Reflection.BindingFlags]::Static)
$null=$leafPolicy.Invoke($null,@())
$browserPolicy=$type.GetMethod('CheckBrowserStructurePolicy',[Reflection.BindingFlags]::Public -bor [Reflection.BindingFlags]::Static)
$null=$browserPolicy.Invoke($null,@())
$rejectedPolicy=$type.GetMethod('CheckRejectedInventoryPolicy',[Reflection.BindingFlags]::Public -bor [Reflection.BindingFlags]::Static)
$null=$rejectedPolicy.Invoke($null,@())
# Pure JSON/UTF-8 digest evidence round-trip only. No native provider or filesystem operation.
$proofType=$type.GetNestedType('MenuProof',[Reflection.BindingFlags]::Public)
$receiptMethod=$type.GetMethod('InventoryReceiptValue',[Reflection.BindingFlags]::Public -bor [Reflection.BindingFlags]::Static)
$example=[Activator]::CreateInstance($proofType);$example.Window=[IntPtr]123;$example.RootId=[int[]]@(42,123);$example.GpuLeaves=1
$inventory="1:synthetic-quote`"-slash\-tab"+[char]9+"-nul"+[char]0+"-unicode"+[char]0x2603+"`n"
$example.Inventory=$inventory
$sha=[Security.Cryptography.SHA256]::Create()
try {$digest=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($inventory)))).Replace('-','').ToLowerInvariant()} finally {$sha.Dispose()}
$receipt=$receiptMethod.Invoke($null,@($example,'first-complete','synthetic-run-123456','1,2,0',$digest))|ConvertFrom-Json
if($receipt.capture_status -cne 'complete' -or $receipt.inventory -cne $inventory -or $receipt.utf8_sha256 -cne $digest -or $receipt.membership_request_digest -cne $digest -or $receipt.matches_issued_request_digest -ne $true -or $receipt.node_count -ne 1 -or $receipt.run_nonce -cne 'synthetic-run-123456' -or ($receipt.source_proof -join ',') -cne '1,2,0' -or ($receipt.root_runtime -join ',') -cne '42,123' -or $receipt.popup -cne '123'){throw 'Complete inventory evidence did not round-trip exact strings, digest or bound identities.'}
$missing=$receiptMethod.Invoke($null,[object[]]@($null,'fresh-before-escape','synthetic-run-123456','1,2,0',$digest))|ConvertFrom-Json
if($missing.capture_status -cne 'unavailable'){throw 'Missing inventory capture must not become empty success.'}
$example.Inventory='1:truncated'
$incomplete=$receiptMethod.Invoke($null,@($example,'fresh-before-escape','synthetic-run-123456','1,2,0',$digest))|ConvertFrom-Json
if($incomplete.capture_status -cne 'unavailable'){throw 'Incomplete inventory capture must not become success.'}
$native=$type.GetMethod('NativeMismatch',$flags)
$absent=$native.Invoke($null,@([IntPtr]::Zero,[IntPtr]::Zero,[IntPtr]::Zero))|ConvertFrom-Json
if($absent.status -cne 'no-native-hwnd'){throw 'No-HWND diagnostic branch changed.'}
$hostType=$compiled['probe-app-host-context-menu.ps1']
$initializedPolicy=$hostType.GetMethod('CheckInitializedClipboardPolicy',[Reflection.BindingFlags]::Public -bor [Reflection.BindingFlags]::Static)
if(!$initializedPolicy){throw 'Exact initialized-fixture synthetic policy entrypoint missing.'}
$null=$initializedPolicy.Invoke($null,@())
$objectFlags=$hostType.GetNestedType('OBJECTFLAGS',[Reflection.BindingFlags]::NonPublic)
# PS5.1 can otherwise bind the generic/object overload and try to marshal RuntimeType.
# Select the non-generic System.Type overload explicitly; retain the exact layout assertion.
$sizeOfType=@([Runtime.InteropServices.Marshal].GetMethods() | Where-Object {
 $_.Name -ceq 'SizeOf' -and !$_.IsGenericMethod -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType -eq [Type]
})
if($sizeOfType.Count -ne 1){throw 'Exact Marshal.SizeOf(System.Type) overload unavailable.'}
if($sizeOfType[0].Invoke($null,@($objectFlags)) -ne 12){throw 'USEROBJECTFLAGS layout is not the required two Win32 BOOLs and DWORD.'}
$basicLayout=$hostType.GetMethod('ObjectBasicLayoutValid',$flags)
if($basicLayout.Invoke($null,@()) -ne $true){throw 'PUBLIC_OBJECT_BASIC_INFORMATION must be 56 bytes with GrantedAccess at offset 4.'}
Write-Output "native-menu helper source checks passed; managed_classes=$($compiled.Count); native_behavior=NOT_RUN"
