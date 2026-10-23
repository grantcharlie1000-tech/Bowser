[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
$module = Join-Path $PSScriptRoot 'native-runner-state.psm1'
Assert-True (Test-Path -LiteralPath $module -PathType Leaf) 'Native runner diagnostic module is missing.'
Import-Module $module -Force
$zero = [BowserNativeRunnerState]::DescribeSequence([uint32]0)
Assert-True ($zero.number -eq 0 -and -not $zero.nonzero_observed -and -not $zero.authority) 'Sequence zero must remain unusable and provide no authority.'
$nonzero = [BowserNativeRunnerState]::DescribeSequence([uint32]42)
Assert-True ($nonzero.number -eq 42 -and $nonzero.nonzero_observed -and -not $nonzero.authority) 'A nonzero observation must not grant clipboard authority.'
$state = Get-BowserNativeRunnerState -Phase test
Assert-True ($state.schema_version -eq 1 -and $state.scope -eq 'read-only-native-runner-state-not-authority') 'Runner state must identify its diagnostic-only scope.'
Assert-True (-not $state.input_authority -and -not $state.clipboard_authority) 'A runner snapshot must never authorize input or clipboard operations.'
Assert-True ($state.status -eq 'captured' -and $state.state.process_id -eq [Diagnostics.Process]::GetCurrentProcess().Id) 'Native state must come from this actual process.'
Assert-True ($state.state.non_atomic -and -not $state.state.authority) 'Native observations must preserve their non-atomic, non-authorizing classification.'
$roundTrip = $state | ConvertTo-Json -Depth 8 | ConvertFrom-Json
Assert-True (-not $roundTrip.clipboard_authority -and -not $roundTrip.state.clipboard_sequence.authority) 'Serialization must not turn diagnostic observations into authority.'
$absent = [BowserNativeRunnerState]::DescribeWindow([IntPtr]::Zero)
Assert-True ($absent.status -eq 'unavailable' -and -not $absent.authority -and $absent.non_atomic) 'A missing foreground window must stay unknown and non-authorizing.'
$failure = Get-BowserNativeRunnerState -Phase failure
Assert-True (-not $failure.state.foreground.window.authority -and $failure.state.foreground.window.non_atomic) 'Failure-only foreground metadata must not grant input authority.'
Write-Output 'Native runner diagnostic regressions: 9 passed; no input or clipboard mutation performed.'
