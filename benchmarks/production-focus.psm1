# Passive fixed-boundary OS keyboard-focus observation. Never calls a focus/input API.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if(-not ('BowserFocusObservation' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class BowserFocusObservation {
 [StructLayout(LayoutKind.Sequential)] public struct RECT {public int Left,Top,Right,Bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO {
  public uint cbSize,flags;
  public IntPtr hwndActive,hwndFocus,hwndCapture,hwndMenuOwner,hwndMoveSize,hwndCaret;
  public RECT rcCaret;
 }
 public sealed class WindowInfo {
  public string handle,relation,class_label,root_handle,root_owner_handle;
  public uint process_id,thread_id;
  public bool owner_query_ok,ancestry_query_ok;
 }
 public sealed class Snapshot {
  public bool query_ok;
  public int error,root_error;
  public uint root_thread_id,root_process_id,gui_flags;
  public WindowInfo owned_root,foreground,active,focus;
 }
 [DllImport("user32.dll",SetLastError=true)] static extern bool GetGUIThreadInfo(uint thread,ref GUITHREADINFO info);
 [DllImport("user32.dll",SetLastError=true)] static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr window,uint flags);
 [DllImport("user32.dll")] static extern bool IsChild(IntPtr parent,IntPtr child);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window,StringBuilder value,int count);
 static string Handle(IntPtr h){return "0x"+h.ToInt64().ToString("x");}
 public static string Relation(bool present,bool ownerKnown,bool same,bool child,bool ownedPopup){
  if(!present)return "none";
  if(!ownerKnown)return "unknown";
  if(same)return "owned-root";
  if(child)return "owned-descendant";
  if(ownedPopup)return "owned-popup";
  return "outside-owned-root";
 }
 static string ClassLabel(IntPtr h){
  var name=new StringBuilder(128);
  if(GetClassName(h,name,name.Capacity)==0)return "unknown";
  switch(name.ToString()){
   case "BowserNativeWindow":case "BowserSettingsPage":case "BowserHtmlPopup":case "BowserHtmlHover":
   case "Chrome_WidgetWin_0":case "Chrome_RenderWidgetHostHWND":case "Chrome Legacy Window":case "Intermediate D3D Window":return name.ToString();
   default:return "other";
  }
 }
 static WindowInfo Describe(IntPtr h,IntPtr root){
  uint pid=0;uint tid=h==IntPtr.Zero?0:GetWindowThreadProcessId(h,out pid);
  var ancestor=h==IntPtr.Zero?IntPtr.Zero:GetAncestor(h,2);
  var owner=h==IntPtr.Zero?IntPtr.Zero:GetAncestor(h,3);
  return new WindowInfo{handle=Handle(h),process_id=pid,thread_id=tid,owner_query_ok=tid!=0,
   root_handle=Handle(ancestor),root_owner_handle=Handle(owner),ancestry_query_ok=ancestor!=IntPtr.Zero&&owner!=IntPtr.Zero,class_label=h==IntPtr.Zero?"none":ClassLabel(h),
   relation=Relation(h!=IntPtr.Zero,tid!=0&&ancestor!=IntPtr.Zero&&owner!=IntPtr.Zero,h==root,h!=IntPtr.Zero&&IsChild(root,h),h!=IntPtr.Zero&&owner==root)};
 }
 public static Snapshot Capture(uint expectedPid,IntPtr root){
  var result=new Snapshot();uint pid=0;
  result.root_thread_id=GetWindowThreadProcessId(root,out pid);result.root_error=result.root_thread_id==0?Marshal.GetLastWin32Error():0;result.root_process_id=pid;
  result.owned_root=Describe(root,root);result.foreground=Describe(GetForegroundWindow(),root);
  if(root==IntPtr.Zero||result.root_thread_id==0||pid!=expectedPid){result.error=-1;return result;}
  var info=new GUITHREADINFO{cbSize=(uint)Marshal.SizeOf(typeof(GUITHREADINFO))};
  result.query_ok=GetGUIThreadInfo(result.root_thread_id,ref info);
  if(!result.query_ok){result.error=Marshal.GetLastWin32Error();return result;}
  result.gui_flags=info.flags;result.active=Describe(info.hwndActive,root);result.focus=Describe(info.hwndFocus,root);
  return result;
 }
 public static int StructureSize(){return Marshal.SizeOf(typeof(GUITHREADINFO));}
}
'@
}
function Get-ProductionNativeFocusSnapshot {
 [CmdletBinding()]
 param([uint32]$RootPid,[IntPtr]$RootWindow,[string]$Boundary,[int]$Attempt)
 $start=[Diagnostics.Stopwatch]::GetTimestamp();$capture=$null;$error=$null
 try{$capture=[BowserFocusObservation]::Capture($RootPid,$RootWindow)}catch{$error=$_.Exception.GetType().Name}
 $end=[Diagnostics.Stopwatch]::GetTimestamp()
 [ordered]@{boundary=$Boundary;attempt=$Attempt;start_qpc=$start;end_qpc=$end;observation_ms=($end-$start)*1000.0/[Diagnostics.Stopwatch]::Frequency;api_exception=$error;native=$capture;scope='Owned root GUI thread; HWND ownership/ancestry only, NOT a controller/view identity; no title/text or focus repair'}
}
Export-ModuleMember -Function Get-ProductionNativeFocusSnapshot
