[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
# NEVER invoke on a local, self-hosted, shared or user desktop. No force/override switch.
Import-Module (Join-Path $PSScriptRoot 'ci-hosted-runner-proof.psm1') -Force
$proof=$null;$native=$null;$journal=$null;$writer=$null;$result=$null
try {
    # Actual retained worker/listener identity + non-secret .runner AgentId + public active job.
    # Environment/zero/access-mask values cannot independently authorize the fixture.
    $proof=Get-BowserHostedRunnerProof
    if (-not $proof.Verified -or -not $proof.Worker.SameAlive() -or -not $proof.Listener.SameAlive()) { throw 'Disposable job ownership unavailable.' }
    $temp=Get-Item -LiteralPath $env:RUNNER_TEMP -ErrorAction Stop
    if (-not $temp.PSIsContainer -or ($temp.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Exact disposable runner temporary directory unavailable.' }
    $name='bowser-ci-empty-clipboard-'+$proof.RunId+'-'+$proof.Attempt+'-native-safety.jsonl'
    $path=Join-Path $temp.FullName $name
    # One retained CreateNew journal forbids repeated preparation in this job/attempt.
    $journal=[IO.File]::Open($path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
    $writer=[IO.StreamWriter]::new($journal,[Text.UTF8Encoding]::new($false),1024,$true)
    function Write-FixtureJournal([string]$Stage,$Value) {
        $entry=[ordered]@{schema=1;scope='verified-disposable-github-job-empty-clipboard-fixture';stage=$Stage;run_id=$proof.RunId;run_attempt=$proof.Attempt;runner_id=$proof.RunnerId;result=$Value}
        $line=$entry|ConvertTo-Json -Depth 6 -Compress
        if($line.Length -gt 8192){throw 'Fixture journal bound exceeded.'}
        $writer.WriteLine($line);$writer.Flush();$journal.Flush($true)
    }
    Write-FixtureJournal 'disposable-job-verified-before-any-native-clipboard-call' $null
    $source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'native-clipboard-policy.cs'))+[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'ci-clipboard-native.cs'))
    Add-Type -TypeDefinition $source
    $alive=[Func[bool]]{ $proof.Worker.SameAlive() -and $proof.Listener.SameAlive() }
    $native=[BowserCiClipboardNative]::new($alive)
    $checkpoint=[Action[BowserCiClipboardResult]]{param($value) Write-FixtureJournal 'one-empty-call-intent-before-mutation' $value}
    $result=[BowserCiClipboardPolicy]::Prepare($native,$true,$checkpoint)
    Write-FixtureJournal 'policy-finished' ([ordered]@{policy=$result;native_empty_calls=$native.NativeEmptyCalls;native_open_calls=$native.NativeOpenCalls;native_close_calls=$native.NativeCloseCalls})
    if(-not $result.Prepared -or $native.NativeEmptyCalls -gt 1 -or $native.NativeOpenCalls -ne 1 -or $native.NativeCloseCalls -ne 1) { throw 'Disposable clipboard prerequisite refused; no retry or alternate path.' }
    $native.Dispose()
    if(-not $native.WindowCleanupConfirmed -or $native.Sequence() -ne $result.FinalSequence -or $native.Sequence() -eq 0 -or $native.Owner() -ne 0) { throw 'Post-cleanup empty baseline unavailable; no retry or alternate path.' }
    Write-FixtureJournal 'prepared-ownerless-empty-nonzero-baseline-after-checked-close-and-window-cleanup' ([ordered]@{sequence=$result.FinalSequence;native_empty_calls=$native.NativeEmptyCalls})
    Write-Output "CI_EMPTY_CLIPBOARD prepared=true native_empty_calls=$($native.NativeEmptyCalls) close_confirmed=true payload_read_or_set=false"
} finally {
    if($native){$native.Dispose()}
    if($writer){$writer.Dispose()};if($journal){$journal.Dispose()}
    if($proof){$proof.Worker.Dispose();$proof.Listener.Dispose()}
}
