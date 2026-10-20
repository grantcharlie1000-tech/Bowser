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
$native=$type.GetMethod('NativeMismatch',$flags)
$absent=$native.Invoke($null,@([IntPtr]::Zero,[IntPtr]::Zero,[IntPtr]::Zero))|ConvertFrom-Json
if($absent.status -cne 'no-native-hwnd'){throw 'No-HWND diagnostic branch changed.'}
$hostType=$compiled['probe-app-host-context-menu.ps1']
$objectFlags=$hostType.GetNestedType('OBJECTFLAGS',[Reflection.BindingFlags]::NonPublic)
# PS5.1 can otherwise bind the generic/object overload and try to marshal RuntimeType.
# Select the non-generic System.Type overload explicitly; retain the exact layout assertion.
$sizeOfType=@([Runtime.InteropServices.Marshal].GetMethods() | Where-Object {
 $_.Name -ceq 'SizeOf' -and !$_.IsGenericMethod -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType -eq [Type]
})
if($sizeOfType.Count -ne 1){throw 'Exact Marshal.SizeOf(System.Type) overload unavailable.'}
if($sizeOfType[0].Invoke($null,@($objectFlags)) -ne 12){throw 'USEROBJECTFLAGS layout is not the required two Win32 BOOLs and DWORD.'}
Write-Output "native-menu helper source checks passed; managed_classes=$($compiled.Count); native_behavior=NOT_RUN"
