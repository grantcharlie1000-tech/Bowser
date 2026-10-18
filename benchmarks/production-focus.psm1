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

# Pure coverage accounting. Registration completion bounds callbacks, not controller creation.
function Get-ProductionControllerFocusCoverage {
 [CmdletBinding()]
 param([object[]]$Rows,[string[]]$ExpectedViews=@('toolbar','tabs','rail','content'))
 $Rows=@($Rows|Where-Object {$null -ne $_})
 $reasons=[Collections.Generic.List[string]]::new();$controllers=[Collections.Generic.List[object]]::new()
 $created=@($Rows|Where-Object event -eq interaction_focus_controller_created)
 if(-not $created.Count){$reasons.Add('no_controller_identities')}
 foreach($view in $ExpectedViews){if(-not @($created|Where-Object view -CEQ $view).Count){$reasons.Add("missing_expected_view:$view")}}
 if(@($Rows|Where-Object event -eq interaction_focus_coverage_error).Count){$reasons.Add('native_focus_coverage_error')}
 $seen=@{}
 foreach($row in $created){
  $id=[uint64]0
  if(-not [uint64]::TryParse([string]$row.navigation_id,[ref]$id) -or $id -eq 0){$reasons.Add('invalid_controller_identity');continue}
  if($seen.ContainsKey([string]$id)){$reasons.Add('duplicate_controller_identity');continue};$seen[[string]$id]=$true
  $events=@($Rows|Where-Object {$_.navigation_id -ceq [string]$id -and $_.event -like 'interaction_*'})
  $start=@($events|Where-Object event -eq interaction_focus_registration_started)
  $got=@($events|Where-Object event -eq interaction_focus_got_registration)
  $lost=@($events|Where-Object event -eq interaction_focus_lost_registration)
  $complete=$start.Count -eq 1 -and $got.Count -eq 1 -and $lost.Count -eq 1
  if($complete){$complete=$got[0].success -ceq 'true' -and $lost[0].success -ceq 'true' -and [long]$start[0].qpc_ticks -ge [long]$row.qpc_ticks -and [long]$got[0].qpc_ticks -ge [long]$start[0].qpc_ticks -and [long]$lost[0].qpc_ticks -ge [long]$start[0].qpc_ticks}
  if(-not $complete){$reasons.Add("incomplete_registration:$id")}
  if(@($events|Where-Object {$_.view -cne $row.view -or $_.tab_id -cne $row.tab_id}).Count){$reasons.Add("controller_identity_mismatch:$id")}
  $pending=0
  foreach($event in $events){
   if($event.event -in @('interaction_chrome_move_request','interaction_content_move_request')){$pending++}
   if($event.event -in @('interaction_chrome_move_result','interaction_content_move_result')){$pending--;if($pending -lt 0){$reasons.Add("unmatched_move_result:$id")}}
  }
  if($pending -ne 0){$reasons.Add("unmatched_move_request:$id")}
  $coverage=if($complete){[Math]::Max([long]$got[0].qpc_ticks,[long]$lost[0].qpc_ticks)}else{$null}
  $controllers.Add([ordered]@{controller_id=$id;view=$row.view;tab_id=$row.tab_id;created_qpc=$row.qpc_ticks;callback_coverage_start_qpc=$coverage;callbacks_before_coverage=if($complete){@($events|Where-Object {$_.event -in @('interaction_controller_got_focus','interaction_controller_lost_focus') -and [long]$_.qpc_ticks -lt $coverage}).Count}else{$null};scope='Callback absence is interpretable only after this controller coverage start; no earlier focus/activation absence inferred'})
 }
 $traceNames=@('interaction_focus_registration_started','interaction_focus_got_registration','interaction_focus_lost_registration','interaction_controller_got_focus','interaction_controller_lost_focus','interaction_chrome_move_request','interaction_chrome_move_result','interaction_chrome_move_busy','interaction_chrome_move_closed','interaction_content_move_request','interaction_content_move_result')
 foreach($row in @($Rows|Where-Object {$_.event -in $traceNames})){if(-not $seen.ContainsKey([string]$row.navigation_id)){$reasons.Add('trace_without_controller_identity')}}
 [ordered]@{complete=($reasons.Count -eq 0);expected_views=$ExpectedViews;invalid_reasons=@($reasons.ToArray());controllers=@($controllers.ToArray());scope='Complete registered callback and explicit Bowser MoveFocus coverage, NOT every SetFocus/activation/visibility side effect; HWND observations do not alone identify a controller receiver'}
}
Export-ModuleMember -Function Get-ProductionNativeFocusSnapshot,Get-ProductionControllerFocusCoverage
