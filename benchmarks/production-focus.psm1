# Passive fixed-boundary OS keyboard-focus observation. Never calls a focus/input API.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if(-not ('BowserFocusObservation' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Collections.Generic;
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
 public sealed class TopologyNode {
  public WindowInfo window;
  public RECT rect;
  public bool rect_ok,visible,style_ok,ex_style_ok,identity_stable;
  public int rect_error,style_error,ex_style_error;
  public long style,ex_style;
 }
 public sealed class TopologyChain {
  public bool complete;
  public string reason,container_handle,container_class;
  public List<TopologyNode> nodes=new List<TopologyNode>();
 }
 public sealed class TopologyPair {
  public string classification;
  public bool root_identity_stable;
  public TopologyChain original,current;
 }
 [DllImport("user32.dll",SetLastError=true)] static extern bool GetGUIThreadInfo(uint thread,ref GUITHREADINFO info);
 [DllImport("user32.dll",SetLastError=true)] static extern uint GetWindowThreadProcessId(IntPtr window,out uint pid);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr window,uint flags);
 [DllImport("user32.dll")] static extern bool IsChild(IntPtr parent,IntPtr child);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window,StringBuilder value,int count);
 [DllImport("user32.dll",SetLastError=true)] static extern bool GetWindowRect(IntPtr window,out RECT rect);
 [DllImport("user32.dll")] static extern bool IsWindow(IntPtr window);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
 [DllImport("user32.dll",EntryPoint="GetWindowLongPtrW",SetLastError=true)] static extern IntPtr GetWindowLongPtr64(IntPtr window,int index);
 [DllImport("user32.dll",EntryPoint="GetWindowLongW",SetLastError=true)] static extern int GetWindowLong32(IntPtr window,int index);
 [DllImport("kernel32.dll",EntryPoint="SetLastError")] static extern void ClearLastError(uint error);
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
 static bool KnownContainer(string label){return label=="BowserSettingsPage"||label=="BowserHtmlPopup"||label=="BowserHtmlHover"||label=="Chrome_WidgetWin_0";}
 public static bool KnownOwnedTopLevel(bool ancestryKnown,string handle,string gaRoot,string gaOwner,string ownedRoot,string label){
  return ancestryKnown&&handle!="0x0"&&handle!=ownedRoot&&handle==gaRoot&&gaOwner==ownedRoot&&KnownContainer(label);
 }
 static long Style(IntPtr h,int index,out bool ok,out int error){
  ClearLastError(0);long value=IntPtr.Size==8?GetWindowLongPtr64(h,index).ToInt64():GetWindowLong32(h,index);
  error=value==0?Marshal.GetLastWin32Error():0;ok=error==0;return value;
 }
 static TopologyChain Chain(IntPtr start,uint expectedPid,uint expectedTid,IntPtr root){
  var result=new TopologyChain{reason="unknown",container_handle="0x0",container_class="unknown"};
  if(start==IntPtr.Zero||expectedPid==0||expectedTid==0){result.reason="missing_anchor_or_owner";return result;}
  var seen=new HashSet<long>();var h=start;
  for(int depth=0;depth<16;depth++){
   if(h==IntPtr.Zero||!IsWindow(h)){result.reason="null_or_destroyed_handle";return result;}
   if(!seen.Add(h.ToInt64())){result.reason="parent_cycle";return result;}
   var before=Describe(h,root);
   if(!before.owner_query_ok||(depth==0&&(before.process_id!=expectedPid||before.thread_id!=expectedTid))){result.reason="owner_missing_or_changed";return result;}
   var node=new TopologyNode{window=before};
   node.rect_ok=GetWindowRect(h,out node.rect);node.rect_error=node.rect_ok?0:Marshal.GetLastWin32Error();
   node.visible=IsWindowVisible(h);node.style=Style(h,-16,out node.style_ok,out node.style_error);node.ex_style=Style(h,-20,out node.ex_style_ok,out node.ex_style_error);
   var parent=GetAncestor(h,1);uint afterPid=0;uint afterTid=GetWindowThreadProcessId(h,out afterPid);
   node.identity_stable=IsWindow(h)&&afterTid!=0&&afterTid==before.thread_id&&afterPid==before.process_id;
   result.nodes.Add(node);
   if(!node.identity_stable||!node.rect_ok||!node.style_ok||!node.ex_style_ok){result.reason="identity_or_geometry_query_incomplete";return result;}
   if(h==root){result.complete=true;result.reason="owned_parent_chain";return result;}
   if(KnownOwnedTopLevel(before.ancestry_query_ok,before.handle,before.root_handle,before.root_owner_handle,Handle(root),before.class_label)){
    result.container_handle=before.handle;result.container_class=before.class_label;result.complete=true;result.reason="known_owned_top_level_chain";return result;
   }
   if(parent==root){result.container_handle=before.handle;result.container_class=before.class_label;}
   if(parent==IntPtr.Zero){result.reason="parent_lookup_or_endpoint_unknown";return result;}
   h=parent;
  }
  result.reason="depth_limit";return result;
 }
 public static string Classify(bool originalComplete,bool currentComplete,string root,string[] original,string[] current,string originalContainer,string originalClass,string currentContainer,string currentClass){
  if(!originalComplete||!currentComplete)return "unknown";
  foreach(var a in original)foreach(var b in current)if(a!="0x0"&&a!=root&&a==b)return "same_native_subtree";
  if(originalContainer!="0x0"&&currentContainer!="0x0"&&originalContainer!=currentContainer&&KnownContainer(originalClass)&&KnownContainer(currentClass))return "different_known_container";
  return "unknown";
 }
 public static TopologyPair CaptureTopology(IntPtr root,WindowInfo expectedRoot,WindowInfo original,WindowInfo current){
  var result=new TopologyPair{classification="unknown"};
  result.original=Chain(original==null?IntPtr.Zero:new IntPtr(Convert.ToInt64(original.handle.Substring(2),16)),original==null?0:original.process_id,original==null?0:original.thread_id,root);
  result.current=Chain(current==null?IntPtr.Zero:new IntPtr(Convert.ToInt64(current.handle.Substring(2),16)),current==null?0:current.process_id,current==null?0:current.thread_id,root);
  var a=new List<string>();foreach(var n in result.original.nodes)a.Add(n.window.handle);var b=new List<string>();foreach(var n in result.current.nodes)b.Add(n.window.handle);
  result.classification=Classify(result.original.complete,result.current.complete,Handle(root),a.ToArray(),b.ToArray(),result.original.container_handle,result.original.container_class,result.current.container_handle,result.current.container_class);
  uint rootPid=0;uint rootTid=GetWindowThreadProcessId(root,out rootPid);
  result.root_identity_stable=expectedRoot!=null&&IsWindow(root)&&rootTid!=0&&rootTid==expectedRoot.thread_id&&rootPid==expectedRoot.process_id;
  if(!result.root_identity_stable)result.classification="unknown";
  return result;
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
 param([uint32]$RootPid,[IntPtr]$RootWindow,[string]$Boundary,[int]$Attempt,[switch]$IncludeTopology,[object]$OriginalFocus)
 $start=[Diagnostics.Stopwatch]::GetTimestamp();$capture=$null;$error=$null;$topology=$null;$topologyError=$null
 try{$capture=[BowserFocusObservation]::Capture($RootPid,$RootWindow)}catch{$error=$_.Exception.GetType().Name}
 if($IncludeTopology -and $null -ne $capture -and $capture.query_ok){try{$topology=[BowserFocusObservation]::CaptureTopology($RootWindow,$capture.owned_root,$OriginalFocus,$capture.focus)}catch{$topologyError=$_.Exception.GetType().Name}}
 $end=[Diagnostics.Stopwatch]::GetTimestamp()
 [ordered]@{boundary=$Boundary;attempt=$Attempt;start_qpc=$start;end_qpc=$end;observation_ms=($end-$start)*1000.0/[Diagnostics.Stopwatch]::Frequency;api_exception=$error;native=$capture;topology=$topology;topology_exception=$topologyError;scope='Owned root GUI thread; non-atomic HWND ownership/ancestry/geometry only, NOT controller identity; same-PID/TID handle reuse cannot be excluded; no title/text or focus repair'}
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
