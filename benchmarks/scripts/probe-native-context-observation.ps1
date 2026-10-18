# Observation/cancellation only. Never selects or recreates an extension menu item.
[CmdletBinding()]
param([Parameter(Mandatory)][uint32]$OwnerPid,[Parameter(Mandatory)][long]$OwnerHandle,[Parameter(Mandatory)][string]$OwnerExe,
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
New-Item -ItemType Directory -Force $Out|Out-Null
$report=[ordered]@{scope='single-test-observation-and-cancellation-not-selection';status='failed';right_input_called=$false;escape_called=$false;error=$null;failure_cleanup=$null}
$h=[IntPtr]$OwnerHandle;$pidNow=[uint32]0;$tid=[BowserContextObservation]::GetWindowThreadProcessId($h,[ref]$pidNow);$guard=$null
function Save-Report {[IO.File]::WriteAllText((Join-Path $Out 'native-context-observation.json'),($report|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))}
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
 $point=[BowserContextObservation+POINT]::new();$point.X=$ClientX;$point.Y=$ClientY
 if(![BowserContextObservation]::ClientToScreen($h,[ref]$point)){throw 'Exact controlled point conversion failed.'}
 Neutral
 if(![BowserContextObservation]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point)){throw 'Exact owned input target unavailable before cursor.'}
 if(![BowserContextObservation]::SetCursorPos($point.X,$point.Y)){throw 'Native cursor placement failed.'}
 Neutral
 if(![BowserContextObservation]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point)){throw 'Exact owned hit target changed before real right-click.'}
 try{[BowserContextObservation]::mouse_event(0x0008,0,0,0,[UIntPtr]::Zero);$report.right_input_called=$true}finally{[BowserContextObservation]::mouse_event(0x0010,0,0,0,[UIntPtr]::Zero)}
 $watch=[Diagnostics.Stopwatch]::StartNew();$report.readiness=@{elapsed_ms=0;last_predicate='not-observed';polls=0};$menu=$null
 do {
  $report.readiness.polls++;$menu=Menu-State -AllowPending;$report.readiness.elapsed_ms=$watch.ElapsedMilliseconds
  if($watch.ElapsedMilliseconds -ge 10000){throw 'Native popup readiness window expired; topology unavailable.'}
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
 try{Save-Report}catch{[Console]::Error.WriteLine('Native observation failure report write failed.')}
 if($report.right_input_called -and $guard){$report.failure_cleanup=$guard.FailOnly($h,$tid);try{Save-Report}catch{[Console]::Error.WriteLine('Native observation cleanup report write failed.')}}
 [Console]::Error.WriteLine('Native context observation failed; no selection compatibility verdict.');exit 75
} finally {if($guard){$guard.Dispose()}}
Write-Output 'NATIVE_CONTEXT_OBSERVATION exact_owned_right_input=true exact_visible_native_popup=true real_escape_and_disappearance=true selection_performed=false'
