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
 [StructLayout(LayoutKind.Sequential)] public struct MENUBARINFO { public uint cbSize; public RECT rcBar; public IntPtr hMenu,hwndMenu; public uint flags; }
 [StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO { public uint cbSize,flags; public IntPtr hwndActive,hwndFocus,hwndCapture,hwndMenuOwner,hwndMoveSize,hwndCaret; public RECT rcCaret; }
 public delegate bool EnumWindowProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowProc f,IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h,EnumWindowProc f,IntPtr p);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint thread,ref GUITHREADINFO info);
 [DllImport("user32.dll")] public static extern bool GetMenuBarInfo(IntPtr window,int obj,int item,ref MENUBARINFO info);
 [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr count,StringBuilder text,uint flags,uint timeout,out UIntPtr result);
 [DllImport("user32.dll",SetLastError=true)] public static extern IntPtr GetDlgItem(IntPtr h,int id);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT point);
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
 static readonly System.Diagnostics.Stopwatch clock=System.Diagnostics.Stopwatch.StartNew(); static int diagnostics;
 public static string DiagnosticsFile;
 static readonly System.Collections.Generic.List<string> diagnosticLines=new System.Collections.Generic.List<string>();
 public static void Diag(string phase) {
  if(++diagnostics>128)return;
  try { string line="utc="+DateTime.UtcNow.ToString("O")+" elapsed_ms="+clock.ElapsedMilliseconds+" "+phase;
   diagnosticLines.Add("\""+line.Replace("\\", "\\\\").Replace("\"", "\\\"")+"\"");
   if(DiagnosticsFile!=null)System.IO.File.WriteAllText(DiagnosticsFile,"["+String.Join(",",diagnosticLines)+"]",new UTF8Encoding(false));
  } catch { /* Diagnostic I/O never changes the authoritative selection/input predicate. */ }
 }
 public static void DialogState(IntPtr dialog,IntPtr owner,uint pid,uint tid,string phase) {
  uint actual;uint thread=GetWindowThreadProcessId(dialog,out actual);var cls=new StringBuilder(64);GetClassName(dialog,cls,64);
  Diag(phase+" alive="+IsWindow(dialog)+" pid_match="+(actual==pid)+" thread_match="+(thread==tid)+" owner_match="+(GetWindow(dialog,4)==owner)+" class_dialog="+(cls.ToString()=="#32770"));
 }
 public static void FailedDialogChildren(IntPtr dialog,IntPtr owner,uint pid,uint tid) {
  uint parentPid;uint parentThread=GetWindowThreadProcessId(dialog,out parentPid);var parentClass=new StringBuilder(64);
  GetClassName(dialog,parentClass,64);
  if(!IsWindow(dialog)||parentPid!=pid||parentThread!=tid||GetWindow(dialog,4)!=owner||parentClass.ToString()!="#32770"){Diag("failed_child_inventory skipped_parent_identity=true");return;}
  var title=new StringBuilder(64);GetWindowText(dialog,title,64);
  if(title.ToString()!="Extension page links"){Diag("failed_child_inventory skipped_parent_caption=true");return;}
  int count=0;EnumChildWindows(dialog,(child,p)=>{if(++count>16)return false;uint actual;uint thread=GetWindowThreadProcessId(child,out actual);
   if(actual!=pid||thread!=tid||GetParent(child)!=dialog){Diag("failed_child index="+count+" direct_owned=false");return true;}
   var cls=new StringBuilder(64);GetClassName(child,cls,64);int id=GetDlgCtrlID(child);var text=ReadOwnedChildText(child,dialog,pid,tid);
   string kind=text==null?"unreadable":text=="OK"?"exact-OK":text=="&OK"?"standard-mnemonic-OK":text=="No eligible same-extension page links are present on this page."?"exact-body":"other";
   Diag("failed_child index="+count+" direct_owned=true id="+id+" class_button="+(cls.ToString()=="Button")+" class_static="+(cls.ToString()=="Static")+" caption_kind="+kind);return true;},IntPtr.Zero);
 }
 public static IntPtr[] OwnedWindows(uint pid,uint tid,string cls) {
  var found=new System.Collections.Generic.List<IntPtr>();
  EnumWindows((h,p)=>{uint actual;uint thread=GetWindowThreadProcessId(h,out actual);var name=new StringBuilder(64);GetClassName(h,name,64);if(actual==pid&&thread==tid&&IsWindowVisible(h)&&name.ToString()==cls)found.Add(h);return true;},IntPtr.Zero);
  return found.ToArray();
 }
 public static string ReadOwnedChildText(IntPtr child,IntPtr parent,uint pid,uint tid) {
  uint actual;uint thread=GetWindowThreadProcessId(child,out actual);
  if(actual!=pid||thread!=tid||GetParent(child)!=parent)return null;
  var text=new StringBuilder(512);UIntPtr copied;
  Diag("wm_gettext_begin");var started=clock.ElapsedMilliseconds;
  var result=SendMessageTimeout(child,0x000D,(UIntPtr)512,text,0x23,250,out copied);
  Diag("wm_gettext_end success="+(result!=IntPtr.Zero)+" duration_ms="+(clock.ElapsedMilliseconds-started));
  if(result==IntPtr.Zero)return null;
  return text.ToString();
 }
 public static bool HasExactStaticText(IntPtr parent,string expected) {
  bool found=false;int count=0;uint pid;uint tid=GetWindowThreadProcessId(parent,out pid);
  EnumChildWindows(parent,(h,p)=>{if(++count>16)return false;var cls=new StringBuilder(64);GetClassName(h,cls,64);if(cls.ToString()=="Static"&&ReadOwnedChildText(h,parent,pid,tid)==expected)found=true;return true;},IntPtr.Zero);return found;
 }
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
trap { [BowserExtensionLink]::Diag('helper_failure_exit'); throw }
New-Item -ItemType Directory -Force $Out | Out-Null
[BowserExtensionLink]::DiagnosticsFile=Join-Path $Out 'native-notice-chronology.json'
$root=[Windows.Automation.AutomationElement]::RootElement
$window=$null; $deadline=[DateTime]::UtcNow.AddSeconds(15)
do {
 foreach($item in $root.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition)) {
  try { if($item.Current.ProcessId -ne $OwnerPid){continue};$h=[IntPtr]$item.Current.NativeWindowHandle;$class=[Text.StringBuilder]::new(128);[void][BowserExtensionLink]::GetClassName($h,$class,128);if($class.ToString() -eq 'BowserExtensionPage' -and [BowserExtensionLink]::IsWindowVisible($h)){$window=$item;break} }catch{}
 }
 if(!$window){Start-Sleep -Milliseconds 100}
}while(!$window -and [DateTime]::UtcNow -lt $deadline)
if(!$window){throw 'Exact owned visible extension page was not found; no input sent.'}
$h=[IntPtr]$window.Current.NativeWindowHandle;$owner=[uint32]0;$ownerThread=[BowserExtensionLink]::GetWindowThreadProcessId($h,[ref]$owner)
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
if($Mode -eq 'ContextMenu') { foreach($item in $window.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)) {
 try {$names+=@{name=$item.Current.Name;type=$item.Current.ControlType.ProgrammaticName;automation_id=$item.Current.AutomationId}}catch{}
}
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
 $commandPoint=[BowserExtensionLink+POINT]::new();$commandPoint.X=[int](($commandRect.Left+$commandRect.Right)/2);$commandPoint.Y=[int](($commandRect.Top+$commandRect.Bottom)/2)
 if([BowserExtensionLink]::WindowFromPoint($commandPoint) -ne $h -or ![BowserExtensionLink]::SetCursorPos($commandPoint.X,$commandPoint.Y)){throw 'Native command is obscured or cursor placement failed; no input sent.'}
 $actual=[uint32]0;$threadNow=[BowserExtensionLink]::GetWindowThreadProcessId($h,[ref]$actual)
 if($actual -ne $OwnerPid -or $threadNow -ne $ownerThread -or [BowserExtensionLink]::GetForegroundWindow() -ne $h -or [BowserExtensionLink]::WindowFromPoint($commandPoint) -ne $h -or [BowserExtensionLink]::GetMenu($h) -ne $nativeMenu -or [BowserExtensionLink]::GetMenuItemID($nativeMenu,0) -ne 0x4100 -or ([BowserExtensionLink]::GetMenuState($nativeMenu,0,0x400) -band 3) -ne 0){throw 'Native command owner/thread/foreground/menu changed before input.'}
 [BowserExtensionLink]::mouse_event(0x0002,0,0,0,[UIntPtr]::Zero);[BowserExtensionLink]::mouse_event(0x0004,0,0,0,[UIntPtr]::Zero)

}
function Read-NativeText([IntPtr]$Handle) {
 $text=[Text.StringBuilder]::new(512);[void][BowserExtensionLink]::GetWindowText($Handle,$text,512);return $text.ToString()
}
function Test-ContainedRect($Rect,$Outer) {
 return $Rect.Right -gt $Rect.Left -and $Rect.Bottom -gt $Rect.Top -and $Rect.Left -ge $Outer.Left -and $Rect.Top -ge $Outer.Top -and $Rect.Right -le $Outer.Right -and $Rect.Bottom -le $Outer.Bottom
}
function Send-OwnedPoint($Rect,[IntPtr]$HitWindow,[IntPtr]$Foreground) {
 $pidNow=[uint32]0;$tidNow=[BowserExtensionLink]::GetWindowThreadProcessId($HitWindow,[ref]$pidNow)
 if($pidNow -ne $OwnerPid -or $tidNow -ne $ownerThread -or [BowserExtensionLink]::GetForegroundWindow() -ne $Foreground){throw 'Exact native input owner/thread/foreground changed; no input sent.'}
 $point=[BowserExtensionLink+POINT]::new();$point.X=[int]([double]$Rect.Left+([double]$Rect.Right-$Rect.Left)/2);$point.Y=[int]([double]$Rect.Top+([double]$Rect.Bottom-$Rect.Top)/2)
 if([BowserExtensionLink]::WindowFromPoint($point) -ne $HitWindow){throw 'Actual native item is obscured or changed; no input sent.'}
 if(![BowserExtensionLink]::SetCursorPos($point.X,$point.Y)){throw 'Native item cursor placement failed; no input sent.'}
 if($Mode -eq 'PageLinks') {
  $gui=[BowserExtensionLink+GUITHREADINFO]::new();$gui.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($gui)
  if(![BowserExtensionLink]::GetGUIThreadInfo($ownerThread,[ref]$gui) -or $gui.hwndMenuOwner -ne $h -or ($gui.flags -band 0x10) -eq 0){throw 'Native menu owner changed immediately before input.'}
 }
 if([BowserExtensionLink]::WindowFromPoint($point) -ne $HitWindow -or [BowserExtensionLink]::GetForegroundWindow() -ne $Foreground){throw 'Native input target changed; no input sent.'}
 [BowserExtensionLink]::mouse_event(0x0002,0,0,0,[UIntPtr]::Zero);[BowserExtensionLink]::mouse_event(0x0004,0,0,0,[UIntPtr]::Zero)
}
[BowserExtensionLink]::Diag('command_input_returned')
if($Mode -eq 'PageLinksEmpty') {
 $dialog=[IntPtr]::Zero;$deadline=[DateTime]::UtcNow.AddSeconds(15)
 do {
  $matches=@([BowserExtensionLink]::OwnedWindows($OwnerPid,$ownerThread,'#32770')|Where-Object {
   [BowserExtensionLink]::GetWindow($_,4) -eq $h -and (Read-NativeText $_) -ceq 'Extension page links' -and [BowserExtensionLink]::HasExactStaticText($_,'No eligible same-extension page links are present on this page.')
  })
  if($matches.Count -gt 1){throw 'Native no-links notice is ambiguous; no input sent.'}
  if($matches.Count -eq 1){$dialog=$matches[0]}else{Start-Sleep -Milliseconds 100}
 }while($dialog -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
 if($dialog -eq [IntPtr]::Zero){throw 'Exact owned native no-links notice did not appear; no fallback used.'}
 [BowserExtensionLink]::DialogState($dialog,$h,$OwnerPid,$ownerThread,'exact_candidate_body_discovered')
 [BowserExtensionLink]::DialogState($dialog,$h,$OwnerPid,$ownerThread,'before_getdlgitem')
 $button=[BowserExtensionLink]::GetDlgItem($dialog,1);$getDlgError=[Runtime.InteropServices.Marshal]::GetLastWin32Error()
 [BowserExtensionLink]::Diag("getdlgitem_returned idok_nonzero=$($button -ne [IntPtr]::Zero) last_error_diagnostic_only=$getDlgError")
 [BowserExtensionLink]::DialogState($dialog,$h,$OwnerPid,$ownerThread,'after_getdlgitem')
 $class=[Text.StringBuilder]::new(64);[void][BowserExtensionLink]::GetClassName($button,$class,64)
 $buttonText=[BowserExtensionLink]::ReadOwnedChildText($button,$dialog,$OwnerPid,$ownerThread)
 $captionKind=if($null -eq $buttonText){'unreadable'}elseif($buttonText -ceq 'OK'){'exact-OK'}elseif($buttonText -ceq '&OK'){'standard-mnemonic-OK'}else{'unexpected'}
 $classMatches=$class.ToString() -ceq 'Button';$visible=[BowserExtensionLink]::IsWindowVisible($button);$enabled=[BowserExtensionLink]::IsWindowEnabled($button)
 Write-Output "NATIVE_EMPTY_NOTICE idok_nonzero=$($button -ne [IntPtr]::Zero) class_button=$classMatches caption_kind=$captionKind visible=$visible enabled=$enabled"
 if($button -eq [IntPtr]::Zero -or !$classMatches -or $captionKind -notin @('exact-OK','standard-mnemonic-OK') -or !$visible -or !$enabled){[BowserExtensionLink]::FailedDialogChildren($dialog,$h,$OwnerPid,$ownerThread);throw 'Exact native IDOK button was not verified; no input sent.'}
 $r=[BowserExtensionLink+RECT]::new();$outer=[BowserExtensionLink+RECT]::new()
 if(![BowserExtensionLink]::GetWindowRect($button,[ref]$r) -or ![BowserExtensionLink]::GetWindowRect($dialog,[ref]$outer) -or !(Test-ContainedRect $r $outer)){throw 'Native notice button bounds were not verified; no input sent.'}
 if([BowserExtensionLink]::GetWindow($dialog,4) -ne $h -or (Read-NativeText $dialog) -cne 'Extension page links' -or ![BowserExtensionLink]::HasExactStaticText($dialog,'No eligible same-extension page links are present on this page.')){throw 'Native notice identity changed; no input sent.'}
 Send-OwnedPoint $r $button $dialog
 $dismissDeadline=[DateTime]::UtcNow.AddSeconds(5)
 while([BowserExtensionLink]::IsWindowVisible($dialog) -and [DateTime]::UtcNow -lt $dismissDeadline){Start-Sleep -Milliseconds 100}
 if([BowserExtensionLink]::IsWindowVisible($dialog)){throw 'Native no-links notice remained visible after actual input; dismissal was not proven.'}
 Write-Output 'NATIVE_EXTENSION_LINK deferred_empty_notice_observed_and_real_os_dismissed=true';return
}
if($Mode -eq 'PageLinks') {
 function Read-OwnedMenuMatches {
  $info=[BowserExtensionLink+GUITHREADINFO]::new();$info.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($info)
  if(![BowserExtensionLink]::GetGUIThreadInfo($ownerThread,[ref]$info) -or $info.hwndMenuOwner -ne $h -or ($info.flags -band 0x10) -eq 0){return}
  foreach($popup in [BowserExtensionLink]::OwnedWindows($OwnerPid,$ownerThread,'#32768')) {
   $bar=[BowserExtensionLink+MENUBARINFO]::new();$bar.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($bar)
   if(![BowserExtensionLink]::GetMenuBarInfo($popup,-4,0,[ref]$bar) -or $bar.hMenu -eq [IntPtr]::Zero){continue}
   $count=[BowserExtensionLink]::GetMenuItemCount($bar.hMenu);if($count -lt 1 -or $count -gt 32){continue}
   for($index=0;$index -lt $count;$index++) {
    $label=[Text.StringBuilder]::new(128);$copied=[BowserExtensionLink]::GetMenuString($bar.hMenu,$index,$label,128,0x400)
    if($copied -le 0 -or $label.ToString() -cne $ExpectedLinkLabel){continue}
    $id=[BowserExtensionLink]::GetMenuItemID($bar.hMenu,$index);$state=[BowserExtensionLink]::GetMenuState($bar.hMenu,$index,0x400)
    if($id -ne $index+1 -or $state -eq [uint32]::MaxValue -or ($state -band 0x803) -ne 0){continue}
    $r=[BowserExtensionLink+RECT]::new();$outer=[BowserExtensionLink+RECT]::new()
    if([BowserExtensionLink]::GetMenuItemRect([IntPtr]::Zero,$bar.hMenu,$index,[ref]$r) -and [BowserExtensionLink]::GetWindowRect($popup,[ref]$outer) -and (Test-ContainedRect $r $outer)) {
     [pscustomobject]@{popup=$popup;menu=$bar.hMenu;index=$index;id=$id;rect=$r}
    }
   }
  }
 }
 $matches=@();$deadline=[DateTime]::UtcNow.AddSeconds(10)
 do {$matches=@(Read-OwnedMenuMatches);if($matches.Count -gt 1){throw 'Native exact-label menu item is ambiguous; no input sent.'};if($matches.Count -eq 0){Start-Sleep -Milliseconds 100}}while($matches.Count -eq 0 -and [DateTime]::UtcNow -lt $deadline)
 if($matches.Count -ne 1){Save-OwnedWindow '02-native-menu-not-found.png';throw 'Exact owned native menu item was not verified; no UIA or scripted fallback used.'}
 $selected=$matches[0]
 if($SelectionDelayMs -gt 0){Start-Sleep -Milliseconds $SelectionDelayMs}
 $fresh=@(Read-OwnedMenuMatches)
 if($fresh.Count -ne 1 -or $fresh[0].popup -ne $selected.popup -or $fresh[0].menu -ne $selected.menu -or $fresh[0].index -ne $selected.index -or $fresh[0].id -ne $selected.id){throw 'Native menu changed before actual input; no input sent.'}
 Send-OwnedPoint $fresh[0].rect $fresh[0].popup $h
 Start-Sleep -Seconds 2;Save-OwnedWindow '02-after-native-link-selection.png'
 @{owner_pid=$OwnerPid;owner_thread=$ownerThread;owner_window=$h.ToInt64();expected_label_verified=$true;native_menu_id=$selected.id;route='real-owned-native-menu-geometry-and-os-click'}|ConvertTo-Json|Set-Content (Join-Path $Out 'native-link-report.json') -Encoding utf8
 Write-Output 'NATIVE_EXTENSION_LINK mode=PageLinks exact_native_owner_thread_menu_label_id_state_bounds=true real_os_menu_click=true';return
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
