# Real native extension-page command/menu. Disposable Windows diagnostic, never a host bridge.
[CmdletBinding()]
param(
 [Parameter(Mandatory)][uint32]$OwnerPid,
 [Parameter(Mandatory)][string]$Out,
 [Parameter(Mandatory)][int]$ClientX,
 [Parameter(Mandatory)][int]$ClientY,
 [Parameter(Mandatory)][int]$ViewportWidth,
 [Parameter(Mandatory)][int]$ViewportHeight,
 [ValidateSet('ContextMenu','PageLinks','PageLinksEmpty')][string]$Mode='PageLinks',
 [string]$ExpectedLinkLabel,
 [ValidateRange(0,5000)][int]$SelectionDelayMs=0
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
 [DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr h);
 [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr menu);
 [DllImport("user32.dll")] public static extern uint GetMenuItemID(IntPtr menu,int position);
 [DllImport("user32.dll")] public static extern uint GetMenuState(IntPtr menu,uint item,uint flags);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetMenuString(IntPtr menu,uint item,StringBuilder text,int count,uint flags);
 [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr window,IntPtr menu,uint position,out RECT rect);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h,uint command);
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
if($Mode -eq 'ContextMenu') {
$client=[BowserExtensionLink+RECT]::new()
if(![BowserExtensionLink]::GetClientRect($h,[ref]$client)){throw 'Exact owned client bounds unavailable; no input sent.'}
$width=$client.Right-$client.Left;$height=$client.Bottom-$client.Top
if([Math]::Abs($width-$ViewportWidth) -gt 2 -or [Math]::Abs($height-$ViewportHeight) -gt 2){throw 'Observed COM viewport and native client dimensions disagree; no input sent.'}
if($ClientX -lt 0 -or $ClientY -lt 0 -or $ClientX -ge $width -or $ClientY -ge $height){throw 'Observed link point lies outside the exact owned client; no input sent.'}
$point=[BowserExtensionLink+POINT]::new();$point.X=$ClientX;$point.Y=$ClientY
if(![BowserExtensionLink]::ClientToScreen($h,[ref]$point)){throw 'Exact owned client-to-screen conversion failed; no input sent.'}
if([BowserExtensionLink]::GetForegroundWindow() -ne $h){throw 'Foreground changed before input; no input sent.'}
if(![BowserExtensionLink]::SetCursorPos($point.X,$point.Y)){throw 'Actual native cursor placement failed; no input sent.'}
[BowserExtensionLink]::mouse_event(0x0008,0,0,0,[UIntPtr]::Zero);[BowserExtensionLink]::mouse_event(0x0010,0,0,0,[UIntPtr]::Zero)
}else{
 if($Mode -eq 'PageLinks' -and !$ExpectedLinkLabel){throw 'Actual observed same-extension link label was not supplied; no command invoked.'}
 # Standard USER32 menu ownership and screen bounds are the authority for the command. Some
 # custom-window UIA providers expose only the child WebView, not its genuine native menu bar.
 # This is always a real mouse click on the verified native item, never a WM_COMMAND or DOM send.
 $nativeMenu=[BowserExtensionLink]::GetMenu($h)
 $commandName=[Text.StringBuilder]::new(128)
 if($nativeMenu -eq [IntPtr]::Zero -or [BowserExtensionLink]::GetMenuItemCount($nativeMenu) -ne 1 -or [BowserExtensionLink]::GetMenuItemID($nativeMenu,0) -ne 0x4100 -or [BowserExtensionLink]::GetMenuString($nativeMenu,0,$commandName,128,0x400) -le 0 -or $commandName.ToString() -ne ('Page links'+[char]0x2026)){throw 'Exact owned native Page links menu identity was not verified; no input sent.'}
 $commandState=[BowserExtensionLink]::GetMenuState($nativeMenu,0,0x400)
 if($commandState -eq [uint32]::MaxValue -or ($commandState -band 3) -ne 0){throw 'Native Page links command is disabled or unreadable; no input sent.'}
 $commandRect=[BowserExtensionLink+RECT]::new()
 if(![BowserExtensionLink]::GetMenuItemRect($h,$nativeMenu,0,[ref]$commandRect)){throw 'Exact native command screen bounds unavailable; no input sent.'}
 $r=$window.Current.BoundingRectangle
 if($commandRect.Right -le $commandRect.Left -or $commandRect.Bottom -le $commandRect.Top -or $commandRect.Left -lt $r.Left -or $commandRect.Top -lt $r.Top -or $commandRect.Right -gt $r.Right -or $commandRect.Bottom -gt $r.Bottom){throw 'Native menu bounds lie outside the exact owned window; no input sent.'}
 if([BowserExtensionLink]::GetForegroundWindow() -ne $h){throw 'Foreground changed before native menu click; no input sent.'}
 $actual=[uint32]0;[void][BowserExtensionLink]::GetWindowThreadProcessId($h,[ref]$actual)
 if($actual -ne $OwnerPid){throw 'Native owner changed before menu click; no input sent.'}
 if(![BowserExtensionLink]::SetCursorPos([int](($commandRect.Left+$commandRect.Right)/2),[int](($commandRect.Top+$commandRect.Bottom)/2))){throw 'Actual native menu cursor placement failed; no input sent.'}
 [BowserExtensionLink]::mouse_event(0x0002,0,0,0,[UIntPtr]::Zero);[BowserExtensionLink]::mouse_event(0x0004,0,0,0,[UIntPtr]::Zero)

}
if($Mode -eq 'PageLinksEmpty') {
 $dialog=$null;$deadline=[DateTime]::UtcNow.AddSeconds(15)
 do {
  foreach($candidate in $root.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition)) {
   try {if($candidate.Current.ProcessId -eq $OwnerPid -and $candidate.Current.Name -eq 'Extension page links'){$dialog=$candidate;break}}catch{}
  }
  if(!$dialog){Start-Sleep -Milliseconds 100}
 }while(!$dialog -and [DateTime]::UtcNow -lt $deadline)
 if(!$dialog){throw 'Deferred native no-links notice did not appear; no page fallback used.'}
 $dialogH=[IntPtr]$dialog.Current.NativeWindowHandle;$dialogClass=[Text.StringBuilder]::new(128);[void][BowserExtensionLink]::GetClassName($dialogH,$dialogClass,128)
 if($dialogClass.ToString() -ne '#32770' -or [BowserExtensionLink]::GetWindow($dialogH,4) -ne $h){throw 'Notice is not the exact native dialog owned by the extension window.'}
 $textCondition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'No eligible same-extension page links are present on this page.')
 if(!$dialog.FindFirst([Windows.Automation.TreeScope]::Descendants,$textCondition)){throw 'Exact no-links notice text was not observed.'}
 $okCondition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'OK')
 $ok=$dialog.FindFirst([Windows.Automation.TreeScope]::Descendants,$okCondition)
 if(!$ok -or $ok.Current.ProcessId -ne $OwnerPid -or $ok.Current.ControlType -ne [Windows.Automation.ControlType]::Button){throw 'Owned native notice dismissal was not found.'}
 $invoke=$null;if(!$ok.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$invoke)){throw 'Native notice OK has no InvokePattern.'}
 $invoke.Invoke();Write-Output 'NATIVE_EXTENSION_LINK deferred_empty_notice_observed_and_dismissed=true';return
}
# The menu can live in the embedded browser process. Restrict selection to the exact owner's
# descendant process tree and the Bowser-owned command name/type, not a similarly named website.
$processes=Get-CimInstance Win32_Process;$owned=[Collections.Generic.HashSet[uint32]]::new();[void]$owned.Add($OwnerPid)
do { $changed=$false;foreach($p in $processes){if($owned.Contains([uint32]$p.ParentProcessId) -and $owned.Add([uint32]$p.ProcessId)){$changed=$true}} }while($changed)
$menu=$null;$deadline=[DateTime]::UtcNow.AddSeconds(10)
do {
 foreach($item in $root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)) {
  try {
   $name=$item.Current.Name
   $matchName=if($Mode -eq 'ContextMenu'){$name -eq 'Open link in this extension window'}else{$name -eq $ExpectedLinkLabel}
   $matchOwner=if($Mode -eq 'PageLinks'){$item.Current.ProcessId -eq $OwnerPid}else{$owned.Contains([uint32]$item.Current.ProcessId)}
   if($matchName -and $matchOwner -and $item.Current.FrameworkId -eq 'Win32' -and $item.Current.ControlType -eq [Windows.Automation.ControlType]::MenuItem -and $item.Current.IsEnabled){$menu=$item;break}
  }catch{}
 }
 if(!$menu){Start-Sleep -Milliseconds 100}
}while(!$menu -and [DateTime]::UtcNow -lt $deadline)
if(!$menu){Save-OwnedWindow '02-native-menu-not-found.png';throw 'Owned native same-extension menu item did not appear.'}
$invoke=$null;if(!$menu.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$invoke)){throw 'Native menu item has no InvokePattern.'}
$selectedName=$menu.Current.Name
if($SelectionDelayMs -gt 0){Start-Sleep -Milliseconds $SelectionDelayMs}
$invoke.Invoke()
Start-Sleep -Seconds 2
Save-OwnedWindow '02-after-native-link-selection.png'
@{owner_pid=$OwnerPid;owner_window=$h.ToInt64();link='actual observed extension-page link';client_x=$ClientX;client_y=$ClientY;menu=$selectedName;route=if($Mode -eq 'PageLinks'){'real-owned-native-page-links-command-and-menu'}else{'real-native-right-click-and-owned-menu-invoke'}}|ConvertTo-Json|Set-Content (Join-Path $Out 'native-link-report.json') -Encoding utf8
Write-Output "NATIVE_EXTENSION_LINK mode=$Mode verified_owned_foreground=true actual_native_command_or_right_click=true owned_native_menu_invoked=true"
