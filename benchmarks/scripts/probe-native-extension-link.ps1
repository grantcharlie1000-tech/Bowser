# Real native extension-page context menu. Disposable Windows diagnostic, never a host bridge.
[CmdletBinding()]
param(
 [Parameter(Mandatory)][uint32]$OwnerPid,
 [Parameter(Mandatory)][string]$Out,
 [Parameter(Mandatory)][int]$ClientX,
 [Parameter(Mandatory)][int]$ClientY,
 [Parameter(Mandatory)][int]$ViewportWidth,
 [Parameter(Mandatory)][int]$ViewportHeight
)
$ErrorActionPreference='Stop'; Set-StrictMode -Version Latest
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class BowserExtensionLink {
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
}
'@
New-Item -ItemType Directory -Force $Out | Out-Null
$root=[Windows.Automation.AutomationElement]::RootElement
$window=$null; $deadline=[DateTime]::UtcNow.AddSeconds(15)
do {
 foreach($item in $root.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition)) {
  try { if($item.Current.ProcessId -ne $OwnerPid){continue};$h=[IntPtr]$item.Current.NativeWindowHandle;$class=[Text.StringBuilder]::new(128);[void][BowserExtensionLink]::GetClassName($h,$class,128);if($class.ToString() -eq 'BowserExtensionPage' -and [BowserExtensionLink]::IsWindowVisible($h)){$window=$item;break} }catch{}
 }
 if(!$window){Start-Sleep -Milliseconds 100}
}while(!$window -and [DateTime]::UtcNow -lt $deadline)
if(!$window){throw 'Exact owned visible extension page was not found; no input sent.'}
$h=[IntPtr]$window.Current.NativeWindowHandle;$owner=[uint32]0;[void][BowserExtensionLink]::GetWindowThreadProcessId($h,[ref]$owner)
if($owner -ne $OwnerPid){throw 'Native owner changed; no input sent.'}
[void][BowserExtensionLink]::SetForegroundWindow($h);Start-Sleep -Milliseconds 300
if([BowserExtensionLink]::GetForegroundWindow() -ne $h){throw 'Exact extension page is not foreground; no input sent.'}
function Save-OwnedWindow([string]$Name) {
 $actual=[uint32]0;[void][BowserExtensionLink]::GetWindowThreadProcessId($h,[ref]$actual)
 if($actual -ne $OwnerPid -or ![BowserExtensionLink]::IsWindowVisible($h)){throw 'Owned screenshot window disappeared.'}
 $r=$window.Current.BoundingRectangle
 if($r.Width -le 0 -or $r.Height -le 0){throw 'Owned screenshot bounds are empty.'}
 $bitmap=[Drawing.Bitmap]::new([int]$r.Width,[int]$r.Height)
 $graphics=[Drawing.Graphics]::FromImage($bitmap)
 try {$graphics.CopyFromScreen([int]$r.X,[int]$r.Y,0,0,$bitmap.Size);$bitmap.Save((Join-Path $Out $Name),[Drawing.Imaging.ImageFormat]::Png)}finally{$graphics.Dispose();$bitmap.Dispose()}
}
Save-OwnedWindow '01-actual-signed-popup.png'
# The exact signed link's real bounds are observed through the already-owned in-process test
# COM document, not inferred from an ambiguous icon accessible name or a guessed screen pixel.
$names=@()
foreach($item in $window.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)) {
 try {$names+=@{name=$item.Current.Name;type=$item.Current.ControlType.ProgrammaticName;automation_id=$item.Current.AutomationId}}catch{}
}
$names|ConvertTo-Json|Set-Content (Join-Path $Out 'link-accessibility.json') -Encoding utf8
$client=[BowserExtensionLink+RECT]::new()
if(![BowserExtensionLink]::GetClientRect($h,[ref]$client)){throw 'Exact owned client bounds unavailable; no input sent.'}
$width=$client.Right-$client.Left;$height=$client.Bottom-$client.Top
if([Math]::Abs($width-$ViewportWidth) -gt 2 -or [Math]::Abs($height-$ViewportHeight) -gt 2){throw 'Observed COM viewport and native client dimensions disagree; no input sent.'}
if($ClientX -lt 0 -or $ClientY -lt 0 -or $ClientX -ge $width -or $ClientY -ge $height){throw 'Observed link point lies outside the exact owned client; no input sent.'}
$point=[BowserExtensionLink+POINT]::new();$point.X=$ClientX;$point.Y=$ClientY
if(![BowserExtensionLink]::ClientToScreen($h,[ref]$point)){throw 'Exact owned client-to-screen conversion failed; no input sent.'}
if([BowserExtensionLink]::GetForegroundWindow() -ne $h){throw 'Foreground changed before input; no input sent.'}
[void][BowserExtensionLink]::SetCursorPos($point.X,$point.Y)
[BowserExtensionLink]::mouse_event(0x0008,0,0,0,[UIntPtr]::Zero);[BowserExtensionLink]::mouse_event(0x0010,0,0,0,[UIntPtr]::Zero)
# The menu can live in the embedded browser process. Restrict selection to the exact owner's
# descendant process tree and the Bowser-owned command name/type, not a similarly named website.
$processes=Get-CimInstance Win32_Process;$owned=[Collections.Generic.HashSet[uint32]]::new();[void]$owned.Add($OwnerPid)
do { $changed=$false;foreach($p in $processes){if($owned.Contains([uint32]$p.ParentProcessId) -and $owned.Add([uint32]$p.ProcessId)){$changed=$true}} }while($changed)
$menu=$null;$deadline=[DateTime]::UtcNow.AddSeconds(10)
do {
 $condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Open link in this extension window')
 foreach($item in $root.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)) {
  try {if($owned.Contains([uint32]$item.Current.ProcessId) -and $item.Current.ControlType -eq [Windows.Automation.ControlType]::MenuItem -and $item.Current.IsEnabled){$menu=$item;break}}catch{}
 }
 if(!$menu){Start-Sleep -Milliseconds 100}
}while(!$menu -and [DateTime]::UtcNow -lt $deadline)
if(!$menu){throw 'Owned native same-extension menu item did not appear.'}
$invoke=$null;if(!$menu.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$invoke)){throw 'Native menu item has no InvokePattern.'}
$invoke.Invoke()
Start-Sleep -Seconds 2
Save-OwnedWindow '02-after-native-link-selection.png'
@{owner_pid=$OwnerPid;owner_window=$h.ToInt64();link='actual signed #popup-manage-button, COM-observed client bounds';client_x=$ClientX;client_y=$ClientY;menu='Open link in this extension window';route='real-native-right-click-and-owned-menu-invoke'}|ConvertTo-Json|Set-Content (Join-Path $Out 'native-link-report.json') -Encoding utf8
Write-Output 'NATIVE_EXTENSION_LINK actual_signed_link_com_bounds=true verified_owned_foreground=true real_right_click=true owned_native_menu_invoked=true'
