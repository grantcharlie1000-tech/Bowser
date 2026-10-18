# Genuine native storefront smoke. No fake host, no remote debugger, no install consent bypass.
# Captures are evidence of rendering only, not installation/functionality acceptance.
[CmdletBinding()]
param([string]$Out = "$env:RUNNER_TEMP/bowser-native-store", [switch]$InstallJourney)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$exe = Join-Path $root 'target/debug/bowser.exe'
if (!(Test-Path $exe)) { throw 'Build the HTML-default debug executable first.' }
New-Item -ItemType Directory -Force $Out | Out-Null
$run = Join-Path ([IO.Path]::GetTempPath()) ('bowser-native-store-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $run | Out-Null
$initialFile=Join-Path $run 'start.html'
'<!doctype html><title>Disposable Bowser Store probe</title><h1>Disposable native control</h1>' | Set-Content -LiteralPath $initialFile -Encoding utf8
$initialUrl=([uri]$initialFile).AbsoluteUri
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class BowserStoreSmoke {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder value,int count);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int command);
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
 $class=[Text.StringBuilder]::new(128);[void][BowserStoreSmoke]::GetClassName($script:window,$class,$class.Capacity)
 if($pidOwner -eq $app.Id -and $class.ToString() -eq '#32770') {
  $dialog=[Windows.Automation.AutomationElement]::FromHandle($script:window)
  $texts=$dialog.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
  $report.startup_error_dialog=@($texts | ForEach-Object {try{$_.Current.Name}catch{''}} | Where-Object {$_})
  throw ('Owned startup dialog, not browser UI: ' + ($report.startup_error_dialog -join ' | '))
 }
 if ($pidOwner -ne $app.Id -or ![BowserStoreSmoke]::IsWindowVisible($script:window) -or $class.ToString() -ne 'BowserNativeWindow') { throw ('Not the disposable native browser window. Class=' + $class.ToString()) }
}
function Keys([string]$Value) {
 Assert-Owned
 [void][BowserStoreSmoke]::ShowWindow($script:window,9)
 for($attempt=0;$attempt -lt 3;$attempt++) {
  try { [Windows.Automation.AutomationElement]::FromHandle($script:window).SetFocus() } catch {}
  [void][BowserStoreSmoke]::SetForegroundWindow($script:window)
  Start-Sleep -Milliseconds 250
  if([BowserStoreSmoke]::GetForegroundWindow() -eq $script:window){break}
 }
 if ([BowserStoreSmoke]::GetForegroundWindow() -ne $script:window) { $report.foreground_window=[BowserStoreSmoke]::GetForegroundWindow().ToInt64();$report.owned_window=$script:window.ToInt64();throw 'Owned foreground unavailable; no keyboard sent.' }
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
function Find-Control([string]$Name, [switch]$Prefix) {
 Assert-Owned
 $owned=[Windows.Automation.AutomationElement]::FromHandle($script:window)
 $all=$owned.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
 foreach($item in $all) {
  try { if($item.Current.IsOffscreen -or !$item.Current.IsEnabled -or $item.Current.ControlType -eq [Windows.Automation.ControlType]::Text){continue}; $actual=$item.Current.Name; if(($Prefix -and $actual.StartsWith($Name,[StringComparison]::Ordinal)) -or (!$Prefix -and $actual -eq $Name)){return $item} } catch {}
 }
 return $null
}
function Wait-Control([string]$Name,[switch]$Prefix) {
 $deadline=[DateTime]::UtcNow.AddSeconds(90)
 do { $found=Find-Control $Name -Prefix:$Prefix; if($found){return $found}; $app.Refresh();if($app.HasExited){throw 'Native app exited during journey.'};Start-Sleep -Milliseconds 300 } while([DateTime]::UtcNow -lt $deadline)
 throw "Native accessible control did not appear: $Name"
}
function Invoke-Control($Control) {
 Assert-Owned
 $pattern=$null
 if(!$Control.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){throw "No native InvokePattern for $($Control.Current.Name)"}
 $pattern.Invoke();Start-Sleep -Milliseconds 400
}
function Wait-Window {
 $deadline=[DateTime]::UtcNow.AddSeconds(60);$script:window=[IntPtr]::Zero
 do{Start-Sleep -Milliseconds 200;$app.Refresh();if($app.HasExited){throw 'Disposable native app exited.'};$script:window=$app.MainWindowHandle}while($script:window -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
 Assert-Owned;[void][BowserStoreSmoke]::MoveWindow($script:window,20,20,1100,800,$true);Start-Sleep -Seconds 5
}
function Records {
 return @(Get-ChildItem -LiteralPath $run -Filter extensions.txt -File -Recurse | Where-Object { (Get-Content $_.FullName -Raw) -match '\S' })
}
function Run-InstallJourney {
 # These are standard Windows accessibility actions against this exact disposable app window.
 # No WebView script, test bridge, native consent bypass or user credential is involved.
 $search=Wait-Control 'Search extensions';$value=$null
 if(!$search.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$value)){throw 'Search ValuePattern unavailable.'}
 $value.SetValue('uBlock Origin Lite');Start-Sleep -Milliseconds 500
 Invoke-Control (Wait-Control 'uBlock Origin Lite')
 Capture '04-native-store-details'
 Invoke-Control (Wait-Control 'Get uBlock Origin Lite')
 $approve=Wait-Control 'Install uBlock Origin Lite ' -Prefix
 if(@(Records).Count -ne 0){throw 'An extension was recorded before consent.'}
 Capture '05-native-full-permission-review'
 $report.review_button=$approve.Current.Name
 $owned=[Windows.Automation.AutomationElement]::FromHandle($script:window)
 $texts=$owned.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
 @($texts | ForEach-Object { try{$_.Current.Name}catch{''} } | Where-Object {$_}) | ConvertTo-Json | Set-Content (Join-Path $Out 'review-accessibility-text.json') -Encoding utf8
 Invoke-Control (Wait-Control 'Cancel installation')
 if(@(Records).Count -ne 0){throw 'Cancellation recorded an installation.'}
 $report.cancel_before_consent='passed-no-record'
 Capture '06-native-cancelled'
 Invoke-Control (Wait-Control 'Get uBlock Origin Lite')
 $approve=Wait-Control 'Install uBlock Origin Lite ' -Prefix
 # Explicit authorized disposable-test consent to the real displayed package/version/permissions.
 $report.explicit_test_consent=$approve.Current.Name
 Invoke-Control $approve
 Invoke-Control (Wait-Control 'Manage installed extensions')
 $deadline=[DateTime]::UtcNow.AddSeconds(30)
 while(@(Records).Count -eq 0 -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 300}
 if(@(Records).Count -ne 1){throw 'Expected exactly one disposable owning-profile extension record.'}
 $recordFile=(Records)[0].FullName
 $report.installed_record=Get-Content $recordFile -Raw
 Capture '07-native-installed-before-restart'
 # Real process restart of the same persistent disposable profile, not a fresh engine simulation.
 Stop-Process -Id $app.Id -Force;$app.WaitForExit(10000)|Out-Null;Start-Sleep -Seconds 2
 $script:app=Start-Process $exe -ArgumentList @('--dev','--data-dir',"`"$env:BOWSER_DATA_DIR`"",'--open-url',$initialUrl) -WorkingDirectory $root -PassThru
 Wait-Window
 Keys '^l';Keys 'bowser://store{ENTER}';Start-Sleep -Seconds 4
 Invoke-Control (Wait-Control 'Installed')
 $popup=Wait-Control 'Open uBlock Origin Lite popup'
 $report.restart_persisted='record-and-running-popup-control-present'
 Capture '08-native-installed-after-restart'
 Invoke-Control $popup
 Start-Sleep -Seconds 3
 # Popup hosting is genuine; this does not by itself prove blocker behavior or targeting.
 $report.popup_invoked='native-owned-extension-popup-control'
 Capture '09-native-popup-invoked'
 # Toggle via the actual accessible switch, then re-enable before removal.
 $toggle=Wait-Control 'uBlock Origin Lite on';$toggle.SetFocus();Keys ' ';Start-Sleep -Seconds 2
 $report.disabled_record=Get-Content $recordFile -Raw
 if($report.disabled_record -notmatch '^ext-[a-f0-9]{16}\t0\t'){throw 'Native disable switch did not persist disabled state.'}
 $toggle=Wait-Control 'uBlock Origin Lite on';$toggle.SetFocus();Keys ' ';Start-Sleep -Seconds 2
 if((Get-Content $recordFile -Raw) -notmatch '^ext-[a-f0-9]{16}\t1\t'){throw 'Native re-enable switch did not persist enabled state.'}
 Invoke-Control (Wait-Control 'Remove uBlock Origin Lite')
 $confirm=Wait-Control 'Confirm removing uBlock Origin Lite'
 $remove=$confirm.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Remove uBlock Origin Lite'))
 if(!$remove){throw 'Explicit remove confirmation control absent.'}
 Invoke-Control $remove
 $deadline=[DateTime]::UtcNow.AddSeconds(30)
 while(@(Records).Count -ne 0 -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 300}
 if(@(Records).Count -ne 0){throw 'Native removal did not clear the owning profile record.'}
 $report.removal='passed-no-record'
 Capture '10-native-removed'
}

try {
 foreach($name in @('LOCALAPPDATA','BOWSER_DATA_DIR','BOWSER_UI')) { $saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process'); [Environment]::SetEnvironmentVariable($name,$null,'Process') }
 $env:LOCALAPPDATA=Join-Path $run 'localappdata'; $env:BOWSER_DATA_DIR=Join-Path $run 'appdata'
 New-Item -ItemType Directory -Force $env:LOCALAPPDATA,$env:BOWSER_DATA_DIR | Out-Null
 # --dev skips production updater; unlike benchmark mode it does not disable extension startup.
 $app=Start-Process $exe -ArgumentList @('--dev','--data-dir',"`"$env:BOWSER_DATA_DIR`"",'--open-url',$initialUrl) -WorkingDirectory $root -PassThru
 $deadline=[DateTime]::UtcNow.AddSeconds(60); $script:window=[IntPtr]::Zero
 do { Start-Sleep -Milliseconds 200; $app.Refresh(); if($app.HasExited){throw 'Disposable native app exited.'}; $script:window=$app.MainWindowHandle } while($script:window -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
 Assert-Owned; [void][BowserStoreSmoke]::MoveWindow($script:window,20,20,1100,800,$true)
 Start-Sleep -Seconds 5
 Capture '00-native-control-before-keyboard'
 Keys '^l';Keys 'bowser://store{ENTER}';Start-Sleep -Seconds 4
 Capture '01-native-store-discover'
 # This is ordinary omnibox navigation, not an install bridge or injected UI command.
 Keys '^l'; Keys 'bowser://settings{ENTER}'; Start-Sleep -Seconds 3
 Capture '02-native-settings'
 Keys '^l'; Keys 'bowser://store{ENTER}'; Start-Sleep -Seconds 3
 Capture '03-native-store-return'
 if($InstallJourney){Run-InstallJourney}
 $app.Refresh(); $report.native_process_working_set_bytes=$app.WorkingSet64
 $report.status=if($InstallJourney){'native-journey-awaiting-visual-review-not-functional-compatibility'}else{'captured-awaiting-visual-review'}
} catch { $report.error=$_.Exception.Message; throw }
finally {
 if($app -and !$app.HasExited) { Stop-Process -Id $app.Id -Force; $app.WaitForExit(10000) | Out-Null }
 foreach($entry in $saved.GetEnumerator()){[Environment]::SetEnvironmentVariable($entry.Key,$entry.Value,'Process')}
 $report | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $Out 'native-store-report.json') -Encoding utf8
 # Only our randomly named disposable root. No installed browser/user profile is touched.
 for($i=0;$i -lt 10 -and (Test-Path $run);$i++){try{Remove-Item -LiteralPath $run -Recurse -Force}catch{Start-Sleep -Milliseconds 500}}
}
