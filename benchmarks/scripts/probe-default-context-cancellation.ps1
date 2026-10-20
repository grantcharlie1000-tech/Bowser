# Separate Chromium Views default-menu cancellation probe. Never selects/recreates an extension item.
[CmdletBinding()]
param([Parameter(Mandatory)][uint32]$OwnerPid,[Parameter(Mandatory)][long]$OwnerHandle,[Parameter(Mandatory)][string]$OwnerExe,[Parameter(Mandatory)][uint32]$BrowserPid,
 [Parameter(Mandatory)][int]$ClientX,[Parameter(Mandatory)][int]$ClientY,[Parameter(Mandatory)][int]$ViewportWidth,[Parameter(Mandatory)][int]$ViewportHeight,
 [Parameter(Mandatory)][string]$Out,[ValidateSet('DefaultCancellation','ExtensionObservation')][string]$ProbeKind='DefaultCancellation',
 [uint32]$GpuPid=0,[UInt64]$GpuCreation=0,[UInt64]$BrowserCreation=0,[string]$OwnershipNonce='',[string]$SourceProof='',[string]$EnvironmentIdentity='',[UInt64]$AuthorityDeadlineTick=0)
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
  public IntPtr Window,Focus;public uint Thread;public int[] RootId,MenuId,ItemId;public double Left,Top,Width,Height;public string Inventory;public int GpuLeaves;
 }
 static string Number(long n){return n.ToString(System.Globalization.CultureInfo.InvariantCulture);}
 static string JsonIds(int[] ids){return ids==null?"null":"["+String.Join(",",Array.ConvertAll(ids,n=>Number(n)))+"]";}
 static bool RootFence(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,uint menuTid,ReadOnlyBrowser browser,AutomationElement root,int[] rootId){
  return browser.SameAlive()&&Host(host,testPid,hostTid)&&PopupIdentity(host,popup,browserPid,menuTid)&&Same(Runtime(root),rootId)&&root.Current.ProcessId==(int)browserPid&&root.Current.NativeWindowHandle==popup.ToInt32()&&PopupIdentity(host,popup,browserPid,menuTid)&&Host(host,testPid,hostTid)&&browser.SameAlive();
 }
 // Failure-only metadata for the exact HWND surfaced by the mismatched provider. Never
 // read foreign titles/content, enumerate desktop UIA roots, or use this as input authority.
 static string JsonText(string value){
  var result=new StringBuilder();foreach(char unit in value){
   if(unit=='\\')result.Append("\\\\");else if(unit=='"')result.Append("\\\"");
   else if(unit<32)result.Append("\\u").Append(((int)unit).ToString("x4",System.Globalization.CultureInfo.InvariantCulture));
   else result.Append(unit);
  }return result.ToString();
 }
 static string NativeMismatch(IntPtr node,IntPtr popup,IntPtr host){
  if(node==IntPtr.Zero)return "{\"status\":\"no-native-hwnd\"}";
  uint pid;uint tid=GetWindowThreadProcessId(node,out pid);
  if(tid==0||pid==0||!IsWindow(node))return "{\"status\":\"native-unavailable-before-read\"}";
  string name=Class(node);RECT bounds;bool geometry=GetWindowRect(node,out bounds);
  IntPtr parent=GetParent(node),owner=GetWindow(node,4),root=GetAncestor(node,2),rootOwner=GetAncestor(node,3);
  DiagnosticSetLastError(0);IntPtr child=DiagnosticGetWindow(node,5);int childError=Marshal.GetLastWin32Error();
  bool visible=IsWindowVisible(node),enabled=IsWindowEnabled(node),popupChild=IsChild(popup,node),hostChild=IsChild(host,node);
  uint afterPid;uint afterTid=GetWindowThreadProcessId(node,out afterPid);
  if(!IsWindow(node)||afterTid!=tid||afterPid!=pid||Class(node)!=name)return "{\"status\":\"native-identity-changed-details-discarded\"}";
  // Native class is a bounded OS identifier, not window text or a UIA Name. Escape it even
  // though this is diagnostic only, and never call GetWindowText on the foreign HWND.
  name=JsonText(name);
  return "{\"status\":\"captured-nonatomic-not-authority\",\"pid\":"+Number(pid)+",\"thread\":"+Number(tid)+",\"class_name\":\""+name+"\",\"parent\":"+Number(parent.ToInt64())+",\"owner\":"+Number(owner.ToInt64())+",\"root\":"+Number(root.ToInt64())+",\"root_owner\":"+Number(rootOwner.ToInt64())+",\"first_native_child\":"+Number(child.ToInt64())+",\"first_native_child_zero_observed\":"+(child==IntPtr.Zero?"true":"false")+",\"first_native_child_last_error\":"+Number(childError)+",\"is_popup_child\":"+(popupChild?"true":"false")+",\"is_host_child\":"+(hostChild?"true":"false")+",\"visible\":"+(visible?"true":"false")+",\"enabled\":"+(enabled?"true":"false")+",\"bounds_ok\":"+(geometry?"true":"false")+",\"left\":"+Number(bounds.Left)+",\"top\":"+Number(bounds.Top)+",\"right\":"+Number(bounds.Right)+",\"bottom\":"+Number(bounds.Bottom)+"}";
 }
 // No-default values distinguish an actual false from UIA's fallback/default value.
 // This pure formatter is also tested without invoking any provider or native API.
 static string BooleanValue(object raw){
  if(raw is bool)return "{\"kind\":\"boolean\",\"value\":"+((bool)raw?"true":"false")+"}";
  if(Object.ReferenceEquals(raw,AutomationElement.NotSupported))return "{\"kind\":\"not-supported\",\"value\":null}";
  return "{\"kind\":\"other-kind\",\"value\":null}";
 }
 static string BooleanProperty(AutomationElement node,AutomationProperty property){
  try{return BooleanValue(node.GetCurrentPropertyValue(property,true));}
  catch{return "{\"kind\":\"getter-failed\",\"value\":null}";}
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
    int[] parentId=null;int parentPid=0,parentType=0,parentNative=0;bool parentPresent=false,parentMetadataRead=false;
    // Only the known enqueued parent is inspected; never walk beyond the captured root.
    if(expectedParent!=null){var parent=TreeWalker.RawViewWalker.GetParent(node);if(parent!=null){parentPresent=true;parentId=Runtime(parent);if(Same(parentId,expectedParent)){var pc=parent.Current;parentPid=pc.ProcessId;parentType=pc.ControlType.Id;parentNative=pc.NativeWindowHandle;parentMetadataRead=true;}}}
    // Read only shape/interaction booleans from this exact rejected node. No Name/value,
    // subtree traversal, provider exception, or input authority follows these diagnostics.
    string control=BooleanProperty(node,AutomationElement.IsControlElementProperty),content=BooleanProperty(node,AutomationElement.IsContentElementProperty),enabled=BooleanProperty(node,AutomationElement.IsEnabledProperty);
    string focusable=BooleanProperty(node,AutomationElement.IsKeyboardFocusableProperty),focused=BooleanProperty(node,AutomationElement.HasKeyboardFocusProperty);
    bool rawChildAbsent=TreeWalker.RawViewWalker.GetFirstChild(node)==null;
    string nativeMetadata=NativeMismatch((IntPtr)native,popup,host);
    bool after=RootFence(host,testPid,hostTid,popup,browserPid,menuTid,browser,root,rootId)&&Same(Runtime(node),id)&&node.Current.ProcessId==providerPid;
    if(!after){json=prefix+"\"status\":\"stale-after-read-details-discarded\"}";}
    else{
     string kind=Same(id,rootId)?"root":type==ControlType.Menu.Id?"menu":type==ControlType.MenuItem.Id?"menu-item-unattributed":"structural-other";
     json=prefix+"\"status\":\"captured-nonatomic-not-authority\",\"root_identity_before_after\":true,\"node_kind\":\""+kind+"\",\"control_type_id\":"+Number(type)+",\"native_hwnd\":"+Number(native)+",\"native_window\":"+nativeMetadata+",\"uia_is_control_no_default\":"+control+",\"uia_is_content_no_default\":"+content+",\"uia_enabled_no_default\":"+enabled+",\"uia_keyboard_focusable_no_default\":"+focusable+",\"uia_has_keyboard_focus_no_default\":"+focused+",\"raw_child_absent\":"+(rawChildAbsent?"true":"false")+",\"pid_no_default_kind\":\""+pidNoDefaultKind+"\",\"pid_no_default_value\":"+pidNoDefaultValue+",\"runtime_id\":"+JsonIds(id)+",\"root_runtime_id\":"+JsonIds(rootId)+",\"parent_already_encountered\":"+(expectedParent!=null?"true":"false")+",\"expected_parent_runtime_id\":"+JsonIds(expectedParent)+",\"parent_present\":"+(parentPresent?"true":"false")+",\"parent_runtime_id\":"+JsonIds(parentId)+",\"parent_matches_enqueued_parent\":"+(expectedParent!=null&&parentId!=null&&Same(parentId,expectedParent)?"true":"false")+",\"parent_metadata_read\":"+(parentMetadataRead?"true":"false")+",\"parent_provider_pid\":"+Number(parentPid)+",\"parent_control_type_id\":"+Number(parentType)+",\"parent_native_hwnd\":"+Number(parentNative)+"}";
    }
   }
  }catch{ /* Getter failures remain diagnostic unavailable; never suppress the original rejection. */ }
  if(json.Length>8192)json=prefix+"\"status\":\"diagnostic-bound-exceeded\"}";
  try{System.IO.File.WriteAllText(System.IO.Path.Combine(directory,"native-provider-mismatch.json"),json,new UTF8Encoding(false));}catch{ /* Missing evidence is inconclusive, never permission to continue. */ }
 }
 // Cancellation-only owned compositor category. Optional UIA flags remain unknown; they
 // are not relabeled noncontrol/noncontent/nonfocusable. No foreign Name/value is read.
 [DllImport("kernel32.dll")] public static extern ulong GetTickCount64();
 static void OwnershipBudget(ulong deadline){if(deadline==0||GetTickCount64()>=deadline)throw new InvalidOperationException("Original shared ownership deadline expired.");}
 static T Required<T>(AutomationElement node,AutomationProperty property){
  object raw=node.GetCurrentPropertyValue(property,true);
  if(!(raw is T))throw new InvalidOperationException("Required no-default GPU-leaf property unavailable.");
  return (T)raw;
 }
 static string OptionalBoolean(AutomationElement node,AutomationProperty property){
  object raw=node.GetCurrentPropertyValue(property,true);
  if(!(raw is bool)&&!Object.ReferenceEquals(raw,AutomationElement.NotSupported))throw new InvalidOperationException("Optional GPU property returned unknown kind.");
  return BooleanValue(raw);
 }
 // Pure diagnostic representation only. Infinity/NaN are labeled strings, never zeros.
 public static string BoundsValue(object value){
  if(Object.ReferenceEquals(value,AutomationElement.NotSupported))return "{\"kind\":\"not-supported\",\"rectangle\":null}";
  if(!(value is System.Windows.Rect))return "{\"kind\":\"other-kind\",\"rectangle\":null}";
  var r=(System.Windows.Rect)value;
  Func<double,string> raw=n=>"\""+JsonText(n.ToString("R",System.Globalization.CultureInfo.InvariantCulture))+"\"";
  return "{\"kind\":\"rectangle-value\",\"rectangle\":{\"is_empty\":"+(r.IsEmpty?"true":"false")+",\"canonical_empty\":"+(r.X==Double.PositiveInfinity&&r.Y==Double.PositiveInfinity&&r.Width==Double.NegativeInfinity&&r.Height==Double.NegativeInfinity?"true":"false")+",\"all_finite\":"+(Finite(r.X)&&Finite(r.Y)&&Finite(r.Width)&&Finite(r.Height)?"true":"false")+",\"x\":"+raw(r.X)+",\"y\":"+raw(r.Y)+",\"width\":"+raw(r.Width)+",\"height\":"+raw(r.Height)+"}}";
 }
 static string IdentityProperty(AutomationElement node,AutomationProperty property,bool controlType){
  try{
   object raw=node.GetCurrentPropertyValue(property,true);
   if(Object.ReferenceEquals(raw,AutomationElement.NotSupported))return "{\"kind\":\"not-supported\",\"value\":null}";
   if(controlType&&raw is ControlType)return "{\"kind\":\"control-type\",\"value\":"+Number(((ControlType)raw).Id)+"}";
   if(!controlType&&raw is int)return "{\"kind\":\"integer\",\"value\":"+Number((int)raw)+"}";
   return "{\"kind\":\"other-kind\",\"value\":null}";
  }catch{return "{\"kind\":\"getter-failed\",\"value\":null}";}
 }
 static string RuntimeMetadata(AutomationElement node){
  try{return "{\"kind\":\"runtime-id\",\"value\":"+JsonIds(Runtime(node))+"}";}catch{return "{\"kind\":\"getter-failed\",\"value\":null}";}
 }
 static string NodeIdentityMetadata(AutomationElement node,bool inspectParent){
  string parent;if(!inspectParent){parent="{\"kind\":\"captured-root-parent-not-queried\",\"value\":null}";}
  else try{var p=TreeWalker.RawViewWalker.GetParent(node);parent=p==null?"{\"kind\":\"absent\",\"value\":null}":RuntimeMetadata(p);}catch{parent="{\"kind\":\"getter-failed\",\"value\":null}";}
  return "{\"runtime\":"+RuntimeMetadata(node)+",\"pid\":"+IdentityProperty(node,AutomationElement.ProcessIdProperty,false)+",\"type\":"+IdentityProperty(node,AutomationElement.ControlTypeProperty,true)+",\"hwnd\":"+IdentityProperty(node,AutomationElement.NativeWindowHandleProperty,false)+",\"parent_runtime\":"+parent+"}";
 }
 static void WriteBoundsFailure(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,uint menuTid,ReadOnlyBrowser browser,AutomationElement root,int[] rootId,AutomationElement node,int[] observedId,int[] parentId,int depth,int index,int observedPid,int observedType,int observedNative,System.Windows.Rect observedBox,bool observedGpuCategory,string directory){
  // The failed outer observation is retained separately, never replaced by a later getter.
  // This is failure-only metadata. The caller must still throw, without a retry or Escape.
  try{
   bool before=false,attempted=false;
   string beforeFence,afterFence="{\"kind\":\"not-attempted\",\"value\":null}";
   try{before=RootFence(host,testPid,hostTid,popup,browserPid,menuTid,browser,root,rootId);beforeFence=BooleanValue(before);}catch{beforeFence="{\"kind\":\"getter-failed\",\"value\":null}";}
   string later="{\"kind\":\"not-attempted\",\"rectangle\":null}";
   string identityBefore="{\"kind\":\"not-attempted\"}",identityAfter=identityBefore;
   if(before){
    identityBefore=NodeIdentityMetadata(node,parentId!=null);
    attempted=true;try{later=BoundsValue(node.GetCurrentPropertyValue(AutomationElement.BoundingRectangleProperty,true));}catch{later="{\"kind\":\"getter-failed\",\"rectangle\":null}";}
    identityAfter=NodeIdentityMetadata(node,parentId!=null);
    try{afterFence=BooleanValue(RootFence(host,testPid,hostTid,popup,browserPid,menuTid,browser,root,rootId));}catch{afterFence="{\"kind\":\"getter-failed\",\"value\":null}";}
   }
   string json="{\"scope\":\"failure-only-raw-bounds-not-input-authority\",\"non_atomic\":true,\"visited_count\":"+Number(index)+",\"depth\":"+Number(depth)+",\"observed_pid\":"+Number(observedPid)+",\"expected_browser_pid\":"+Number(browserPid)+",\"control_type_id\":"+Number(observedType)+",\"native_hwnd\":"+Number(observedNative)+",\"runtime_id\":"+JsonIds(observedId)+",\"root_runtime_id\":"+JsonIds(rootId)+",\"popup_hwnd\":"+Number(popup.ToInt64())+",\"expected_parent_runtime_id\":"+JsonIds(parentId)+",\"observed_category\":\""+(observedGpuCategory?"owned-gpu-category-at-outer-read":observedPid==(int)browserPid?"browser-pid-at-outer-read":"unproved-foreign")+"\",\"observed_outer_bounds\":"+BoundsValue(observedBox)+",\"root_before_later_query\":"+beforeFence+",\"later_no_default_query_attempted\":"+(attempted?"true":"false")+",\"later_no_default_bounds\":"+later+",\"node_identity_before_later_query\":"+identityBefore+",\"node_identity_after_later_query\":"+identityAfter+",\"root_after_later_query\":"+afterFence+"}";
   if(json.Length>8192)return;
   byte[] bytes=new UTF8Encoding(false,true).GetBytes(json);
   using(var file=new System.IO.FileStream(System.IO.Path.Combine(directory,"raw-bounds-failure.json"),System.IO.FileMode.CreateNew,System.IO.FileAccess.Write,System.IO.FileShare.Read)){file.Write(bytes,0,bytes.Length);}
  }catch{} // Optional metadata must not replace the original strict refusal.
 }
 static string Real(double value){if(!Finite(value))throw new InvalidOperationException("Nonfinite raw inventory bounds.");return value.ToString("R",System.Globalization.CultureInfo.InvariantCulture);}
 static bool NoPattern(AutomationElement node,AutomationPattern pattern){
  object current;bool present=node.TryGetCurrentPattern(pattern,out current);
  if(present||current!=null)throw new InvalidOperationException("Virtualized/container GPU node refused.");
  return true;
 }
 public sealed class NativeLeafFacts {
  public long Node,Popup,Host,Parent,Root,RootOwner,Owner,Child;public uint Pid,ExpectedPid,Thread;
  public int OwnerError,ChildError,Left,Top,Right,Bottom,PopupLeft,PopupTop,PopupRight,PopupBottom;
  public bool Alive,Visible,Enabled,PopupChild,BoundsAvailable;public string ClassName;
 }
 static bool NativeLeafPolicy(NativeLeafFacts f){
  return f!=null&&f.Node!=0&&f.Popup!=0&&f.Host!=0&&f.Pid!=0&&f.Pid==f.ExpectedPid&&f.Thread!=0
   &&f.Alive&&f.Visible&&!f.Enabled&&f.ClassName=="Intermediate D3D Window"
   &&f.Parent==f.Popup&&f.Root==f.Popup&&f.RootOwner==f.Host&&f.Owner==0&&f.OwnerError==0
   &&f.Child==0&&f.ChildError==0&&f.PopupChild&&f.BoundsAvailable&&f.Right>f.Left&&f.Bottom>f.Top
   &&f.Left>=f.PopupLeft&&f.Top>=f.PopupTop&&f.Right<=f.PopupRight&&f.Bottom<=f.PopupBottom;
 }
 public sealed class UiaLeafFacts {
  public bool RequiredAvailable,ParentMatches,RawChildAbsent,PatternsAbsent,Enabled,Focused;
  public int Depth,Pid,ExpectedPid,Native,Type;public double X,Y,Width,Height,PopupLeft,PopupTop,PopupRight,PopupBottom;
 }
 static bool UiaLeafPolicy(UiaLeafFacts f){
  return f!=null&&f.RequiredAvailable&&f.ParentMatches&&f.RawChildAbsent&&f.PatternsAbsent
   &&f.Depth==1&&f.Pid>0&&f.Pid==f.ExpectedPid&&f.Native>0&&f.Type==ControlType.Pane.Id&&!f.Enabled&&!f.Focused
   &&Finite(f.X)&&Finite(f.Y)&&Finite(f.Width)&&Finite(f.Height)&&f.Width>0&&f.Height>0
   &&f.X>=f.PopupLeft&&f.Y>=f.PopupTop&&f.X+f.Width<=f.PopupRight&&f.Y+f.Height<=f.PopupBottom;
 }
 public static void CheckOwnedLeafPolicy(){
  Func<NativeLeafFacts> native=()=>new NativeLeafFacts{Node=1,Popup=2,Host=3,Parent=2,Root=2,RootOwner=3,Pid=4,ExpectedPid=4,Thread=5,Alive=true,Visible=true,PopupChild=true,BoundsAvailable=true,ClassName="Intermediate D3D Window",Left=10,Top=10,Right=20,Bottom=20,PopupLeft=0,PopupTop=0,PopupRight=30,PopupBottom=30};
  if(!NativeLeafPolicy(native()))throw new InvalidOperationException("Synthetic valid native leaf shape refused.");
  Action<NativeLeafFacts>[] badNative={f=>f.Node=0,f=>f.Pid=6,f=>f.ExpectedPid=0,f=>f.Thread=0,f=>f.Alive=false,f=>f.Visible=false,f=>f.Enabled=true,f=>f.ClassName="unknown",f=>f.Parent=7,f=>f.Root=7,f=>f.RootOwner=7,f=>f.Owner=7,f=>f.OwnerError=5,f=>f.Child=7,f=>f.ChildError=5,f=>f.PopupChild=false,f=>f.BoundsAvailable=false,f=>f.Right=f.Left,f=>f.Bottom=f.Top,f=>f.Left=-1,f=>f.Top=-1,f=>f.Right=31,f=>f.Bottom=31};
  foreach(var mutate in badNative){var facts=native();mutate(facts);if(NativeLeafPolicy(facts))throw new InvalidOperationException("Synthetic invalid native leaf accepted.");}
  Func<UiaLeafFacts> uia=()=>new UiaLeafFacts{RequiredAvailable=true,ParentMatches=true,RawChildAbsent=true,PatternsAbsent=true,Depth=1,Pid=4,ExpectedPid=4,Native=1,Type=ControlType.Pane.Id,X=10,Y=10,Width=10,Height=10,PopupRight=30,PopupBottom=30};
  if(!UiaLeafPolicy(uia()))throw new InvalidOperationException("Synthetic valid UIA leaf shape refused.");
  Action<UiaLeafFacts>[] badUia={f=>f.RequiredAvailable=false,f=>f.ParentMatches=false,f=>f.RawChildAbsent=false,f=>f.PatternsAbsent=false,f=>f.Depth=0,f=>f.Depth=2,f=>f.Pid=6,f=>f.Native=0,f=>f.Type=ControlType.Menu.Id,f=>f.Type=ControlType.MenuItem.Id,f=>f.Enabled=true,f=>f.Focused=true,f=>f.X=Double.NaN,f=>f.Y=Double.PositiveInfinity,f=>f.Width=Double.NegativeInfinity,f=>f.Width=0,f=>f.Height=-1,f=>f.X=-1,f=>f.Y=-1,f=>f.Width=31,f=>f.Height=31};
  foreach(var mutate in badUia){var facts=uia();mutate(facts);if(UiaLeafPolicy(facts))throw new InvalidOperationException("Synthetic invalid UIA leaf accepted.");}
  var box=new System.Windows.Rect(10,10,10,10);int[] runtime={42,1},parent={42,2};
  var witness=new GpuLeafWitness(4,1,ControlType.Pane.Id,box,runtime,parent,"synthetic");
  if(!witness.Matches(4,1,ControlType.Pane.Id,box,runtime,parent,false,false))throw new InvalidOperationException("Synthetic exact outer GPU witness refused.");
  if(witness.Matches(5,1,ControlType.Pane.Id,box,runtime,parent,false,false)||witness.Matches(4,7,ControlType.Pane.Id,box,runtime,parent,false,false)||witness.Matches(4,1,ControlType.Menu.Id,box,runtime,parent,false,false)||witness.Matches(4,1,ControlType.MenuItem.Id,box,runtime,parent,false,false)||witness.Matches(4,1,ControlType.Pane.Id,new System.Windows.Rect(11,10,10,10),runtime,parent,false,false)||witness.Matches(4,1,ControlType.Pane.Id,box,new[]{42,7},parent,false,false)||witness.Matches(4,1,ControlType.Pane.Id,box,runtime,new[]{42,7},false,false)||witness.Matches(4,1,ControlType.Pane.Id,box,runtime,parent,true,false)||witness.Matches(4,1,ControlType.Pane.Id,box,runtime,parent,false,true))throw new InvalidOperationException("Synthetic inconsistent outer GPU witness accepted.");
 }
 static string GpuNativeShape(IntPtr node,IntPtr popup,IntPtr host,ReadOnlyBrowser gpu){
  uint pid;uint tid=GetWindowThreadProcessId(node,out pid);RECT r,p;
  if(node==IntPtr.Zero||!gpu.SameAlive())throw new InvalidOperationException("Retained GPU native leaf unavailable.");
  IntPtr parent=GetParent(node),root=GetAncestor(node,2),rootOwner=GetAncestor(node,3);
  DiagnosticSetLastError(0);IntPtr owner=DiagnosticGetWindow(node,4);int ownerError=Marshal.GetLastWin32Error();
  DiagnosticSetLastError(0);IntPtr child=DiagnosticGetWindow(node,5);int childError=Marshal.GetLastWin32Error();
  bool nodeBounds=GetWindowRect(node,out r),popupBounds=GetWindowRect(popup,out p);
  var facts=new NativeLeafFacts{Node=node.ToInt64(),Popup=popup.ToInt64(),Host=host.ToInt64(),Pid=pid,ExpectedPid=gpu.Pid,Thread=tid,Parent=parent.ToInt64(),Root=root.ToInt64(),RootOwner=rootOwner.ToInt64(),Owner=owner.ToInt64(),OwnerError=ownerError,Child=child.ToInt64(),ChildError=childError,Alive=IsWindow(node),Visible=IsWindowVisible(node),Enabled=IsWindowEnabled(node),ClassName=Class(node),PopupChild=IsChild(popup,node),BoundsAvailable=nodeBounds&&popupBounds,Left=r.Left,Top=r.Top,Right=r.Right,Bottom=r.Bottom,PopupLeft=p.Left,PopupTop=p.Top,PopupRight=p.Right,PopupBottom=p.Bottom};
  if(!NativeLeafPolicy(facts))throw new InvalidOperationException("GPU native parent/owner/leaf/bounds proof unavailable.");
  uint after;uint afterTid=GetWindowThreadProcessId(node,out after);
  if(after!=pid||afterTid!=tid||!IsWindow(node)||Class(node)!="Intermediate D3D Window"||!gpu.SameAlive())throw new InvalidOperationException("GPU native identity changed.");
  return Number(node.ToInt64())+":"+Number(pid)+":"+Number(tid)+":"+Number(parent.ToInt64())+":"+Number(root.ToInt64())+":"+Number(rootOwner.ToInt64())+":0:0:0:0:"+Number(r.Left)+":"+Number(r.Top)+":"+Number(r.Right)+":"+Number(r.Bottom);
 }
 sealed class GpuLeafWitness {
  public readonly int Pid,Native,Type;public readonly System.Windows.Rect Bounds;public readonly string Signature;
  readonly int[] runtime,parent;
  public GpuLeafWitness(int pid,int native,int type,System.Windows.Rect bounds,int[] id,int[] parentId,string signature){Pid=pid;Native=native;Type=type;Bounds=bounds;runtime=(int[])id.Clone();parent=(int[])parentId.Clone();Signature=signature;}
  public bool Matches(int pid,int native,int type,System.Windows.Rect bounds,int[] id,int[] parentId,bool enabled,bool focused){
   return Pid==pid&&Native==native&&Type==type&&type==ControlType.Pane.Id&&Bounds==bounds&&Same(runtime,id)&&Same(parent,parentId)&&!enabled&&!focused;
  }
 }
 static void RecheckGpuTyped(AutomationElement node,GpuLeafWitness proof){
  var parent=TreeWalker.RawViewWalker.GetParent(node);if(parent==null)throw new InvalidOperationException("GPU typed parent disappeared.");
  if(!proof.Matches(Required<int>(node,AutomationElement.ProcessIdProperty),Required<int>(node,AutomationElement.NativeWindowHandleProperty),Required<ControlType>(node,AutomationElement.ControlTypeProperty).Id,Required<System.Windows.Rect>(node,AutomationElement.BoundingRectangleProperty),Runtime(node),Runtime(parent),Required<bool>(node,AutomationElement.IsEnabledProperty),Required<bool>(node,AutomationElement.HasKeyboardFocusProperty))||TreeWalker.RawViewWalker.GetFirstChild(node)!=null)throw new InvalidOperationException("GPU typed identity changed after native reads.");
 }
 static GpuLeafWitness GpuShape(IntPtr host,IntPtr popup,AutomationElement node,int depth,int[] expectedParent,int[] rootId,uint browserPid,ReadOnlyBrowser gpu,ulong deadline){
  OwnershipBudget(deadline);
  if(gpu==null||depth!=1||expectedParent==null||!Same(expectedParent,rootId)||!gpu.SameAlive())throw new InvalidOperationException("No retained exact GPU leaf authority.");
  int pid=Required<int>(node,AutomationElement.ProcessIdProperty),native=Required<int>(node,AutomationElement.NativeWindowHandleProperty);
  ControlType type=Required<ControlType>(node,AutomationElement.ControlTypeProperty);
  bool enabled=Required<bool>(node,AutomationElement.IsEnabledProperty),focused=Required<bool>(node,AutomationElement.HasKeyboardFocusProperty);
  var bounds=Required<System.Windows.Rect>(node,AutomationElement.BoundingRectangleProperty);
  RECT popupBounds;if(!GetWindowRect(popup,out popupBounds))throw new InvalidOperationException("GPU popup bounds query failed.");
  var parent=TreeWalker.RawViewWalker.GetParent(node);
  bool parentMatches=parent!=null&&Same(Runtime(parent),rootId)&&parent.Current.ProcessId==(int)browserPid&&parent.Current.NativeWindowHandle==popup.ToInt32();
  bool noChild=TreeWalker.RawViewWalker.GetFirstChild(node)==null;
  bool noVirtualization=NoPattern(node,ItemContainerPattern.Pattern)&&NoPattern(node,VirtualizedItemPattern.Pattern);
  var facts=new UiaLeafFacts{RequiredAvailable=true,ParentMatches=parentMatches,RawChildAbsent=noChild,PatternsAbsent=noVirtualization,Depth=depth,Pid=pid,ExpectedPid=(int)gpu.Pid,Native=native,Type=type.Id,Enabled=enabled,Focused=focused,X=bounds.X,Y=bounds.Y,Width=bounds.Width,Height=bounds.Height,PopupLeft=popupBounds.Left,PopupTop=popupBounds.Top,PopupRight=popupBounds.Right,PopupBottom=popupBounds.Bottom};
  if(!UiaLeafPolicy(facts))throw new InvalidOperationException("GPU node is not an exact disabled/unfocused direct raw root leaf.");
  string optional=OptionalBoolean(node,AutomationElement.IsControlElementProperty)+":"+OptionalBoolean(node,AutomationElement.IsContentElementProperty)+":"+OptionalBoolean(node,AutomationElement.IsKeyboardFocusableProperty);
  var id=Runtime(node);var parentId=Runtime(parent);
  string nativeProof=GpuNativeShape((IntPtr)native,popup,host,gpu);
  string signature=JsonIds(id)+":"+Number(pid)+":"+Number(native)+":"+Number(type.Id)+":"+Real(bounds.X)+":"+Real(bounds.Y)+":"+Real(bounds.Width)+":"+Real(bounds.Height)+":disabled:unfocused:no-virtualization:"+optional+":"+nativeProof;
  var proof=new GpuLeafWitness(pid,native,type.Id,bounds,id,parentId,signature);
  RecheckGpuTyped(node,proof);NoPattern(node,ItemContainerPattern.Pattern);NoPattern(node,VirtualizedItemPattern.Pattern);
  OwnershipBudget(deadline);return proof;
 }
 static GpuLeafWitness ReadGpuLeaf(IntPtr host,IntPtr popup,AutomationElement node,int depth,int[] expectedParent,int[] rootId,uint browserPid,ReadOnlyBrowser gpu,ulong deadline){
  var before=GpuShape(host,popup,node,depth,expectedParent,rootId,browserPid,gpu,deadline);
  var after=GpuShape(host,popup,node,depth,expectedParent,rootId,browserPid,gpu,deadline);
  if(before.Signature!=after.Signature)throw new InvalidOperationException("GPU leaf proof changed across reads.");return before;
 }
 static bool SameOptional(int[] a,int[] b){return a==null||b==null?a==null&&b==null:Same(a,b);}
 static bool CanonicalEmpty(System.Windows.Rect r){return r.IsEmpty&&r.X==Double.PositiveInfinity&&r.Y==Double.PositiveInfinity&&r.Width==Double.NegativeInfinity&&r.Height==Double.NegativeInfinity;}
 sealed class BrowserNodeWitness {
  public readonly int Pid,Type,Native,Depth;public readonly System.Windows.Rect Bounds;
  public readonly bool NativeUnknown,BoundsUnknown;public readonly string NativeToken,BoundsToken,Signature;
  readonly int[] id,parent;
  public BrowserNodeWitness(int pid,int type,int native,System.Windows.Rect bounds,int depth,int[] runtime,int[] parentId,bool nativeUnknown,bool boundsUnknown){
   Pid=pid;Type=type;Native=native;Bounds=bounds;Depth=depth;id=(int[])runtime.Clone();parent=parentId==null?null:(int[])parentId.Clone();NativeUnknown=nativeUnknown;BoundsUnknown=boundsUnknown;
   NativeToken=nativeUnknown?"not-supported":"integer:"+Number(native);
   BoundsToken=boundsUnknown?"not-supported":"finite:"+Real(bounds.X)+":"+Real(bounds.Y)+":"+Real(bounds.Width)+":"+Real(bounds.Height);
   Signature="browser:"+Number(pid)+":"+Number(type)+":"+Number(depth)+":"+JsonIds(id)+":"+JsonIds(parent)+":"+NativeToken+":"+BoundsToken+((nativeUnknown||boundsUnknown)?":structural-pane-unknown-no-virtualization":":supported-values");
  }
  public int[] Parent(){return parent==null?null:(int[])parent.Clone();}
  public int[] Id(){return (int[])id.Clone();}
  public bool MatchesOuter(int pid,int type,int native,System.Windows.Rect bounds,int[] runtime,int[] parentId){
   return pid==Pid&&type==Type&&Same(id,runtime)&&SameOptional(parent,parentId)
    &&(NativeUnknown?native==0:native==Native)&&(BoundsUnknown?CanonicalEmpty(bounds):bounds==Bounds);
  }
 }
 static BrowserNodeWitness CreateBrowserWitness(uint browserPid,int depth,int[] id,int[] parent,object pidValue,object typeValue,object nativeValue,object boundsValue,bool patternsAbsent){
  if(!(pidValue is int)||!(typeValue is ControlType)||browserPid==0||(int)pidValue<=0||(uint)(int)pidValue!=browserPid||id==null||id.Length==0||id.Length>32||depth<0||depth>128||(depth==0)!=(parent==null)||(parent!=null&&(parent.Length==0||parent.Length>32)))throw new InvalidOperationException("Browser node supported identity/parent unavailable.");
  int pid=(int)pidValue,type=((ControlType)typeValue).Id;
  bool nativeUnknown=Object.ReferenceEquals(nativeValue,AutomationElement.NotSupported),boundsUnknown=Object.ReferenceEquals(boundsValue,AutomationElement.NotSupported);
  if(nativeUnknown||boundsUnknown){
   if(!nativeUnknown||type!=ControlType.Pane.Id||depth==0||parent==null||!patternsAbsent)throw new InvalidOperationException("Unsupported browser geometry is not a counted non-root structural Pane category.");
   if(boundsUnknown)return new BrowserNodeWitness(pid,type,0,System.Windows.Rect.Empty,depth,id,parent,true,true);
   if(!(boundsValue is System.Windows.Rect))throw new InvalidOperationException("Browser Pane bounds getter returned unknown kind.");
   var paneBounds=(System.Windows.Rect)boundsValue;
   if(!Finite(paneBounds.X)||!Finite(paneBounds.Y)||!Finite(paneBounds.Width)||!Finite(paneBounds.Height)||paneBounds.Width<=0||paneBounds.Height<=0)throw new InvalidOperationException("Mixed unknown-HWND browser Pane requires supported finite non-empty bounds.");
   return new BrowserNodeWitness(pid,type,0,paneBounds,depth,id,parent,true,false);
  }
  if(!(nativeValue is int)||!(boundsValue is System.Windows.Rect))throw new InvalidOperationException("Browser geometry getter returned unknown kind.");
  var bounds=(System.Windows.Rect)boundsValue;
  if(!Finite(bounds.X)||!Finite(bounds.Y)||!Finite(bounds.Width)||!Finite(bounds.Height))throw new InvalidOperationException("Supported browser geometry is nonfinite; no Empty exception.");
  return new BrowserNodeWitness(pid,type,(int)nativeValue,bounds,depth,id,parent,false,false);
 }
 static BrowserNodeWitness CaptureBrowserNode(AutomationElement node,uint browserPid,int depth,int[] expectedParent,ReadOnlyBrowser browser){
  // No-default values are evidence of availability, never replaced by Current defaults.
  object pid=node.GetCurrentPropertyValue(AutomationElement.ProcessIdProperty,true),type=node.GetCurrentPropertyValue(AutomationElement.ControlTypeProperty,true);
  object native=node.GetCurrentPropertyValue(AutomationElement.NativeWindowHandleProperty,true),bounds=node.GetCurrentPropertyValue(AutomationElement.BoundingRectangleProperty,true);
  var id=Runtime(node);int[] parentId=null;
  if(expectedParent!=null){var parent=TreeWalker.RawViewWalker.GetParent(node);if(parent==null||!Same(Runtime(parent),expectedParent)||Required<int>(parent,AutomationElement.ProcessIdProperty)!=(int)browserPid)throw new InvalidOperationException("Browser structural parent changed or is foreign.");parentId=Runtime(parent);}
  bool unknown=Object.ReferenceEquals(native,AutomationElement.NotSupported)||Object.ReferenceEquals(bounds,AutomationElement.NotSupported);
  bool patternsAbsent=unknown&&type is ControlType&&((ControlType)type).Id==ControlType.Pane.Id&&depth>0&&NoPattern(node,ItemContainerPattern.Pattern)&&NoPattern(node,VirtualizedItemPattern.Pattern);
  var proof=CreateBrowserWitness(browserPid,depth,id,parentId,pid,type,native,bounds,patternsAbsent);
  if(!browser.SameAlive())throw new InvalidOperationException("Retained browser changed during typed structure read.");return proof;
 }
 static bool SameBrowserWitness(BrowserNodeWitness before,BrowserNodeWitness after){return before!=null&&after!=null&&before.Signature==after.Signature;}
 static void RecheckBrowserNode(AutomationElement node,BrowserNodeWitness proof,uint browserPid,ReadOnlyBrowser browser){
  var fresh=CaptureBrowserNode(node,browserPid,proof.Depth,proof.Parent(),browser);
  if(!SameBrowserWitness(proof,fresh))throw new InvalidOperationException("Typed browser node/availability/parent changed across reads.");
 }
 static BrowserNodeWitness CountedAncestor(Dictionary<string,BrowserNodeWitness> nodes,int[] runtime){
  BrowserNodeWitness proof;if(!nodes.TryGetValue(JsonIds(runtime),out proof)||proof==null||!Same(proof.Id(),runtime))throw new InvalidOperationException("Ancestor absent from exact counted typed inventory.");return proof;
 }
 static bool IsBrowserRow(BrowserNodeWitness node,int type){return node!=null&&!node.NativeUnknown&&!node.BoundsUnknown&&node.Type==type;}
 static bool MatchesParentAdvance(int[] expected,int[] observed){return expected!=null&&observed!=null&&Same(expected,observed);}
 static System.Windows.Rect FixtureBounds(BrowserNodeWitness node){
  if(!IsBrowserRow(node,ControlType.MenuItem.Id)||!Finite(node.Bounds.X)||!Finite(node.Bounds.Y)||!Finite(node.Bounds.Width)||!Finite(node.Bounds.Height)||node.Bounds.Width<=0||node.Bounds.Height<=0)throw new InvalidOperationException("Fixture geometry requires its supported counted MenuItem witness.");return node.Bounds;
 }
 static BrowserNodeWitness CountedParent(Dictionary<string,BrowserNodeWitness> nodes,BrowserNodeWitness child,int[] observedParent){
  if(child==null||!MatchesParentAdvance(child.Parent(),observedParent))throw new InvalidOperationException("Ancestry advance differs from counted exact parent.");
  var parent=CountedAncestor(nodes,observedParent);if(parent.Pid!=child.Pid)throw new InvalidOperationException("Counted ancestor PID differs.");return parent;
 }
 static bool InventoryCapacity(int examined,int queued){return examined>0&&examined<=128&&queued>=0&&examined+queued<=128;}
 static void AppendInventoryNode(StringBuilder inventory,int index,int depth,int[] parent,int[] runtime,int pid,int type,string native,string bounds,string proof){
  inventory.Append(Number(index)).Append(':').Append(Number(depth)).Append(':').Append(JsonIds(parent)).Append(':').Append(JsonIds(runtime)).Append(':').Append(Number(pid)).Append(':').Append(Number(type)).Append(':').Append(native).Append(':').Append(bounds).Append(':').Append(proof).Append('\n');
  if(inventory.Length>65536)throw new InvalidOperationException("Complete raw inventory exceeds bound.");
 }
 static string SyntheticBrowserInventory(params BrowserNodeWitness[] nodes){
  var result=new StringBuilder();var seen=new HashSet<string>(StringComparer.Ordinal);int count=0;
  foreach(var node in nodes){if(!InventoryCapacity(++count,0)||!seen.Add(JsonIds(node.Id())))throw new InvalidOperationException("Synthetic duplicate/cycle/cap refused by inventory predicate.");AppendInventoryNode(result,count,node.Depth,node.Parent(),node.Id(),node.Pid,node.Type,node.NativeToken,node.BoundsToken,node.Signature);}return result.ToString();
 }
 // Pure typed/wire/ancestry policy only; no provider, HWND, process, filesystem or input.
 public static void CheckBrowserStructurePolicy(){
  var id=new[]{42,6};var parent=new[]{42,4};var missing=AutomationElement.NotSupported;var finite=new System.Windows.Rect(1,2,3,4);
  var unknown=CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,missing,missing,true);
  var mixed=CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,missing,finite,true);
  var supported=CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,0,finite,false);
  if(!unknown.MatchesOuter(10,ControlType.Pane.Id,0,System.Windows.Rect.Empty,id,parent)||!mixed.MatchesOuter(10,ControlType.Pane.Id,0,finite,id,parent)||!supported.MatchesOuter(10,ControlType.Pane.Id,0,finite,id,parent)||!mixed.NativeUnknown||mixed.BoundsUnknown||mixed.NativeToken!="not-supported"||!mixed.BoundsToken.StartsWith("finite:",StringComparison.Ordinal)||unknown.NativeToken!="not-supported"||unknown.BoundsToken!="not-supported"||supported.NativeToken!="integer:0"||!supported.BoundsToken.StartsWith("finite:",StringComparison.Ordinal))throw new InvalidOperationException("Synthetic exact browser witness/tag rejected.");
  foreach(Func<BrowserNodeWitness> bad in new Func<BrowserNodeWitness>[]{
   ()=>CreateBrowserWitness(10,2,id,parent,missing,ControlType.Pane,missing,missing,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,missing,missing,missing,true),
   ()=>CreateBrowserWitness(10,2,id,parent,11,ControlType.Pane,missing,missing,true),
   ()=>CreateBrowserWitness(10,0,id,null,10,ControlType.Pane,missing,missing,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Menu,missing,missing,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.MenuItem,missing,missing,true),
   ()=>CreateBrowserWitness(10,2,id,parent,20,ControlType.Pane,missing,missing,true),
   ()=>CreateBrowserWitness(10,2,id,parent,null,ControlType.Pane,missing,missing,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,null,missing,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,missing,new object(),true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,0,missing,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,missing,finite,false),
   ()=>CreateBrowserWitness(10,0,id,null,10,ControlType.Pane,missing,finite,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Menu,missing,finite,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.MenuItem,missing,finite,true),
   ()=>CreateBrowserWitness(10,2,id,parent,11,ControlType.Pane,missing,finite,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,missing,System.Windows.Rect.Empty,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,missing,new System.Windows.Rect(1,2,0,4),true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,missing,new System.Windows.Rect(Double.NaN,2,3,4),true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,missing,new System.Windows.Rect(Double.PositiveInfinity,2,3,4),true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,0,System.Windows.Rect.Empty,true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,0,new System.Windows.Rect(Double.NaN,2,3,4),true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,0,new System.Windows.Rect(Double.PositiveInfinity,2,3,4),true),
   ()=>CreateBrowserWitness(10,2,id,parent,10,ControlType.Pane,missing,missing,false),
   ()=>CreateBrowserWitness(10,2,id,null,10,ControlType.Pane,missing,missing,true),
   ()=>CreateBrowserWitness(10,2,new int[0],parent,10,ControlType.Pane,missing,missing,true)
  }){bool refused=false;try{bad();}catch(InvalidOperationException){refused=true;}if(!refused)throw new InvalidOperationException("Synthetic unproved browser shape accepted.");}
  if(unknown.MatchesOuter(11,ControlType.Pane.Id,0,System.Windows.Rect.Empty,id,parent)||unknown.MatchesOuter(10,ControlType.Menu.Id,0,System.Windows.Rect.Empty,id,parent)||unknown.MatchesOuter(10,ControlType.Pane.Id,7,System.Windows.Rect.Empty,id,parent)||unknown.MatchesOuter(10,ControlType.Pane.Id,0,finite,id,parent)||unknown.MatchesOuter(10,ControlType.Pane.Id,0,System.Windows.Rect.Empty,new[]{42,7},parent)||unknown.MatchesOuter(10,ControlType.Pane.Id,0,System.Windows.Rect.Empty,id,new[]{42,5})||supported.MatchesOuter(10,ControlType.Pane.Id,1,finite,id,parent)||supported.MatchesOuter(10,ControlType.Pane.Id,0,new System.Windows.Rect(2,2,3,4),id,parent))throw new InvalidOperationException("Synthetic inconsistent typed/outer browser witness accepted.");
  if(mixed.MatchesOuter(10,ControlType.Pane.Id,1,finite,id,parent)||mixed.MatchesOuter(10,ControlType.Pane.Id,0,new System.Windows.Rect(2,2,3,4),id,parent)||mixed.MatchesOuter(11,ControlType.Pane.Id,0,finite,id,parent)||mixed.MatchesOuter(10,ControlType.MenuItem.Id,0,finite,id,parent)||mixed.MatchesOuter(10,ControlType.Pane.Id,0,finite,new[]{42,7},parent)||mixed.MatchesOuter(10,ControlType.Pane.Id,0,finite,id,new[]{42,5}))throw new InvalidOperationException("Synthetic inconsistent mixed browser witness accepted.");
  var root=CreateBrowserWitness(10,0,new[]{42,1},null,10,ControlType.Window,123,new System.Windows.Rect(0,0,100,100),false);
  var baseline=SyntheticMenuProof(SyntheticBrowserInventory(root,unknown));
  var changedParent=CreateBrowserWitness(10,2,id,new[]{42,5},10,ControlType.Pane,missing,missing,true);
  foreach(string changed in new[]{SyntheticBrowserInventory(root,supported),SyntheticBrowserInventory(root,changedParent),SyntheticBrowserInventory(root),SyntheticBrowserInventory(unknown,root)}){
   var fresh=SyntheticMenuProof(changed);if(SameProof(baseline,fresh)||InventoryDigest(baseline)==InventoryDigest(fresh))throw new InvalidOperationException("Synthetic real serializer unknown/known/count/order/parent change accepted.");
  }
  var mixedBaseline=SyntheticMenuProof(SyntheticBrowserInventory(root,mixed));
  foreach(var changed in new[]{unknown,supported}){
   var fresh=SyntheticMenuProof(SyntheticBrowserInventory(root,changed));
   if(SameBrowserWitness(mixed,changed)||SameProof(mixedBaseline,fresh)||InventoryDigest(mixedBaseline)==InventoryDigest(fresh))throw new InvalidOperationException("Synthetic mixed property availability change accepted.");
  }
  var map=new Dictionary<string,BrowserNodeWitness>{{JsonIds(id),unknown}};
  if(!Object.ReferenceEquals(CountedAncestor(map,id),unknown))throw new InvalidOperationException("Synthetic ancestry did not return its counted witness.");
  foreach(Action bad in new Action[]{()=>CountedAncestor(map,new[]{42,7}),()=>{var wrong=new Dictionary<string,BrowserNodeWitness>{{JsonIds(id),root}};CountedAncestor(wrong,id);},()=>SyntheticBrowserInventory(root,root)}){bool refused=false;try{bad();}catch(InvalidOperationException){refused=true;}if(!refused)throw new InvalidOperationException("Synthetic missing/wrong/duplicate ancestor accepted.");}
  if(IsBrowserRow(unknown,ControlType.Menu.Id)||IsBrowserRow(unknown,ControlType.MenuItem.Id)||IsBrowserRow(mixed,ControlType.Menu.Id)||IsBrowserRow(mixed,ControlType.MenuItem.Id)||IsBrowserRow(null,ControlType.MenuItem.Id)||!MatchesParentAdvance(parent,parent)||MatchesParentAdvance(parent,new[]{42,5})||MatchesParentAdvance(null,parent)||MatchesParentAdvance(parent,null))throw new InvalidOperationException("Synthetic unknown command or changed ancestry advance accepted.");
  var item=CreateBrowserWitness(10,2,new[]{42,8},parent,10,ControlType.MenuItem,0,finite,false);
  if(!IsBrowserRow(item,ControlType.MenuItem.Id)||FixtureBounds(item)!=finite)throw new InvalidOperationException("Synthetic supported fixture bounds rejected.");
  var middle=CreateBrowserWitness(10,1,parent,root.Id(),10,ControlType.Pane,0,finite,false);
  var chain=new Dictionary<string,BrowserNodeWitness>{{JsonIds(root.Id()),root},{JsonIds(parent),middle},{JsonIds(id),unknown}};
  if(!Object.ReferenceEquals(CountedParent(chain,unknown,parent),middle))throw new InvalidOperationException("Synthetic counted parent not resolved exactly.");
  foreach(Action bad in new Action[]{()=>FixtureBounds(unknown),()=>FixtureBounds(mixed),()=>FixtureBounds(supported),()=>FixtureBounds(root),()=>CountedParent(chain,unknown,null),()=>CountedParent(chain,unknown,root.Id()),()=>CountedParent(chain,unknown,id),()=>CountedParent(chain,unknown,new[]{42,99})}){bool refused=false;try{bad();}catch(InvalidOperationException){refused=true;}if(!refused)throw new InvalidOperationException("Synthetic unknown fixture geometry or null/wrong/skipped counted edge accepted.");}
  if(!InventoryCapacity(128,0)||InventoryCapacity(129,0)||InventoryCapacity(128,1)||InventoryCapacity(0,0))throw new InvalidOperationException("Synthetic inventory count/cap policy changed.");
  var capped=new BrowserNodeWitness[129];for(int n=0;n<capped.Length;n++)capped[n]=CreateBrowserWitness(10,1,new[]{42,300+n},new[]{42,1},10,ControlType.Pane,0,finite,false);
  bool capRefused=false;try{SyntheticBrowserInventory(capped);}catch(InvalidOperationException){capRefused=true;}if(!capRefused)throw new InvalidOperationException("Synthetic real serializer accepted129 raw nodes.");
 }
 public static string InventoryDigest(MenuProof proof){
  if(proof==null||proof.Inventory==null||proof.Inventory.Length==0||proof.Inventory.Length>65536)throw new InvalidOperationException("Complete raw inventory outside digest bound.");
  using(var sha=System.Security.Cryptography.SHA256.Create()){return BitConverter.ToString(sha.ComputeHash(Encoding.UTF8.GetBytes(proof.Inventory))).Replace("-","").ToLowerInvariant();}
 }
 public static string InventoryReceiptValue(MenuProof proof,string phase,string nonce,string generation,string requestDigest){
  if(phase!="first-complete"&&phase!="fresh-before-escape")throw new InvalidOperationException("Inventory evidence phase unavailable.");
  Func<string,string> unavailable=reason=>"{\"phase\":\""+phase+"\",\"capture_status\":\"unavailable\",\"reason\":\""+reason+"\"}";
  if(proof==null)return unavailable("not-captured");
  if(proof.Inventory==null||proof.Inventory.Length==0||proof.Inventory.Length>65536||!proof.Inventory.EndsWith("\n",StringComparison.Ordinal)||proof.Window==IntPtr.Zero||proof.RootId==null||proof.RootId.Length==0||proof.RootId.Length>32||proof.GpuLeaves<0||proof.GpuLeaves>1)return unavailable("invalid-bound-or-completeness");
  string[] rows=proof.Inventory.Split('\n');int count=rows.Length-1;if(count==0||count>128||rows[count]!="")return unavailable("incomplete-row-count");
  for(int n=0;n<count;n++)if(!rows[n].StartsWith(Number(n+1)+":",StringComparison.Ordinal))return unavailable("incomplete-row-order");
  bool bound=!String.IsNullOrEmpty(nonce)&&!String.IsNullOrEmpty(generation);
  if((String.IsNullOrEmpty(nonce)!=String.IsNullOrEmpty(generation))||(!bound&&proof.GpuLeaves!=0))return unavailable("source-binding-unavailable");
  string nonceJson=bound?"\""+ProtocolNonce(nonce)+"\"":"null",generationJson=bound?ProtocolProof(generation):"null";
  if(String.IsNullOrEmpty(requestDigest))requestDigest=null;
  if(requestDigest!=null&&!System.Text.RegularExpressions.Regex.IsMatch(requestDigest,"^[0-9a-f]{64}$"))return unavailable("request-digest-unavailable");
  string digest=InventoryDigest(proof);
  return "{\"phase\":\""+phase+"\",\"capture_status\":\"complete\",\"node_count\":"+Number(count)+",\"gpu_leaves\":"+Number(proof.GpuLeaves)+",\"run_nonce\":"+nonceJson+",\"source_proof\":"+generationJson+",\"source_binding\":\""+(bound?"provided-original-source":"not-provided-browser-only")+"\",\"popup\":\""+Number(proof.Window.ToInt64())+"\",\"root_runtime\":"+JsonIds(proof.RootId)+",\"utf8_sha256\":\""+digest+"\",\"membership_request_digest\":"+(requestDigest==null?"null":"\""+requestDigest+"\"")+",\"matches_issued_request_digest\":"+(requestDigest==null?"null":requestDigest==digest?"true":"false")+",\"inventory\":\""+JsonText(proof.Inventory)+"\"}";
 }
 static string ProtocolProof(string proof){
  string[] parts=proof.Split(',');if(parts.Length!=3)throw new InvalidOperationException("Source generation triple unavailable.");
  foreach(string part in parts){ulong value;if(!UInt64.TryParse(part,System.Globalization.NumberStyles.None,System.Globalization.CultureInfo.InvariantCulture,out value)||value.ToString(System.Globalization.CultureInfo.InvariantCulture)!=part)throw new InvalidOperationException("Source generation is not canonical.");}
  return "[\""+String.Join("\",\"",parts)+"\"]";
 }
 static string ProtocolNonce(string nonce){if(!System.Text.RegularExpressions.Regex.IsMatch(nonce,"^[0-9A-Za-z-]{16,128}$"))throw new InvalidOperationException("Run nonce unavailable.");return nonce;}
 static string ProtocolEnvironment(string identity){ulong value;if(!UInt64.TryParse(identity,System.Globalization.NumberStyles.None,System.Globalization.CultureInfo.InvariantCulture,out value)||value==0||value.ToString(System.Globalization.CultureInfo.InvariantCulture)!=identity)throw new InvalidOperationException("Retained environment identity unavailable.");return identity;}
 static string MembershipFields(MenuProof proof,string nonce,string generation){
  return "\"nonce\":\""+ProtocolNonce(nonce)+"\",\"phase\":\"pre-escape\",\"source_proof\":"+ProtocolProof(generation)+",\"popup\":\""+Number(proof.Window.ToInt64())+"\",\"root_runtime\":\""+String.Join(",",Array.ConvertAll(proof.RootId,n=>Number(n)))+"\",\"inventory_digest\":\""+InventoryDigest(proof)+"\"";
 }
 public static string MembershipRequest(MenuProof proof,string nonce,string generation){
  return "{\"schema\":\"bowser-gpu-menu-membership-request-v1\","+MembershipFields(proof,nonce,generation)+"}";
 }
 public static string MembershipResponse(MenuProof proof,string nonce,string generation,uint browser,uint gpu,ulong browserCreated,ulong gpuCreated,string environment,ulong deadline){
  return "{\"schema\":\"bowser-gpu-menu-membership-response-v1\",\"request_schema\":\"bowser-gpu-menu-membership-request-v1\","+MembershipFields(proof,nonce,generation)+",\"browser_pid\":\""+Number(browser)+"\",\"gpu_pid\":\""+Number(gpu)+"\",\"browser_creation\":\""+browserCreated.ToString(System.Globalization.CultureInfo.InvariantCulture)+"\",\"gpu_creation\":\""+gpuCreated.ToString(System.Globalization.CultureInfo.InvariantCulture)+"\",\"environment_identity\":\""+ProtocolEnvironment(environment)+"\",\"deadline_tick\":\""+deadline.ToString(System.Globalization.CultureInfo.InvariantCulture)+"\",\"status\":\"fresh-exact-retained-membership\"}";
 }
 public static bool ExactMembershipResponse(string received,string expected){return received!=null&&received.Length>0&&received.Length<=8192&&String.Equals(received,expected,StringComparison.Ordinal);}
 public static void WriteMembershipRequest(string directory,string request){
  if(request.Length==0||request.Length>8192)throw new InvalidOperationException("Membership request outside bound.");
  string pending=System.IO.Path.Combine(directory,"gpu-membership-request.pending"),path=System.IO.Path.Combine(directory,"gpu-membership-request.json");
  if(System.IO.File.Exists(path)||System.IO.File.Exists(System.IO.Path.Combine(directory,"gpu-membership-response.json"))||System.IO.File.Exists(System.IO.Path.Combine(directory,"gpu-membership-response.pending")))throw new InvalidOperationException("Preexisting/replayed membership protocol file.");
  byte[] bytes=new UTF8Encoding(false,true).GetBytes(request);
  using(var file=new System.IO.FileStream(pending,System.IO.FileMode.CreateNew,System.IO.FileAccess.Write,System.IO.FileShare.None)){file.Write(bytes,0,bytes.Length);file.Flush(true);}
  System.IO.File.Move(pending,path);
 }
 public static string ReadMembershipResponse(string path){
  byte[] bytes=new byte[8193];int count=0;
  using(var file=new System.IO.FileStream(path,System.IO.FileMode.Open,System.IO.FileAccess.Read,System.IO.FileShare.Read)){
   while(count<bytes.Length){int read=file.Read(bytes,count,bytes.Length-count);if(read==0)break;count+=read;}
  }
  if(count==0||count>8192)throw new InvalidOperationException("Membership response outside bound.");
  return new UTF8Encoding(false,true).GetString(bytes,0,count);
 }
 // Synthetic policy tests only: no provider, process, HWND, filesystem or input is queried.
 // The actual two native lanes must prove a real owned leaf and cancellation independently.
 public static void CheckMembershipPolicy(){
  var proof=new MenuProof{Window=(IntPtr)123,RootId=new[]{42,123},Inventory="synthetic-complete-inventory",GpuLeaves=1};
  const string nonce="synthetic-run-123456";string expected=MembershipResponse(proof,nonce,"1,2,0",10,20,30,40,"50",60);
  if(!ExactMembershipResponse(expected,expected)||ExactMembershipResponse(null,expected))throw new InvalidOperationException("Synthetic exact/null receipt policy failed.");
  var badReceipts=new Dictionary<string,string>{
   {"empty",""},{"trailing",expected+"\n"},{"truncated",expected.Substring(0,expected.Length-1)},
   {"schema",expected.Replace("response-v1","response-v2")},{"request_schema",expected.Replace("request-v1","request-v2")},
   {"nonce",expected.Replace(nonce,"wrong-run-123456789")},{"phase",expected.Replace("pre-escape","wrong-phase")},
   {"generation",expected.Replace("\"1\",\"2\",\"0\"","\"1\",\"3\",\"0\"")},
   {"popup",expected.Replace("\"123\"","\"124\"")},{"root_runtime",expected.Replace("42,123","42,124")},
   {"inventory_digest",expected.Replace(InventoryDigest(proof),new string('f',64))},
   {"browser_pid",expected.Replace("\"browser_pid\":\"10\"","\"browser_pid\":\"11\"")},
   {"gpu_pid",expected.Replace("\"gpu_pid\":\"20\"","\"gpu_pid\":\"21\"")},
   {"browser_creation",expected.Replace("\"browser_creation\":\"30\"","\"browser_creation\":\"31\"")},
   {"gpu_creation",expected.Replace("\"gpu_creation\":\"40\"","\"gpu_creation\":\"41\"")},
   {"environment_identity",expected.Replace("\"environment_identity\":\"50\"","\"environment_identity\":\"51\"")},
   {"deadline_tick",expected.Replace("\"deadline_tick\":\"60\"","\"deadline_tick\":\"61\"")},
   {"status",expected.Replace("fresh-exact-retained-membership","stale-membership")},
   {"extra_field",expected.Replace("{","{\"extra\":\"unknown\",")},
   {"duplicate_field",expected.Replace("{","{\"nonce\":\"duplicate\",")},{"oversized",new string('x',8193)}
  };
  foreach(var bad in badReceipts){
   if(bad.Value==expected||ExactMembershipResponse(bad.Value,expected))throw new InvalidOperationException("Synthetic mismatched receipt accepted or case did not mutate: "+bad.Key);
  }
  foreach(string bad in new[]{"1,2","01,2,3","1,-2,3","1,2,18446744073709551616"}){
   bool rejected=false;try{ProtocolProof(bad);}catch(InvalidOperationException){rejected=true;}if(!rejected)throw new InvalidOperationException("Synthetic invalid source proof accepted.");
  }
 }
 static MenuProof SyntheticMenuProof(string inventory){
  return new MenuProof{Window=(IntPtr)123,Focus=(IntPtr)124,Thread=10,RootId=new[]{42,123},MenuId=new[]{42,125},ItemId=new[]{42,126},Left=20,Top=30,Width=40,Height=50,Inventory=inventory,GpuLeaves=1};
 }
 public static void CheckInventoryPolicy(){
  // Complete invented rows exercise the actual SameProof/digest integration, not UIA.
  const string root="1:0:null:[42,123]:10:50032:123:0:0:100:100:browser-owned\n";
  const string gpu="2:1:[42,123]:[42,456]:20:50033:456:10:10:20:20:synthetic-gpu-proof\n";
  const string menu="3:1:[42,123]:[42,125]:10:50009:0:0:0:100:100:browser-owned\n";
  const string item="4:2:[42,125]:[42,126]:10:50011:0:20:30:40:50:browser-owned\n";
  string inventory=root+gpu+menu+item;var baseline=SyntheticMenuProof(inventory);
  if(!SameProof(baseline,SyntheticMenuProof(inventory)))throw new InvalidOperationException("Synthetic unchanged complete inventory rejected.");
  var changedInventories=new Dictionary<string,string>{
   {"count",root+gpu+menu},{"order",root+menu+gpu+item},
   {"parent",root+gpu.Replace("2:1:[42,123]","2:1:[42,127]")+menu+item},
   {"runtime",root+gpu.Replace("[42,456]","[42,457]")+menu+item},
   {"pid",root+gpu.Replace(":20:50033:",":21:50033:")+menu+item},
   {"hwnd",root+gpu.Replace(":50033:456:",":50033:457:")+menu+item},
   {"type",root+gpu.Replace(":50033:",":50009:")+menu+item},
   {"bounds",root+gpu.Replace(":10:10:20:20:",":11:10:20:20:")+menu+item},
   {"gpu_proof",root+gpu.Replace("synthetic-gpu-proof","changed-gpu-proof")+menu+item}
  };
  foreach(var changed in changedInventories){
   var fresh=SyntheticMenuProof(changed.Value);
   if(changed.Value==inventory||SameProof(baseline,fresh)||InventoryDigest(baseline)==InventoryDigest(fresh))throw new InvalidOperationException("Synthetic changed inventory accepted or case did not mutate: "+changed.Key);
  }
  foreach(Action<MenuProof> mutate in new Action<MenuProof>[]{
   p=>p.GpuLeaves=0,p=>p.GpuLeaves=2,p=>p.Window=(IntPtr)127,p=>p.Focus=(IntPtr)128,p=>p.Thread=11,
   p=>p.RootId=new[]{42,127},p=>p.MenuId=new[]{42,128},p=>p.ItemId=new[]{42,129},
   p=>p.Left=21,p=>p.Top=31,p=>p.Width=41,p=>p.Height=51
  }){var fresh=SyntheticMenuProof(inventory);mutate(fresh);if(SameProof(baseline,fresh))throw new InvalidOperationException("Synthetic changed root/fixture/native/count proof accepted.");}
 }
 // Failure-only, non-atomic property shapes. This separate buffer is never a MenuProof,
 // membership digest, input authority, or a reason to retry the irrevocable rejection.
 // Each provider call has cooperative before/after checks. A blocked UIA call itself
 // cannot be preempted; the 750ms cap does not claim a hard wall-clock timeout.
 // Large per-node records can hit the output cap before all shapes are read. Partial
 // diagnostic output is never a complete native inventory or an accepted menu proof.
 static string rejectedInventoryBuffer;
 const int RejectedNodeCap=128,RejectedRuntimeCap=32,RejectedOutputCap=65536,RejectedOutputReserve=8192;
 sealed class RejectedStop:Exception {public readonly string Reason;public RejectedStop(string reason){Reason=reason;}}
 sealed class RejectedValue {
  public string Status="not-attempted",Kind="unqueried",Json="null";public object Raw;
  public void Failed(){Status="getter-failed";Kind="unavailable";Json="null";Raw=null;}
  public void Store(object raw,string kind){
   Raw=raw;Kind=kind;Status="supported";Json="null";
   if(Object.ReferenceEquals(raw,AutomationElement.NotSupported)){Status="not-supported";Raw=null;return;}
   if(kind=="runtime-id"&&raw is int[]){var id=(int[])raw;if(id.Length==0||id.Length>RejectedRuntimeCap){Status="wrong-kind";Kind="runtime-outside-bound";Raw=null;throw new RejectedStop("runtime-cap");}Raw=(int[])id.Clone();Json=JsonIds((int[])Raw);return;}
   if(kind=="integer"&&raw is int){Json=Number((int)raw);return;}
   if(kind=="control-type"&&raw is ControlType){Json=Number(((ControlType)raw).Id);return;}
   if(kind=="boolean"&&raw is bool){Json=(bool)raw?"true":"false";return;}
   if(kind=="rectangle"&&raw is System.Windows.Rect){Json=RejectedRectangle((System.Windows.Rect)raw);return;}
   if(kind=="element-presence"){Json=raw==null?"false":"true";Raw=null;return;}
   Status="wrong-kind";Raw=null;
  }
  public string Render(){return "{\"status\":\""+Status+"\",\"kind\":\""+Kind+"\",\"value\":"+Json+"}";}
 }
 static string RejectedNumber(double value){return Finite(value)?Real(value):"null";}
 static string RejectedNumberShape(double value){return Double.IsNaN(value)?"nan":Double.IsPositiveInfinity(value)?"positive-infinity":Double.IsNegativeInfinity(value)?"negative-infinity":"finite";}
 static string RejectedRectangle(System.Windows.Rect r){
  return "{\"is_empty\":"+(r.IsEmpty?"true":"false")+",\"all_finite\":"+(Finite(r.X)&&Finite(r.Y)&&Finite(r.Width)&&Finite(r.Height)?"true":"false")+",\"x\":"+RejectedNumber(r.X)+",\"y\":"+RejectedNumber(r.Y)+",\"width\":"+RejectedNumber(r.Width)+",\"height\":"+RejectedNumber(r.Height)+",\"x_shape\":\""+RejectedNumberShape(r.X)+"\",\"y_shape\":\""+RejectedNumberShape(r.Y)+"\",\"width_shape\":\""+RejectedNumberShape(r.Width)+"\",\"height_shape\":\""+RejectedNumberShape(r.Height)+"\"}";
 }
 sealed class RejectedReading {
  public readonly RejectedValue Runtime=new RejectedValue(),Pid=new RejectedValue(),Type=new RejectedValue(),Hwnd=new RejectedValue(),Bounds=new RejectedValue(),Control=new RejectedValue(),Content=new RejectedValue(),Enabled=new RejectedValue(),Focusable=new RejectedValue(),Focused=new RejectedValue(),Offscreen=new RejectedValue();
  public string IdentityRender(){return "{\"runtime\":"+Runtime.Render()+",\"pid\":"+Pid.Render()+"}";}
  public string Render(){return "{\"runtime\":"+Runtime.Render()+",\"pid\":"+Pid.Render()+",\"type\":"+Type.Render()+",\"hwnd\":"+Hwnd.Render()+",\"bounds\":"+Bounds.Render()+",\"is_control\":"+Control.Render()+",\"is_content\":"+Content.Render()+",\"enabled\":"+Enabled.Render()+",\"keyboard_focusable\":"+Focusable.Render()+",\"keyboard_focused\":"+Focused.Render()+",\"offscreen\":"+Offscreen.Render()+"}";}
 }
 sealed class RejectedNode {
  public int Index,Depth;public int[] ExpectedParent;public string Status="incomplete";
  public readonly RejectedReading First=new RejectedReading(),Later=new RejectedReading(),ParentFirst=new RejectedReading(),ParentLater=new RejectedReading();
  public readonly RejectedValue ParentPresenceFirst=new RejectedValue(),ParentPresenceLater=new RejectedValue(),FirstChild=new RejectedValue(),NextSibling=new RejectedValue(),SiblingSourcePresence=new RejectedValue(),ReturnedSiblingPresence=new RejectedValue(),LastQueuedRuntime=new RejectedValue();
  public readonly RejectedReading SiblingSourceParent=new RejectedReading(),ReturnedSiblingParent=new RejectedReading(),EnumerationParentLater=new RejectedReading();
  public string Render(){return "{\"index\":"+Number(Index)+",\"depth\":"+Number(Depth)+",\"expected_parent_runtime\":"+JsonIds(ExpectedParent)+",\"status\":\""+Status+"\",\"first\":"+First.Render()+",\"later\":"+Later.Render()+",\"parent_first\":"+ParentFirst.IdentityRender()+",\"parent_later\":"+ParentLater.IdentityRender()+",\"parent_presence_first\":"+ParentPresenceFirst.Render()+",\"parent_presence_later\":"+ParentPresenceLater.Render()+",\"first_child\":"+FirstChild.Render()+",\"last_sibling_query\":"+NextSibling.Render()+",\"last_sibling_source_parent\":"+SiblingSourceParent.IdentityRender()+",\"last_sibling_source_parent_presence\":"+SiblingSourcePresence.Render()+",\"last_returned_sibling_parent\":"+ReturnedSiblingParent.IdentityRender()+",\"last_returned_sibling_parent_presence\":"+ReturnedSiblingPresence.Render()+",\"last_queued_runtime\":"+LastQueuedRuntime.Render()+",\"enumeration_parent_later\":"+EnumerationParentLater.IdentityRender()+",\"patterns\":\"not-queried\"}";}
 }
 sealed class RejectedContext {
  public ulong Deadline,ReadinessDeadline,OwnershipDeadline;public long Popup;public int[] RootId;public uint BrowserPid;public bool Partial;public string Stop="not-started";public string Outer="null";public Func<ulong> SyntheticClock;public bool GetterFailed;
  public ulong Now(){return SyntheticClock==null?GetTickCount64():SyntheticClock();}
  public RejectedReading RootFence=new RejectedReading();public readonly List<RejectedNode> Nodes=new List<RejectedNode>();
  public void Check(){
   if(Now()>=Deadline)throw new RejectedStop("cooperative-budget-expired");
   int length=RejectedJson(this).Length;
   if(Now()>=Deadline)throw new RejectedStop("cooperative-budget-expired");
   if(length>RejectedOutputCap-RejectedOutputReserve)throw new RejectedStop("output-cap");
  }
 }
 static string RejectedJson(RejectedContext c){
  var rows=new StringBuilder();foreach(var node in c.Nodes){if(rows.Length!=0)rows.Append(',');rows.Append(node.Render());}
  return "{\"schema\":\"bowser-rejected-popup-shapes-v1\",\"scope\":\"failure-only-non-atomic-not-input-authority\",\"non_atomic\":true,\"original_browser_witness_rejection_selected\":true,\"original_rejection_preserved\":true,\"acceptance_retry\":false,\"patterns\":\"not-queried\",\"cooperative_budget_ms\":750,\"blocking_provider_call_cannot_be_preempted\":true,\"coverage\":\""+(c.Partial?"partial":"complete-browser-subtrees-only")+"\",\"stop_reason\":\""+c.Stop+"\",\"expected_browser_pid\":"+Number(c.BrowserPid)+",\"captured_popup_hwnd\":"+Number(c.Popup)+",\"captured_root_runtime\":"+JsonIds(c.RootId)+",\"original_readiness_deadline_tick\":"+c.ReadinessDeadline.ToString(System.Globalization.CultureInfo.InvariantCulture)+",\"original_ownership_deadline_tick\":"+c.OwnershipDeadline.ToString(System.Globalization.CultureInfo.InvariantCulture)+",\"diagnostic_deadline_tick\":"+c.Deadline.ToString(System.Globalization.CultureInfo.InvariantCulture)+",\"failed_outer_observation\":"+c.Outer+",\"last_root_fence\":"+c.RootFence.Render()+",\"node_count\":"+Number(c.Nodes.Count)+",\"nodes\":["+rows+"]}";
 }
 static string RejectedBoundedJson(RejectedContext c){
  string json=RejectedJson(c);if(json.Length<=RejectedOutputCap)return json;
  // Defensive only: live checks reserve more than the largest bounded field/record.
  // Never emit an oversized or truncated JSON document or turn missing output into proof.
  return "{\"schema\":\"bowser-rejected-popup-shapes-v1\",\"scope\":\"failure-only-non-atomic-not-input-authority\",\"non_atomic\":true,\"original_rejection_preserved\":true,\"coverage\":\"partial\",\"stop_reason\":\"output-cap-details-discarded\",\"getter_failure_details\":\"not-retained\",\"node_count\":"+Number(c.Nodes.Count)+",\"nodes\":[{\"index\":"+Number(c.Nodes.Count)+",\"status\":\"details-discarded\",\"getter_state\":{\"status\":\""+(c.GetterFailed?"getter-failed":"not-attempted")+"\"}}]}";
 }
 static ulong RejectedDeadline(ulong now,ulong readiness,ulong ownership){
  ulong local=now>UInt64.MaxValue-750?UInt64.MaxValue:now+750;
  if(readiness<local)local=readiness;if(ownership!=0&&ownership<local)local=ownership;return local;
 }
 static T RejectedProvider<T>(RejectedContext c,Func<T> query,RejectedValue target,string kind){
  c.Check();T raw;
  try{raw=query();}catch{target.Failed();c.GetterFailed=true;c.Check();throw new RejectedStop("provider-getter-failed");}
  // Save getter state before the after-call budget check, even when the call overruns.
  try{target.Store(raw,kind);}finally{c.Check();}return raw;
 }
 static void RejectedProperty(RejectedContext c,AutomationElement node,AutomationProperty property,RejectedValue target,string kind){RejectedProvider(c,()=>node.GetCurrentPropertyValue(property,true),target,kind);}
 static void RejectedIdentity(RejectedContext c,AutomationElement node,RejectedReading reading){
  RejectedProvider(c,()=>node.GetRuntimeId(),reading.Runtime,"runtime-id");
  RejectedProperty(c,node,AutomationElement.ProcessIdProperty,reading.Pid,"integer");
 }
 static bool RejectedBrowserPid(RejectedValue pid,uint expected){return pid.Status=="supported"&&pid.Raw is int&&(int)pid.Raw>0&&(long)(int)pid.Raw==expected;}
 static int[] RejectedRuntime(RejectedValue runtime){if(runtime.Status!="supported"||!(runtime.Raw is int[]))throw new RejectedStop("runtime-unavailable");return (int[])runtime.Raw;}
 static void RejectedVisit(HashSet<string> seen,int[] runtime,int depth,int count){
  if(depth<0||depth>128||count<1||count>RejectedNodeCap)throw new RejectedStop("node-or-depth-cap");
  if(runtime==null||runtime.Length==0||runtime.Length>RejectedRuntimeCap)throw new RejectedStop("runtime-cap");
  if(!seen.Add(JsonIds(runtime)))throw new RejectedStop("duplicate-or-cyclic-runtime");
 }
 static void RejectedParentPolicy(int[] expected,RejectedReading observed,uint browserPid,bool present){
  if(expected==null||!present||!Same(RejectedRuntime(observed.Runtime),expected)||!RejectedBrowserPid(observed.Pid,browserPid))throw new RejectedStop("queued-parent-runtime-or-pid-changed");
 }
 static void RejectedParent(RejectedContext c,AutomationElement node,int[] expected,RejectedReading reading,RejectedValue presence){
  // Called only for an enqueued descendant. Never ask for the captured root's parent.
  var parent=RejectedProvider(c,()=>TreeWalker.RawViewWalker.GetParent(node),presence,"element-presence");
  if(parent!=null)RejectedIdentity(c,parent,reading);RejectedParentPolicy(expected,reading,c.BrowserPid,parent!=null);
 }
 static void RejectedShapes(RejectedContext c,AutomationElement node,RejectedReading reading){
  RejectedProperty(c,node,AutomationElement.ControlTypeProperty,reading.Type,"control-type");
  RejectedProperty(c,node,AutomationElement.NativeWindowHandleProperty,reading.Hwnd,"integer");
  RejectedProperty(c,node,AutomationElement.BoundingRectangleProperty,reading.Bounds,"rectangle");
  RejectedProperty(c,node,AutomationElement.IsControlElementProperty,reading.Control,"boolean");
  RejectedProperty(c,node,AutomationElement.IsContentElementProperty,reading.Content,"boolean");
  RejectedProperty(c,node,AutomationElement.IsEnabledProperty,reading.Enabled,"boolean");
  RejectedProperty(c,node,AutomationElement.IsKeyboardFocusableProperty,reading.Focusable,"boolean");
  RejectedProperty(c,node,AutomationElement.HasKeyboardFocusProperty,reading.Focused,"boolean");
  RejectedProperty(c,node,AutomationElement.IsOffscreenProperty,reading.Offscreen,"boolean");
 }
 static void RejectedNativeFence(RejectedContext c,IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint menuTid,ReadOnlyBrowser browser){
  c.Check();bool valid=browser.SameAlive()&&Host(host,testPid,hostTid)&&PopupIdentity(host,popup,c.BrowserPid,menuTid);c.Check();
  if(!valid)throw new RejectedStop("retained-process-or-native-root-changed");
 }
 static void RejectedRootFence(RejectedContext c,IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint menuTid,ReadOnlyBrowser browser,AutomationElement root,int[] rootId){
  RejectedNativeFence(c,host,testPid,hostTid,popup,menuTid,browser);
  var reading=new RejectedReading();c.RootFence=reading;RejectedIdentity(c,root,reading);
  RejectedProperty(c,root,AutomationElement.NativeWindowHandleProperty,reading.Hwnd,"integer");
  if(!Same(RejectedRuntime(reading.Runtime),rootId)||!RejectedBrowserPid(reading.Pid,c.BrowserPid)||reading.Hwnd.Status!="supported"||!(reading.Hwnd.Raw is int)||(int)reading.Hwnd.Raw!=popup.ToInt32())throw new RejectedStop("captured-uia-root-changed");
  RejectedNativeFence(c,host,testPid,hostTid,popup,menuTid,browser);
 }
 static bool RejectedSameIdentity(RejectedReading a,RejectedReading b){
  return Same(RejectedRuntime(a.Runtime),RejectedRuntime(b.Runtime))&&a.Pid.Render()==b.Pid.Render()&&a.Type.Render()==b.Type.Render()&&a.Hwnd.Render()==b.Hwnd.Render();
 }
 static void BufferRejectedInventory(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,uint menuTid,ReadOnlyBrowser browser,AutomationElement root,int[] rootId,int[] failedRuntime,int[] failedParent,int failedDepth,int failedIndex,int failedPid,int failedType,int failedHwnd,System.Windows.Rect failedBounds,ulong readinessDeadline,ulong ownershipDeadline){
  if(rejectedInventoryBuffer!=null)return; // At most the original rejected run; no retries.
  var c=new RejectedContext{BrowserPid=browserPid,Popup=popup.ToInt64(),RootId=(int[])rootId.Clone(),ReadinessDeadline=readinessDeadline,OwnershipDeadline=ownershipDeadline,Deadline=RejectedDeadline(GetTickCount64(),readinessDeadline,ownershipDeadline)};
  c.Outer="{\"source\":\"failed-outer-default-backed-observation-not-no-default\",\"index\":"+Number(failedIndex)+",\"depth\":"+Number(failedDepth)+",\"runtime\":"+JsonIds(failedRuntime)+",\"expected_parent_runtime\":"+JsonIds(failedParent)+",\"pid\":"+Number(failedPid)+",\"type\":"+Number(failedType)+",\"hwnd\":"+Number(failedHwnd)+",\"bounds\":"+RejectedRectangle(failedBounds)+"}";
  try{
   c.Check();var queue=new Queue<Tuple<AutomationElement,int,int[],int[]>>();var seen=new HashSet<string>(StringComparer.Ordinal);var scheduled=new HashSet<string>(StringComparer.Ordinal){JsonIds(rootId)};queue.Enqueue(Tuple.Create(root,0,(int[])null,(int[])rootId.Clone()));
   while(queue.Count!=0){
    c.Check();if(c.Nodes.Count>=RejectedNodeCap)throw new RejectedStop("node-or-depth-cap");
    var work=queue.Dequeue();var row=new RejectedNode{Index=c.Nodes.Count+1,Depth=work.Item2,ExpectedParent=work.Item3};c.Nodes.Add(row);
    RejectedRootFence(c,host,testPid,hostTid,popup,menuTid,browser,root,rootId);
    RejectedIdentity(c,work.Item1,row.First);int[] runtime=RejectedRuntime(row.First.Runtime);RejectedVisit(seen,runtime,row.Depth,c.Nodes.Count);
    if(!Same(runtime,work.Item4))throw new RejectedStop("queued-node-runtime-changed");
    if(work.Item3!=null)RejectedParent(c,work.Item1,work.Item3,row.ParentFirst,row.ParentPresenceFirst);
    else if(row.Depth!=0||!Same(runtime,rootId))throw new RejectedStop("captured-root-runtime-changed");
    if(!RejectedBrowserPid(row.First.Pid,browserPid)){c.Partial=true;row.Status="foreign-or-unknown-pid-subtree-excluded";RejectedRootFence(c,host,testPid,hostTid,popup,menuTid,browser,root,rootId);continue;}
    RejectedShapes(c,work.Item1,row.First);
    RejectedIdentity(c,work.Item1,row.Later);
    if(!RejectedBrowserPid(row.Later.Pid,browserPid)||!Same(RejectedRuntime(row.Later.Runtime),runtime))throw new RejectedStop("node-runtime-or-pid-changed");
    if(work.Item3!=null)RejectedParent(c,work.Item1,work.Item3,row.ParentLater,row.ParentPresenceLater);
    RejectedShapes(c,work.Item1,row.Later);
    if(!RejectedBrowserPid(row.Later.Pid,browserPid)||!RejectedSameIdentity(row.First,row.Later))throw new RejectedStop("node-runtime-pid-type-or-hwnd-changed");
    RejectedRootFence(c,host,testPid,hostTid,popup,menuTid,browser,root,rootId);row.Status="browser-shapes-recorded-not-authority";
    var child=RejectedProvider(c,()=>TreeWalker.RawViewWalker.GetFirstChild(work.Item1),row.FirstChild,"element-presence");
    while(child!=null){
     if(row.Depth>=128||!InventoryCapacity(c.Nodes.Count,queue.Count+1))throw new RejectedStop("node-queue-or-depth-cap");
     RejectedRootFence(c,host,testPid,hostTid,popup,menuTid,browser,root,rootId);
     // Before reading/advancing this child, prove only its exact retained browser
     // parent's raw runtime/PID edge. This never asks for the captured root's parent.
     RejectedParent(c,child,runtime,row.SiblingSourceParent,row.SiblingSourcePresence);
     var scheduledChild=child;RejectedProvider(c,()=>scheduledChild.GetRuntimeId(),row.LastQueuedRuntime,"runtime-id");
     int[] childRuntime=RejectedRuntime(row.LastQueuedRuntime);if(!scheduled.Add(JsonIds(childRuntime)))throw new RejectedStop("duplicate-or-cyclic-runtime");
     queue.Enqueue(Tuple.Create(child,row.Depth+1,(int[])runtime.Clone(),(int[])childRuntime.Clone()));
     // A boundary child's sibling is still in the validated browser parent's list,
     // not inside the boundary subtree. No GetFirstChild is called on that boundary.
     // Its parent edge is re-proved immediately before this separately identifiable call.
     RejectedParent(c,child,runtime,row.SiblingSourceParent,row.SiblingSourcePresence);
     var siblingSource=child;child=RejectedProvider(c,()=>TreeWalker.RawViewWalker.GetNextSibling(siblingSource),row.NextSibling,"element-presence");
     if(child!=null)RejectedParent(c,child,runtime,row.ReturnedSiblingParent,row.ReturnedSiblingPresence);
     RejectedRootFence(c,host,testPid,hostTid,popup,menuTid,browser,root,rootId);
    }
    RejectedIdentity(c,work.Item1,row.EnumerationParentLater);
    if(!Same(RejectedRuntime(row.EnumerationParentLater.Runtime),runtime)||!RejectedBrowserPid(row.EnumerationParentLater.Pid,browserPid))throw new RejectedStop("queued-parent-runtime-or-pid-changed");
    RejectedRootFence(c,host,testPid,hostTid,popup,menuTid,browser,root,rootId);
   }
   RejectedRootFence(c,host,testPid,hostTid,popup,menuTid,browser,root,rootId);
   c.Stop=c.Partial?"foreign-or-unknown-subtree-excluded":"captured-browser-subtrees-exhausted";
  }catch(RejectedStop stop){c.Partial=true;c.Stop=stop.Reason;}
  catch{c.Partial=true;c.Stop="unexpected-diagnostic-failure";}
  finally{rejectedInventoryBuffer=RejectedBoundedJson(c);}
 }
 // Read this captured string only after the exact failed test process was terminated.
 // No provider/native calls, I/O, digest, MenuProof, membership, retry, or input here.
 public static string RejectedInventoryEvidence(){return rejectedInventoryBuffer;}
 // Synthetic only: these are the actual collector/serializer predicates, no native/UIA/I/O.
 public static void CheckRejectedInventoryPolicy(){
  if(RejectedDeadline(100,1000,0)!=850||RejectedDeadline(100,700,0)!=700||RejectedDeadline(100,1000,600)!=600||RejectedDeadline(100,99,0)!=99||RejectedDeadline(UInt64.MaxValue-1,UInt64.MaxValue,0)!=UInt64.MaxValue)throw new InvalidOperationException("Synthetic rejected diagnostic budget changed.");
  Action<Action> refuses=action=>{bool refused=false;try{action();}catch(RejectedStop){refused=true;}if(!refused)throw new InvalidOperationException("Synthetic rejected diagnostic guard accepted invalid input.");};
  var seen=new HashSet<string>(StringComparer.Ordinal);RejectedVisit(seen,new[]{42,1},0,1);refuses(()=>RejectedVisit(seen,new[]{42,1},1,2));
  foreach(Action bad in new Action[]{()=>RejectedVisit(new HashSet<string>(),new[]{42},129,1),()=>RejectedVisit(new HashSet<string>(),new[]{42},0,129),()=>RejectedVisit(new HashSet<string>(),new int[33],0,1),()=>RejectedVisit(new HashSet<string>(),new int[0],0,1)})refuses(bad);
  RejectedVisit(new HashSet<string>(),new int[32],128,128);
  var parent=new RejectedReading();parent.Runtime.Store(new[]{42,1},"runtime-id");parent.Pid.Store(10,"integer");RejectedParentPolicy(new[]{42,1},parent,10,true);
  foreach(Action bad in new Action[]{()=>RejectedParentPolicy(new[]{42,2},parent,10,true),()=>RejectedParentPolicy(new[]{42,1},parent,11,true),()=>RejectedParentPolicy(null,parent,10,true),()=>RejectedParentPolicy(new[]{42,1},parent,10,false)})refuses(bad);
  var unknown=new RejectedValue();unknown.Store(AutomationElement.NotSupported,"integer");if(RejectedBrowserPid(unknown,10))throw new InvalidOperationException("Synthetic unknown PID became browser authority.");
  var wrong=new RejectedValue();wrong.Store(false,"integer");if(wrong.Status!="wrong-kind"||RejectedBrowserPid(wrong,10))throw new InvalidOperationException("Synthetic wrong-kind PID accepted.");
  var boolean=new RejectedValue();boolean.Store(false,"boolean");if(boolean.Status!="supported"||boolean.Json!="false"||unknown.Status!="not-supported")throw new InvalidOperationException("Synthetic supported false or unknown erased.");
  var failure=new RejectedValue();failure.Failed();if(!failure.Render().Contains("getter-failed")||new RejectedValue().Status!="not-attempted")throw new InvalidOperationException("Synthetic failed/unattempted property erased.");
  var providerContext=new RejectedContext{Deadline=1000,SyntheticClock=()=>100};var providerFailure=new RejectedValue();
  refuses(()=>RejectedProvider<object>(providerContext,()=>{throw new InvalidOperationException("synthetic-only");},providerFailure,"integer"));
  if(providerFailure.Status!="getter-failed")throw new InvalidOperationException("Synthetic actual provider wrapper lost getter failure.");
  int ticks=0,calls=0;var lateContext=new RejectedContext{Deadline=1000,SyntheticClock=()=>++ticks<=2?100UL:1000UL};var lateValue=new RejectedValue();
  refuses(()=>RejectedProvider(lateContext,()=>{calls++;return 10;},lateValue,"integer"));if(calls!=1||lateValue.Status!="supported"||lateValue.Json!="10")throw new InvalidOperationException("Synthetic after-call expiry erased first reading or retried.");
  var expiredContext=new RejectedContext{Deadline=100,SyntheticClock=()=>100};var unattempted=new RejectedValue();calls=0;
  refuses(()=>RejectedProvider(expiredContext,()=>{calls++;return 10;},unattempted,"integer"));if(calls!=0||unattempted.Status!="not-attempted")throw new InvalidOperationException("Synthetic expired budget still called provider.");
  if(!InventoryCapacity(1,127)||InventoryCapacity(1,128)||InventoryCapacity(128,1))throw new InvalidOperationException("Synthetic rejected queue capacity changed.");
  var bounds=new RejectedValue();bounds.Store(System.Windows.Rect.Empty,"rectangle");if(!bounds.Json.Contains("positive-infinity")||!bounds.Json.Contains("negative-infinity")||!bounds.Json.Contains("\"x\":null")||bounds.Json.Contains("Infinity"))throw new InvalidOperationException("Synthetic nonfinite bounds serialized as number/default.");
  if(RejectedNumber(Double.NaN)!="null"||RejectedNumberShape(Double.NaN)!="nan"||RejectedNumber(1.25)!="1.25")throw new InvalidOperationException("Synthetic finite/nonfinite serializer changed.");
  var c=new RejectedContext{BrowserPid=10,Partial=true,Stop="provider-getter-failed"};var row=new RejectedNode{Index=1,Depth=0};row.First.Hwnd.Failed();c.Nodes.Add(row);
  string json=RejectedBoundedJson(c);if(!json.Contains("getter-failed")||!json.Contains("\"first\":")||!json.Contains("\"later\":")||!json.Contains("\"coverage\":\"partial\"")||json.Contains("inventory_digest")||json.Contains("inventory_first")||json.Contains("inventory_fresh"))throw new InvalidOperationException("Synthetic rejected schema mixed with authority or lost failure state.");
  var liveCap=new RejectedContext{Deadline=1000,SyntheticClock=()=>100};bool stopped=false;
  for(int n=0;n<128;n++){try{liveCap.Check();liveCap.Nodes.Add(new RejectedNode{Index=n+1});liveCap.Check();}catch(RejectedStop){stopped=true;liveCap.Partial=true;liveCap.Stop="output-cap";break;}}
  json=RejectedBoundedJson(liveCap);if(!stopped||json.Length>RejectedOutputCap||json.Contains("output-cap-details-discarded"))throw new InvalidOperationException("Synthetic live collector reserve failed.");
  c.GetterFailed=true;for(int n=0;n<128;n++)c.Nodes.Add(new RejectedNode{Index=n+2});json=RejectedBoundedJson(c);if(json.Length>RejectedOutputCap||!json.Contains("output-cap-details-discarded")||!json.Contains("getter-failed"))throw new InvalidOperationException("Synthetic rejected serializer exceeded output cap or erased failure state.");
 }
 public static MenuProof ReadMenu(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,ReadOnlyBrowser browser,ReadOnlyBrowser gpu,ulong ownershipDeadline,string diagnosticDirectory,ulong diagnosticReadinessDeadline){
  uint helperPid=NativeCurrentProcessId(); // Known local identity, captured before UIA traversal.
  uint beforePid;uint tid=GetWindowThreadProcessId(popup,out beforePid);if(beforePid!=browserPid||tid==0||!IsWindow(popup))throw new InvalidOperationException("Exact native UIA root unavailable.");
  var root=AutomationElement.FromHandle(popup);if(root==null||root.Current.ProcessId!=(int)browserPid||root.Current.NativeWindowHandle!=popup.ToInt32())throw new InvalidOperationException("Exact native UIA root mismatch.");
  var focusGui=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};if(!GetGUIThreadInfo(tid,ref focusGui)||!FocusOwned(host,focusGui.hwndFocus,browserPid,tid))throw new InvalidOperationException("Native focus witness unavailable before UIA read.");
  int[] rootId=Runtime(root);var walker=TreeWalker.RawViewWalker;var queue=new Queue<Tuple<AutomationElement,int,int[]>>();queue.Enqueue(Tuple.Create(root,0,(int[])null));int examined=0,menus=0,items=0,gpuLeaves=0;int[] menuId=null,itemId=null;System.Windows.Rect chosen=new System.Windows.Rect();var inventory=new StringBuilder();var seen=new HashSet<string>(StringComparer.Ordinal);var browserNodes=new Dictionary<string,BrowserNodeWitness>(StringComparer.Ordinal);
  while(queue.Count>0){if(!InventoryCapacity(++examined,0))throw new InvalidOperationException("UIA raw menu inventory incomplete.");
   if(!RootFence(host,testPid,hostTid,popup,browserPid,tid,browser,root,rootId))throw new InvalidOperationException("Exact root changed during complete raw inventory.");
   var queued=queue.Dequeue();var e=queued.Item1;var c=e.Current;int providerPid=c.ProcessId;
   int[] currentId=Runtime(e);string runtimeKey=JsonIds(currentId);if(!seen.Add(runtimeKey))throw new InvalidOperationException("Duplicate/cyclic raw UIA identity.");
   if(queued.Item3!=null){var parent=walker.GetParent(e);if(parent==null||!Same(Runtime(parent),queued.Item3))throw new InvalidOperationException("Raw parent identity changed.");}
   GpuLeafWitness gpuLeaf=null;string gpuProof=null;
   if(providerPid!=(int)browserPid){
    try{
     if(gpu==null||providerPid!=(int)gpu.Pid||gpuLeaves!=0)throw new InvalidOperationException("Unproved or additional foreign provider.");
     gpuLeaf=ReadGpuLeaf(host,popup,e,queued.Item2,queued.Item3,rootId,browserPid,gpu,ownershipDeadline);gpuProof=gpuLeaf.Signature;gpuLeaves++;
    }catch{
     WriteMismatch(host,testPid,hostTid,popup,browserPid,tid,browser,root,rootId,e,queued.Item2,queued.Item3,examined,providerPid,helperPid,diagnosticDirectory);
     throw new InvalidOperationException("UIA menu provider PID/owned-leaf proof mismatch.");
    }
   }
   var box=c.BoundingRectangle;int observedType=c.ControlType.Id,observedNative=c.NativeWindowHandle;
   if(gpuLeaf!=null){
    if(!gpuLeaf.Matches(c.ProcessId,observedNative,observedType,box,currentId,queued.Item3,c.IsEnabled,c.HasKeyboardFocus))throw new InvalidOperationException("Outer inventory differs from immutable GPU Pane/native/bounds witness.");
    RecheckGpuTyped(e,gpuLeaf);
   }
   BrowserNodeWitness browserNode=null;
   if(providerPid==(int)browserPid){
    try{
     browserNode=CaptureBrowserNode(e,browserPid,queued.Item2,queued.Item3,browser);
     if(!browserNode.MatchesOuter(c.ProcessId,observedType,observedNative,box,currentId,queued.Item3))throw new InvalidOperationException("Outer browser inventory differs from immutable typed witness.");
     if(queued.Item2==0&&(browserNode.NativeUnknown||browserNode.BoundsUnknown||browserNode.Native!=popup.ToInt32()||!Same(browserNode.Id(),rootId)))throw new InvalidOperationException("Typed browser root differs from exact native popup.");
     if(!RootFence(host,testPid,hostTid,popup,browserPid,tid,browser,root,rootId))throw new InvalidOperationException("Root changed before typed browser recheck.");
     RecheckBrowserNode(e,browserNode,browserPid,browser);browserNodes.Add(runtimeKey,browserNode);
    }catch{
     // The original strict browser rejection is already selected. Optional collection
     // cannot return proof, retry acceptance, send input, or replace that same error.
     try{BufferRejectedInventory(host,testPid,hostTid,popup,browserPid,tid,browser,root,rootId,currentId,queued.Item3,queued.Item2,examined,providerPid,observedType,observedNative,box,diagnosticReadinessDeadline,ownershipDeadline);}catch{}
     WriteBoundsFailure(host,testPid,hostTid,popup,browserPid,tid,browser,root,rootId,e,currentId,queued.Item3,queued.Item2,examined,providerPid,observedType,observedNative,box,false,diagnosticDirectory);throw;
    }
   }
   string nativeToken=browserNode==null?Number(observedNative):browserNode.NativeToken;
   string boundsToken=browserNode==null?Real(box.X)+":"+Real(box.Y)+":"+Real(box.Width)+":"+Real(box.Height):browserNode.BoundsToken;
   AppendInventoryNode(inventory,examined,queued.Item2,queued.Item3,currentId,providerPid,observedType,nativeToken,boundsToken,gpuProof??browserNode.Signature);
   if(IsBrowserRow(browserNode,ControlType.Menu.Id)){menus++;menuId=browserNode.Id();}
   if(IsBrowserRow(browserNode,ControlType.MenuItem.Id)&&String.Equals(c.Name,"Bowser native context permission probe",StringComparison.Ordinal)){
    if(!c.IsEnabled||c.IsOffscreen)throw new InvalidOperationException("Fixture menu leaf unavailable.");
    var child=walker.GetFirstChild(e);if(child!=null)throw new InvalidOperationException("Fixture item is not a leaf.");
    var ancestor=e;bool reached=false;int menuAncestors=0;for(int hop=0;hop<32&&ancestor!=null;hop++){
     var ancestorId=Runtime(ancestor);var counted=CountedAncestor(browserNodes,ancestorId);var ac=ancestor.Current;
     if(!counted.MatchesOuter(ac.ProcessId,ac.ControlType.Id,ac.NativeWindowHandle,ac.BoundingRectangle,ancestorId,counted.Parent()))throw new InvalidOperationException("Ancestor differs from its exact counted typed witness.");
     RecheckBrowserNode(ancestor,counted,browserPid,browser);
     if(counted.Type==ControlType.Menu.Id)menuAncestors++;if(Same(ancestorId,rootId)){reached=true;break;}
     if(!counted.NativeUnknown&&counted.Native!=0&&(IntPtr)counted.Native!=popup)throw new InvalidOperationException("Unexpected supported native ancestor in menu hierarchy.");
     var next=walker.GetParent(ancestor);CountedParent(browserNodes,counted,next==null?null:Runtime(next));ancestor=next;
    }if(!reached||menuAncestors!=1)throw new InvalidOperationException("UIA raw Menu ancestry incomplete or ambiguous.");
    items++;itemId=browserNode.Id();chosen=FixtureBounds(browserNode);
   }
   if(!Same(Runtime(e),currentId)||e.Current.ProcessId!=providerPid)throw new InvalidOperationException("Raw node identity changed across inventory read.");
   var firstChild=walker.GetFirstChild(e);if(gpuProof!=null&&firstChild!=null)throw new InvalidOperationException("GPU leaf gained a child.");
   for(var child=firstChild;child!=null;child=walker.GetNextSibling(child)){if(!InventoryCapacity(examined,queue.Count+1))throw new InvalidOperationException("UIA child inventory exceeds bound.");queue.Enqueue(Tuple.Create(child,queued.Item2+1,currentId));}
  }
  RECT r;uint afterPid;uint afterTid=GetWindowThreadProcessId(popup,out afterPid);
  if(!RootFence(host,testPid,hostTid,popup,browserPid,tid,browser,root,rootId)||(gpuLeaves!=0&&(gpu==null||!gpu.SameAlive()))||!FocusOwned(host,focusGui.hwndFocus,browserPid,tid)||menus!=1||items!=1||!Same(rootId,Runtime(root))||afterPid!=browserPid||afterTid!=tid||!IsWindow(popup)||!GetWindowRect(popup,out r)||!Finite(chosen.X)||!Finite(chosen.Y)||!Finite(chosen.Width)||!Finite(chosen.Height)||chosen.Width<=0||chosen.Height<=0||chosen.Left<r.Left||chosen.Top<r.Top||chosen.Right>r.Right||chosen.Bottom>r.Bottom)throw new InvalidOperationException("Unique bounded current Menu/MenuItem proof unavailable.");
  return new MenuProof{Window=popup,Focus=focusGui.hwndFocus,Thread=tid,RootId=rootId,MenuId=menuId,ItemId=itemId,Left=chosen.Left,Top=chosen.Top,Width=chosen.Width,Height=chosen.Height,Inventory=inventory.ToString(),GpuLeaves=gpuLeaves};
 }
 public static bool SameProof(MenuProof a,MenuProof b){return a.Inventory==b.Inventory&&a.GpuLeaves==b.GpuLeaves&&a.Window==b.Window&&a.Focus==b.Focus&&a.Thread==b.Thread&&Same(a.RootId,b.RootId)&&Same(a.MenuId,b.MenuId)&&Same(a.ItemId,b.ItemId)&&a.Left==b.Left&&a.Top==b.Top&&a.Width==b.Width&&a.Height==b.Height;}
 public static bool Released(IntPtr host,uint testPid,uint hostTid,IntPtr popup,uint browserPid,uint menuTid,IntPtr focus){
  var a=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};var b=a;
  bool hidden=!IsWindow(popup)||(PopupIdentity(host,popup,browserPid,menuTid)&&!IsWindowVisible(popup));
  return Host(host,testPid,hostTid)&&GetForegroundWindow()==host&&hidden&&FocusOwned(host,focus,browserPid,menuTid)&&GetGUIThreadInfo(hostTid,ref a)&&GetGUIThreadInfo(menuTid,ref b)&&a.hwndActive==host&&b.hwndActive==host&&a.hwndFocus==focus&&b.hwndFocus==focus&&a.hwndCapture==IntPtr.Zero&&b.hwndCapture==IntPtr.Zero&&FocusOwned(host,focus,browserPid,menuTid)&&Host(host,testPid,hostTid)&&GetForegroundWindow()==host&&(!IsWindow(popup)||(PopupIdentity(host,popup,browserPid,menuTid)&&!IsWindowVisible(popup)));
 }
 [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h,uint command);
 [DllImport("user32.dll",EntryPoint="GetWindow",SetLastError=true)] static extern IntPtr DiagnosticGetWindow(IntPtr h,uint command);
 [DllImport("kernel32.dll",EntryPoint="SetLastError")] static extern void DiagnosticSetLastError(uint error);
 [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent,EnumProc callback,IntPtr data);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 public sealed class ReadOnlyBrowser:IDisposable {
  IntPtr handle;uint pid;ulong created;string image;
  public ReadOnlyBrowser(uint expected){handle=OpenProcess(0x00101000,false,expected);if(handle==IntPtr.Zero)throw new InvalidOperationException("Native browser read-only handle unavailable.");try{pid=expected;image=Image();created=Creation();if(!String.Equals(System.IO.Path.GetFileName(image),"msedgewebview2.exe",StringComparison.OrdinalIgnoreCase)||!SameAlive())throw new InvalidOperationException("Native browser read-only identity unavailable.");}catch{Dispose();throw;}}
  string Image(){var b=new StringBuilder(32768);uint n=32768;if(!QueryFullProcessImageName(handle,0,b,ref n))throw new InvalidOperationException("Native browser image query unavailable.");return System.IO.Path.GetFullPath(b.ToString());}
  ulong Creation(){TIME a,b,c,d;if(!GetProcessTimes(handle,out a,out b,out c,out d))throw new InvalidOperationException("Native browser creation query unavailable.");return a.Bits;}
  public bool SameAlive(){return handle!=IntPtr.Zero&&GetProcessId(handle)==pid&&Creation()==created&&String.Equals(Image(),image,StringComparison.OrdinalIgnoreCase)&&WaitForSingleObject(handle,0)==258;}
  public uint Pid {get{return pid;}}public ulong Created {get{return created;}}
  public bool SameImageAs(ReadOnlyBrowser other){return other!=null&&String.Equals(image,other.image,StringComparison.OrdinalIgnoreCase);}
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
# Both exact tests use the real unhandled WebView2 Chromium Views backend. They retain
# independent Rust observers/profiles/receipts, but never substitute a guessed Win32 HMENU.
$reportName=if($ProbeKind -ceq 'ExtensionObservation'){'native-context-observation.json'}else{'default-context-cancellation.json'}
$reportScope=if($ProbeKind -ceq 'ExtensionObservation'){'single-test-observation-and-cancellation-not-selection'}else{'separate-default-Chromium-Views-menu-cancellation-not-selection'}
$report=[ordered]@{scope=$reportScope;backend='unhandled-WebView2-Chromium-Views';status='failed';right_input_called=$false;escape_called=$false;error=$null;failure_cleanup=$null}
$observationClock=[Diagnostics.Stopwatch]::StartNew();$report.topology=@()
$firstInventoryProof=$null;$freshInventoryProof=$null;$issuedRequestDigest=$null;$diagnosticReadinessDeadline=[UInt64]0
$report.inventory_first=@{phase='first-complete';capture_status='unavailable';reason='not-serialized-before-release-or-cleanup'}
$report.inventory_fresh=@{phase='fresh-before-escape';capture_status='unavailable';reason='not-serialized-before-release-or-cleanup'}
$h=[IntPtr]$OwnerHandle;$pidNow=[uint32]0;$tid=[BowserDefaultContext]::GetWindowThreadProcessId($h,[ref]$pidNow);$guard=$null;$browserGuard=$null;$gpuGuard=$null
function Save-Report {[IO.File]::WriteAllText((Join-Path $Out $reportName),($report|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))}
# Use only already-captured bounded strings. No provider/native read, retry or input.
# Call only after proved release or confirmed exact test-process failure cleanup.
function Attach-InventoryEvidence {
 try {
  $report.inventory_first=[BowserDefaultContext]::InventoryReceiptValue($firstInventoryProof,'first-complete',$OwnershipNonce,$SourceProof,$issuedRequestDigest)|ConvertFrom-Json
  $report.inventory_fresh=[BowserDefaultContext]::InventoryReceiptValue($freshInventoryProof,'fresh-before-escape',$OwnershipNonce,$SourceProof,$issuedRequestDigest)|ConvertFrom-Json
 } catch {
  $report.inventory_first=@{phase='first-complete';capture_status='unavailable';reason='evidence-serialization-failed'}
  $report.inventory_fresh=@{phase='fresh-before-escape';capture_status='unavailable';reason='evidence-serialization-failed'}
 }
}
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
 $proof=[BowserDefaultContext]::ReadMenu($h,$OwnerPid,$tid,$popup,$BrowserPid,$browserGuard,$gpuGuard,$AuthorityDeadlineTick,$Out,$diagnosticReadinessDeadline)
 if(($gpuGuard -and [BowserDefaultContext]::GetTickCount64() -ge $AuthorityDeadlineTick) -or ![BowserDefaultContext]::NativeReady($h,$OwnerPid,$tid,$popup,$BrowserPid,$menuTid,$proof.Focus)){throw 'Native ownership changed across UIA read.'}
 return $proof
}

try {
 if($pidNow -ne $OwnerPid -or ![BowserDefaultContext]::Host($h,$OwnerPid,$tid)){throw 'Exact disposable host unavailable; no input.'}
 $guard=[BowserDefaultContext+OriginalProcess]::new($OwnerPid,$OwnerExe)
 $browserGuard=[BowserDefaultContext+ReadOnlyBrowser]::new($BrowserPid)
 if($GpuPid -ne 0){
  if($GpuPid -eq $BrowserPid -or $GpuPid -eq $OwnerPid -or !$GpuCreation -or !$BrowserCreation -or $browserGuard.Created -ne $BrowserCreation -or $OwnershipNonce -cnotmatch '^[0-9A-Za-z-]{16,128}$' -or $SourceProof -cnotmatch '^(0|[1-9][0-9]*),(0|[1-9][0-9]*),(0|[1-9][0-9]*)$' -or $EnvironmentIdentity -cnotmatch '^[1-9][0-9]*$' -or !$AuthorityDeadlineTick -or [BowserDefaultContext]::GetTickCount64() -ge $AuthorityDeadlineTick){throw 'Exact source/GPU process authority arguments unavailable.'}
  $gpuGuard=[BowserDefaultContext+ReadOnlyBrowser]::new($GpuPid)
  if($gpuGuard.Created -ne $GpuCreation -or !$gpuGuard.SameImageAs($browserGuard) -or !$gpuGuard.SameAlive()){throw 'GPU helper process lease differs from retained Rust lease.'}
 }
 $baselineComplete=$false;$baseline=@([BowserDefaultContext]::Candidates($h,$BrowserPid,[ref]$baselineComplete))
 if(!$baselineComplete -or $baseline.Count -ne 0){throw 'Fresh popup absence not established before input.'}
 Observe-Topology 'before-input'
 $point=[BowserDefaultContext+POINT]::new();$point.X=$ClientX;$point.Y=$ClientY
 if(![BowserDefaultContext]::ClientToScreen($h,[ref]$point)){throw 'Exact controlled point conversion failed.'}
 Neutral
 if(![BowserDefaultContext]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point)){throw 'Exact owned input target unavailable before cursor.'}
 if(![BowserDefaultContext]::SetCursorPos($point.X,$point.Y)){throw 'Native cursor placement failed.'}
 Neutral
 if(($gpuGuard -and [BowserDefaultContext]::GetTickCount64() -ge $AuthorityDeadlineTick) -or ![BowserDefaultContext]::Input($h,$OwnerPid,$tid,$ClientX,$ClientY,$ViewportWidth,$ViewportHeight,$point)){throw 'Exact owned hit target or shared ownership deadline changed before real right-click.'}
 if($gpuGuard -and [BowserDefaultContext]::GetTickCount64() -ge $AuthorityDeadlineTick){throw 'Shared ownership deadline expired after final pre-click native reads; no input.'}
 try{[BowserDefaultContext]::mouse_event(0x0008,0,0,0,[UIntPtr]::Zero);$report.right_input_called=$true}finally{[BowserDefaultContext]::mouse_event(0x0010,0,0,0,[UIntPtr]::Zero)}
 $diagnosticReadinessDeadline=[BowserDefaultContext]::GetTickCount64()+[UInt64]10000
 $watch=[Diagnostics.Stopwatch]::StartNew();Observe-Topology 'after-right-input';$report.readiness=@{elapsed_ms=0;last_predicate='not-observed';polls=0};$menu=$null
 do {
  $report.readiness.polls++;$menu=Menu-State -AllowPending;$report.readiness.elapsed_ms=$watch.ElapsedMilliseconds
  if($watch.ElapsedMilliseconds -ge 10000){Observe-Topology 'readiness-expired';throw 'Native popup readiness window expired; topology unavailable.'}
  if($menu){$report.readiness.last_predicate='strict-native-menu-ready';break}
  Start-Sleep -Milliseconds 50
 } while($true)
 $firstInventoryProof=$menu
 $report.popup=@{window=$menu.Window.ToInt64();thread=$menu.Thread;focus_witness=$menu.Focus.ToInt64();root_runtime=$menu.RootId;menu_runtime=$menu.MenuId;item_runtime=$menu.ItemId;label='exact-fixture-label';provider_pid=$BrowserPid;left=$menu.Left;top=$menu.Top;width=$menu.Width;height=$menu.Height}
 if($menu.GpuLeaves -eq 1){
  if(!$gpuGuard -or !$gpuGuard.SameAlive() -or !$browserGuard.SameAlive() -or $watch.ElapsedMilliseconds -ge 10000){throw 'GPU ownership freshness unavailable before challenge.'}
  $request=[BowserDefaultContext]::MembershipRequest($menu,$OwnershipNonce,$SourceProof)
  $expected=[BowserDefaultContext]::MembershipResponse($menu,$OwnershipNonce,$SourceProof,$BrowserPid,$GpuPid,$BrowserCreation,$GpuCreation,$EnvironmentIdentity,$AuthorityDeadlineTick)
  [BowserDefaultContext]::WriteMembershipRequest($Out,$request)
  $issuedRequestDigest=[BowserDefaultContext]::InventoryDigest($menu)
  $responsePath=Join-Path $Out 'gpu-membership-response.json'
  while(![IO.File]::Exists($responsePath)){
   if($watch.ElapsedMilliseconds -ge 10000 -or [BowserDefaultContext]::GetTickCount64() -ge $AuthorityDeadlineTick -or !$guard.SameAlive() -or !$browserGuard.SameAlive() -or !$gpuGuard.SameAlive() -or ![BowserDefaultContext]::NativeReady($h,$OwnerPid,$tid,$menu.Window,$BrowserPid,$menu.Thread,$menu.Focus)){throw 'Fresh membership response unavailable within original readiness budget.'}
   Start-Sleep -Milliseconds 10
  }
  $response=[BowserDefaultContext]::ReadMembershipResponse($responsePath)
  if($watch.ElapsedMilliseconds -ge 10000 -or [BowserDefaultContext]::GetTickCount64() -ge $AuthorityDeadlineTick -or ![BowserDefaultContext]::ExactMembershipResponse($response,$expected)){throw 'Fresh membership receipt mismatched, malformed, replayed or late.'}
  $report.owned_gpu_leaf=@{count=$menu.GpuLeaves;complete_inventory_digest=[BowserDefaultContext]::InventoryDigest($menu);fresh_environment_membership=$true;unknown_optional_UIA_flags_not_reinterpreted=$true}
 }
 Neutral;$fresh=Menu-State;$freshInventoryProof=$fresh
 if(![BowserDefaultContext]::SameProof($menu,$fresh)){throw 'Captured native/UIA menu identity changed before Escape.'}
 Neutral
 if($watch.ElapsedMilliseconds -ge 10000 -or ($gpuGuard -and [BowserDefaultContext]::GetTickCount64() -ge $AuthorityDeadlineTick) -or !$guard.SameAlive() -or !$browserGuard.SameAlive() -or ($gpuGuard -and !$gpuGuard.SameAlive()) -or ![BowserDefaultContext]::NativeReady($h,$OwnerPid,$tid,$menu.Window,$BrowserPid,$menu.Thread,$menu.Focus)){throw 'Exact foreground/native menu changed or original deadline expired immediately before Escape.'}
 if($watch.ElapsedMilliseconds -ge 10000 -or ($gpuGuard -and [BowserDefaultContext]::GetTickCount64() -ge $AuthorityDeadlineTick)){throw 'Original readiness/shared deadline expired after final native reads; no Escape.'}
 $watch.Stop()
 try{[BowserDefaultContext]::keybd_event(27,0,0,[UIntPtr]::Zero);$report.escape_called=$true}finally{[BowserDefaultContext]::keybd_event(27,0,2,[UIntPtr]::Zero)}
 $releaseWatch=[Diagnostics.Stopwatch]::StartNew();$released=$false
 do {$released=[BowserDefaultContext]::Released($h,$OwnerPid,$tid,$menu.Window,$BrowserPid,$menu.Thread,$menu.Focus);if($releaseWatch.ElapsedMilliseconds -ge 5000){throw 'Menu disappearance/capture release not confirmed in observation window.'};if($released){break};Start-Sleep -Milliseconds 50}while($true)
 if(!$guard.SameAlive() -or !$browserGuard.SameAlive()){throw 'Process identity unavailable after cancellation.'}
 $report.capture_released=$true;$report.menu_hidden_or_destroyed=$true
 $report.status='observed-owned-popup-and-real-cancellation';Attach-InventoryEvidence;Save-Report
} catch {
 $report.error=$_.Exception.Message
 try{Save-Report}catch{[Console]::Error.WriteLine('Native observation failure report write failed.')}
 if($report.right_input_called -and $guard){$report.failure_cleanup=$guard.FailOnly($h,$tid);if($report.failure_cleanup -ceq 'exact-single-test-process-terminated'){
   Attach-InventoryEvidence
   # New full rejected shapes are buffered only. Serialize/attach after exact cleanup,
   # separately from complete/fresh inventories and never before Escape or to membership.
   try{$rejected=[BowserDefaultContext]::RejectedInventoryEvidence();if($rejected){$report.rejected_popup_shapes=$rejected|ConvertFrom-Json;[IO.File]::WriteAllText((Join-Path $Out 'rejected-popup-shapes.json'),$rejected,[Text.UTF8Encoding]::new($false))}}catch{$report.rejected_popup_shapes=@{schema='bowser-rejected-popup-shapes-v1';coverage='partial';stop_reason='post-cleanup-serialization-failed'}}
  };try{Save-Report}catch{[Console]::Error.WriteLine('Native observation cleanup report write failed.')}}
 [Console]::Error.WriteLine('Native context observation failed; no selection compatibility verdict.');exit 75
} finally {if($gpuGuard){$gpuGuard.Dispose()};if($browserGuard){$browserGuard.Dispose()};if($guard){$guard.Dispose()}}
Write-Output "CHROMIUM_CONTEXT_CANCELLATION probe=$ProbeKind exact_owned_right_input=true exact_visible_native_popup=true exact_UIA_fixture_row=true real_escape_and_disappearance=true selection_performed=false"
