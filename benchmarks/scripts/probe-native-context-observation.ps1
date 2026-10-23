# Observation/cancellation only. Never selects or recreates an extension menu item.
[CmdletBinding()]
param([Parameter(Mandatory)][uint32]$OwnerPid,[Parameter(Mandatory)][long]$OwnerHandle,[Parameter(Mandatory)][string]$OwnerExe,[Parameter(Mandatory)][uint32]$BrowserPid,
 [Parameter(Mandatory)][int]$ClientX,[Parameter(Mandatory)][int]$ClientY,[Parameter(Mandatory)][int]$ViewportWidth,[Parameter(Mandatory)][int]$ViewportHeight,
 [Parameter(Mandatory)][string]$Out)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;using System.Collections.Generic;
public static class BowserContextObservation {
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
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
 [DllImport("user32.dll")] public static extern bool IsChild(IntPtr parent,IntPtr child);
 [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint flags);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 [DllImport("user32.dll")] public static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
 [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
 [DllImport("kernel32.dll")] static extern uint GetProcessId(IntPtr process);
 [DllImport("kernel32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool QueryFullProcessImageName(IntPtr process,uint flags,StringBuilder text,ref uint n);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out TIME created,out TIME exited,out TIME kernel,out TIME user);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateProcess(IntPtr process,uint exitCode);
 [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle,uint ms);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 public static string Class(IntPtr h){var b=new StringBuilder(64);GetClassName(h,b,64);return b.ToString();}
 static bool HostBase(IntPtr h,uint pid,uint tid){uint p;return IsWindow(h)&&GetWindowThreadProcessId(h,out p)==tid&&p==pid&&Class(h)=="Static";}
 public static bool Host(IntPtr h,uint pid,uint tid){
  if(!HostBase(h,pid,tid))return false;
  var text=new StringBuilder(512);UIntPtr copied;const string expected="bowser-extension-probe";
  var result=SendMessageTimeout(h,0x000D,(UIntPtr)512,text,0x23,250,out copied);
  return result!=IntPtr.Zero&&copied.ToUInt64()==(ulong)expected.Length&&text.ToString()==expected&&HostBase(h,pid,tid);
 }
 public static bool Input(IntPtr h,uint pid,uint tid,int x,int y,int w,int height,POINT screen){
  RECT r;POINT now=new POINT{X=x,Y=y};if(!Host(h,pid,tid)||!IsWindowVisible(h)||!IsWindowEnabled(h)||GetForegroundWindow()!=h||!GetClientRect(h,out r)||r.Right-r.Left!=w||r.Bottom-r.Top!=height||x<0||y<0||x>=w||y>=height||!ClientToScreen(h,ref now)||now.X!=screen.X||now.Y!=screen.Y)return false;
  var hit=WindowFromPoint(screen);return hit==h||(hit!=IntPtr.Zero&&IsChild(h,hit)&&GetAncestor(hit,2)==h);
 }
 public static IntPtr[] Popups(uint pid,uint tid,out bool complete){var list=new List<IntPtr>();int count=0;bool bounded=true;bool ok=EnumWindows((h,p)=>{if(++count>128){bounded=false;return false;}uint actual;uint thread=GetWindowThreadProcessId(h,out actual);if(actual==pid&&thread==tid&&Class(h)=="#32768"&&IsWindowVisible(h))list.Add(h);return true;},IntPtr.Zero);complete=bounded&&ok;return list.ToArray();}
 public static bool EscapeReady(IntPtr host,uint pid,uint tid,IntPtr popup,IntPtr menu){
  uint actual;var gui=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};var bar=new MENU{cbSize=(uint)Marshal.SizeOf(typeof(MENU))};
  return Host(host,pid,tid)&&IsWindow(popup)&&IsWindowVisible(popup)&&GetWindowThreadProcessId(popup,out actual)==tid&&actual==pid&&Class(popup)=="#32768"&&GetMenuBarInfo(popup,-4,0,ref bar)&&bar.hMenu==menu&&GetGUIThreadInfo(tid,ref gui)&&gui.hwndActive==host&&gui.hwndMenuOwner==host&&(gui.flags&0x10)!=0&&GetForegroundWindow()==host;
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
$report=[ordered]@{scope='single-test-observation-and-cancellation-not-selection';status='failed';right_input_called=$false;escape_called=$false;error=$null;failure_cleanup=$null}
$observationClock=[Diagnostics.Stopwatch]::StartNew();$report.topology=@()
$h=[IntPtr]$OwnerHandle;$pidNow=[uint32]0;$tid=[BowserContextObservation]::GetWindowThreadProcessId($h,[ref]$pidNow);$guard=$null;$browserGuard=$null
function Save-Report {[IO.File]::WriteAllText((Join-Path $Out 'native-context-observation.json'),($report|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))}
function Observe-Topology([string]$Phase) {
 $observation=[ordered]@{phase=$Phase;elapsed_ms=$observationClock.ElapsedMilliseconds;identity_before=$false;identity_after=$false;status='unavailable';native=$null;screenshot_status='not-attempted'}
 try {
  if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserContextObservation]::Host($h,$OwnerPid,$tid)){throw 'diagnostic-identity-unavailable'}
  $observation.identity_before=$true
  $observation.native=[BowserContextObservation]::Topology($h,$OwnerPid,$BrowserPid)
  $observation.identity_after=($guard.SameAlive() -and $browserGuard.SameAlive() -and [BowserContextObservation]::Host($h,$OwnerPid,$tid))
  $observation.status=if($observation.identity_after){'captured-read-only-not-input-authority'}else{'identity-unavailable-after-read'}
 } catch {$observation.status='read-only-topology-unavailable'}
 $report.topology+=,$observation;Save-Report
 # Native screenshot is diagnostic only; it does not identify or authorize a menu. No desktop capture.
 try {
  if(!$observation.identity_after -or !$guard.SameAlive() -or ![BowserContextObservation]::Host($h,$OwnerPid,$tid)){throw 'diagnostic-capture-identity-unavailable'}
  $rect=[BowserContextObservation+RECT]::new()
  if(![BowserContextObservation]::GetWindowRect($h,[ref]$rect)){throw 'diagnostic-capture-bounds-unavailable'}
  $width=$rect.Right-$rect.Left;$height=$rect.Bottom-$rect.Top
  if($width -le 0 -or $height -le 0 -or $width -gt 4096 -or $height -gt 4096){throw 'diagnostic-capture-bounds-outside-limit'}
  $bitmap=[Drawing.Bitmap]::new($width,$height)
  try {
   $graphics=[Drawing.Graphics]::FromImage($bitmap)
   try {
    $dc=$graphics.GetHdc()
    try {$ok=[BowserContextObservation]::PrintWindow($h,$dc,2)} finally {$graphics.ReleaseHdc($dc)}
   } finally {$graphics.Dispose()}
   if(!$ok -or !$guard.SameAlive() -or ![BowserContextObservation]::Host($h,$OwnerPid,$tid)){throw 'diagnostic-capture-unconfirmed'}
   $name="native-context-$Phase.png";$bitmap.Save((Join-Path $Out $name),[Drawing.Imaging.ImageFormat]::Png)
   $observation.screenshot_status='captured-owned-host-only-not-independent-menu-proof';$observation.screenshot=$name
  } finally {$bitmap.Dispose()}
 } catch {$observation.screenshot_status='owned-host-capture-unavailable'}
 Save-Report
}
function Neutral {foreach($key in @(1,2,16,17,18,27,91,92)){if([BowserContextObservation]::GetAsyncKeyState($key) -lt 0){throw 'Unexpected held modifier/Escape; no repair or input.'}}}
function Menu-State {
 param([switch]$AllowPending)
 if(!$guard.SameAlive()){throw 'Exact original process identity changed.'}
 if(![BowserContextObservation]::Host($h,$OwnerPid,$tid) -or [BowserContextObservation]::GetForegroundWindow() -ne $h){throw 'Exact menu foreground/host unavailable or changed.'}
 $gui=[BowserContextObservation+GUI]::new();$gui.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($gui)
 if(![BowserContextObservation]::GetGUIThreadInfo($tid,[ref]$gui) -or $gui.hwndActive -ne $h){throw 'Exact owning STA active state unavailable.'}
 if(($gui.flags -band 0x10) -eq 0){if($AllowPending){$report.readiness.last_predicate='popup-mode-not-present';return $null};throw 'Exact owning STA native popup state unavailable.'}
 if($gui.hwndMenuOwner -ne $h){throw 'Native popup owner changed.'}
 $complete=$false;$popups=@([BowserContextObservation]::Popups($OwnerPid,$tid,[ref]$complete))
 if(!$complete -or $popups.Count -gt 1){throw 'Visible native popup inventory incomplete or not unique.'}
 if($popups.Count -eq 0){if($AllowPending){$report.readiness.last_predicate='popup-window-not-present';return $null};throw 'Visible native popup unavailable.'}
 $popup=$popups[0];$bar=[BowserContextObservation+MENU]::new();$bar.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($bar)
 $rect=[BowserContextObservation+RECT]::new()
 if(![BowserContextObservation]::IsWindowEnabled($popup) -or ![BowserContextObservation]::GetWindowRect($popup,[ref]$rect) -or $rect.Right -le $rect.Left -or $rect.Bottom -le $rect.Top -or ![BowserContextObservation]::GetMenuBarInfo($popup,-4,0,[ref]$bar) -or $bar.hMenu -eq [IntPtr]::Zero){throw 'Exact visible native menu geometry/handle unavailable.'}
 $count=[BowserContextObservation]::GetMenuItemCount($bar.hMenu);if($count -lt 1 -or $count -gt 64){throw 'Native menu item count unavailable or outside bound.'}
 return @{window=$popup;menu=$bar.hMenu;count=$count}
}
try {
 if($pidNow -ne $OwnerPid -or ![BowserContextObservation]::Host($h,$OwnerPid,$tid)){throw 'Exact disposable host unavailable; no input.'}
 $guard=[BowserContextObservation+OriginalProcess]::new($OwnerPid,$OwnerExe)
 $browserGuard=[BowserContextObservation+ReadOnlyBrowser]::new($BrowserPid)
 Observe-Topology 'before-input'
 $point=[BowserContextObservation+POINT]::new();$point.X=$ClientX;$point.Y=$ClientY
 if(![BowserContextObservation]::ClientToScreen($h,[ref]$point)){throw 'Exact controlled point conversion failed.'}
 Neutral
 if(![BowserContextObservation]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point)){throw 'Exact owned input target unavailable before cursor.'}
 if(![BowserContextObservation]::SetCursorPos($point.X,$point.Y)){throw 'Native cursor placement failed.'}
 Neutral
 if(![BowserContextObservation]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point)){throw 'Exact owned hit target changed before real right-click.'}
 try{[BowserContextObservation]::mouse_event(0x0008,0,0,0,[UIntPtr]::Zero);$report.right_input_called=$true}finally{[BowserContextObservation]::mouse_event(0x0010,0,0,0,[UIntPtr]::Zero)}
 $watch=[Diagnostics.Stopwatch]::StartNew();Observe-Topology 'after-right-input';$report.readiness=@{elapsed_ms=0;last_predicate='not-observed';polls=0};$menu=$null
 do {
  $report.readiness.polls++;$menu=Menu-State -AllowPending;$report.readiness.elapsed_ms=$watch.ElapsedMilliseconds
  if($watch.ElapsedMilliseconds -ge 10000){Observe-Topology 'readiness-expired';throw 'Native popup readiness window expired; topology unavailable.'}
  if($menu){$report.readiness.last_predicate='strict-native-menu-ready';break}
  Start-Sleep -Milliseconds 50
 } while($true)
 $watch.Stop();$report.popup=@{window=$menu.window.ToInt64();menu=$menu.menu.ToInt64();count=$menu.count;owner_thread=$tid;owner_pid=$OwnerPid}
 Neutral;$fresh=Menu-State
 if($fresh.window -ne $menu.window -or $fresh.menu -ne $menu.menu -or $fresh.count -ne $menu.count){throw 'Captured native popup changed before Escape.'}
 Neutral
 if(![BowserContextObservation]::EscapeReady($h,$OwnerPid,$tid,$menu.window,$menu.menu)){throw 'Exact native popup/foreground changed immediately before Escape.'}
 try{[BowserContextObservation]::keybd_event(27,0,0,[UIntPtr]::Zero);$report.escape_called=$true}finally{[BowserContextObservation]::keybd_event(27,0,2,[UIntPtr]::Zero)}
 $deadline=[DateTime]::UtcNow.AddSeconds(5)
 while([BowserContextObservation]::IsWindow($menu.window) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 50}
 $after=[BowserContextObservation+GUI]::new();$after.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($after)
 if([BowserContextObservation]::IsWindow($menu.window) -or ![BowserContextObservation]::GetGUIThreadInfo($tid,[ref]$after) -or ($after.flags -band 0x10) -ne 0 -or ![BowserContextObservation]::Host($h,$OwnerPid,$tid)){throw 'Native popup disappearance and cleared native popup mode not both observed.'}
 $report.status='observed-owned-popup-and-real-cancellation';Save-Report
} catch {
 $report.error=$_.Exception.Message
 # Failure-only observation; no input/clipboard/process authority is derived from this snapshot.
 try {
  Import-Module (Join-Path $PSScriptRoot 'native-runner-state.psm1') -ErrorAction Stop
  $report.runner_state=Get-BowserNativeRunnerState -Phase failure
 } catch { $report.runner_state=@{status='unavailable';input_authority=$false;clipboard_authority=$false} }
 try{Save-Report}catch{[Console]::Error.WriteLine('Native observation failure report write failed.')}
 if($report.right_input_called -and $guard){$report.failure_cleanup=$guard.FailOnly($h,$tid);try{Save-Report}catch{[Console]::Error.WriteLine('Native observation cleanup report write failed.')}}
 [Console]::Error.WriteLine('Native context observation failed; no selection compatibility verdict.');exit 75
} finally {if($browserGuard){$browserGuard.Dispose()};if($guard){$guard.Dispose()}}
Write-Output 'NATIVE_CONTEXT_OBSERVATION exact_owned_right_input=true exact_visible_native_popup=true real_escape_and_disappearance=true selection_performed=false'
