# Compile the exact Store helper and execute supplied-data diagnostics/predicates only.
# No probe body, LegacyValue call, native/UIA query, browser, profile, network or input.
[CmdletBinding()]
param()
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$path=Join-Path $PSScriptRoot 'probe-native-store.ps1'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)
if($errors.Count){throw ('Store helper does not parse: '+$errors[0].Message)}
$native=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.StringConstantExpressionAst] -and $node.Value.Contains('public static class BowserNativeDocument')},$true))
if($native.Count -ne 1){throw 'Exact Store managed/native declarations unavailable.'}
Add-Type -TypeDefinition $native[0].Value
$definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Record-StoreDocumentFailure'},$true)
if(!$definition){throw 'Actual failure-only report function missing.'}
. ([scriptblock]::Create($definition.Extent.Text))
$checks=0
function Check([bool]$Condition,[string]$Description){if(!$Condition){throw $Description};$script:checks++}
$flags=[Reflection.BindingFlags]::NonPublic -bor [Reflection.BindingFlags]::Static
$same=[BowserNativeDocument].GetMethod('Same',$flags)
foreach($case in @(
 @{a=$null;b=[int[]]@(1);expected=$false},
 @{a=[int[]]@(1);b=$null;expected=$false},
 @{a=[int[]]@(1);b=[int[]]@(1,2);expected=$false},
 @{a=[int[]]@(42,-7,0);b=[int[]]@(42,-7,1);expected=$false},
 @{a=[int[]]@(42,-7,0);b=[int[]]@(42,-7,0);expected=$true})){
 Check ($same.Invoke($null,[object[]]@($case.a,$case.b)) -eq $case.expected) 'Actual opaque runtime comparison predicate changed.'
}
$runtime=[BowserNativeDocument+RuntimeObservation]::new()
foreach($case in @(
 @{id=$null;status='missing';length=$null},
 @{id=[int[]]@();status='empty';length=0},
 @{id=[int[]]@(1..129);status='over-bound';length=129})){
 $runtime.Record($case.id)
 Check ($runtime.Status -ceq $case.status -and $runtime.Length -eq $case.length -and $null -eq $runtime.Id) 'Missing/empty/over-bound identity became a captured identity.'
}
$ids=[int[]]@(1..128);$runtime.Record($ids);$ids[0]=999
Check ($runtime.Status -ceq 'observed' -and $runtime.Id.Length -eq 128 -and $runtime.Id[0] -eq 1) 'Diagnostic did not copy the already-read bounded identity.'

# Actual serializer and failure bookkeeping on generated observations, not native evidence.
$observation=[BowserNativeDocument+IdentityObservation]::new()
$observation.Requested.Record([int[]]@(42,-7,0))
$observation.RecordOwner($false,7,8,18,$true,$true)
$observation.RecordOwner($true,9,8,18,$true,$false)
$observation.NativeDocumentCount=2;$observation.ExactMatchCount=0
$observation.CandidateElementsRead=2;$observation.CandidateRuntimeIdsRead=2;$observation.InventoryComplete=$true
$observation.RecordCandidate(0,[int[]]@(42,-7,1),$false)
$observation.RecordCandidate(1,[int[]]@(42,-7,2),$false)
$mark=[BowserNativeDocument].GetMethod('Mark',$flags)
$failure=[BowserNativeDocument].GetMethod('Failure',$flags)
$checked=[BowserNativeDocument].GetMethod('ObservedCheck',$flags)
$null=$mark.Invoke($null,@($observation,[BowserNativeDocument+Stage]::CandidateRuntimeBefore))
$getterFailed=$false
try{$null=$checked.Invoke($null,@(-2147467259,$observation))}catch{$getterFailed=$true;$null=$failure.Invoke($null,@($observation,-2147467259))}
Check $getterFailed 'Actual HRESULT check did not preserve a failing getter.'
$null=$mark.Invoke($null,@($observation,[BowserNativeDocument+Stage]::ReleaseRoot))
$null=$failure.Invoke($null,@($observation,-2147024891))
$syntheticJson=$observation.ToJson();$decoded=ConvertFrom-Json -InputObject $syntheticJson
Check ($decoded.failure_stage -ceq 'CandidateRuntimeBefore' -and $decoded.failure_hresult -eq -2147467259 -and $decoded.terminal_hresult -eq -2147024891) 'Original getter failure was erased by a later cleanup failure.'
Check ($decoded.last_hresult -eq -2147467259 -and $decoded.last_hresult_stage -ceq 'CandidateRuntimeBefore') 'HRESULT provenance stage was lost.'
Check ($decoded.owner_before.valid -eq $true -and $decoded.owner_after.valid -eq $false -and $decoded.owner_after.pid -eq 9) 'Already-observed owner validity was coerced into success.'
Check ($decoded.exact_match_count -eq 0 -and $decoded.candidates.Count -eq 2 -and $decoded.inventory_complete -eq $true) 'Complete zero-match observation was coerced into an attributed document.'
Check ($decoded.matched_values_read -eq 0 -and $decoded.matched_runtime_ids_rechecked -eq 0 -and $decoded.returned -eq $false) 'Unread property/recheck coverage became completed coverage.'
Check ($decoded.scope.Contains('non-atomic') -and $null -eq $decoded.candidates[0].runtime_after_same) 'Non-atomic or not-read metadata was erased.'
$observation.ExactMatchCount=2
Check ((ConvertFrom-Json -InputObject ($observation.ToJson())).exact_match_count -eq 2) 'Ambiguous exact-match count was collapsed into one.'

$bounded=[BowserNativeDocument+IdentityObservation]::new()
$large=[int[]]@(1..128|ForEach-Object{[int]::MinValue})
$bounded.Requested.Record($large)
for($i=0;$i -lt 65;$i++){$bounded.RecordCandidate($i,$large,$false)}
$boundedJson=$bounded.ToJson();$boundedDecoded=ConvertFrom-Json -InputObject $boundedJson
Check ($boundedDecoded.candidates.Count -eq 64 -and $boundedDecoded.capture_incomplete -eq $true) 'Diagnostic candidate cap did not retain incomplete coverage.'
Check ($boundedJson.Length -le [BowserNativeDocument]::DiagnosticJsonLimit -and $boundedDecoded.candidates[63].runtime.id.Count -eq 128) 'Maximum supplied bounded identity output exceeded its explicit cap.'
$over=[BowserNativeDocument+IdentityObservation]::new();$over.RecordCandidate(0,[int[]]@(1..129),$false)
$overDecoded=ConvertFrom-Json -InputObject ($over.ToJson())
Check ($overDecoded.candidates[0].runtime.status -ceq 'over-bound' -and $null -eq $overDecoded.candidates[0].runtime.id) 'Over-bound candidate contents leaked into the report.'

# Execute the production failure-only wrapper, including broken/unavailable serialization.
$report=[ordered]@{status='failed'}
Record-StoreDocumentFailure 'synthetic-native-getter' -2147467259 { $syntheticJson }
Check ($report.native_document_identity_failure.capture_status -ceq 'captured' -and $report.native_document_identity_failure.native_observation.exact_match_count -eq 0) 'Actual report wrapper did not preserve supplied failure evidence.'
foreach($case in @(
 @{read=$null;status='native-query-not-attempted'},
 @{read={ $null };status='unavailable-or-over-bound'},
 @{read={ 'x'*131073 };status='unavailable-or-over-bound'},
 @{read={ '{not-json' };status='capture-failed'},
 @{read={ throw 'synthetic-diagnostic-failure' };status='capture-failed'})){
 $report=[ordered]@{status='failed'};$original=$null
 try{throw 'synthetic-original-selector-refusal'}catch{
  Record-StoreDocumentFailure 'synthetic-stage' -2147467259 $case.read
  $original=$_.Exception.Message
 }
 Check ($original -ceq 'synthetic-original-selector-refusal') 'Diagnostic failure replaced the original refusal.'
 Check ($report.native_document_identity_failure.capture_status -ceq $case.status -and $null -eq $report.native_document_identity_failure.native_observation) 'Broken diagnostic did not retain a minimal failure-only report.'
}

# Last-observation serialization reads only the supplied object, even if serialization breaks.
$last=[BowserNativeDocument].GetField('lastObservation',$flags)
$last.SetValue($null,$observation)
Check ((ConvertFrom-Json -InputObject ([BowserNativeDocument]::LastIdentityObservationJson())).exact_match_count -eq 2) 'Production last-observation serializer omitted supplied ambiguity.'
$observation.Candidates=$null
Check ($null -eq [BowserNativeDocument]::LastIdentityObservationJson()) 'Broken optional serializer escaped instead of becoming unavailable.'
$last.SetValue($null,$null)
Write-Output "STORE_DOCUMENT_DIAGNOSTIC_CONTRACT passed=$checks source=generated-data-only native_provider=NOT_RUN LegacyValue=NOT_CALLED input=NOT_SENT"
