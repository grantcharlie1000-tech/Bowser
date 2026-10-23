Set-StrictMode -Version Latest

# Pure binding predicate, also used by fake-fact tests. Environment variables alone fail.
function Test-BowserHostedRunnerBinding {
    param($Context, $Worker, $Settings, $Job)
    try {
        if ($Context.Actions -cne 'true' -or $Context.Environment -cne 'github-hosted' -or
            $Context.PlatformEnvironment -cne 'github-hosted' -or $Context.OS -cne 'Windows' -or
            $Context.Repository -cne 'grantcharlie1000-tech/Bowser' -or $Context.Job -cne 'native-safety' -or
            $Context.Server -cne 'https://github.com' -or $Context.Api -cne 'https://api.github.com' -or
            $Context.WorkflowRef -notlike 'grantcharlie1000-tech/Bowser/.github/workflows/store-probe.yml@*' -or
            $Context.RunId -notmatch '^[1-9][0-9]*$' -or $Context.Attempt -notmatch '^[1-9][0-9]*$') { return $false }
        if (-not $Worker.Live -or -not $Worker.ListenerParent -or -not $Worker.SettingsPathBound) { return $false }
        if ([uint64]$Settings.AgentId -eq 0 -or $Settings.AgentName -cne $Context.RunnerName) { return $false }
        if ($Job.status -cne 'in_progress' -or $Job.name -cne 'native-safety' -or
            [string]$Job.run_id -cne $Context.RunId -or [string]$Job.run_attempt -cne $Context.Attempt -or
            [uint64]$Job.runner_id -ne [uint64]$Settings.AgentId -or $Job.runner_name -cne $Settings.AgentName -or
            $Job.runner_group_name -cne 'GitHub Actions' -or [int]$Job.runner_group_id -ne 0 -or
            @($Job.labels) -notcontains 'windows-latest' -or @($Job.labels) -contains 'self-hosted') { return $false }
        return $true
    } catch { return $false }
}

function Get-BowserHostedRunnerProof {
    $context = [pscustomobject]@{
        Actions=$env:GITHUB_ACTIONS; Environment=$env:RUNNER_ENVIRONMENT
        PlatformEnvironment=$env:BOWSER_PLATFORM_RUNNER_ENVIRONMENT; OS=$env:RUNNER_OS
        Repository=$env:GITHUB_REPOSITORY; Job=$env:GITHUB_JOB; Server=$env:GITHUB_SERVER_URL
        Api=$env:GITHUB_API_URL; WorkflowRef=$env:GITHUB_WORKFLOW_REF
        RunId=$env:GITHUB_RUN_ID; Attempt=$env:GITHUB_RUN_ATTEMPT; RunnerName=$env:RUNNER_NAME
    }
    # Fail before looking at processes/configuration on any ordinary local or shared session.
    if ($context.Actions -cne 'true' -or $context.Environment -cne 'github-hosted' -or
        $context.PlatformEnvironment -cne 'github-hosted' -or $context.OS -cne 'Windows' -or
        $context.Repository -cne 'grantcharlie1000-tech/Bowser' -or $context.Job -cne 'native-safety' -or
        $context.Server -cne 'https://github.com' -or $context.Api -cne 'https://api.github.com' -or
        $context.RunId -notmatch '^[1-9][0-9]*$' -or $context.Attempt -notmatch '^[1-9][0-9]*$') {
        throw 'Not the reviewed GitHub-hosted Windows native-safety job; no clipboard action.'
    }
    if (-not ('BowserRunnerReadHandle' -as [type])) {
        Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;
public sealed class BowserRunnerReadHandle:IDisposable {
 [StructLayout(LayoutKind.Sequential)] struct Time {public uint low,high;public ulong Bits{get{return ((ulong)high<<32)|low;}}}
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
 [DllImport("kernel32.dll")] static extern uint GetProcessId(IntPtr handle);
 [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle,uint milliseconds);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr handle,out Time created,out Time exited,out Time kernel,out Time user);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern bool QueryFullProcessImageName(IntPtr handle,uint flags,StringBuilder image,ref uint count);
 IntPtr handle;readonly uint pid;readonly string image;readonly ulong creation;
 string Image(){var value=new StringBuilder(32768);uint count=32768;if(!QueryFullProcessImageName(handle,0,value,ref count))throw new InvalidOperationException("runner-image-unavailable");return System.IO.Path.GetFullPath(value.ToString());}
 ulong Created(){Time a,b,c,d;if(!GetProcessTimes(handle,out a,out b,out c,out d))throw new InvalidOperationException("runner-creation-unavailable");return a.Bits;}
 public BowserRunnerReadHandle(uint pid,string expectedImage){
  this.pid=pid;image=System.IO.Path.GetFullPath(expectedImage);handle=OpenProcess(0x00101000,false,pid);
  if(handle==IntPtr.Zero)throw new InvalidOperationException("runner-readonly-handle-unavailable");
  try{creation=Created();if(!SameAlive())throw new InvalidOperationException("runner-process-identity-mismatch");}catch{Dispose();throw;}
 }
 public bool SameAlive(){try{return handle!=IntPtr.Zero&&GetProcessId(handle)==pid&&WaitForSingleObject(handle,0)==258&&Created()==creation&&String.Equals(Image(),image,StringComparison.OrdinalIgnoreCase);}catch{return false;}}
 public bool MatchesCreation(long utcTicks){return DateTime.FromFileTimeUtc((long)creation).Ticks/10==utcTicks/10;}
 public void Dispose(){if(handle!=IntPtr.Zero){CloseHandle(handle);handle=IntPtr.Zero;}}
}
'@
    }
    $worker = $null
    $cursor = [uint32][Diagnostics.Process]::GetCurrentProcess().Id
    $seen = @{}
    for ($index=0; $index -lt 8; $index++) {
        if ($cursor -eq 0 -or $seen.ContainsKey($cursor)) { throw 'Runner parent chain unavailable; no clipboard action.' }
        $seen[$cursor]=$true
        $process = Get-CimInstance Win32_Process -Filter "ProcessId=$cursor" -Property ProcessId,ParentProcessId,Name,ExecutablePath,CreationDate -ErrorAction Stop
        if ($null -eq $process) { throw 'Runner parent process unavailable; no clipboard action.' }
        if ($process.Name -ceq 'Runner.Worker.exe') { $worker=$process; break }
        $cursor=[uint32]$process.ParentProcessId
    }
    if ($null -eq $worker -or -not $worker.ExecutablePath) { throw 'No actual Runner.Worker ancestor; no clipboard action.' }
    $listener = Get-CimInstance Win32_Process -Filter "ProcessId=$($worker.ParentProcessId)" -Property ProcessId,Name,ExecutablePath,CreationDate -ErrorAction Stop
    $bin = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($worker.ExecutablePath))
    if ($null -eq $listener -or $listener.Name -cne 'Runner.Listener.exe' -or
        -not [string]::Equals([IO.Path]::GetFullPath($listener.ExecutablePath),(Join-Path $bin 'Runner.Listener.exe'),[StringComparison]::OrdinalIgnoreCase) -or
        -not [string]::Equals([IO.Path]::GetFileName($worker.ExecutablePath),'Runner.Worker.exe',[StringComparison]::Ordinal)) {
        throw 'Actual worker/listener lineage unavailable; no clipboard action.'
    }
    $settingsPath = Join-Path ([IO.Path]::GetDirectoryName($bin)) '.runner'
    $settingsFile = Get-Item -LiteralPath $settingsPath -ErrorAction Stop
    if ($settingsFile.PSIsContainer -or $settingsFile.Length -le 0 -or $settingsFile.Length -gt 16384 -or
        ($settingsFile.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Non-secret runner settings unavailable; no clipboard action.' }
    # Only .runner AgentId/AgentName are used. Never open .credentials or token/key files.
    $settings = [IO.File]::ReadAllText($settingsPath) | ConvertFrom-Json
    $workerHandle=$null;$listenerHandle=$null
    try {
        $workerHandle=[BowserRunnerReadHandle]::new([uint32]$worker.ProcessId,[string]$worker.ExecutablePath)
        $listenerHandle=[BowserRunnerReadHandle]::new([uint32]$listener.ProcessId,[string]$listener.ExecutablePath)
        if (-not $workerHandle.MatchesCreation(([datetime]$worker.CreationDate).ToUniversalTime().Ticks) -or
            -not $listenerHandle.MatchesCreation(([datetime]$listener.CreationDate).ToUniversalTime().Ticks)) { throw 'Runner PID/creation identity changed; no clipboard action.' }
        $uri='https://api.github.com/repos/grantcharlie1000-tech/Bowser/actions/runs/'+$context.RunId+'/jobs?filter=latest&per_page=100'
        # Public metadata only: no token, new permissions, redirects, retries or fallback.
        $response=Invoke-WebRequest -Uri $uri -UseBasicParsing -MaximumRedirection 0 -TimeoutSec 20 -Headers @{'User-Agent'='Bowser-disposable-native-fixture';'Accept'='application/vnd.github+json'} -ErrorAction Stop
        if ($response.StatusCode -ne 200) { throw 'GitHub job metadata unavailable; no clipboard action.' }
        $document=$response.Content | ConvertFrom-Json
        if ($document.total_count -gt 100) { throw 'Job inventory incomplete; no clipboard action.' }
        $jobs=@($document.jobs | Where-Object { $_.name -ceq 'native-safety' -and [string]$_.run_attempt -ceq $context.Attempt })
        if ($jobs.Count -ne 1) { throw 'Exact active job ambiguous; no clipboard action.' }
        $facts=[pscustomobject]@{Live=($workerHandle.SameAlive() -and $listenerHandle.SameAlive());ListenerParent=$true;SettingsPathBound=$true}
        if (-not (Test-BowserHostedRunnerBinding $context $facts $settings $jobs[0])) { throw 'Actual runner/configuration/server binding failed; no clipboard action.' }
        $result=[pscustomobject]@{Verified=$true;RunId=$context.RunId;Attempt=$context.Attempt;RunnerId=[uint64]$settings.AgentId;RunnerName=[string]$settings.AgentName;Worker=$workerHandle;Listener=$listenerHandle}
        $workerHandle=$null;$listenerHandle=$null
        return $result
    } finally {if($workerHandle){$workerHandle.Dispose()};if($listenerHandle){$listenerHandle.Dispose()}}
}
Export-ModuleMember -Function Test-BowserHostedRunnerBinding,Get-BowserHostedRunnerProof
