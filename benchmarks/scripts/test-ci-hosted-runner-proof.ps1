[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
# Fake facts only. Never call Get-BowserHostedRunnerProof or a clipboard/native adapter locally.
Import-Module (Join-Path $PSScriptRoot 'ci-hosted-runner-proof.psm1') -Force
$context=[pscustomobject]@{Actions='true';Environment='github-hosted';PlatformEnvironment='github-hosted';OS='Windows';Repository='grantcharlie1000-tech/Bowser';Job='native-safety';Server='https://github.com';Api='https://api.github.com';WorkflowRef='grantcharlie1000-tech/Bowser/.github/workflows/store-probe.yml@refs/pull/91/merge';RunId='123';Attempt='1';RunnerName='GitHub Actions 456'}
$worker=[pscustomobject]@{Live=$true;ListenerParent=$true;SettingsPathBound=$true}
$settings=[pscustomobject]@{AgentId=456;AgentName='GitHub Actions 456'}
$job=[pscustomobject]@{status='in_progress';name='native-safety';run_id=123;run_attempt=1;runner_id=456;runner_name='GitHub Actions 456';runner_group_name='GitHub Actions';runner_group_id=0;labels=@('windows-latest')}
function Assert-Binding([bool]$Expected,$ContextValue,$WorkerValue,$SettingsValue,$JobValue) {
    if ((Test-BowserHostedRunnerBinding $ContextValue $WorkerValue $SettingsValue $JobValue) -ne $Expected) { throw 'Disposable runner binding regression failed.' }
}
function Copy-Facts($Value) { $Value | ConvertTo-Json -Depth 5 | ConvertFrom-Json }
Assert-Binding $true $context $worker $settings $job
$absent=[pscustomobject]@{Live=$false;ListenerParent=$false;SettingsPathBound=$false}
Assert-Binding $false $context $absent $settings $job
$changed=Copy-Facts $context;$changed.Environment='self-hosted';Assert-Binding $false $changed $worker $settings $job
$changed=Copy-Facts $context;$changed.Actions='false';Assert-Binding $false $changed $worker $settings $job
$changed=Copy-Facts $worker;$changed.Live=$false;Assert-Binding $false $context $changed $settings $job
$changed=Copy-Facts $worker;$changed.ListenerParent=$false;Assert-Binding $false $context $changed $settings $job
$changed=Copy-Facts $settings;$changed.AgentId=789;Assert-Binding $false $context $worker $changed $job
$changed=Copy-Facts $job;$changed.status='completed';Assert-Binding $false $context $worker $settings $changed
$changed=Copy-Facts $job;$changed.runner_group_name='Default';Assert-Binding $false $context $worker $settings $changed
$changed=Copy-Facts $job;$changed.labels=@('self-hosted','Windows');Assert-Binding $false $context $worker $settings $changed
$changed=Copy-Facts $job;$changed.run_attempt=2;Assert-Binding $false $context $worker $settings $changed
Write-Output 'Disposable runner binding regressions: 11 passed; fake facts only, no clipboard or runner action.'
