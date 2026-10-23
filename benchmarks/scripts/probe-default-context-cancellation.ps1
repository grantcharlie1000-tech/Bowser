# Separate Chromium Views default-menu cancellation probe. Never selects/recreates an extension item.
[CmdletBinding()]
param([Parameter(Mandatory)][uint32]$OwnerPid,[Parameter(Mandatory)][long]$OwnerHandle,[Parameter(Mandatory)][string]$OwnerExe,[Parameter(Mandatory)][uint32]$BrowserPid,
 [Parameter(Mandatory)][int]$ClientX,[Parameter(Mandatory)][int]$ClientY,[Parameter(Mandatory)][int]$ViewportWidth,[Parameter(Mandatory)][int]$ViewportHeight,
 [Parameter(Mandatory)][string]$Out)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,WindowsBase
$uiaReferences=@([System.Windows.Automation.AutomationElement].Assembly.Location,[System.Windows.Automation.ControlType].Assembly.Location,[System.Windows.Rect].Assembly.Location)
Add-Type -ReferencedAssemblies $uiaReferences @'
using System;using System.Text;using System.Runtime.InteropServices;using System.Collections.Generic;using System.Windows.Automation;
public static class BowserDefaultContext {
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
 [DllImport("kernel32.dll",EntryPoint="GetCurrentProcessId")] static extern uint NativeCurrentProcessId();
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
 public static IntPtr[] Candidates(IntPtr host,uint browserPid,out bool complete){
  var list=new List<IntPtr>();int count=0;bool bounded=true;
  bool ok=EnumWindows((h,p)=>{if(++count>128){bounded=false;return false;}uint pid;uint tid=GetWindowThreadProcessId(h,out pid);if(pid==browserPid&&tid!=0&&IsWindowVisible(h)&&IsWindowEnabled(h)&&Class(h)=="Chrome_WidgetWin_2"&&GetParent(h)==host&&GetWindow(h,4)==host&&GetAncestor(h,3)==host&&GetAncestor(h,2)==h)list.Add(h);return true;},IntPtr.Zero);
  complete=bounded&&ok;return list.ToArray();
 }
 static bool FocusOwned(IntPtr host,IntPtr focus,uint pid,uint tid){uint actual;return focus!=IntPtr.Zero&&IsWindow(focus)&&GetWindowThreadProcessId(focus,out actual)==tid&&actual==pid&&IsChild(host,focus)&&GetAncestor(focus,2)==host;}
 static bool PopupIdentity(IntPtr host,IntPtr popup,uint browserPid,uint menuTid){uint p;return IsWindow(popup)&&GetWindowThreadProcessId(popup,out p)==menuTid&&p==browserPid&&Class(popup)=="Chrome_WidgetWin_2"&&GetParent(popup)==host&&GetWindow(popup,4)==host&&GetAncestor(popup,3)==host&&GetAncestor(popup,2)==popup;}
 public static uint CandidateThread(IntPtr popup,uint browserPid){uint p;uint t=GetWindowThreadProcessId(popup,out p);return p==browserPid?t:0;}
 public static bool NativeReady(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,uint menuTid,IntPtr expectedFocus){
  var a=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};var b=a;RECT r;
  return Host(host,testPid,hostTid)&&GetForegroundWindow()==host&&PopupIdentity(host,popup,browserPid,menuTid)&&IsWindowVisible(popup)&&IsWindowEnabled(popup)&&GetWindowRect(popup,out r)&&r.Right>r.Left&&r.Bottom>r.Top&&GetGUIThreadInfo(hostTid,ref a)&&GetGUIThreadInfo(menuTid,ref b)&&a.hwndActive==host&&b.hwndActive==host&&a.hwndFocus==b.hwndFocus&&(expectedFocus==IntPtr.Zero||a.hwndFocus==expectedFocus)&&FocusOwned(host,a.hwndFocus,browserPid,menuTid)&&a.hwndCapture==popup&&b.hwndCapture==popup&&PopupIdentity(host,popup,browserPid,menuTid)&&Host(host,testPid,hostTid)&&GetForegroundWindow()==host;
 }
 static int[] Runtime(AutomationElement e){var id=e.GetRuntimeId();if(id==null||id.Length==0||id.Length>32)throw new InvalidOperationException("UIA runtime identity outside bound.");return id;}
 static bool Same(int[] a,int[] b){if(a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true;}
 static bool Finite(double n){return !Double.IsNaN(n)&&!Double.IsInfinity(n);}
 public sealed class MenuProof {
  public IntPtr Window,Focus;public uint Thread;public int[] RootId,MenuId,ItemId;public double Left,Top,Width,Height;
 }
 static string Number(long n){return n.ToString(System.Globalization.CultureInfo.InvariantCulture);}
 static string JsonIds(int[] ids){return ids==null?"null":"["+String.Join(",",Array.ConvertAll(ids,n=>Number(n)))+"]";}
 static bool RootFence(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,uint menuTid,ReadOnlyBrowser browser,AutomationElement root,int[] rootId){
  return browser.SameAlive()&&Host(host,testPid,hostTid)&&PopupIdentity(host,popup,browserPid,menuTid)&&Same(Runtime(root),rootId)&&root.Current.ProcessId==(int)browserPid&&root.Current.NativeWindowHandle==popup.ToInt32()&&PopupIdentity(host,popup,browserPid,menuTid)&&Host(host,testPid,hostTid)&&browser.SameAlive();
 }
 static string RetainedChildMetadata(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,uint menuTid,ReadOnlyBrowser browser,AutomationElement root,int[] rootId,AutomationElement node,int[] nodeId,int providerPid,int native,int type){
  const string prefix="{\"scope\":\"failure-only-retained-process-direct-child-not-traversal-authority\",\"authority\":false,\"fresh_request_membership_proved\":false,";
  try{
   // Inspect only the rejected structural node's own HWND. Never enumerate other providers,
   // traverse through the rejected node, or infer GPU kind from the process image/class.
   if(providerPid<=0||native==0||type!=ControlType.Pane.Id)return prefix+"\"status\":\"not-a-native-structural-pane\"}";
   IntPtr child=new IntPtr(native);uint nativePid;uint tid=GetWindowThreadProcessId(child,out nativePid);
   if(tid==0||nativePid!=(uint)providerPid||!IsWindow(child)||Class(child)!="Intermediate D3D Window")return prefix+"\"status\":\"native-node-identity-unavailable\"}";
   using(var retained=new ReadOnlyBrowser((uint)providerPid)){
    bool before=retained.SameAlive()&&retained.SameRuntimeImage(browser)&&RootFence(host,testPid,hostTid,popup,browserPid,menuTid,browser,root,rootId)&&NativeReady(host,testPid,hostTid,popup,browserPid,menuTid,IntPtr.Zero)&&Same(Runtime(node),nodeId)&&node.Current.NativeWindowHandle==native&&node.Current.ProcessId==providerPid;
    if(!before)return prefix+"\"status\":\"identity-or-current-root-fence-unavailable-before-read\"}";
    uint pidBefore;uint threadBefore=GetWindowThreadProcessId(child,out pidBefore);
    IntPtr parentBefore=GetParent(child),rootBefore=GetAncestor(child,2);
    bool directBefore=parentBefore==popup&&rootBefore==popup&&IsChild(popup,child)&&Class(child)=="Intermediate D3D Window";
    uint pidAfter;uint threadAfter=GetWindowThreadProcessId(child,out pidAfter);
    IntPtr parentAfter=GetParent(child),rootAfter=GetAncestor(child,2);
    bool directAfter=parentAfter==popup&&rootAfter==popup&&IsChild(popup,child)&&Class(child)=="Intermediate D3D Window";
    bool after=retained.SameAlive()&&retained.SameRuntimeImage(browser)&&RootFence(host,testPid,hostTid,popup,browserPid,menuTid,browser,root,rootId)&&NativeReady(host,testPid,hostTid,popup,browserPid,menuTid,IntPtr.Zero)&&Same(Runtime(node),nodeId)&&node.Current.NativeWindowHandle==native&&node.Current.ProcessId==providerPid;
    if(!after)return prefix+"\"status\":\"identity-or-current-root-fence-unavailable-after-read-details-discarded\"}";
    return prefix+"\"status\":\"captured-nonatomic-not-hwnd-lifetime-proof\",\"retained_process_alive_before_after\":true,\"same_retained_runtime_image_before_after\":true,\"process_creation_identity\":\""+retained.CreationIdentity+"\",\"pid\":"+Number(providerPid)+",\"native_hwnd\":"+Number(native)+",\"popup\":"+Number(popup.ToInt64())+",\"native_pid_thread_same\":"+(pidBefore==(uint)providerPid&&pidAfter==pidBefore&&threadBefore==tid&&threadAfter==tid?"true":"false")+",\"direct_child_before\":"+(directBefore?"true":"false")+",\"direct_child_after\":"+(directAfter?"true":"false")+",\"native_parent_before\":"+Number(parentBefore.ToInt64())+",\"native_parent_after\":"+Number(parentAfter.ToInt64())+",\"native_root_before\":"+Number(rootBefore.ToInt64())+",\"native_root_after\":"+Number(rootAfter.ToInt64())+",\"uia_runtime_and_native_identity_same_before_after\":true,\"foreground_capture_root_fences_before_after\":true}";
   }
  }catch{return prefix+"\"status\":\"read-only-process-or-node-query-unavailable-no-fallback\"}";}
 }
 static void WriteMismatch(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,uint menuTid,ReadOnlyBrowser browser,AutomationElement root,int[] rootId,AutomationElement node,int depth,int[] expectedParent,int index,int providerPid,uint helperPid,string directory){
  string prefix="{\"scope\":\"failure-only-popup-rooted-numeric-identity-not-input-authority\",\"first_mismatch\":true,\"visited_count\":"+Number(index)+",\"depth\":"+Number(depth)+",\"expected_pid\":"+Number(browserPid)+",\"observed_pid\":"+Number(providerPid)+",\"helper_pid\":"+Number(helperPid)+",\"helper_pid_available\":"+(helperPid!=0?"true":"false")+",\"observed_pid_equals_helper\":"+(helperPid!=0&&providerPid==helperPid?"true":"false")+",\"observed_pid_equals_test\":"+(providerPid==testPid?"true":"false")+",\"observed_pid_equals_browser\":"+(providerPid==browserPid?"true":"false")+",\"expected_pid_equal\":false,";
  string json=prefix+"\"status\":\"diagnostic-unavailable\"}";
  try{
   if(!RootFence(host,testPid,hostTid,popup,browserPid,menuTid,browser,root,rootId)){json=prefix+"\"status\":\"stale-before-read\"}";}
   else{
    var id=Runtime(node);var current=node.Current;int type=current.ControlType.Id;int native=current.NativeWindowHandle;
    string pidNoDefaultKind="getter-failed",pidNoDefaultValue="null";
    try{var raw=node.GetCurrentPropertyValue(AutomationElement.ProcessIdProperty,true);if(raw is int){pidNoDefaultKind="integer";pidNoDefaultValue=Number((int)raw);}else if(Object.ReferenceEquals(raw,AutomationElement.NotSupported)){pidNoDefaultKind="not-supported";}else{pidNoDefaultKind="other-kind";}}catch{}
    // Numeric metadata from this rejected node's own HWND only; never process or input authority.
    IntPtr nativeWindow=new IntPtr(native);uint nativePid=0;uint nativeTid=native==0?0:GetWindowThreadProcessId(nativeWindow,out nativePid);
    string nativeClass=native==0?null:Class(nativeWindow);IntPtr nativeParent=native==0?IntPtr.Zero:GetParent(nativeWindow);
    uint nativePidAfter=0;uint nativeTidAfter=native==0?0:GetWindowThreadProcessId(nativeWindow,out nativePidAfter);
    bool nativeStable=native!=0&&IsWindow(nativeWindow)&&nativeTid!=0&&nativeTid==nativeTidAfter&&nativePid==nativePidAfter;
    int[] parentId=null;int parentPid=0,parentType=0,parentNative=0;bool parentPresent=false,parentMetadataRead=false;
    // Only the known enqueued parent is inspected; never walk beyond the captured root.
    if(expectedParent!=null){var parent=TreeWalker.RawViewWalker.GetParent(node);if(parent!=null){parentPresent=true;parentId=Runtime(parent);if(Same(parentId,expectedParent)){var pc=parent.Current;parentPid=pc.ProcessId;parentType=pc.ControlType.Id;parentNative=pc.NativeWindowHandle;parentMetadataRead=true;}}}
    string retainedChild=RetainedChildMetadata(host,testPid,hostTid,popup,browserPid,menuTid,browser,root,rootId,node,id,providerPid,native,type);
    bool after=RootFence(host,testPid,hostTid,popup,browserPid,menuTid,browser,root,rootId)&&Same(Runtime(node),id)&&node.Current.ProcessId==providerPid;
    if(!after){json=prefix+"\"status\":\"stale-after-read-details-discarded\"}";}
    else{
     string kind=Same(id,rootId)?"root":type==ControlType.Menu.Id?"menu":type==ControlType.MenuItem.Id?"menu-item-unattributed":"structural-other";
     string nativeKind=nativeClass=="Intermediate D3D Window"?"intermediate-d3d":nativeClass=="Chrome_WidgetWin_2"?"chromium-popup":"other-or-unavailable";
     prefix+="\"native_query\":{\"non_atomic\":true,\"authority\":false,\"stable_pid_thread\":"+(nativeStable?"true":"false")+",\"pid\":"+Number(nativePid)+",\"thread\":"+Number(nativeTid)+",\"parent\":"+Number(nativeParent.ToInt64())+",\"class_kind\":\""+nativeKind+"\"},";
     prefix+="\"retained_child_metadata\":"+retainedChild+",";
     json=prefix+"\"status\":\"captured-nonatomic-not-authority\",\"root_identity_before_after\":true,\"node_kind\":\""+kind+"\",\"control_type_id\":"+Number(type)+",\"native_hwnd\":"+Number(native)+",\"pid_no_default_kind\":\""+pidNoDefaultKind+"\",\"pid_no_default_value\":"+pidNoDefaultValue+",\"runtime_id\":"+JsonIds(id)+",\"root_runtime_id\":"+JsonIds(rootId)+",\"parent_already_encountered\":"+(expectedParent!=null?"true":"false")+",\"expected_parent_runtime_id\":"+JsonIds(expectedParent)+",\"parent_present\":"+(parentPresent?"true":"false")+",\"parent_runtime_id\":"+JsonIds(parentId)+",\"parent_matches_enqueued_parent\":"+(expectedParent!=null&&parentId!=null&&Same(parentId,expectedParent)?"true":"false")+",\"parent_metadata_read\":"+(parentMetadataRead?"true":"false")+",\"parent_provider_pid\":"+Number(parentPid)+",\"parent_control_type_id\":"+Number(parentType)+",\"parent_native_hwnd\":"+Number(parentNative)+"}";
    }
   }
  }catch{ /* Getter failures remain diagnostic unavailable; never suppress the original rejection. */ }
  if(json.Length>8192)json=prefix+"\"status\":\"diagnostic-bound-exceeded\"}";
  try{System.IO.File.WriteAllText(System.IO.Path.Combine(directory,"native-provider-mismatch.json"),json,new UTF8Encoding(false));}catch{ /* Missing evidence is inconclusive, never permission to continue. */ }
 }
 public static MenuProof ReadMenu(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,ReadOnlyBrowser browser,string diagnosticDirectory){
  uint helperPid=NativeCurrentProcessId(); // Known local identity, captured before UIA traversal.
  uint beforePid;uint tid=GetWindowThreadProcessId(popup,out beforePid);if(beforePid!=browserPid||tid==0||!IsWindow(popup))throw new InvalidOperationException("Exact native UIA root unavailable.");
  var root=AutomationElement.FromHandle(popup);if(root==null||root.Current.ProcessId!=(int)browserPid||root.Current.NativeWindowHandle!=popup.ToInt32())throw new InvalidOperationException("Exact native UIA root mismatch.");
  var focusGui=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};if(!GetGUIThreadInfo(tid,ref focusGui)||!FocusOwned(host,focusGui.hwndFocus,browserPid,tid))throw new InvalidOperationException("Native focus witness unavailable before UIA read.");
  int[] rootId=Runtime(root);var walker=TreeWalker.RawViewWalker;var queue=new Queue<Tuple<AutomationElement,int,int[]>>();queue.Enqueue(Tuple.Create(root,0,(int[])null));int examined=0,menus=0,items=0;int[] menuId=null,itemId=null;System.Windows.Rect chosen=new System.Windows.Rect();
  while(queue.Count>0){if(++examined>128)throw new InvalidOperationException("UIA raw menu inventory incomplete.");var queued=queue.Dequeue();var e=queued.Item1;var c=e.Current;int providerPid=c.ProcessId;
   if(providerPid!=(int)browserPid){
    WriteMismatch(host,testPid,hostTid,popup,browserPid,tid,browser,root,rootId,e,queued.Item2,queued.Item3,examined,providerPid,helperPid,diagnosticDirectory);
    throw new InvalidOperationException("UIA menu provider PID mismatch.");
   }
   int[] currentId=Runtime(e);
   if(c.ControlType==ControlType.Menu){menus++;menuId=Runtime(e);}
   if(c.ControlType==ControlType.MenuItem&&String.Equals(c.Name,"Bowser native context permission probe",StringComparison.Ordinal)){
    if(!c.IsEnabled||c.IsOffscreen)throw new InvalidOperationException("Fixture menu leaf unavailable.");
    var child=walker.GetFirstChild(e);if(child!=null)throw new InvalidOperationException("Fixture item is not a leaf.");
    var ancestor=e;bool reached=false;int menuAncestors=0;for(int hop=0;hop<32&&ancestor!=null;hop++){var ac=ancestor.Current;if(ac.ProcessId!=(int)browserPid)throw new InvalidOperationException("UIA raw ancestry provider mismatch.");if(ac.ControlType==ControlType.Menu)menuAncestors++;if(Same(Runtime(ancestor),rootId)){reached=true;break;}if(ac.NativeWindowHandle!=0&&(IntPtr)ac.NativeWindowHandle!=popup)throw new InvalidOperationException("Unexpected native ancestor in menu hierarchy.");ancestor=walker.GetParent(ancestor);}if(!reached||menuAncestors!=1)throw new InvalidOperationException("UIA raw Menu ancestry incomplete or ambiguous.");
    items++;itemId=Runtime(e);chosen=c.BoundingRectangle;
   }
   for(var child=walker.GetFirstChild(e);child!=null;child=walker.GetNextSibling(child)){if(queue.Count+examined>=128)throw new InvalidOperationException("UIA child inventory exceeds bound.");queue.Enqueue(Tuple.Create(child,queued.Item2+1,currentId));}
  }
  RECT r;uint afterPid;uint afterTid=GetWindowThreadProcessId(popup,out afterPid);
  if(!FocusOwned(host,focusGui.hwndFocus,browserPid,tid)||menus!=1||items!=1||!Same(rootId,Runtime(root))||afterPid!=browserPid||afterTid!=tid||!IsWindow(popup)||!GetWindowRect(popup,out r)||!Finite(chosen.X)||!Finite(chosen.Y)||!Finite(chosen.Width)||!Finite(chosen.Height)||chosen.Width<=0||chosen.Height<=0||chosen.Left<r.Left||chosen.Top<r.Top||chosen.Right>r.Right||chosen.Bottom>r.Bottom)throw new InvalidOperationException("Unique bounded current Menu/MenuItem proof unavailable.");
  return new MenuProof{Window=popup,Focus=focusGui.hwndFocus,Thread=tid,RootId=rootId,MenuId=menuId,ItemId=itemId,Left=chosen.Left,Top=chosen.Top,Width=chosen.Width,Height=chosen.Height};
 }
 public static bool SameProof(MenuProof a,MenuProof b){return a.Window==b.Window&&a.Focus==b.Focus&&a.Thread==b.Thread&&Same(a.RootId,b.RootId)&&Same(a.MenuId,b.MenuId)&&Same(a.ItemId,b.ItemId)&&a.Left==b.Left&&a.Top==b.Top&&a.Width==b.Width&&a.Height==b.Height;}
 public static bool Released(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,uint menuTid,IntPtr focus){
  var a=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};var b=a;
  bool hidden=!IsWindow(popup)||(PopupIdentity(host,popup,browserPid,menuTid)&&!IsWindowVisible(popup));
  return Host(host,testPid,hostTid)&&GetForegroundWindow()==host&&hidden&&FocusOwned(host,focus,browserPid,menuTid)&&GetGUIThreadInfo(hostTid,ref a)&&GetGUIThreadInfo(menuTid,ref b)&&a.hwndActive==host&&b.hwndActive==host&&a.hwndFocus==focus&&b.hwndFocus==focus&&a.hwndCapture==IntPtr.Zero&&b.hwndCapture==IntPtr.Zero&&FocusOwned(host,focus,browserPid,menuTid)&&Host(host,testPid,hostTid)&&GetForegroundWindow()==host&&(!IsWindow(popup)||(PopupIdentity(host,popup,browserPid,menuTid)&&!IsWindowVisible(popup)));
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
  public bool SameRuntimeImage(ReadOnlyBrowser other){return String.Equals(image,other.image,StringComparison.OrdinalIgnoreCase);}
  public string CreationIdentity {get{return created.ToString(System.Globalization.CultureInfo.InvariantCulture);}}
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
$report=[ordered]@{scope='separate-default-Chromium-Views-menu-cancellation-not-selection';status='failed';right_input_called=$false;escape_called=$false;error=$null;failure_cleanup=$null}
$observationClock=[Diagnostics.Stopwatch]::StartNew();$report.topology=@()
$h=[IntPtr]$OwnerHandle;$pidNow=[uint32]0;$tid=[BowserDefaultContext]::GetWindowThreadProcessId($h,[ref]$pidNow);$guard=$null;$browserGuard=$null
function Save-Report {[IO.File]::WriteAllText((Join-Path $Out 'default-context-cancellation.json'),($report|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))}
function Observe-Topology([string]$Phase) {
 $observation=[ordered]@{phase=$Phase;elapsed_ms=$observationClock.ElapsedMilliseconds;identity_before=$false;identity_after=$false;status='unavailable';native=$null;screenshot_status='not-attempted'}
 try {
  if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserDefaultContext]::Host($h,$OwnerPid,$tid)){throw 'diagnostic-identity-unavailable'}
  $observation.identity_before=$true
  $observation.native=[BowserDefaultContext]::Topology($h,$OwnerPid,$BrowserPid)
  $observation.identity_after=($guard.SameAlive() -and $browserGuard.SameAlive() -and [BowserDefaultContext]::Host($h,$OwnerPid,$tid))
  $observation.status=if($observation.identity_after){'captured-read-only-not-input-authority'}else{'identity-unavailable-after-read'}
 } catch {$observation.status='read-only-topology-unavailable'}
 $report.topology+=,$observation;Save-Report
 # Native screenshot is diagnostic only; it does not identify or authorize a menu. No desktop capture.
 try {
  if(!$observation.identity_after -or !$guard.SameAlive() -or ![BowserDefaultContext]::Host($h,$OwnerPid,$tid)){throw 'diagnostic-capture-identity-unavailable'}
  $rect=[BowserDefaultContext+RECT]::new()
  if(![BowserDefaultContext]::GetWindowRect($h,[ref]$rect)){throw 'diagnostic-capture-bounds-unavailable'}
  $width=$rect.Right-$rect.Left;$height=$rect.Bottom-$rect.Top
  if($width -le 0 -or $height -le 0 -or $width -gt 4096 -or $height -gt 4096){throw 'diagnostic-capture-bounds-outside-limit'}
  $bitmap=[Drawing.Bitmap]::new($width,$height)
  try {
   $graphics=[Drawing.Graphics]::FromImage($bitmap)
   try {
    $dc=$graphics.GetHdc()
    try {$ok=[BowserDefaultContext]::PrintWindow($h,$dc,2)} finally {$graphics.ReleaseHdc($dc)}
   } finally {$graphics.Dispose()}
   if(!$ok -or !$guard.SameAlive() -or ![BowserDefaultContext]::Host($h,$OwnerPid,$tid)){throw 'diagnostic-capture-unconfirmed'}
   $name="native-context-$Phase.png";$bitmap.Save((Join-Path $Out $name),[Drawing.Imaging.ImageFormat]::Png)
   $observation.screenshot_status='captured-owned-host-only-not-independent-menu-proof';$observation.screenshot=$name
  } finally {$bitmap.Dispose()}
 } catch {$observation.screenshot_status='owned-host-capture-unavailable'}
 Save-Report
}
function Neutral {foreach($key in @(1,2,16,17,18,27,91,92)){if([BowserDefaultContext]::GetAsyncKeyState($key) -lt 0){throw 'Unexpected held modifier/Escape; no repair or input.'}}}
function Menu-State {
 param([switch]$AllowPending)
 if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserDefaultContext]::Host($h,$OwnerPid,$tid) -or [BowserDefaultContext]::GetForegroundWindow() -ne $h){throw 'Exact process/foreground host unavailable or changed.'}
 $complete=$false;$candidates=@([BowserDefaultContext]::Candidates($h,$BrowserPid,[ref]$complete))
 if(!$complete -or $candidates.Count -gt 1){throw 'Native candidate inventory incomplete or ambiguous.'}
 if($candidates.Count -eq 0){if($AllowPending){$report.readiness.last_predicate='native-candidate-not-present';return $null};throw 'Native menu candidate unavailable.'}
 $popup=$candidates[0];$menuTid=[BowserDefaultContext]::CandidateThread($popup,$BrowserPid)
 if(!$menuTid -or ![BowserDefaultContext]::NativeReady($h,$OwnerPid,$tid,$popup,$BrowserPid,$menuTid,[IntPtr]::Zero)){if($AllowPending){$report.readiness.last_predicate='exact-native-focus-capture-not-ready';return $null};throw 'Exact menu native focus/capture unavailable.'}
 $proof=[BowserDefaultContext]::ReadMenu($h,$OwnerPid,$tid,$popup,$BrowserPid,$browserGuard,$Out)
 if(![BowserDefaultContext]::NativeReady($h,$OwnerPid,$tid,$popup,$BrowserPid,$menuTid,$proof.Focus)){throw 'Native ownership changed across UIA read.'}
 return $proof
}

try {
 if($pidNow -ne $OwnerPid -or ![BowserDefaultContext]::Host($h,$OwnerPid,$tid)){throw 'Exact disposable host unavailable; no input.'}
 $guard=[BowserDefaultContext+OriginalProcess]::new($OwnerPid,$OwnerExe)
 $browserGuard=[BowserDefaultContext+ReadOnlyBrowser]::new($BrowserPid)
 $baselineComplete=$false;$baseline=@([BowserDefaultContext]::Candidates($h,$BrowserPid,[ref]$baselineComplete))
 if(!$baselineComplete -or $baseline.Count -ne 0){throw 'Fresh popup absence not established before input.'}
 Observe-Topology 'before-input'
 $point=[BowserDefaultContext+POINT]::new();$point.X=$ClientX;$point.Y=$ClientY
 if(![BowserDefaultContext]::ClientToScreen($h,[ref]$point)){throw 'Exact controlled point conversion failed.'}
 Neutral
 if(![BowserDefaultContext]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point)){throw 'Exact owned input target unavailable before cursor.'}
 if(![BowserDefaultContext]::SetCursorPos($point.X,$point.Y)){throw 'Native cursor placement failed.'}
 Neutral
 if(![BowserDefaultContext]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point)){throw 'Exact owned hit target changed before real right-click.'}
 try{[BowserDefaultContext]::mouse_event(0x0008,0,0,0,[UIntPtr]::Zero);$report.right_input_called=$true}finally{[BowserDefaultContext]::mouse_event(0x0010,0,0,0,[UIntPtr]::Zero)}
 $watch=[Diagnostics.Stopwatch]::StartNew();Observe-Topology 'after-right-input';$report.readiness=@{elapsed_ms=0;last_predicate='not-observed';polls=0};$menu=$null
 do {
  $report.readiness.polls++;$menu=Menu-State -AllowPending;$report.readiness.elapsed_ms=$watch.ElapsedMilliseconds
  if($watch.ElapsedMilliseconds -ge 10000){Observe-Topology 'readiness-expired';throw 'Native popup readiness window expired; topology unavailable.'}
  if($menu){$report.readiness.last_predicate='strict-native-menu-ready';break}
  Start-Sleep -Milliseconds 50
 } while($true)
 $watch.Stop();$report.popup=@{window=$menu.Window.ToInt64();thread=$menu.Thread;focus_witness=$menu.Focus.ToInt64();root_runtime=$menu.RootId;menu_runtime=$menu.MenuId;item_runtime=$menu.ItemId;label='exact-fixture-label';provider_pid=$BrowserPid;left=$menu.Left;top=$menu.Top;width=$menu.Width;height=$menu.Height}
 Neutral;$fresh=Menu-State
 if(![BowserDefaultContext]::SameProof($menu,$fresh)){throw 'Captured native/UIA menu identity changed before Escape.'}
 Neutral
 if(!$guard.SameAlive() -or !$browserGuard.SameAlive() -or ![BowserDefaultContext]::NativeReady($h,$OwnerPid,$tid,$menu.Window,$BrowserPid,$menu.Thread,$menu.Focus)){throw 'Exact foreground/native menu changed immediately before Escape.'}
 try{[BowserDefaultContext]::keybd_event(27,0,0,[UIntPtr]::Zero);$report.escape_called=$true}finally{[BowserDefaultContext]::keybd_event(27,0,2,[UIntPtr]::Zero)}
 $releaseWatch=[Diagnostics.Stopwatch]::StartNew();$released=$false
 do {$released=[BowserDefaultContext]::Released($h,$OwnerPid,$tid,$menu.Window,$BrowserPid,$menu.Thread,$menu.Focus);if($releaseWatch.ElapsedMilliseconds -ge 5000){throw 'Menu disappearance/capture release not confirmed in observation window.'};if($released){break};Start-Sleep -Milliseconds 50}while($true)
 if(!$guard.SameAlive() -or !$browserGuard.SameAlive()){throw 'Process identity unavailable after cancellation.'}
 $report.capture_released=$true;$report.menu_hidden_or_destroyed=$true
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
Write-Output 'DEFAULT_CONTEXT_CANCELLATION exact_owned_right_input=true exact_visible_native_popup=true real_escape_and_disappearance=true selection_performed=false'
