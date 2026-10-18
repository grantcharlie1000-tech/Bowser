# Actual release executable, actual persistent disposable profiles. NO benchmark flags: those
# intentionally disable extension startup. This is a small paired observation, not a full audit.
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$ProfilesRoot,[string]$Out="$env:RUNNER_TEMP/bowser-extension-metrics",[ValidateRange(1,5)][int]$Trials=3)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'));$exe=Join-Path $root 'target/release/bowser.exe'
$ProfilesRoot=[IO.Path]::GetFullPath($ProfilesRoot)
$allowed=[IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')+'\'
if(!$ProfilesRoot.StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase) -or !([IO.Path]::GetFileName($ProfilesRoot).StartsWith('bowser-extension-measurement-'))){throw 'Not the approved fresh runner measurement root.'}
foreach($name in @('zero','several')){ $item=Get-Item -LiteralPath (Join-Path $ProfilesRoot $name);if(!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Profile must be a plain disposable directory.'} }
if(!(Test-Path $exe)){throw 'The exact HTML-default release executable must be built first.'}
New-Item -ItemType Directory -Force $Out | Out-Null
$run=Join-Path $ProfilesRoot 'fixture';New-Item -ItemType Directory $run | Out-Null
$portFile=Join-Path $run 'port.txt';$events=Join-Path $run 'ready.jsonl'
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class BowserExtensionMeasure {
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern int GetClassName(IntPtr h,StringBuilder s,int n);
}
'@
@'
const http=require('node:http'),fs=require('node:fs');const[port,events]=process.argv.slice(2);
const server=http.createServer((req,res)=>{const url=new URL(req.url,'http://127.0.0.1');res.setHeader('Cache-Control','no-store');
 if(url.pathname==='/ready'){fs.appendFileSync(events,JSON.stringify({token:url.searchParams.get('token'),time:Date.now()})+'\n');res.writeHead(204);res.end();return;}
 res.setHeader('Content-Type','text/html');res.setHeader('Content-Security-Policy',"default-src 'none'; script-src 'unsafe-inline'; connect-src 'self'; style-src 'unsafe-inline'");
 res.end(`<!doctype html><title>Disposable native extension measurement</title><style>body{font:20px system-ui;background:#edf4ff;padding:40px}</style><h1>Controlled local page</h1><p>No credentials, accounts, or external page content.</p><script>addEventListener('load',()=>fetch('/ready?token='+encodeURIComponent(new URL(location.href).searchParams.get('token')),{cache:'no-store'}));</script>`);
});server.listen(0,'127.0.0.1',()=>fs.writeFileSync(port,String(server.address().port)));
'@ | Set-Content (Join-Path $run 'server.cjs') -Encoding utf8
$app=$null;$server=$null;$saved=@{};$rows=@()
$report=[ordered]@{scope='Paired release-executable observation; zero vs four real extensions; not a full performance audit';source_commit=(& git -C $root rev-parse HEAD).Trim();executable_sha256=(Get-FileHash $exe -Algorithm SHA256).Hash.ToLowerInvariant();status='failed';samples=@();notes=@('No benchmark flags; --dev only suppresses production updater.','Same persistent profiles reused after first trial; first samples cold-profile, later samples warm-profile.','Startup-to-ready includes process launch and local page load; navigation-to-ready includes keyboard interaction.','Memory is sum of owned process-tree private committed bytes and working sets, not PSS; shared pages may be counted twice.','Four packages were signature-verified and explicitly consented in the preceding runtime probe.');error=$null}
function Ready([string]$Token){if(!(Test-Path $events)){return $false};return [bool](@(Get-Content $events | ConvertFrom-Json | Where-Object {$_.token -eq $Token}).Count)}
function Await([scriptblock]$Condition,[string]$Description){$deadline=[DateTime]::UtcNow.AddSeconds(90);do{if($app){$app.Refresh();if($app.HasExited){throw "App exited before $Description"}};if(& $Condition){return};Start-Sleep -Milliseconds 50}while([DateTime]::UtcNow -lt $deadline);throw "Timed out: $Description"}
function Native-Count([string]$Profile){$file=Join-Path $Profile 'extension_ids.txt';if(!(Test-Path $file)){return 0};return @(Get-Content $file | Where-Object {$_ -match '^ext-[a-f0-9]{16}\t[a-p]{32}$'}).Count}
function Owned-Keys([string]$Keys){$owner=[uint32]0;[void][BowserExtensionMeasure]::GetWindowThreadProcessId($app.MainWindowHandle,[ref]$owner);$class=[Text.StringBuilder]::new(128);[void][BowserExtensionMeasure]::GetClassName($app.MainWindowHandle,$class,128);if($owner -ne $app.Id -or $class.ToString() -ne 'BowserNativeWindow'){throw 'Not an owned native browser window.'};[void][BowserExtensionMeasure]::SetForegroundWindow($app.MainWindowHandle);Start-Sleep -Milliseconds 100;if([BowserExtensionMeasure]::GetForegroundWindow() -ne $app.MainWindowHandle){throw 'No owned foreground; keyboard not sent.'};[Windows.Forms.SendKeys]::SendWait($Keys)}
function Memory-Tree {
 $snapshot=@(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe' OR ProcessId = $($app.Id)")
 $ids=[Collections.Generic.HashSet[int]]::new();[void]$ids.Add($app.Id)
 do{$changed=$false;foreach($p in $snapshot){if($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)){$changed=$true}}}while($changed)
 $entries=@();foreach($id in $ids){try{$p=Get-Process -Id $id -ErrorAction Stop;$entries+=@{pid=$p.Id;name=$p.ProcessName;private_bytes=$p.PrivateMemorySize64;working_set_bytes=$p.WorkingSet64}}catch{}}
 return $entries
}
try{
 foreach($name in @('LOCALAPPDATA','BOWSER_DATA_DIR','BOWSER_UI')){$saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process');[Environment]::SetEnvironmentVariable($name,$null,'Process')}
 $env:LOCALAPPDATA=Join-Path $ProfilesRoot 'isolated-localappdata';New-Item -ItemType Directory -Force $env:LOCALAPPDATA|Out-Null
 $server=Start-Process (Get-Command node).Source -ArgumentList @("`"$(Join-Path $run 'server.cjs')`"","`"$portFile`"","`"$events`"") -PassThru -WindowStyle Hidden
 Await {Test-Path $portFile} 'loopback server';$base='http://127.0.0.1:'+(Get-Content $portFile -Raw).Trim()
 for($trial=1;$trial -le $Trials;$trial++){
  foreach($case in @(@{name='zero';count=0},@{name='several';count=4})){
   $profile=Join-Path $ProfilesRoot $case.name;$token=[guid]::NewGuid().ToString('N');$env:BOWSER_DATA_DIR=$profile
   $clock=[Diagnostics.Stopwatch]::StartNew()
   $app=Start-Process $exe -ArgumentList @('--dev','--data-dir',"`"$profile`"",'--open-url',"$base/page?token=$token") -WorkingDirectory $root -PassThru -RedirectStandardError (Join-Path $Out "$($case.name)-$trial-stderr.log") -RedirectStandardOutput (Join-Path $Out "$($case.name)-$trial-stdout.log")
   Await {Ready $token} 'first native page ready';$ready=$clock.Elapsed.TotalMilliseconds
   Await {(Native-Count $profile) -eq $case.count} 'exact native signing-ID count';$extensionsReady=$clock.Elapsed.TotalMilliseconds
   Await {$app.Refresh();$app.MainWindowHandle -ne [IntPtr]::Zero} 'owned native window'
   Start-Sleep -Seconds 4
   $memory=@(Memory-Tree);$private=($memory | Measure-Object private_bytes -Sum).Sum;$working=($memory | Measure-Object working_set_bytes -Sum).Sum
   $second=[guid]::NewGuid().ToString('N');$nav=[Diagnostics.Stopwatch]::StartNew();Owned-Keys '^l';Owned-Keys "$base/page?token=$second{ENTER}";Await {Ready $second} 'second native page ready';$navMs=$nav.Elapsed.TotalMilliseconds
   $versions=@();$record=Join-Path $profile 'extensions.txt';if(Test-Path $record){$versions=@(Get-Content $record | ForEach-Object {$fields=$_ -split "`t";@{bowser_id=$fields[0];name=$fields[2];version=$fields[3]}})}
   $rows+=@{case=$case.name;trial=$trial;native_extension_count=(Native-Count $profile);process_to_first_page_ready_ms=$ready;process_to_extension_ready_ms=$extensionsReady;navigation_interaction_to_ready_ms=$navMs;private_bytes_sum=$private;working_set_bytes_sum=$working;processes=$memory;extensions=$versions}
   Stop-Process -Id $app.Id -Force;$app.WaitForExit(10000)|Out-Null;$app=$null;Start-Sleep -Seconds 2
  }
 }
 $report.status='measured-no-compatibility-or-performance-improvement-claim'
}catch{$report.error=$_.Exception.Message;throw}
finally{
 if($app -and !$app.HasExited){Stop-Process -Id $app.Id -Force;$app.WaitForExit(10000)|Out-Null}
 if($server -and !$server.HasExited){Stop-Process -Id $server.Id -Force}
 $report.samples=$rows;$report | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $Out 'extension-metrics.json') -Encoding utf8
 foreach($entry in $saved.GetEnumerator()){[Environment]::SetEnvironmentVariable($entry.Key,$entry.Value,'Process')}
 # Profiles include downloaded packages; never upload them. Only this validated disposable root.
 for($i=0;$i -lt 20 -and (Test-Path $ProfilesRoot);$i++){try{Remove-Item -LiteralPath $ProfilesRoot -Recurse -Force}catch{Start-Sleep -Milliseconds 500}}
}
