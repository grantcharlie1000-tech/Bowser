# Actual native HWND pixels, never a renderer mock or performance measurement.
param([string]$Exe="$PSScriptRoot/../../target/release/bowser.exe",[string]$Out="$env:RUNNER_TEMP/bowser-layout-capture")
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class LC {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int c);
 [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int ht,bool repaint);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
}
'@
[LC]::SetProcessDPIAware()|Out-Null
New-Item -ItemType Directory -Force "$Out/frames"|Out-Null
$Out=(Resolve-Path $Out).Path;$Exe=(Resolve-Path $Exe).Path
$oldLocal=$env:LOCALAPPDATA;$oldUi=$env:BOWSER_UI
$profile=Join-Path $env:TEMP ('bowser-layout-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $profile|Out-Null
$frames=[Collections.Generic.List[object]]::new();$watch=[Diagnostics.Stopwatch]::StartNew()
$report=[ordered]@{status='not_started';source_commit=(git rev-parse HEAD).Trim();executable_sha256=(Get-FileHash $Exe -Algorithm SHA256).Hash.ToLowerInvariant();frames=0}
function Shot([string]$name){
 if([LC]::GetForegroundWindow() -ne $script:h){throw 'Bowser lost foreground ownership; refusing capture'}
 $r=[LC+RECT]::new();if(-not [LC]::GetWindowRect($script:h,[ref]$r)){throw 'GetWindowRect failed'}
 $w=$r.R-$r.L;$ht=$r.B-$r.T;if($w -lt 500 -or $ht -lt 400){throw 'Invalid dimensions'}
 $bmp=[Drawing.Bitmap]::new($w,$ht);$g=[Drawing.Graphics]::FromImage($bmp)
 try{$g.CopyFromScreen($r.L,$r.T,0,0,$bmp.Size);$bmp.Save("$Out/$name",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bmp.Dispose()}
}
function Record([string]$phase,[int]$duration){
 $end=$watch.ElapsedMilliseconds+$duration
 while($watch.ElapsedMilliseconds -lt $end){$name='frames/{0:D4}.png' -f $frames.Count;Shot $name;$frames.Add(@{file=$name;ms=$watch.ElapsedMilliseconds;phase=$phase});Start-Sleep -Milliseconds 100}
}
try{
 $env:LOCALAPPDATA=$profile;$env:BOWSER_UI='html'
 "tab_layout=vertical`nsidebar_collapsed=true`nsidebar_width=260`nstartup=last_session`n"|Set-Content "$profile/settings.txt" -Encoding utf8
 $session=[Collections.Generic.List[string]]::new()
 $session.Add('bowser-session 2');$session.Add('active 13')
 for($i=1;$i -le 13;$i++){$session.Add("tab`t1`thttps://example.com/?pin=$i`tPinned $i")}
 for($i=1;$i -le 4;$i++){$session.Add("tab`t0`thttps://example.com/?tab=$i`tNormal tab $i")}
 $session|Set-Content "$profile/session.txt" -Encoding utf8
 $script:p=Start-Process $Exe -ArgumentList @('--data-dir',$profile) -PassThru
 for($i=0;$i -lt 120;$i++){$script:p.Refresh();$script:h=$script:p.MainWindowHandle;if($script:h -ne [IntPtr]::Zero){break};if($script:p.HasExited){throw 'Bowser exited without window'};Start-Sleep -Milliseconds 500}
 if($script:h -eq [IntPtr]::Zero){throw 'No native window in runner session'}
 [LC]::ShowWindow($script:h,9)|Out-Null
 $work=[Windows.Forms.Screen]::PrimaryScreen.WorkingArea
 $width=[Math]::Min(1200,$work.Width-40);$height=[Math]::Min(760,$work.Height-40)
 if($width -lt 500 -or $height -lt 400){throw 'Runner desktop too small for useful capture'}
 [LC]::MoveWindow($script:h,$work.Left+20,$work.Top+20,$width,$height,$true)|Out-Null
 Start-Sleep -Seconds 10
 [LC]::SetForegroundWindow($script:h)|Out-Null;Start-Sleep -Milliseconds 250
 $origin=[LC+POINT]::new();if(-not [LC]::ClientToScreen($script:h,[ref]$origin)){throw 'Cannot locate client origin'}
 $dpi=[LC]::GetDpiForWindow($script:h);$report.dpi=$dpi
 # Windowed sensor sits inside the8 logical-pixel resize grip.
 $sensorX=$origin.X+[int][Math]::Round(12*$dpi/96.0)
 [LC]::SetCursorPos($origin.X+[int]($width*0.8),$origin.Y+[int]($height*0.65))|Out-Null
 Record 'closed' 1000;Shot '01-vertical-closed.png'
 [LC]::SetCursorPos($sensorX,$origin.Y+250)|Out-Null
 Record 'edge-reveal' 2000;Shot '02-vertical-revealed.png'
 [LC]::SetCursorPos($origin.X+100,$origin.Y+300)|Out-Null
 Record 'inside-sidebar' 1000;Shot '03-pinned-selected-unselected.png'
 [LC]::SetCursorPos($origin.X+[int]($width*0.8),$origin.Y+[int]($height*0.65))|Out-Null
 Record 'leave-and-hide' 2000;Shot '04-vertical-hidden-again.png'
 $report.status='captured_pending_visual_review'
}catch{$report.status='blocked';$report.error=$_.Exception.Message;throw}finally{
 $report.frames=$frames.Count
 ConvertTo-Json -InputObject $frames.ToArray() -Depth 4|Set-Content "$Out/frames.json" -Encoding utf8
 $report|ConvertTo-Json -Depth 4|Set-Content "$Out/capture-report.json" -Encoding utf8
 $json=ConvertTo-Json -InputObject $frames.ToArray() -Depth 4 -Compress
 @"
<!doctype html><meta charset="utf-8"><title>Real Bowser capture</title><style>body{background:#222;color:white;font:16px system-ui}img{max-width:100%}</style><h1>Real native Bowser capture</h1><p>PNG playback, not a video or frame-rate benchmark. Review visuals before claiming correctness.</p><button id="play">Play / restart</button> <span id="label"></span><br><img id="frame"><script>const frames=$json;let timer;function show(i){if(!frames[i])return;frame.src=frames[i].file;label.textContent=frames[i].phase+' '+frames[i].ms+'ms';if(frames[i+1])timer=setTimeout(()=>show(i+1),frames[i+1].ms-frames[i].ms)}play.onclick=()=>{clearTimeout(timer);show(0)};show(0)</script>
"@|Set-Content "$Out/recording.html" -Encoding utf8
 if($script:p){& taskkill /PID $script:p.Id /T /F|Out-Null}
 $env:LOCALAPPDATA=$oldLocal;$env:BOWSER_UI=$oldUi
}
