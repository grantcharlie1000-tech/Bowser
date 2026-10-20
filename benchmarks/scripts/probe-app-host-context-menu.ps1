# Separate actual App host/native regression helper. No accessibility Invoke, WM_COMMAND,
# manually invoked handler, synthesized extension API, guessed ID, or label-based authority.
# A retained external clipboard guard survives termination of the Rust test. It reads no prior
# contents: non-empty or locked clipboard is a precondition failure before any mutation.
[CmdletBinding()]
param([Parameter(Mandatory)][uint32]$OwnerPid,[Parameter(Mandatory)][long]$OwnerHandle,[Parameter(Mandatory)][string]$OwnerExe,[Parameter(Mandatory)][uint32]$BrowserPid,
 [Parameter(Mandatory)][int]$ClientX,[Parameter(Mandatory)][int]$ClientY,[Parameter(Mandatory)][int]$ViewportWidth,[Parameter(Mandatory)][int]$ViewportHeight,
 [Parameter(Mandatory)][int]$ControlLeft,[Parameter(Mandatory)][int]$ControlTop,[Parameter(Mandatory)][int]$ControlRight,[Parameter(Mandatory)][int]$ControlBottom,
 [Parameter(Mandatory)][string]$RequestReceipt,[Parameter(Mandatory)][string]$CompletedReceipt,[Parameter(Mandatory)][string]$ProbeNonce,[Parameter(Mandatory)][string]$SourceViewIdentity,
 [Parameter(Mandatory)][string]$Proof0,[Parameter(Mandatory)][string]$Proof1,[Parameter(Mandatory)][string]$Proof2,
 [Parameter(Mandatory)][ValidateSet('Cancel','Copy','Paste','Note','Split')][string]$Mode,
 [Parameter(Mandatory)][string]$SyntheticToken,[Parameter(Mandatory)][string]$Out)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;using System.Collections.Generic;
public static class BowserHostMenu {
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
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint thread,ref GUI info);
 [DllImport("user32.dll")] public static extern bool GetMenuBarInfo(IntPtr h,int obj,int item,ref MENU info);
 [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr menu);
 [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr window,IntPtr menu,uint index,out RECT bounds);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
 [DllImport("user32.dll")] public static extern bool IsChild(IntPtr parent,IntPtr child);
 [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint flags);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
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
 public static bool EscapeReady(IntPtr host,uint pid,uint tid,IntPtr popup,IntPtr menu,OriginalProcess guard){
  uint actual;var gui=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};var bar=new MENU{cbSize=(uint)Marshal.SizeOf(typeof(MENU))};
  if(!guard.SameAlive()||!Host(host,pid,tid)||!IsWindow(popup)||!IsWindowVisible(popup)||GetWindowThreadProcessId(popup,out actual)!=tid||actual!=pid||Class(popup)!="#32768"||!GetMenuBarInfo(popup,-4,0,ref bar)||bar.hMenu!=menu||!GetGUIThreadInfo(tid,ref gui)||gui.hwndActive!=host||gui.hwndMenuOwner!=host||(gui.flags&0x10)==0)return false;
  bar=new MENU{cbSize=(uint)Marshal.SizeOf(typeof(MENU))};
  return Host(host,pid,tid)&&IsWindow(popup)&&IsWindowVisible(popup)&&GetWindowThreadProcessId(popup,out actual)==tid&&actual==pid&&Class(popup)=="#32768"&&GetMenuBarInfo(popup,-4,0,ref bar)&&bar.hMenu==menu&&HostBase(host,pid,tid)&&GetForegroundWindow()==host&&guard.SameAlive();
 }
 [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
 public sealed class ReadOnlyBrowser:IDisposable {
  IntPtr handle;uint pid;ulong created;string image;
  public ReadOnlyBrowser(uint expected){handle=OpenProcess(0x00101000,false,expected);if(handle==IntPtr.Zero)throw new InvalidOperationException("Native browser read-only handle unavailable.");try{pid=expected;image=Image();created=Creation();if(!String.Equals(System.IO.Path.GetFileName(image),"msedgewebview2.exe",StringComparison.OrdinalIgnoreCase)||!SameAlive())throw new InvalidOperationException("Native browser read-only identity unavailable.");}catch{Dispose();throw;}}
  string Image(){var b=new StringBuilder(32768);uint n=32768;if(!QueryFullProcessImageName(handle,0,b,ref n))throw new InvalidOperationException("Native browser image query unavailable.");return System.IO.Path.GetFullPath(b.ToString());}
  ulong Creation(){TIME a,b,c,d;if(!GetProcessTimes(handle,out a,out b,out c,out d))throw new InvalidOperationException("Native browser creation query unavailable.");return a.Bits;}
  public bool SameAlive(){return handle!=IntPtr.Zero&&GetProcessId(handle)==pid&&Creation()==created&&String.Equals(Image(),image,StringComparison.OrdinalIgnoreCase)&&WaitForSingleObject(handle,0)==258;}
  public void Dispose(){if(handle!=IntPtr.Zero){CloseHandle(handle);handle=IntPtr.Zero;}}
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
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct ITEM {public uint cbSize,fMask,fType,fState,wID;public IntPtr hSubMenu,hbmpChecked,hbmpUnchecked,dwItemData,dwTypeData;public uint cch;public IntPtr hbmpItem;}
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern bool GetMenuItemInfo(IntPtr menu,uint index,bool position,ref ITEM info);
 [DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr menu,int index);
 public sealed class NativeItem {public uint Type,State,Id;public IntPtr Child;public string Fingerprint;public int Units;}
 [DllImport("user32.dll")] static extern IntPtr CreatePopupMenu();
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern bool AppendMenu(IntPtr menu,uint flags,UIntPtr id,string text);
 [DllImport("user32.dll")] static extern bool DestroyMenu(IntPtr menu);
 public static void CheckSeparatorProjection(){
  IntPtr menu=CreatePopupMenu();if(menu==IntPtr.Zero)throw new InvalidOperationException("Generated separator menu creation failed.");
  try{
   if(!AppendMenu(menu,0x800,UIntPtr.Zero,null))throw new InvalidOperationException("Generated separator append failed.");
   NativeItem item=Item(menu,0);
   if(GetMenuItemCount(menu)!=1||item.Type!=0x800||item.State!=3||item.Units!=0||item.Child!=IntPtr.Zero||item.Fingerprint!="e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")throw new InvalidOperationException("Generated native separator projection contract failed.");
  }finally{if(!DestroyMenu(menu))throw new InvalidOperationException("Generated separator menu destruction failed.");}
 }

 public static void CheckGrayedProjection(){
  IntPtr menu=CreatePopupMenu();if(menu==IntPtr.Zero)throw new InvalidOperationException("Generated grayed menu creation failed.");
  try{
   uint[] flags={0,1,9};
   for(int i=0;i<flags.Length;i++)if(!AppendMenu(menu,flags[i],new UIntPtr((uint)(101+i)),"Bowser generated projection"))throw new InvalidOperationException("Generated grayed control append failed.");
   if(GetMenuItemCount(menu)!=3)throw new InvalidOperationException("Generated grayed menu count mismatch.");
   for(int i=0;i<flags.Length;i++){
    NativeItem item=Item(menu,i);
    if(item.Type!=0||item.State!=flags[i]||item.Id!=(uint)(101+i)||item.Units!=27||item.Child!=IntPtr.Zero||item.Fingerprint!="80c5a92a945d70f897287729703ed1e1854110020d7482f0e3b9266f0ac23a23")throw new InvalidOperationException("Generated exact enabled/grayed/checked projection contract failed.");
   }
  }finally{if(!DestroyMenu(menu))throw new InvalidOperationException("Generated grayed menu destruction failed.");}
 }

 public static NativeItem Item(IntPtr menu,int index){
  IntPtr buffer=Marshal.AllocHGlobal(2052);try{
   for(int i=0;i<1026;i++)Marshal.WriteInt16(buffer,i*2,0);
   var item=new ITEM {cbSize=(uint)Marshal.SizeOf(typeof(ITEM)),fMask=0x1c7,dwTypeData=buffer,cch=1026};
   if(!GetMenuItemInfo(menu,(uint)index,true,ref item)||item.cch>1025||item.hbmpItem!=IntPtr.Zero)throw new InvalidOperationException("Complete native item presentation unavailable.");
   byte[] bytes=new byte[(int)item.cch*2];Marshal.Copy(buffer,bytes,0,bytes.Length);string fingerprint;
   using(var sha=System.Security.Cryptography.SHA256.Create()){fingerprint=BitConverter.ToString(sha.ComputeHash(bytes)).Replace("-","").ToLowerInvariant();}
   return new NativeItem {Type=item.fType,State=item.fState,Id=item.wID,Child=item.hSubMenu,Fingerprint=fingerprint,Units=(int)item.cch};
  }finally{Marshal.FreeHGlobal(buffer);}
 }
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr CreateWindowEx(uint ex,string cls,string title,uint style,int x,int y,int w,int h,IntPtr parent,IntPtr menu,IntPtr instance,IntPtr parameter);
 [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr h);
 [DllImport("user32.dll")] static extern bool OpenClipboard(IntPtr owner);
 [DllImport("user32.dll")] static extern bool CloseClipboard();
 [DllImport("user32.dll")] static extern bool EmptyClipboard();
 [DllImport("user32.dll",SetLastError=true)] static extern uint EnumClipboardFormats(uint previous);
 [DllImport("kernel32.dll")] static extern void SetLastError(uint error);
 [DllImport("user32.dll")] static extern uint GetClipboardSequenceNumber();
 [DllImport("user32.dll")] static extern IntPtr GetClipboardOwner();
 [DllImport("user32.dll")] static extern IntPtr SetClipboardData(uint format,IntPtr data);
 [DllImport("user32.dll")] static extern IntPtr GetClipboardData(uint format);
 [DllImport("kernel32.dll")] static extern IntPtr GlobalAlloc(uint flags,UIntPtr bytes);
 [DllImport("kernel32.dll")] static extern IntPtr GlobalLock(IntPtr memory);
 [DllImport("kernel32.dll")] static extern bool GlobalUnlock(IntPtr memory);
 [DllImport("kernel32.dll")] static extern IntPtr GlobalFree(IntPtr memory);
 [DllImport("kernel32.dll")] static extern UIntPtr GlobalSize(IntPtr memory);
 // Failure-only window-station diagnostics. These do not inspect clipboard contents,
 // repair attachment/access, or establish sequence authority. Borrowed handles are not closed.
 [StructLayout(LayoutKind.Sequential)] struct OBJECTFLAGS {public int Inherit,Reserved;public uint Flags;}
 // Documented generic kernel-object query, optional on this platform. No handle is duplicated,
 // closed, switched or granted access. A successful mask observation is NOT sequence authority.
 [StructLayout(LayoutKind.Sequential)] struct OBJECTBASIC {
  public uint Attributes,GrantedAccess,HandleCount,PointerCount;
  public uint Reserved0,Reserved1,Reserved2,Reserved3,Reserved4,Reserved5,Reserved6,Reserved7,Reserved8,Reserved9;
 }
 [DllImport("ntdll.dll",EntryPoint="NtQueryObject",ExactSpelling=true)] static extern int QueryObjectBasic(IntPtr handle,int informationClass,out OBJECTBASIC data,uint bytes,out uint returned);
 static bool ObjectBasicLayoutValid(){return Marshal.SizeOf(typeof(OBJECTBASIC))==56&&Marshal.OffsetOf(typeof(OBJECTBASIC),"GrantedAccess").ToInt64()==4;}
 public sealed class StationAccessDiagnostic {
  public string scope="failure-only-attached-handle-mask-not-sequence-or-input-authority",status="unavailable",reason="not-attempted";
  public bool layout_valid,query_attempted,handle_before_matches,handle_after_matches;
  public int? ntstatus,before_error,after_error;public uint bytes_returned;public uint? granted_access;public bool? accessclipboard_bit;
 }
 static StationAccessDiagnostic AttachedStationAccess(IntPtr expected){
  var result=new StationAccessDiagnostic();
  try{
   result.layout_valid=ObjectBasicLayoutValid();if(!result.layout_valid){result.reason="public-layout-mismatch";return result;}
   if(expected==IntPtr.Zero){result.reason="attached-station-unavailable";return result;}
   IntPtr before=GetProcessWindowStation();result.before_error=before==IntPtr.Zero?(int?)Marshal.GetLastWin32Error():0;
   result.handle_before_matches=before==expected;if(!result.handle_before_matches){result.reason="attached-handle-changed-before-query";return result;}
   OBJECTBASIC data;uint returned;result.query_attempted=true;
   int status=QueryObjectBasic(expected,0,out data,56,out returned);result.ntstatus=status;result.bytes_returned=returned;
   IntPtr after=GetProcessWindowStation();result.after_error=after==IntPtr.Zero?(int?)Marshal.GetLastWin32Error():0;
   result.handle_after_matches=after!=IntPtr.Zero&&after==expected;
   if(!result.handle_after_matches){result.reason="attached-handle-changed-after-query";return result;}
   if(status!=0){result.reason="native-status-unavailable";return result;}
   if(returned!=56){result.reason="public-returned-size-mismatch";return result;}
   result.granted_access=data.GrantedAccess;result.accessclipboard_bit=(data.GrantedAccess&0x0004)!=0;
   result.status="captured-nonatomic-not-authority";result.reason=null;
  }catch(EntryPointNotFoundException){result.reason="native-entry-point-unavailable";}
   catch(DllNotFoundException){result.reason="native-library-unavailable";}
   catch{result.reason="interop-or-layout-unavailable";}
  return result;
 }
 [DllImport("user32.dll",SetLastError=true)] static extern IntPtr GetProcessWindowStation();
 [DllImport("user32.dll",SetLastError=true)] static extern IntPtr GetThreadDesktop(uint thread);
 [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
 [DllImport("kernel32.dll",EntryPoint="GetCurrentProcessId")] static extern uint DiagnosticProcessId();
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool ProcessIdToSessionId(uint pid,out uint session);
 [DllImport("user32.dll",EntryPoint="GetUserObjectInformationW",SetLastError=true)] static extern bool UserObjectName(IntPtr handle,int index,IntPtr data,uint bytes,out uint needed);
 [DllImport("user32.dll",EntryPoint="GetUserObjectInformationW",SetLastError=true)] static extern bool UserObjectFlags(IntPtr handle,int index,out OBJECTFLAGS data,uint bytes,out uint needed);
 [DllImport("user32.dll",EntryPoint="GetUserObjectInformationW",SetLastError=true)] static extern bool UserObjectInput(IntPtr handle,int index,out int data,uint bytes,out uint needed);
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr OpenWindowStation(string name,bool inherit,uint access);
 [DllImport("user32.dll",SetLastError=true)] static extern bool CloseWindowStation(IntPtr station);
 static string BoundedObjectName(IntPtr handle,out int error){
  error=0;if(handle==IntPtr.Zero)return null;
  const uint cap=512;IntPtr data=Marshal.AllocHGlobal((int)cap);
  try{
   uint needed;bool ok=UserObjectName(handle,2,data,cap,out needed);int callError=ok?0:Marshal.GetLastWin32Error();
   if(!ok){error=callError;return null;}
   // nLength and needed are BYTES. Require a whole bounded, explicitly terminated UTF-16
   // string and reject embedded NULs before passing the exact name to OpenWindowStation.
   if(needed<2||needed>cap||(needed&1)!=0)return null;
   int units=(int)needed/2;if(Marshal.ReadInt16(data,(units-1)*2)!=0)return null;
   for(int index=0;index<units-1;index++)if(Marshal.ReadInt16(data,index*2)==0)return null;
   return Marshal.PtrToStringUni(data,units-1);
  }finally{Marshal.FreeHGlobal(data);}
 }
 static object DiagnosticSession(uint pid){
  uint session;bool ok=ProcessIdToSessionId(pid,out session);int error=ok?0:Marshal.GetLastWin32Error();
  return new {pid=pid,query_ok=ok,error=error,session_id=ok?(uint?)session:null};
 }
 public static object ClipboardEnvironment(uint testPid,uint browserPid){
  IntPtr station=GetProcessWindowStation();int stationError=station==IntPtr.Zero?Marshal.GetLastWin32Error():0;
  IntPtr desktop=GetThreadDesktop(GetCurrentThreadId());int desktopError=desktop==IntPtr.Zero?Marshal.GetLastWin32Error():0;
  int stationNameError,desktopNameError;string stationName=BoundedObjectName(station,out stationNameError),desktopName=BoundedObjectName(desktop,out desktopNameError);
  OBJECTFLAGS flags=new OBJECTFLAGS();uint flagsNeeded=0;
  bool flagsOk=station!=IntPtr.Zero&&UserObjectFlags(station,1,out flags,12,out flagsNeeded);int flagsError=flagsOk?0:station==IntPtr.Zero?stationError:Marshal.GetLastWin32Error();
  bool flagsNativeOk=flagsOk;flagsOk=flagsOk&&flagsNeeded==12;
  int input=0;uint inputNeeded=0;
  bool inputOk=desktop!=IntPtr.Zero&&UserObjectInput(desktop,6,out input,4,out inputNeeded);int inputError=inputOk?0:desktop==IntPtr.Zero?desktopError:Marshal.GetLastWin32Error();
  bool inputNativeOk=inputOk;inputOk=inputOk&&inputNeeded==4;
  bool accessAttempted=false,accessOpened=false,accessCloseAttempted=false,accessClosed=false;int accessError=0,accessCloseError=0;
  if(stationName!=null&&stationName.Length>0){
   // A new non-inheritable, non-mutating access probe only. Never change the process station, DACL,
   // privileges, desktop, or the borrowed attachment handle. Success is NOT sequence authority.
   accessAttempted=true;IntPtr probe=OpenWindowStation(stationName,false,0x0004);
   accessOpened=probe!=IntPtr.Zero;accessError=accessOpened?0:Marshal.GetLastWin32Error();
   if(accessOpened){try{}finally{accessCloseAttempted=true;accessClosed=CloseWindowStation(probe);accessCloseError=accessClosed?0:Marshal.GetLastWin32Error();}}
  }
  return new {scope="failure-only-read-only-not-clipboard-or-input-authority",sequence_zero_still_refused=true,
   attached_station_access=AttachedStationAccess(station),
   station_handle_available=station!=IntPtr.Zero,station_error=stationError,station_name_available=stationName!=null,station_name=stationName,station_name_error=stationNameError,
   station_flags_query_attempted=station!=IntPtr.Zero,station_flags_native_ok=flagsNativeOk,station_flags_available=flagsOk,station_flags_error=flagsError,station_flags_bytes=flagsNeeded,station_flags_size_valid=flagsNeeded==12,station_flags=flagsOk?(uint?)flags.Flags:null,station_flags_are_not_granted_access=true,
   desktop_handle_available=desktop!=IntPtr.Zero,desktop_error=desktopError,desktop_name_available=desktopName!=null,desktop_name=desktopName,desktop_name_error=desktopNameError,
   desktop_input_query_attempted=desktop!=IntPtr.Zero,desktop_input_native_ok=inputNativeOk,desktop_input_query_ok=inputOk,desktop_input_query_error=inputError,desktop_input_bytes=inputNeeded,desktop_input_size_valid=inputNeeded==4,desktop_receives_input=inputOk?(bool?)(input!=0):null,
   clipboard_right_probe_attempted=accessAttempted,clipboard_right_requested=4,clipboard_right_opened=accessOpened,clipboard_right_open_error=accessError,
   clipboard_right_close_attempted=accessCloseAttempted,clipboard_right_closed=accessClosed,clipboard_right_close_error=accessCloseError,
   helper_session=DiagnosticSession(DiagnosticProcessId()),test_session=DiagnosticSession(testPid),browser_session=DiagnosticSession(browserPid)};
 }
 /// External to Rust: stores only generated text and clipboard sequence metadata. No backup
 /// claim, no prior content getter, no clipboard logger, no retry on lock/race, no forced clear.
 public sealed class ClipboardGuard:IDisposable {
  // Fixed scalars from existing calls only; never clipboard contents or admission authority.
  public static int AdmissionStage,AdmissionFailureStage,AdmissionEnumError;
  public static bool AdmissionStarted,AdmissionOpened,AdmissionFormatObserved,AdmissionFirstFormatZero,AdmissionSequenceObserved,AdmissionCloseAttempted,AdmissionCloseConfirmed;
  public static uint AdmissionSequence;
  bool admission=true;
  IntPtr owner;uint last;bool changed,finished,uncertain;public string Status="not-started";public List<uint> OwnedSequences=new List<uint>();
  uint Sequence(){uint value=GetClipboardSequenceNumber();if(admission&&AdmissionStage==4){AdmissionSequence=value;AdmissionSequenceObserved=true;}if(value==0){uncertain=true;Status="sequence-unavailable-left-untouched";throw new InvalidOperationException("Clipboard sequence unavailable; no authority to read or restore.");}return value;}
  void CloseChecked(){bool closed=CloseClipboard();if(admission){AdmissionCloseAttempted=true;AdmissionCloseConfirmed=closed;}if(!closed){uncertain=true;Status="close-unconfirmed-left-untouched";throw new InvalidOperationException("Clipboard close was not confirmed.");}}
  public ClipboardGuard(string generated){
   AdmissionStarted=true;AdmissionStage=1;AdmissionFailureStage=0;AdmissionEnumError=0;AdmissionOpened=false;AdmissionFormatObserved=false;AdmissionFirstFormatZero=false;AdmissionSequenceObserved=false;AdmissionSequence=0;AdmissionCloseAttempted=false;AdmissionCloseConfirmed=false;
   if(!System.Text.RegularExpressions.Regex.IsMatch(generated,"^(sentinel|paste)-[0-9]+-[0-9]+$"))throw new InvalidOperationException("Only generated synthetic clipboard payloads are allowed.");
   owner=CreateWindowEx(0,"STATIC","",0,0,0,0,0,new IntPtr(-3),IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
   if(owner==IntPtr.Zero)throw new InvalidOperationException("External clipboard guard window unavailable.");
   IntPtr memory=IntPtr.Zero;bool opened=false;try{
    byte[] bytes=Encoding.Unicode.GetBytes(generated+"\0");memory=GlobalAlloc(2,(UIntPtr)bytes.Length);if(memory==IntPtr.Zero)throw new InvalidOperationException("Synthetic payload allocation failed.");
    var target=GlobalLock(memory);if(target==IntPtr.Zero)throw new InvalidOperationException("Synthetic payload lock failed.");try{Marshal.Copy(bytes,0,target,bytes.Length);}finally{GlobalUnlock(memory);}
    AdmissionStage=2;bool openResult=OpenClipboard(owner);AdmissionOpened=openResult;if(!openResult)throw new InvalidOperationException("Clipboard locked: no clipboard mutation.");opened=true;
    AdmissionStage=3;SetLastError(0);uint first=EnumClipboardFormats(0);int error=Marshal.GetLastWin32Error();
    AdmissionFormatObserved=true;AdmissionFirstFormatZero=first==0;AdmissionEnumError=error;
    if(first!=0||error!=0)throw new InvalidOperationException("Clipboard is nonempty or its empty state is unavailable; no content read or mutation.");
    AdmissionStage=4;last=Sequence();AdmissionStage=5;if(!EmptyClipboard())throw new InvalidOperationException("Empty precondition could not be claimed.");
    changed=true;last=Sequence();OwnedSequences.Add(last);
    if(SetClipboardData(13,memory)==IntPtr.Zero)throw new InvalidOperationException("Synthetic payload write failed.");memory=IntPtr.Zero;
    last=Sequence();OwnedSequences.Add(last);Status="synthetic-payload-owned";
    opened=false;CloseChecked();
   }catch{AdmissionFailureStage=AdmissionStage;if(opened){opened=false;try{CloseChecked();}catch{}}Finish();DestroyWindow(owner);owner=IntPtr.Zero;throw;}
   finally{admission=false;if(memory!=IntPtr.Zero)GlobalFree(memory);}
  }
  public void RequireOwned(){if(!OpenClipboard(owner))throw new InvalidOperationException("Clipboard locked before native input; no input.");try{if(Sequence()!=last||GetClipboardOwner()!=owner)throw new InvalidOperationException("Clipboard sequence changed before native input; no input.");}finally{CloseChecked();}}
  /// The only content getter follows the settled physical Copy. Unknown/zero sequences and
  /// different owners are refused before content access; no unrelated clipboard backup exists.
  public void ObserveNativeCopy(uint browserPid,ReadOnlyBrowser browser,string expected){
   if(!browser.SameAlive()||!OpenClipboard(owner))throw new InvalidOperationException("Native Copy clipboard ownership unavailable.");
   try{
    uint sequence=Sequence();IntPtr source=GetClipboardOwner();uint pid;
    if(sequence==last||source==IntPtr.Zero||!IsWindow(source)||GetWindowThreadProcessId(source,out pid)==0||pid!=browserPid||!browser.SameAlive())throw new InvalidOperationException("Changed clipboard is not owned by the exact disposable native Copy source; contents not read.");
    last=sequence;OwnedSequences.Add(last);
    IntPtr data=GetClipboardData(13);if(data==IntPtr.Zero||GlobalSize(data).ToUInt64()>8192)throw new InvalidOperationException("Known native Copy text format/bound unavailable.");
    // GetClipboardData may synchronously request delayed rendering; fence again before reading.
    if(Sequence()!=last||GetClipboardOwner()!=source||!IsWindow(source)||GetWindowThreadProcessId(source,out pid)==0||pid!=browserPid||!browser.SameAlive())throw new InvalidOperationException("Native Copy ownership changed before content verification.");
    IntPtr p=GlobalLock(data);if(p==IntPtr.Zero)throw new InvalidOperationException("Known native Copy data unavailable.");
    try{var units=new List<char>();bool end=false;ulong max=GlobalSize(data).ToUInt64()/2;for(ulong i=0;i<max&&i<4096;i++){char unit=(char)(ushort)Marshal.ReadInt16(p,(int)(i*2));if(unit=='\0'){end=true;break;}units.Add(unit);}if(!end||new string(units.ToArray())!=expected)throw new InvalidOperationException("Native Copy does not equal the generated fixture text.");}finally{GlobalUnlock(data);}
    if(Sequence()!=last||GetClipboardOwner()!=source||!IsWindow(source)||GetWindowThreadProcessId(source,out pid)==0||pid!=browserPid||!browser.SameAlive())throw new InvalidOperationException("Native Copy ownership changed during exclusive verification.");Status="native-copy-generated-text-observed";
   }finally{CloseChecked();}
  }
  public bool Finish(){
   if(finished)return Status=="restored-empty";finished=true;
   if(uncertain){return false;}
   if(!changed){Status="unchanged-precondition-failed";return true;}
   if(last==0){Status="sequence-unavailable-left-untouched";return false;}
   if(!OpenClipboard(owner)){Status="locked-finally-left-untouched";return false;}
   bool restored=false;
   try{
    uint current=GetClipboardSequenceNumber();
    if(current==0){uncertain=true;Status="sequence-unavailable-left-untouched";}
    else if(current!=last){Status="changed-by-other-actor-left-untouched";}
    else if(!EmptyClipboard()){Status="empty-restoration-failed";}
    else{uint after=GetClipboardSequenceNumber();if(after==0){uncertain=true;Status="restoration-sequence-unavailable";}else{last=after;OwnedSequences.Add(last);restored=true;}}
   }finally{if(!CloseClipboard()){uncertain=true;restored=false;Status="restoration-close-unconfirmed";}}
   if(restored)Status="restored-empty";return restored;
  }
  public void Dispose(){Finish();if(owner!=IntPtr.Zero){DestroyWindow(owner);owner=IntPtr.Zero;}}
 }

}
'@
New-Item -ItemType Directory -Force $Out|Out-Null
$report=[ordered]@{scope='separate-real-App-native-and-host-regression';status='failed';mode=$Mode;separator_projection_contract=$false;grayed_projection_contract=$false;right_input_called=$false;selection_called=$false;selected_command_id=$null;clipboard='not-used';owned_clipboard_sequences=@();error=$null;failure_cleanup=$null}
$h=[IntPtr]$OwnerHandle;$pidNow=[uint32]0;$tid=[BowserHostMenu]::GetWindowThreadProcessId($h,[ref]$pidNow)
$guard=$null;$browserGuard=$null;$clipboard=$null;$script:request=$null;$script:requestText=$null
function Save-Report {[IO.File]::WriteAllText((Join-Path $Out 'app-host-context-menu.json'),($report|ConvertTo-Json -Depth 18),[Text.UTF8Encoding]::new($false))}
function Neutral {foreach($key in @(1,2,16,17,18,27,91,92)){if([BowserHostMenu]::GetAsyncKeyState($key) -lt 0){throw 'Unexpected held modifier/Escape; no repair or input.'}}}
function Request-State([switch]$AllowPending) {
 if(!(Test-Path -LiteralPath $RequestReceipt -PathType Leaf)){if($AllowPending){return $null};throw 'Exact full-tree coordinator witness absent.'}
 $file=Get-Item -LiteralPath $RequestReceipt;if($file.Length -le 0 -or $file.Length -gt 1048576){throw 'Complete coordinator witness outside bound.'}
 $text=[IO.File]::ReadAllText($file.FullName)
 if($script:requestText -and $text -cne $script:requestText){throw 'Exact complete current request witness changed.'}
 $r=$text|ConvertFrom-Json
 if($r.schema -cne 'bowser-real-app-host-menu-v1' -or $r.nonce -cne $ProbeNonce -or $r.phase -cne 'before-show' -or
    [long]$r.owner -ne $OwnerHandle -or [string]$r.view_identity -cne $SourceViewIdentity -or
    [string]$r.proof0 -cne $Proof0 -or [string]$r.proof1 -cne $Proof1 -or [string]$r.proof2 -cne $Proof2 -or
    [string]$r.request -notmatch '^[1-9][0-9]*$' -or [string]$r.args_identity -notmatch '^[1-9][0-9]*$') {throw 'Exact coordinator/source/document identity mismatch.'}
 if(!$script:requestText){$script:requestText=$text;$script:request=$r;$report.request=$r;Save-Report}
 return $r
}
function Source-Control {
 if($ControlLeft -lt 0 -or $ControlTop -lt 0 -or $ControlRight -le $ControlLeft -or $ControlBottom -le $ControlTop -or
    $ControlRight -gt $ViewportWidth -or $ControlBottom -gt $ViewportHeight -or $ClientX -le $ControlLeft -or $ClientX -ge $ControlRight -or $ClientY -le $ControlTop -or $ClientY -ge $ControlBottom){throw 'Exact COM source/control bounds do not contain input point.'}
}
function Host-State {
 if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserHostMenu]::Host($h,$OwnerPid,$tid) -or [BowserHostMenu]::GetForegroundWindow() -ne $h){throw 'Exact original owner/native browser identity or foreground changed.'}
 $gui=[BowserHostMenu+GUI]::new();$gui.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($gui)
 if(![BowserHostMenu]::GetGUIThreadInfo($tid,[ref]$gui) -or $gui.hwndActive -ne $h -or $gui.hwndMenuOwner -ne $h -or ($gui.flags -band 0x10) -eq 0){throw 'Exact owning STA active/menu owner/popup mode changed.'}
}
function Test-Tree([IntPtr]$Menu,$Nodes,[int[]]$Path,[int]$Depth) {
 $count=[BowserHostMenu]::GetMenuItemCount($Menu)
 if($count -lt 0 -or $count -ne @($Nodes).Count -or ($count -gt 0 -and $Depth -gt 6)){throw 'Complete HMENU count/depth mismatch; no prefix acceptance.'}
 for($index=0;$index -lt $count;$index++){
  $script:nodeCount++;if($script:nodeCount -gt 256){throw 'Complete HMENU exceeds 256 nodes.'}
  $node=$Nodes[$index];$pathNow=@($Path)+@($index);$key=$pathNow -join '.'
  if(($node.path -join '.') -cne $key -or [string]$node.identity -notmatch '^[1-9][0-9]*$' -or [string]$node.collection_identity -notmatch '^[1-9][0-9]*$'){throw 'Exact request-local path or canonical item/collection identity invalid.'}
  $item=[BowserHostMenu]::Item($Menu,$index);$kind=[int]$node.kind
  if($kind -lt 0 -or $kind -gt 4){throw 'Unknown full-tree kind.'}
  $type=if($kind -eq 3){0x800}elseif($kind -eq 2){0x200}else{0}
  # Match exact AppendMenu projection: separator state3; disabled nonseparator MF_GRAYED state1.
  $state=if($kind -eq 3){3}elseif(!$node.enabled){1}else{0}
  if($kind -in @(1,2) -and $node.checked){$state=$state -bor 8}
  if($item.Type -ne $type -or ($item.State -band 0xffffff7fL) -ne $state -or $item.Units -ne [int]$node.rendered_units -or $item.Fingerprint -cne $node.rendered_fingerprint){
   # Failure-only fixed/numeric evidence; this does not accept or select the mismatching node.
   $report.tree_mismatch=@{path=@($pathNow);kind=$kind;expected_type=$type;actual_type=$item.Type;expected_state=$state;actual_state=$item.State;masked_actual_state=($item.State -band 0xffffff7fL);expected_units=[int]$node.rendered_units;actual_units=$item.Units;fingerprint_equal=($item.Fingerprint -ceq $node.rendered_fingerprint)}
   try {Save-Report} catch {} # Diagnostic I/O must never replace the original rejection.
   throw 'Exact full-tree native type/state/rendered UTF-16 fingerprint mismatch.'
  }
  $child=[BowserHostMenu]::GetSubMenu($Menu,$index)
  if($kind -eq 4){
   if($child -eq [IntPtr]::Zero -or $child -ne $item.Child){throw 'Exact current submenu handle mismatch.'}
   $null=Test-Tree $child @($node.children) $pathNow ($Depth+1)
  }else{
   if($child -ne [IntPtr]::Zero -or $item.Child -ne [IntPtr]::Zero -or @($node.children).Count -ne 0){throw 'Unexpected native children.'}
   if($kind -in @(0,1,2)){
    if([int]$node.command_id -le 0 -or $item.Id -ne [uint32]$node.command_id -or !$script:leafIds.Add([int]$item.Id)){throw 'Missing, changed, or duplicate positive request-local native command ID.'}
   }
  }
  $script:tree[$key]=@{menu=$Menu;index=$index;item=$item;node=$node;child=$child}
 }
 if([BowserHostMenu]::GetMenuItemCount($Menu) -ne $count){throw 'Native tree count changed across full read.'}
}
function Full-Tree([IntPtr]$Menu) {
 $script:tree=@{};$script:nodeCount=0;$script:leafIds=[Collections.Generic.HashSet[int]]::new()
 $null=Test-Tree $Menu @($script:request.nodes) @() 1
 if($script:nodeCount -lt 1){throw 'Empty complete menu.'}
}
function Popup-Chain([IntPtr[]]$Expected,[switch]$AllowPending) {
 Host-State;$null=Request-State
 $complete=$false;$popups=@([BowserHostMenu]::Popups($OwnerPid,$tid,[ref]$complete))
 if(!$complete -or $popups.Count -gt 6){throw 'Complete owned visible popup inventory unavailable.'}
 if($popups.Count -ne $Expected.Count){if($AllowPending -and $popups.Count -lt $Expected.Count){return $null};throw 'Visible popup chain is not exactly the expected anchored chain.'}
 $chain=@();$seen=[Collections.Generic.HashSet[long]]::new()
 foreach($menu in $Expected){
  $matches=@();foreach($window in $popups){
   $bar=[BowserHostMenu+MENU]::new();$bar.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($bar)
   if(![BowserHostMenu]::GetMenuBarInfo($window,-4,0,[ref]$bar)){throw 'Native popup HMENU query unavailable.'}
   if($bar.hMenu -eq $menu){$matches+=,$window}
  }
  if($matches.Count -ne 1 -or !$seen.Add($matches[0].ToInt64())){throw 'Exact child HMENU does not identify one owned popup.'}
  $window=$matches[0];$rect=[BowserHostMenu+RECT]::new()
  if(![BowserHostMenu]::EscapeReady($h,$OwnerPid,$tid,$window,$menu,$guard) -or ![BowserHostMenu]::GetWindowRect($window,[ref]$rect) -or $rect.Right -le $rect.Left -or $rect.Bottom -le $rect.Top){throw 'Fresh exact popup owner/STA/foreground/geometry proof unavailable.'}
  $chain+=,@{window=$window;menu=$menu;rect=$rect}
 }
 return ,$chain
}
function Item-Point($Entry,$Popup) {
 $r=[BowserHostMenu+RECT]::new();$outer=$Popup.rect
 if(![BowserHostMenu]::GetMenuItemRect([IntPtr]::Zero,$Entry.menu,$Entry.index,[ref]$r) -or $r.Right -le $r.Left -or $r.Bottom -le $r.Top -or $r.Left -lt $outer.Left -or $r.Right -gt $outer.Right -or $r.Top -lt $outer.Top -or $r.Bottom -gt $outer.Bottom){throw 'Exact path item bounds not contained in its anchored popup.'}
 $point=[BowserHostMenu+POINT]::new();$point.X=[int](($r.Left+$r.Right)/2);$point.Y=[int](($r.Top+$r.Bottom)/2)
 return @{point=$point;rect=$r}
}
try {
 [BowserHostMenu]::CheckSeparatorProjection();$report.separator_projection_contract=$true
 [BowserHostMenu]::CheckGrayedProjection();$report.grayed_projection_contract=$true
 if($SyntheticToken -notmatch '^[0-9]+-[0-9]+$'){throw 'Generated disposable token required.'}
 if($pidNow -ne $OwnerPid -or ![BowserHostMenu]::Host($h,$OwnerPid,$tid)){throw 'Exact disposable native owner unavailable.'}
 $guard=[BowserHostMenu+OriginalProcess]::new($OwnerPid,$OwnerExe);$browserGuard=[BowserHostMenu+ReadOnlyBrowser]::new($BrowserPid)
 if($Mode -in @('Copy','Paste')){
  $payload=if($Mode -ceq 'Copy'){'sentinel-'+$SyntheticToken}else{'paste-'+$SyntheticToken}
  $clipboard=[BowserHostMenu+ClipboardGuard]::new($payload)
 }
 Neutral
 if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserHostMenu]::Host($h,$OwnerPid,$tid) -or ![BowserHostMenu]::IsWindowVisible($h) -or ![BowserHostMenu]::IsWindowEnabled($h)){throw 'Retained exact owner unavailable before initial activation.'}
 # One ordinary exact-owner activation only. Never retry or rescue later foreground failures.
 $report.activation_before_exact_owner=([BowserHostMenu]::GetForegroundWindow() -eq $h)
 $activated=[BowserHostMenu]::SetForegroundWindow($h)
 $report.activation_returned_success=[bool]$activated
 $report.activation_after_exact_owner=([BowserHostMenu]::GetForegroundWindow() -eq $h)
 if(!$activated){throw 'Initial exact-owner activation call denied.'}
 # Cross-input-queue activation is asynchronous. Observe only; never issue a second call.
 $activationWatch=[Diagnostics.Stopwatch]::StartNew();$report.activation_polls=0
 do {
  $report.activation_polls++
  if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserHostMenu]::Host($h,$OwnerPid,$tid) -or ![BowserHostMenu]::IsWindowVisible($h) -or ![BowserHostMenu]::IsWindowEnabled($h)){throw 'Retained exact owner unavailable during activation observation.'}
  $foregroundReady=([BowserHostMenu]::GetForegroundWindow() -eq $h)
  $report.activation_elapsed_ms=$activationWatch.ElapsedMilliseconds
  $report.activation_observed_exact_owner=$foregroundReady
  if($report.activation_elapsed_ms -ge 2000){throw 'Bounded initial exact-owner activation observation expired.'}
  if($foregroundReady){break}
  Start-Sleep -Milliseconds 20
 }while($true)
 $point=[BowserHostMenu+POINT]::new();$point.X=$ClientX;$point.Y=$ClientY
 if(![BowserHostMenu]::ClientToScreen($h,[ref]$point)){throw 'Exact COM input conversion failed.'}
 Source-Control;Neutral
 if(![BowserHostMenu]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point) -or ![BowserHostMenu]::SetCursorPos($point.X,$point.Y)){throw 'Fresh exact source hit/geometry unavailable.'}
 Source-Control;Neutral
 if(![BowserHostMenu]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point) -or !$guard.SameAlive() -or !$browserGuard.SameAlive()){throw 'Exact source/retained process changed before physical right-click.'}
 if($clipboard){$clipboard.RequireOwned()}
 try{[BowserHostMenu]::mouse_event(8,0,0,0,[UIntPtr]::Zero);$report.right_input_called=$true}finally{[BowserHostMenu]::mouse_event(16,0,0,0,[UIntPtr]::Zero)}
 $watch=[Diagnostics.Stopwatch]::StartNew();$rootMenu=[IntPtr]::Zero
 do {
  if(!$guard.SameAlive() -or !$browserGuard.SameAlive()){throw 'Retained process changed during bounded popup readiness.'}
  $witness=Request-State -AllowPending
  if($witness){
   $complete=$false;$windows=@([BowserHostMenu]::Popups($OwnerPid,$tid,[ref]$complete));if(!$complete -or $windows.Count -gt 1){throw 'Root popup inventory incomplete or not unique.'}
   if($windows.Count -eq 1){$bar=[BowserHostMenu+MENU]::new();$bar.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($bar);if(![BowserHostMenu]::GetMenuBarInfo($windows[0],-4,0,[ref]$bar) -or $bar.hMenu -eq [IntPtr]::Zero){throw 'Exact root HMENU unavailable.'};$rootMenu=$bar.hMenu;break}
  }
  if($watch.ElapsedMilliseconds -ge 10000){throw 'Bounded root popup readiness expired.'};Start-Sleep -Milliseconds 40
 }while($true)
 $expected=@($rootMenu);$chain=Popup-Chain $expected;Full-Tree $rootMenu
 if($Mode -ceq 'Cancel'){
  $null=Request-State;Neutral;$chain=Popup-Chain $expected;Full-Tree $rootMenu
  Neutral
  if(![BowserHostMenu]::EscapeReady($h,$OwnerPid,$tid,$chain[0].window,$rootMenu,$guard) -or !$browserGuard.SameAlive()){throw 'Exact native root changed after complete tree proof; Escape not sent.'}
  try{[BowserHostMenu]::keybd_event(27,0,0,[UIntPtr]::Zero)}finally{[BowserHostMenu]::keybd_event(27,0,2,[UIntPtr]::Zero)}
 }else{
  # Current engine Name selects built-ins, actual successful inserted-item identity selects host
  # roles. Labels, numeric ranges, ordering assumptions, and global text search are not authority.
  $path=@($script:request.selected_path);if($path.Count -lt 1 -or $path.Count -gt 6){throw 'Exact current request-selected path absent or outside bound.'}
  $key=$path -join '.';$chosen=$script:tree[$key]
  if(!$chosen -or !$chosen.node.enabled -or [int]$chosen.node.kind -notin @(0,1,2)){throw 'Exact request-selected native path is not one enabled leaf.'}
  $role=$chosen.node.host_role;$name=$chosen.node.current_name
  $matches=($Mode -ceq 'Copy' -and $name -ceq 'copy' -and $null -eq $role) -or
   ($Mode -ceq 'Paste' -and $name -ceq 'paste' -and $null -eq $role) -or
   ($Mode -ceq 'Note' -and $role -ceq 'insert-note-child') -or ($Mode -ceq 'Split' -and $role -ceq 'split-link')
  if(!$matches){throw 'Exact request-selected path does not match its native Name or inserted-item identity role.'}
  $report.selected_path=$path
  for($depth=0;$depth -lt $path.Count-1;$depth++){
   $key=($path[0..$depth] -join '.');$parent=$script:tree[$key]
   if(!$parent -or [int]$parent.node.kind -ne 4 -or !$parent.node.enabled -or $parent.child -eq [IntPtr]::Zero){throw 'Exact enabled current submenu path unavailable.'}
   if($Mode -ceq 'Note' -and $parent.node.host_role -cne 'insert-note-parent'){throw 'Note child is not under the actual host-created parent.'}
   $null=Request-State;Neutral;$chain=Popup-Chain $expected;Full-Tree $rootMenu;$parent=$script:tree[$key]
   $position=Item-Point $parent $chain[-1]
   if([BowserHostMenu]::WindowFromPoint($position.point) -ne $chain[-1].window -or ![BowserHostMenu]::SetCursorPos($position.point.X,$position.point.Y)){throw 'Exact anchored parent submenu hit unavailable.'}
   $null=Request-State;Neutral;Host-State;Full-Tree $rootMenu
   if([BowserHostMenu]::WindowFromPoint($position.point) -ne $chain[-1].window){throw 'Parent submenu hit changed before OS hover.'}
   # Real OS cursor hover opens the submenu. No native/API invocation or fallback click.
   $expected+=,$parent.child;$wait=[Diagnostics.Stopwatch]::StartNew();$next=$null
   do{$next=Popup-Chain $expected -AllowPending;if($next){break};if($wait.ElapsedMilliseconds -ge 3000){throw 'Exact OS-hover child submenu did not appear.'};Start-Sleep -Milliseconds 40}while($true)
   $child=$next[-1].rect;$anchor=$position.rect
   if($child.Top -gt $anchor.Bottom -or $child.Bottom -lt $anchor.Top){throw 'Child popup is not geometrically anchored to its exact parent item.'}
   $chain=$next;Full-Tree $rootMenu
  }
  $key=$path -join '.';$null=Request-State;Neutral;$chain=Popup-Chain $expected;Full-Tree $rootMenu;$chosen=$script:tree[$key]
  if(!$chosen -or !$chosen.node.enabled -or [int]$chosen.node.kind -notin @(0,1,2)){throw 'Exact enabled leaf changed after submenu traversal.'}
  $position=Item-Point $chosen $chain[-1]
  if([BowserHostMenu]::WindowFromPoint($position.point) -ne $chain[-1].window -or ![BowserHostMenu]::SetCursorPos($position.point.X,$position.point.Y)){throw 'Exact current leaf hit unavailable.'}
  $null=Request-State;Neutral;$chain=Popup-Chain $expected;Full-Tree $rootMenu;$fresh=$script:tree[$key];$now=Item-Point $fresh $chain[-1]
  if($fresh.item.Id -ne $chosen.item.Id -or $now.point.X -ne $position.point.X -or $now.point.Y -ne $position.point.Y -or [BowserHostMenu]::WindowFromPoint($position.point) -ne $chain[-1].window){throw 'Exact request/path/leaf/hit changed immediately before one physical click.'}
  if($clipboard){$clipboard.RequireOwned()}
  Host-State;Neutral
  foreach($popup in $chain){if(![BowserHostMenu]::EscapeReady($h,$OwnerPid,$tid,$popup.window,$popup.menu,$guard)){throw 'Final exact popup HWND/HMENU identity unavailable after clipboard and GUI reads.'}}
  if(!$browserGuard.SameAlive() -or [BowserHostMenu]::WindowFromPoint($position.point) -ne $chain[-1].window){throw 'Final exact leaf hit changed.'}
  try{[BowserHostMenu]::mouse_event(2,0,0,0,[UIntPtr]::Zero);$report.selection_called=$true;$report.selected_command_id=[int]$fresh.item.Id}finally{[BowserHostMenu]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}
 }
 $deadline=[Diagnostics.Stopwatch]::StartNew()
 do{
  $complete=$false;$remaining=@([BowserHostMenu]::Popups($OwnerPid,$tid,[ref]$complete));$gui=[BowserHostMenu+GUI]::new();$gui.cbSize=[Runtime.InteropServices.Marshal]::SizeOf($gui)
  if(!$complete -or !$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserHostMenu]::Host($h,$OwnerPid,$tid)){throw 'Exact process/popup disappearance proof unavailable.'}
  if($remaining.Count -eq 0 -and [BowserHostMenu]::GetGUIThreadInfo($tid,[ref]$gui) -and ($gui.flags -band 0x10) -eq 0){break}
  if($deadline.ElapsedMilliseconds -ge 5000){throw 'Complete popup-chain disappearance/cleared STA mode not observed.'};Start-Sleep -Milliseconds 40
 }while($true)
 # The exact current production coordinator must settle BEFORE any clipboard content read.
 $settled=$null;$wait=[Diagnostics.Stopwatch]::StartNew()
 do {
  if(!$guard.SameAlive() -or !$browserGuard.SameAlive()){throw 'Retained process lost before exact production completion.'}
  if(Test-Path -LiteralPath $CompletedReceipt -PathType Leaf){
   $file=Get-Item -LiteralPath $CompletedReceipt;if($file.Length -le 0 -or $file.Length -gt 4096){throw 'Completion witness outside bound.'}
   $settled=[IO.File]::ReadAllText($file.FullName)|ConvertFrom-Json
   if($settled.schema -cne 'bowser-real-app-host-completed-v1' -or $settled.nonce -cne $ProbeNonce -or [string]$settled.request -cne [string]$script:request.request -or $settled.mode -cne $Mode -or $settled.complete_calls -ne 1 -or !$settled.complete_ok -or !$settled.cleanup_ok){throw 'Exact production completion identity/state mismatch.'}
   if($Mode -ceq 'Cancel'){if($null -ne $settled.selected -or $settled.setter_calls -ne 0 -or $settled.setter_ok){throw 'Cancellation dispatched a command.'}}
   elseif($settled.selected -ne $report.selected_command_id -or $settled.setter_calls -ne 1 -or !$settled.setter_ok){throw 'Exact current selected command did not settle successfully.'}
   break
  }
  if($wait.ElapsedMilliseconds -ge 10000){throw 'Actual production completion absent: clipboard contents will not be read.'};Start-Sleep -Milliseconds 40
 }while($true)
 $report.completed=$settled
 if($Mode -ceq 'Copy'){$clipboard.ObserveNativeCopy($BrowserPid,$browserGuard,('copy-'+$SyntheticToken))}
 if($clipboard){if(!$clipboard.Finish()){throw 'Clipboard changed or locked: left untouched; regression gate failed.'};$report.clipboard=$clipboard.Status;$report.owned_clipboard_sequences=@($clipboard.OwnedSequences)}
 $report.status=if($Mode -ceq 'Cancel'){'observed-actual-App-native-cancellation'}else{'observed-actual-App-native-selection'};Save-Report
}catch{
 $report.error=$_.Exception.Message
 # Constructor cleanup has already run, even when assignment never returned an object.
 try {if([BowserHostMenu+ClipboardGuard]::AdmissionStarted){$report.clipboard_admission=@{stage=[BowserHostMenu+ClipboardGuard]::AdmissionStage;failure_stage=[BowserHostMenu+ClipboardGuard]::AdmissionFailureStage;open_succeeded=[BowserHostMenu+ClipboardGuard]::AdmissionOpened;format_observed=[BowserHostMenu+ClipboardGuard]::AdmissionFormatObserved;first_format_zero=[BowserHostMenu+ClipboardGuard]::AdmissionFirstFormatZero;enum_error=[BowserHostMenu+ClipboardGuard]::AdmissionEnumError;sequence_observed=[BowserHostMenu+ClipboardGuard]::AdmissionSequenceObserved;sequence=[BowserHostMenu+ClipboardGuard]::AdmissionSequence;close_attempted=[BowserHostMenu+ClipboardGuard]::AdmissionCloseAttempted;close_confirmed=[BowserHostMenu+ClipboardGuard]::AdmissionCloseConfirmed}}}catch{}
 # Admission has already refused and closed the clipboard. These bounded read-only OS
 # observations cannot re-enter the constructor, clear/seed data, grant access, or allow input.
 try {
  if([BowserHostMenu+ClipboardGuard]::AdmissionFailureStage -gt 0 -and [BowserHostMenu+ClipboardGuard]::AdmissionCloseConfirmed){
   $report.clipboard_environment=[BowserHostMenu]::ClipboardEnvironment($OwnerPid,$BrowserPid)
  }
 }catch{$report.clipboard_environment=@{scope='failure-only-read-only-not-authority';status='diagnostic-unavailable'}}
 # Restore only our exact last-owned sequence BEFORE terminating the exact disposable test.
 if($clipboard){$restored=$clipboard.Finish();$report.clipboard=$clipboard.Status;$report.owned_clipboard_sequences=@($clipboard.OwnedSequences)}
 try{Save-Report}catch{[Console]::Error.WriteLine('Native host regression failure report could not be written.')}
 if($report.right_input_called -and $guard){$report.failure_cleanup=$guard.FailOnly($h,$tid);try{Save-Report}catch{}}
 [Console]::Error.WriteLine('Actual App host regression failed; no compatibility acceptance.');exit 75
}finally{if($clipboard){$clipboard.Dispose()};if($browserGuard){$browserGuard.Dispose()};if($guard){$guard.Dispose()}}
Write-Output "APP_HOST_CONTEXT mode=$Mode physical_native_input=true complete_tree_checked=true setter_and_effect_acceptance_remain_Rust_receipt_checks=true"
