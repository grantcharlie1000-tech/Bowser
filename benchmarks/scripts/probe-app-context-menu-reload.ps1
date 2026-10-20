# Distinct actual-App Reload lane: one right-click, then NO further user input.
# Real App::run/BowserNativeWindow input only. Not the separate Static/Session experiment.
# Coordinates come from the exact COM-owned source document/controller, never global discovery.
# RequestReceipt is written by the cfg(test) production-menu coordinator observer before display.
# Actual retained request/root HMENU and fresh native popup evidence gate one ordinary App Reload.
# Rust owns the only exact-owner activation attempt and requires its actual successful return.
# This helper only verifies fresh foreground/active proof, with no activation, repair or retry.
# This helper does not invoke accessibility, send WM_COMMAND,
# dispatch a selected command, dismiss unrelated UI, kill a browser, or infer missing acceptance.
[CmdletBinding()]
param([Parameter(Mandatory)][uint32]$OwnerPid,[Parameter(Mandatory)][long]$OwnerHandle,[Parameter(Mandatory)][string]$OwnerExe,[Parameter(Mandatory)][uint32]$BrowserPid,
 [Parameter(Mandatory)][int]$ClientX,[Parameter(Mandatory)][int]$ClientY,[Parameter(Mandatory)][int]$ViewportWidth,[Parameter(Mandatory)][int]$ViewportHeight,
 [Parameter(Mandatory)][int]$ControlLeft,[Parameter(Mandatory)][int]$ControlTop,[Parameter(Mandatory)][int]$ControlRight,[Parameter(Mandatory)][int]$ControlBottom,
 [Parameter(Mandatory)][string]$RequestReceipt,[Parameter(Mandatory)][string]$ProbeNonce,[Parameter(Mandatory)][string]$SourceViewIdentity,
 [Parameter(Mandatory)][string]$Proof0,[Parameter(Mandatory)][string]$Proof1,[Parameter(Mandatory)][string]$Proof2,
 [Parameter(Mandatory)][string]$Out)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;using System.Collections.Generic;
public static class BowserAppContext {
 [StructLayout(LayoutKind.Sequential)] public struct POINT {public int X,Y;}
 [StructLayout(LayoutKind.Sequential)] public struct RECT {public int Left,Top,Right,Bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct GUI {public uint cbSize,flags;public IntPtr hwndActive,hwndFocus,hwndCapture,hwndMenuOwner,hwndMoveSize,hwndCaret;public RECT rcCaret;}
 [StructLayout(LayoutKind.Sequential)] public struct MENU {public uint cbSize;public RECT rcBar;public IntPtr hMenu,hwndMenu;public uint flags;}
 [StructLayout(LayoutKind.Sequential)] public struct TIME {public uint Low,High;public ulong Bits {get{return ((ulong)High<<32)|Low;}}}
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f,IntPtr p);
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder text,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr count,StringBuilder text,uint flags,uint timeout,out UIntPtr copied);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint thread,ref GUI info);
 [DllImport("user32.dll")] public static extern bool GetMenuBarInfo(IntPtr h,int obj,int item,ref MENU info);
 [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr menu);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetMenuString(IntPtr menu,uint index,StringBuilder label,int count,uint flags);
 [DllImport("user32.dll")] public static extern uint GetMenuItemID(IntPtr menu,uint index);
 [DllImport("user32.dll")] public static extern uint GetMenuState(IntPtr menu,uint index,uint flags);
 [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr window,IntPtr menu,uint index,out RECT bounds);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
 [DllImport("user32.dll")] public static extern bool IsChild(IntPtr parent,IntPtr child);
 [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint flags);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
 [DllImport("kernel32.dll")] static extern uint GetProcessId(IntPtr process);
 [DllImport("kernel32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool QueryFullProcessImageName(IntPtr process,uint flags,StringBuilder text,ref uint n);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out TIME created,out TIME exited,out TIME kernel,out TIME user);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateProcess(IntPtr process,uint exitCode);
 [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle,uint ms);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 public static string Class(IntPtr h){var b=new StringBuilder(64);GetClassName(h,b,64);return b.ToString();}
 static bool HostBase(IntPtr h,uint pid,uint tid){uint p;return IsWindow(h)&&GetWindowThreadProcessId(h,out p)==tid&&p==pid&&Class(h)=="BowserNativeWindow";}
 public static bool Host(IntPtr h,uint pid,uint tid){
  return tid!=0&&HostBase(h,pid,tid)&&GetAncestor(h,2)==h&&GetParent(h)==IntPtr.Zero&&HostBase(h,pid,tid);
 }
 public static bool Input(IntPtr h,uint pid,uint tid,int x,int y,int w,int height,POINT screen){
  RECT r;POINT now=new POINT{X=x,Y=y};if(!Host(h,pid,tid)||!IsWindowVisible(h)||!IsWindowEnabled(h)||GetForegroundWindow()!=h||!GetClientRect(h,out r)||r.Right-r.Left!=w||r.Bottom-r.Top!=height||x<0||y<0||x>=w||y>=height||!ClientToScreen(h,ref now)||now.X!=screen.X||now.Y!=screen.Y)return false;
  var gui=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};if(!GetGUIThreadInfo(tid,ref gui)||gui.hwndActive!=h||gui.hwndCapture!=IntPtr.Zero)return false;
  var hit=WindowFromPoint(screen);return HostBase(h,pid,tid)&&GetForegroundWindow()==h&&(hit==h||(hit!=IntPtr.Zero&&IsChild(h,hit)&&GetAncestor(hit,2)==h));
 }
 public static IntPtr[] Popups(uint pid,uint tid,out bool complete){var list=new List<IntPtr>();int count=0;bool bounded=true;bool ok=EnumWindows((h,p)=>{if(++count>128){bounded=false;return false;}uint actual;uint thread=GetWindowThreadProcessId(h,out actual);if(actual==pid&&thread==tid&&Class(h)=="#32768"&&IsWindowVisible(h))list.Add(h);return true;},IntPtr.Zero);complete=bounded&&ok;return list.ToArray();}
 public static bool PopupVisibleNow(IntPtr host,uint pid,uint tid,IntPtr popup,IntPtr menu,OriginalProcess guard){
  uint actual;var gui=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};var bar=new MENU{cbSize=(uint)Marshal.SizeOf(typeof(MENU))};
  if(!guard.SameAlive()||!Host(host,pid,tid)||!IsWindow(popup)||!IsWindowVisible(popup)||GetWindowThreadProcessId(popup,out actual)!=tid||actual!=pid||Class(popup)!="#32768"||!GetMenuBarInfo(popup,-4,0,ref bar)||bar.hMenu!=menu||!GetGUIThreadInfo(tid,ref gui)||gui.hwndActive!=host||gui.hwndMenuOwner!=host||(gui.flags&0x10)==0)return false;
  bar=new MENU{cbSize=(uint)Marshal.SizeOf(typeof(MENU))};
  return Host(host,pid,tid)&&IsWindow(popup)&&IsWindowVisible(popup)&&GetWindowThreadProcessId(popup,out actual)==tid&&actual==pid&&Class(popup)=="#32768"&&GetMenuBarInfo(popup,-4,0,ref bar)&&bar.hMenu==menu&&HostBase(host,pid,tid)&&GetForegroundWindow()==host&&guard.SameAlive();
 }
 [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h,uint command);
 [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent,EnumProc callback,IntPtr data);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 public sealed class ReadOnlyBrowser:IDisposable {
  IntPtr handle;uint pid;ulong created;string image;
  public ReadOnlyBrowser(uint expected){handle=OpenProcess(0x00101000,false,expected);if(handle==IntPtr.Zero)throw new InvalidOperationException("Native browser read-only handle unavailable.");try{pid=expected;image=Image();created=Creation();if(!String.Equals(System.IO.Path.GetFileName(image),"msedgewebview2.exe",StringComparison.OrdinalIgnoreCase)||!SameAlive())throw new InvalidOperationException("Native browser read-only identity unavailable.");}catch{Dispose();throw;}}
  string Image(){var b=new StringBuilder(32768);uint n=32768;if(!QueryFullProcessImageName(handle,0,b,ref n))throw new InvalidOperationException("Native browser image query unavailable.");return System.IO.Path.GetFullPath(b.ToString());}
  ulong Creation(){TIME a,b,c,d;if(!GetProcessTimes(handle,out a,out b,out c,out d))throw new InvalidOperationException("Native browser creation query unavailable.");return a.Bits;}
  public bool SameAlive(){return handle!=IntPtr.Zero&&GetProcessId(handle)==pid&&Creation()==created&&String.Equals(Image(),image,StringComparison.OrdinalIgnoreCase)&&WaitForSingleObject(handle,0)==258;}
  public void Dispose(){if(handle!=IntPtr.Zero){CloseHandle(handle);handle=IntPtr.Zero;}}
 }
 static object GuiSnapshot(uint tid,IntPtr witness,uint pid){
  uint beforePid;uint beforeThread=GetWindowThreadProcessId(witness,out beforePid);
  if(beforeThread!=tid||beforePid!=pid||!IsWindow(witness))return new {thread=tid,witness_rechecked=false,query_performed=false};
  var g=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};bool ok=GetGUIThreadInfo(tid,ref g);
  uint afterPid;uint afterThread=GetWindowThreadProcessId(witness,out afterPid);
  if(afterThread!=tid||afterPid!=pid||!IsWindow(witness))return new {thread=tid,witness_rechecked=false,query_performed=true};
  return new {thread=tid,witness=witness.ToInt64(),witness_pid=pid,witness_rechecked=true,non_atomic_observation=true,query_performed=true,query_ok=ok,flags=g.flags,active=g.hwndActive.ToInt64(),focus=g.hwndFocus.ToInt64(),capture=g.hwndCapture.ToInt64(),menu_owner=g.hwndMenuOwner.ToInt64()};
 }
 static object WindowSnapshot(IntPtr h,IntPtr host,uint pid,uint tid,string source){
  uint actual;uint thread=GetWindowThreadProcessId(h,out actual);
  if(actual!=pid||thread!=tid||!IsWindow(h))return new {source=source,window=h.ToInt64(),status="expected-pid-thread-unavailable-before-read"};
  var c=new StringBuilder(64);int n=GetClassName(h,c,64);RECT r;bool bounds=GetWindowRect(h,out r);
  IntPtr p=GetParent(h),owner=GetWindow(h,4),root=GetAncestor(h,2),rootOwner=GetAncestor(h,3);bool chain=false;IntPtr cursor=h;int hops=0;
  for(;hops<16&&cursor!=IntPtr.Zero;hops++){if(cursor==host){chain=true;break;}cursor=GetWindow(cursor,4);}
  bool visible=IsWindowVisible(h),enabled=IsWindowEnabled(h),isChild=IsChild(host,h);
  uint afterPid;uint afterThread=GetWindowThreadProcessId(h,out afterPid);
  if(afterPid!=pid||afterThread!=tid||!IsWindow(h))return new {source=source,window=h.ToInt64(),status="expected-pid-thread-unavailable-after-read-details-discarded"};
  return new {source=source,window=h.ToInt64(),pid=actual,thread=thread,non_atomic_observation=true,pid_thread_same_after=true,window_alive=true,class_query_ok=n>0,class_truncated=n==63,class_name=c.ToString(),visible=visible,enabled=enabled,bounds_ok=bounds,left=r.Left,top=r.Top,right=r.Right,bottom=r.Bottom,parent=p.ToInt64(),owner=owner.ToInt64(),root=root.ToInt64(),root_owner=rootOwner.ToInt64(),is_actual_child=isChild,root_is_host=root==host,owner_chain_reaches_host=chain,owner_chain_complete=chain||cursor==IntPtr.Zero,owner_chain_truncated=!chain&&cursor!=IntPtr.Zero&&hops==16};
 }
 public static object Topology(IntPtr host,uint testPid,uint browserPid){
  var records=new List<object>();var threads=new Dictionary<uint,Tuple<IntPtr,uint>>();var seen=new HashSet<long>();int examinedTop=0,examinedChild=0;bool topCap=false,childCap=false,recordCap=false,readFailure=false;
  Action<IntPtr,string> capture=(h,source)=>{uint pid;uint tid=GetWindowThreadProcessId(h,out pid);if(pid!=testPid&&pid!=browserPid)return;if(!seen.Add(h.ToInt64()))return;if(records.Count>=96){recordCap=true;return;}try{records.Add(WindowSnapshot(h,host,pid,tid,source));uint afterPid;uint afterTid=GetWindowThreadProcessId(h,out afterPid);if(tid!=0&&threads.Count<32&&afterTid==tid&&afterPid==pid&&IsWindow(h))threads[tid]=Tuple.Create(h,pid);}catch{readFailure=true;}};
  bool top=EnumWindows((h,p)=>{if(++examinedTop>128){topCap=true;return false;}capture(h,"top-level");return true;},IntPtr.Zero);
  // EnumChildWindows' documented return is not used as an enumeration-success indicator.
  EnumChildWindows(host,(h,p)=>{if(++examinedChild>128){childCap=true;return false;}capture(h,"host-descendant");return true;},IntPtr.Zero);
  uint foregroundPid;IntPtr foreground=GetForegroundWindow();uint foregroundThread=GetWindowThreadProcessId(foreground,out foregroundPid);
  var gui=new List<object>();int guiCount=0;foreach(var witness in threads){if(guiCount++>=16)break;gui.Add(GuiSnapshot(witness.Key,witness.Value.Item1,witness.Value.Item2));}
  return new {coverage="observed-test-and-native-browser-process-only-not-menu-authority",top_enumeration_return=top,top_cap=topCap,child_cap=childCap,window_record_cap=recordCap,read_failure=readFailure,top_examined=examinedTop,child_examined=examinedChild,thread_cap=threads.Count>16,foreground=foreground.ToInt64(),foreground_pid=foregroundPid,foreground_thread=foregroundThread,windows=records,thread_gui=gui};
 }
 public sealed class OriginalProcess:IDisposable {
  IntPtr handle;uint pid;ulong created;string image;
  public OriginalProcess(uint expected,string path){
   if(!System.Text.RegularExpressions.Regex.IsMatch(System.IO.Path.GetFileName(path),"^bowser-[0-9a-f]+\\.exe$")||!System.IO.Path.GetDirectoryName(path).EndsWith("\\target\\debug\\deps",StringComparison.OrdinalIgnoreCase))throw new InvalidOperationException("Not the exact disposable single-test executable.");
   handle=OpenProcess(0x00101001,false,expected);if(handle==IntPtr.Zero)throw new InvalidOperationException("Original test process handle unavailable.");
   try{pid=expected;image=System.IO.Path.GetFullPath(path);created=Creation();if(GetProcessId(handle)!=pid||!SameImage()||WaitForSingleObject(handle,0)!=258)throw new InvalidOperationException("Original test process identity mismatch.");}catch{Dispose();throw;}
  }
  ulong Creation(){TIME a,b,c,d;if(!GetProcessTimes(handle,out a,out b,out c,out d))throw new InvalidOperationException("Original process creation-time query failed.");return a.Bits;}
  bool SameImage(){var b=new StringBuilder(32768);uint n=32768;return QueryFullProcessImageName(handle,0,b,ref n)&&String.Equals(System.IO.Path.GetFullPath(b.ToString()),image,StringComparison.OrdinalIgnoreCase);}
  public bool SameAlive(){return handle!=IntPtr.Zero&&GetProcessId(handle)==pid&&Creation()==created&&SameImage()&&WaitForSingleObject(handle,0)==258;}
  public string FailOnly(IntPtr host,uint tid){try{if(handle==IntPtr.Zero||GetProcessId(handle)!=pid||Creation()!=created||!SameImage()||WaitForSingleObject(handle,0)!=258||!Host(host,pid,tid))return "identity-unavailable-no-termination";if(!TerminateProcess(handle,74))return "exact-handle-termination-failed";return WaitForSingleObject(handle,3000)==0?"exact-single-test-process-terminated":"exact-handle-termination-wait-unconfirmed";}catch{return "identity-query-failed-no-termination";}}
  public void Dispose(){if(handle!=IntPtr.Zero){CloseHandle(handle);handle=IntPtr.Zero;}}
 }
}
'@
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force $Out|Out-Null
$report=[ordered]@{scope='real-App-run-BowserNativeWindow-current-engine-request-and-genuine-native-input';status='failed';activation_owner='Rust-exact-original-App-owner-single-attempt';activation_requested=$false;activation_returned=$null;right_input_called=$false;escape_called=$false;selection_called=$false;selected_command_id=$null;error=$null;failure_cleanup=$null}
$observationClock=[Diagnostics.Stopwatch]::StartNew();$report.topology=@()
$h=[IntPtr]$OwnerHandle;$pidNow=[uint32]0;$tid=[BowserAppContext]::GetWindowThreadProcessId($h,[ref]$pidNow);$guard=$null;$browserGuard=$null
function Save-Report {[IO.File]::WriteAllText((Join-Path $Out 'app-context-menu.json'),($report|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))}
function Observe-Topology([string]$Phase) {
 $observation=[ordered]@{phase=$Phase;elapsed_ms=$observationClock.ElapsedMilliseconds;identity_before=$false;identity_after=$false;status='unavailable';native=$null;screenshot_status='not-attempted'}
 try {
  if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserAppContext]::Host($h,$OwnerPid,$tid)){throw 'diagnostic-identity-unavailable'}
  $observation.identity_before=$true
  $observation.native=[BowserAppContext]::Topology($h,$OwnerPid,$BrowserPid)
  $observation.identity_after=($guard.SameAlive() -and $browserGuard.SameAlive() -and [BowserAppContext]::Host($h,$OwnerPid,$tid))
  $observation.status=if($observation.identity_after){'captured-read-only-not-input-authority'}else{'identity-unavailable-after-read'}
 } catch {$observation.status='read-only-topology-unavailable'}
 $report.topology+=,$observation;Save-Report
 # Native screenshot is diagnostic only; it does not identify or authorize a menu. No desktop capture.
 try {
  if(!$observation.identity_after -or !$guard.SameAlive() -or ![BowserAppContext]::Host($h,$OwnerPid,$tid)){throw 'diagnostic-capture-identity-unavailable'}
  $rect=[BowserAppContext+RECT]::new()
  if(![BowserAppContext]::GetWindowRect($h,[ref]$rect)){throw 'diagnostic-capture-bounds-unavailable'}
  $width=$rect.Right-$rect.Left;$height=$rect.Bottom-$rect.Top
  if($width -le 0 -or $height -le 0 -or $width -gt 4096 -or $height -gt 4096){throw 'diagnostic-capture-bounds-outside-limit'}
  $bitmap=[Drawing.Bitmap]::new($width,$height)
  try {
   $graphics=[Drawing.Graphics]::FromImage($bitmap)
   try {
    $dc=$graphics.GetHdc()
    try {$ok=[BowserAppContext]::PrintWindow($h,$dc,2)} finally {$graphics.ReleaseHdc($dc)}
   } finally {$graphics.Dispose()}
   if(!$ok -or !$guard.SameAlive() -or ![BowserAppContext]::Host($h,$OwnerPid,$tid)){throw 'diagnostic-capture-unconfirmed'}
   $name="native-app-context-$Phase.png";$bitmap.Save((Join-Path $Out $name),[Drawing.Imaging.ImageFormat]::Png)
   $observation.screenshot_status='captured-owned-host-only-not-independent-menu-proof';$observation.screenshot=$name
  } finally {$bitmap.Dispose()}
 } catch {$observation.screenshot_status='owned-host-capture-unavailable'}
 Save-Report
}
function Neutral {foreach($key in @(1,2,16,17,18,27,91,92)){if([BowserAppContext]::GetAsyncKeyState($key) -lt 0){throw 'Unexpected held modifier/Escape; no repair or input.'}}}
# Read the exact retained request witness before publishing visibility. File reads are not a
# production authority: the coordinator independently fences source COM/document/profile/owner.
$script:request=$null
function Request-State([switch]$AllowPending) {
 if(!(Test-Path -LiteralPath $RequestReceipt -PathType Leaf)){
  if($AllowPending){return $null};throw 'Exact coordinator request witness absent.'
 }
 $file=Get-Item -LiteralPath $RequestReceipt
 if($file.Length -le 0 -or $file.Length -gt 8192){throw 'Coordinator request witness size invalid.'}
 $r=[IO.File]::ReadAllText($file.FullName)|ConvertFrom-Json
 if($r.schema -cne 'bowser-real-app-menu-reload-v1' -or $r.nonce -cne $ProbeNonce -or $r.phase -cne 'before-show' -or
    [long]$r.owner -ne $OwnerHandle -or [string]$r.view_identity -cne $SourceViewIdentity -or
    [string]$r.proof0 -cne $Proof0 -or [string]$r.proof1 -cne $Proof1 -or [string]$r.proof2 -cne $Proof2 -or
    [string]$r.request -notmatch '^[1-9][0-9]*$' -or [int]$r.fixture_command -le 0 -or
    [string]$r.owner_token -notmatch '^[1-9][0-9]*$' -or [string]$r.request_epoch -notmatch '^[1-9][0-9]*$' -or [long]$r.menu_handle -eq 0){throw 'Coordinator request/source/document identity mismatch.'}
 if($script:request -and ($r.request -cne $script:request.request -or $r.fixture_command -ne $script:request.fixture_command -or $r.tab -cne $script:request.tab -or $r.owner_token -cne $script:request.owner_token -or $r.request_epoch -cne $script:request.request_epoch -or $r.menu_handle -ne $script:request.menu_handle)){throw 'Current coordinator request changed; no further input.'}
 if(!$script:request){$script:request=$r;$report.request=$r;Save-Report}
 return $r
}
function Source-Control {
 if($ControlLeft -lt 0 -or $ControlTop -lt 0 -or $ControlRight -le $ControlLeft -or $ControlBottom -le $ControlTop -or
    $ControlRight -gt $ViewportWidth -or $ControlBottom -gt $ViewportHeight -or
    $ClientX -le $ControlLeft -or $ClientX -ge $ControlRight -or $ClientY -le $ControlTop -or $ClientY -ge $ControlBottom){throw 'Exact COM source/control bounds do not contain input point.'}
}
function Menu-State {
 param([switch]$AllowPending)
 if(!$guard.SameAlive() -or !$browserGuard.SameAlive()){throw 'Exact original/native-browser process identity changed.'}
 if(![BowserAppContext]::Host($h,$OwnerPid,$tid) -or [BowserAppContext]::GetForegroundWindow() -ne $h){throw 'Exact menu foreground/host unavailable or changed.'}
 $witness=Request-State -AllowPending:$AllowPending
 if(!$witness){$report.readiness.last_predicate='coordinator-witness-not-present';return $null}
 $gui=[BowserAppContext+GUI]::new();$gui.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($gui)
 if(![BowserAppContext]::GetGUIThreadInfo($tid,[ref]$gui) -or $gui.hwndActive -ne $h){throw 'Exact owning STA active state unavailable.'}
 if(($gui.flags -band 0x10) -eq 0){if($AllowPending){$report.readiness.last_predicate='popup-mode-not-present';return $null};throw 'Exact owning STA native popup state unavailable.'}
 if($gui.hwndMenuOwner -ne $h){throw 'Native popup owner changed.'}
 $complete=$false;$popups=@([BowserAppContext]::Popups($OwnerPid,$tid,[ref]$complete))
 if(!$complete -or $popups.Count -gt 1){throw 'Visible native popup inventory incomplete or not unique.'}
 if($popups.Count -eq 0){if($AllowPending){$report.readiness.last_predicate='popup-window-not-present';return $null};throw 'Visible native popup unavailable.'}
 $popup=$popups[0];$bar=[BowserAppContext+MENU]::new();$bar.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($bar)
 $rect=[BowserAppContext+RECT]::new()
 if(![BowserAppContext]::IsWindowEnabled($popup) -or ![BowserAppContext]::GetWindowRect($popup,[ref]$rect) -or $rect.Right -le $rect.Left -or $rect.Bottom -le $rect.Top -or ![BowserAppContext]::GetMenuBarInfo($popup,-4,0,[ref]$bar) -or $bar.hMenu -eq [IntPtr]::Zero){throw 'Exact visible native menu geometry/handle unavailable.'}
 if($bar.hMenu.ToInt64() -ne [long]$witness.menu_handle){throw 'Visible native HMENU is not this exact retained request.'}
 $count=[BowserAppContext]::GetMenuItemCount($bar.hMenu);if($count -lt 1 -or $count -gt 64){throw 'Native menu item count unavailable or outside bound.'}
 return @{window=$popup;menu=$bar.hMenu;count=$count}
}

try {
 if($pidNow -ne $OwnerPid -or ![BowserAppContext]::Host($h,$OwnerPid,$tid)){throw 'Exact disposable host unavailable; no input.'}
 $guard=[BowserAppContext+OriginalProcess]::new($OwnerPid,$OwnerExe)
 $browserGuard=[BowserAppContext+ReadOnlyBrowser]::new($BrowserPid)
 Observe-Topology 'before-input-conditioning-check'
 Neutral
 if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserAppContext]::Host($h,$OwnerPid,$tid) -or ![BowserAppContext]::IsWindowVisible($h) -or ![BowserAppContext]::IsWindowEnabled($h)){throw 'Exact retained owner unavailable before input; no helper activation.'}
 Observe-Topology 'after-fresh-owner-proof-before-input'
 if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserAppContext]::Host($h,$OwnerPid,$tid) -or [BowserAppContext]::GetForegroundWindow() -ne $h){throw 'Exact Rust-conditioned owner foreground unavailable; no input, activation or retry.'}
 $point=[BowserAppContext+POINT]::new();$point.X=$ClientX;$point.Y=$ClientY
 if(![BowserAppContext]::ClientToScreen($h,[ref]$point)){throw 'Exact controlled point conversion failed.'}
 Source-Control;Neutral
 if(![BowserAppContext]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point)){throw 'Exact owned input target unavailable before cursor.'}
 if(![BowserAppContext]::SetCursorPos($point.X,$point.Y)){throw 'Native cursor placement failed.'}
 Source-Control;Neutral
 if(![BowserAppContext]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point)){throw 'Exact owned hit target changed before real right-click.'}
 if(!$guard.SameAlive() -or !$browserGuard.SameAlive()){throw 'Retained owner/browser identity changed before right input.'}
 try{[BowserAppContext]::mouse_event(0x0008,0,0,0,[UIntPtr]::Zero);$report.right_input_called=$true}finally{[BowserAppContext]::mouse_event(0x0010,0,0,0,[UIntPtr]::Zero)}
 $watch=[Diagnostics.Stopwatch]::StartNew();Observe-Topology 'after-right-input';$report.readiness=@{elapsed_ms=0;last_predicate='not-observed';polls=0};$menu=$null
 do {
  $report.readiness.polls++;$menu=Menu-State -AllowPending;$report.readiness.elapsed_ms=$watch.ElapsedMilliseconds
  if($watch.ElapsedMilliseconds -ge 10000){Observe-Topology 'readiness-expired';throw 'Native popup readiness window expired; topology unavailable.'}
  if($menu){$report.readiness.last_predicate='strict-native-menu-ready';break}
  Start-Sleep -Milliseconds 50
 } while($true)
 $watch.Stop();$report.popup=@{window=$menu.window.ToInt64();menu=$menu.menu.ToInt64();count=$menu.count;owner_thread=$tid;owner_pid=$OwnerPid}
 # Recheck the exact retained request/HMENU immediately before publishing visibility.
 # This is evidence for the App's already-armed timer, never permission for helper input.
 $witness=Request-State;$fresh=Menu-State
 if($fresh.window -ne $menu.window -or $fresh.menu -ne $menu.menu -or $fresh.count -ne $menu.count -or ![BowserAppContext]::PopupVisibleNow($h,$OwnerPid,$tid,$menu.window,$menu.menu,$guard)){throw 'Exact native popup changed before read-only visibility witness.'}
 $visible=[ordered]@{schema='bowser-real-app-menu-reload-visible-v1';nonce=$ProbeNonce;request=[string]$witness.request;owner=$OwnerHandle;owner_token=[string]$witness.owner_token;request_epoch=[string]$witness.request_epoch;menu_handle=$menu.menu.ToInt64();popup_handle=$menu.window.ToInt64();owner_thread=$tid;owner_pid=$OwnerPid}
 $pending=Join-Path $Out 'native-visible.pending';$visiblePath=Join-Path $Out 'native-visible.json'
 $bytes=[Text.UTF8Encoding]::new($false).GetBytes(($visible|ConvertTo-Json -Compress))
 $completionWatch=[Diagnostics.Stopwatch]::StartNew() # Start no later than native-visible publication.
 $file=[IO.File]::Open($pending,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
 try{$file.Write($bytes,0,$bytes.Length);$file.Flush($true)}finally{$file.Dispose()}
 [IO.File]::Move($pending,$visiblePath);$report.native_visible_witness=$visible;Save-Report
 # No Escape, selection, cursor movement, focus repair or other input exists below this point.
 # One monotonic five-second observation starts before publication, not an old-lane extension.
 $completed=$null;$disappeared=$false;$observationAccepted=$false
 do {
  if($completionWatch.ElapsedMilliseconds -ge 5000){break}
  if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserAppContext]::Host($h,$OwnerPid,$tid)){throw 'Exact retained process/owner changed after visibility.'}
  $after=[BowserAppContext+GUI]::new();$after.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($after)
  if(![BowserAppContext]::GetGUIThreadInfo($tid,[ref]$after) -or $after.hwndActive -ne $h -or [BowserAppContext]::GetForegroundWindow() -ne $h){throw 'Exact active/foreground owner lost during no-further-input observation; no focus repair.'}
  $disappeared=(![BowserAppContext]::IsWindow($menu.window) -and ($after.flags -band 0x10) -eq 0 -and $after.hwndCapture -eq [IntPtr]::Zero -and $after.hwndActive -eq $h -and [BowserAppContext]::GetForegroundWindow() -eq $h)
  $completionPath=Join-Path $Out 'actual-completion.json'
  if(Test-Path -LiteralPath $completionPath -PathType Leaf){
   $file=Get-Item -LiteralPath $completionPath;if($file.Length -le 0 -or $file.Length -gt 8192){throw 'Actual completion witness size invalid.'}
   $completed=[IO.File]::ReadAllText($completionPath)|ConvertFrom-Json
   if($completed.schema -cne 'bowser-real-app-menu-reload-complete-v1' -or $completed.nonce -cne $ProbeNonce -or $completed.request -cne $witness.request -or $completed.owner_token -cne $witness.owner_token -or $completed.request_epoch -cne $witness.request_epoch -or [long]$completed.owner -ne $OwnerHandle -or [long]$completed.menu_handle -ne $menu.menu.ToInt64() -or $completed.selected -ne $null -or $completed.setter_calls -ne 0 -or $completed.setter_result -ne $null -or $completed.complete_calls -ne 1 -or $completed.complete_error -ne $null -or $completed.cleanup_ok -ne $true){throw 'Matching actual Complete/cleanup or zero-setter witness refused.'}
  }
  # Completion reads can block or reenter external state. Reject late results BEFORE acceptance,
  # then repeat exact retained process/owner/thread and native popup/capture proof after those reads.
  if($completionWatch.ElapsedMilliseconds -ge 5000){break}
  if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserAppContext]::Host($h,$OwnerPid,$tid)){throw 'Exact retained process/owner changed after completion read.'}
  $final=[BowserAppContext+GUI]::new();$final.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($final)
  if(![BowserAppContext]::GetGUIThreadInfo($tid,[ref]$final) -or $final.hwndActive -ne $h -or [BowserAppContext]::GetForegroundWindow() -ne $h){throw 'Exact active/foreground owner lost after completion read; no focus repair.'}
  $disappeared=(![BowserAppContext]::IsWindow($menu.window) -and ($final.flags -band 0x10) -eq 0 -and $final.hwndCapture -eq [IntPtr]::Zero -and $final.hwndActive -eq $h -and [BowserAppContext]::GetForegroundWindow() -eq $h)
  if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserAppContext]::Host($h,$OwnerPid,$tid) -or [BowserAppContext]::GetForegroundWindow() -ne $h){throw 'Exact retained process/foreground owner changed after final native disappearance read.'}
  if($completionWatch.ElapsedMilliseconds -ge 5000){break}
  if($disappeared -and $completed){$observationAccepted=$true;break}
  Start-Sleep -Milliseconds 50
 } while($completionWatch.ElapsedMilliseconds -lt 5000)
 $report.completion_elapsed_ms=$completionWatch.ElapsedMilliseconds;$report.spontaneous_disappearance=$disappeared;$report.actual_completion=$completed;Save-Report
 if(!$observationAccepted){throw 'Reload did not spontaneously dismiss the exact popup/release capture and actually Complete/clean up within the fixed monotonic 5000ms window; unavailable, not acceptance.'}
 $report.status='observed-real-App-owned-popup-spontaneous-reload-disappearance-and-actual-completion';Save-Report
} catch {
 $report.error=$_.Exception.Message
 try{Save-Report}catch{[Console]::Error.WriteLine('Native observation failure report write failed.')}
 if($report.right_input_called -and $guard){$report.failure_cleanup=$guard.FailOnly($h,$tid);try{Save-Report}catch{[Console]::Error.WriteLine('Native observation cleanup report write failed.')}}
 [Console]::Error.WriteLine('Real App native observation failed; no selection or compatibility acceptance.');exit 75
} finally {if($browserGuard){$browserGuard.Dispose()};if($guard){$guard.Dispose()}}
Write-Output "APP_CONTEXT_MENU_RELOAD right_click_only=true spontaneous_disappearance_and_capture_release=true actual_complete=true no_post_click_input=true"
