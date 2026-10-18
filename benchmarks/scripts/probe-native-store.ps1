# Genuine native storefront smoke. No fake host, no remote debugger, no install consent bypass.
# Captures are evidence of rendering only, not installation/functionality acceptance.
[CmdletBinding()]
param([string]$Out = "$env:RUNNER_TEMP/bowser-native-store")
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$exe = Join-Path $root 'target/debug/bowser.exe'
if (!(Test-Path $exe)) { throw 'Build the HTML-default debug executable first.' }
New-Item -ItemType Directory -Force $Out | Out-Null
$run = Join-Path ([IO.Path]::GetTempPath()) ('bowser-native-store-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $run | Out-Null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class BowserStoreSmoke {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int height,bool repaint);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
}
'@
$app = $null
$saved = @{}
$report = [ordered]@{ scope='Native Store rendering smoke, not installation or functional compatibility'; status='failed'; commit=(& git -C $root rev-parse HEAD).Trim(); screenshots=@(); error=$null }
function Assert-Owned {
 $pidOwner=[uint32]0
 [void][BowserStoreSmoke]::GetWindowThreadProcessId($script:window,[ref]$pidOwner)
 if ($pidOwner -ne $app.Id -or ![BowserStoreSmoke]::IsWindowVisible($script:window)) { throw 'Not the disposable process window.' }
}
function Keys([string]$Value) {
 Assert-Owned
 [void][BowserStoreSmoke]::SetForegroundWindow($script:window)
 Start-Sleep -Milliseconds 250
 if ([BowserStoreSmoke]::GetForegroundWindow() -ne $script:window) { throw 'Owned foreground unavailable; no keyboard sent.' }
 [Windows.Forms.SendKeys]::SendWait($Value)
}
function Capture([string]$Name) {
 Assert-Owned
 $rect=[BowserStoreSmoke+RECT]::new()
 if (![BowserStoreSmoke]::GetWindowRect($script:window,[ref]$rect)) { throw 'No window bounds.' }
 $bitmap=[Drawing.Bitmap]::new($rect.Right-$rect.Left,$rect.Bottom-$rect.Top)
 $graphics=[Drawing.Graphics]::FromImage($bitmap); $dc=$graphics.GetHdc()
 try { $ok=[BowserStoreSmoke]::PrintWindow($script:window,$dc,2) }
 finally { $graphics.ReleaseHdc($dc); $graphics.Dispose() }
 try {
  if (!$ok) { throw 'Native screenshot API failed.' }
  $colors=[Collections.Generic.HashSet[int]]::new()
  for($x=0;$x -lt $bitmap.Width;$x+=19) { for($y=0;$y -lt $bitmap.Height;$y+=19) { [void]$colors.Add($bitmap.GetPixel($x,$y).ToArgb()) } }
  if($colors.Count -lt 4) { throw 'Uniform/blank native GPU capture; rendering is inconclusive.' }
  $path=Join-Path $Out "$Name.png"; $bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png)
  $report.screenshots+=@{file="$Name.png";sha256=(Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant();sampled_colors=$colors.Count}
 } finally { $bitmap.Dispose() }
}
try {
 foreach($name in @('LOCALAPPDATA','BOWSER_DATA_DIR','BOWSER_UI')) { $saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process'); [Environment]::SetEnvironmentVariable($name,$null,'Process') }
 $env:LOCALAPPDATA=Join-Path $run 'localappdata'; $env:BOWSER_DATA_DIR=Join-Path $run 'appdata'
 New-Item -ItemType Directory -Force $env:LOCALAPPDATA,$env:BOWSER_DATA_DIR | Out-Null
 # --dev skips production updater; unlike benchmark mode it does not disable extension startup.
 $app=Start-Process $exe -ArgumentList @('--dev','--data-dir',"`"$env:BOWSER_DATA_DIR`"",'--open-url','bowser://store') -WorkingDirectory $root -PassThru
 $deadline=[DateTime]::UtcNow.AddSeconds(60); $script:window=[IntPtr]::Zero
 do { Start-Sleep -Milliseconds 200; $app.Refresh(); if($app.HasExited){throw 'Disposable native app exited.'}; $script:window=$app.MainWindowHandle } while($script:window -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
 Assert-Owned; [void][BowserStoreSmoke]::MoveWindow($script:window,20,20,1100,800,$true)
 Start-Sleep -Seconds 8
 Capture '01-native-store-discover'
 # This is ordinary omnibox navigation, not an install bridge or injected UI command.
 Keys '^l'; Keys 'bowser://settings{ENTER}'; Start-Sleep -Seconds 3
 Capture '02-native-settings'
 Keys '^l'; Keys 'bowser://store{ENTER}'; Start-Sleep -Seconds 3
 Capture '03-native-store-return'
 $app.Refresh(); $report.native_process_working_set_bytes=$app.WorkingSet64
 $report.status='captured-awaiting-visual-review'
} catch { $report.error=$_.Exception.Message; throw }
finally {
 if($app -and !$app.HasExited) { Stop-Process -Id $app.Id -Force; $app.WaitForExit(10000) | Out-Null }
 foreach($entry in $saved.GetEnumerator()){[Environment]::SetEnvironmentVariable($entry.Key,$entry.Value,'Process')}
 $report | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $Out 'native-store-report.json') -Encoding utf8
 # Only our randomly named disposable root. No installed browser/user profile is touched.
 for($i=0;$i -lt 10 -and (Test-Path $run);$i++){try{Remove-Item -LiteralPath $run -Recurse -Force}catch{Start-Sleep -Milliseconds 500}}
}
