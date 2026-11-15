$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
Add-Type -TypeDefinition @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class PublishedNative {
 public delegate bool Callback(IntPtr h, IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumWindows(Callback cb, IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint id);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int n);
 public static IntPtr Find(uint pid){IntPtr found=IntPtr.Zero;EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);var name=new StringBuilder(256);GetClassName(h,name,256);if(owner==pid&&name.ToString()=="BowserNativeWindow"){found=h;return false;}return true;},IntPtr.Zero);return found;}
}
"@
$r='grantcharlie1000-tech/Bowser'
$source='85f59101dab6f6e3a1b4e5043c33fbdb4c1de011'
$run=Join-Path $env:RUNNER_TEMP ('published-v27-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $run|Out-Null
$out=Join-Path $env:RUNNER_TEMP 'published-v27-evidence'
New-Item -ItemType Directory -Force -Path $out|Out-Null
$report=[ordered]@{status='failed';source=$source;scope='Downloaded public binaries; isolated clean profile, prior-version persistent storage, positional URL handoff. Dev flag disables updater. Not updater install/restart, Spotify DRM or full UI QA.';checks=[ordered]@{};error=$null}
$app=$null;$sender=$null;$server=$null
$env:LOCALAPPDATA=Join-Path $run 'local-app-data'
$env:BOWSER_DATA_DIR=$null;$env:BOWSER_UI=$null;$env:BOWSER_UPDATE_URL=$null;$env:BOWSER_UPDATE_PUBKEY=$null
New-Item -ItemType Directory -Path $env:LOCALAPPDATA|Out-Null
function Wait-For([scriptblock]$test,[string]$name){$end=[DateTime]::UtcNow.AddSeconds(90);do{if(& $test){return};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$end);throw "Timed out: $name"}
$requests=Join-Path $out 'requests.jsonl'
if(Test-Path -LiteralPath $requests){throw 'Refusing preexisting fixture requests; fresh runtime evidence required'}
$portFile=Join-Path $run 'port.txt'
function Has([string]$marker){if(!(Test-Path $requests)){return $false};return [bool](@(Get-Content $requests|ConvertFrom-Json|Where-Object{$_.url-ceq$marker}).Count)}
function Stop-Owned {if($script:app){$script:app.Refresh();if(!$script:app.HasExited){if(!$script:app.CloseMainWindow()){throw 'Owned window did not accept graceful close'};if(!$script:app.WaitForExit(30000)){throw 'Owned window did not exit gracefully'}};if($script:app.ExitCode-ne 0){throw "Owned app exit $($script:app.ExitCode)"};$script:app=$null}}
function Launch([string]$data,[string]$route){New-Item -ItemType Directory -Force -Path $data|Out-Null;$log=Join-Path $out ($route.Trim('/')+'.csv');$script:app=Start-Process -FilePath $script:exe -ArgumentList @('--dev','--data-dir',('"'+$data+'"'),'--diagnostic-log',('"'+$log+'"'),'--open-url',($script:base+$route)) -PassThru -WindowStyle Hidden;Wait-For {$script:app.Refresh();if($script:app.HasExited){throw 'App exited before fixture ready'};($h=[PublishedNative]::Find([uint32]$script:app.Id))-ne[IntPtr]::Zero} 'owned native window';[void][PublishedNative]::ShowWindow([PublishedNative]::Find([uint32].Id),9);Start-Sleep -Milliseconds 300}
try{
 foreach($version in @('0.0.26','0.0.27')){
  $folder=Join-Path $run $version;New-Item -ItemType Directory -Path $folder|Out-Null
  gh release download "v$version" --repo $r --pattern Bowser.exe --pattern build-metadata.json --dir $folder
  if($LASTEXITCODE-ne 0){throw "Download $version failed"}
  $meta=Get-Content (Join-Path $folder 'build-metadata.json') -Raw|ConvertFrom-Json
  $binary=Join-Path $folder 'Bowser.exe';$hash=(Get-FileHash $binary -Algorithm SHA256).Hash.ToLowerInvariant()
  if($meta.version-cne$version-or$meta.sha256-cne$hash-or$meta.bytes-ne(Get-Item $binary).Length){throw "Published identity mismatch $version"}
  if($version-eq'0.0.27'-and($meta.commit-cne$source-or$meta.revision-ne 28-or$hash-cne'b4b8f4788adf1ff2acb19d4894cc8f886167fee50089c18f4213b579159be6e9')){throw 'Wrong v27 release'}
  $report.checks[('verified_public_'+$version)]=@{sha256=$hash;bytes=$meta.bytes;source=$meta.commit}
 }
 $js=@"
const http=require('node:http'),fs=require('node:fs');
const [portFile,requests]=process.argv.slice(2);
http.createServer((req,res)=>{
fs.appendFileSync(requests,JSON.stringify({url:req.url})+'\n');res.setHeader('Cache-Control','no-store');
if(req.url.startsWith('/ready/')){res.writeHead(204);res.end();return;}
const route=req.url;res.setHeader('Content-Type','text/html; charset=utf-8');
res.end('<!doctype html><title>Published Bowser runtime</title><h1>Local release fixture</h1><script>addEventListener("load",()=>{let route='+JSON.stringify(route)+';let value=localStorage.getItem("release-sentinel");if(route==="/old"){localStorage.setItem("release-sentinel","v26");fetch("/ready/old");}else if(route==="/upgraded"){fetch("/ready/upgraded/"+String(value));}else if(route==="/clean"){fetch("/ready/clean/"+String(value));}else if(route==="/external"){fetch("/ready/external/"+String(value));}});</script>');
}).listen(0,'127.0.0.1',function(){fs.writeFileSync(portFile,String(this.address().port));});
"@
 $fixture=Join-Path $run 'fixture.cjs';[IO.File]::WriteAllText($fixture,$js,[Text.UTF8Encoding]::new($false))
 $server=Start-Process -FilePath (Get-Command node).Source -ArgumentList @(('"'+$fixture+'"'),('"'+$portFile+'"'),('"'+$requests+'"')) -PassThru -WindowStyle Hidden
 Wait-For {Test-Path $portFile} 'fixture server';$base='http://127.0.0.1:'+(Get-Content $portFile -Raw)
 $install=Join-Path $run 'isolated-install';New-Item -ItemType Directory -Path $install|Out-Null;$exe=Join-Path $install 'Bowser.exe'
 Copy-Item (Join-Path $run '0.0.27/Bowser.exe') $exe
 Launch (Join-Path $run 'clean-profile') '/clean';Wait-For {Has '/ready/clean/null'} 'clean published v27 JavaScript';Stop-Owned;$report.checks.clean_profile_loaded_local_javascript=$true
 Copy-Item (Join-Path $run '0.0.26/Bowser.exe') $exe -Force
 $profile=Join-Path $run 'upgrade-profile';Launch $profile '/old';Wait-For {Has '/ready/old'} 'old published binary persistence';Stop-Owned
 Copy-Item (Join-Path $run '0.0.27/Bowser.exe') $exe -Force
 Launch $profile '/upgraded';Wait-For {Has '/ready/upgraded/v26'} 'v27 read v26 persistent storage';$report.checks.upgraded_copy_preserved_same_origin_storage=$true
 $owner=$app.Id
 $sender=Start-Process -FilePath $exe -ArgumentList @('--dev','--data-dir',('"'+$profile+'"'),($base+'/external')) -PassThru -WindowStyle Hidden
 if(!$sender.WaitForExit(30000)-or$sender.ExitCode-ne 0){throw 'Positional URL sender did not forward and exit successfully'}
 Wait-For {Has '/ready/external/v26'} 'existing-profile positional URL handoff';$app.Refresh();if($app.HasExited-or$app.Id-ne$owner){throw 'Original owned app was lost'}
 $report.checks.positional_url_forwarded_to_running_profile=$true;Stop-Owned
 $report.status='passed_downloaded_release_runtime';$report.finished_at_utc=[DateTime]::UtcNow.ToString('o')
}catch{$report.error=$_.Exception.Message;throw}finally{
 $report|ConvertTo-Json -Depth 10|Set-Content (Join-Path $out 'report.json') -Encoding utf8
 foreach($p in @($sender,$app,$server)){if($p){try{$p.Refresh();if(!$p.HasExited){Stop-Process -Id $p.Id -Force}}catch{}}}
}