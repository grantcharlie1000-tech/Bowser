# One ordinary click on the source-owned synthetic light fixture, never a focus workaround.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if(-not ('BowserSidebarInput' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class BowserSidebarInput {
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x,y;public POINT(int a,int b){x=a;y=b;} }
 [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx,dy;public uint mouseData,dwFlags,time;public UIntPtr dwExtraInfo; }
 [StructLayout(LayoutKind.Explicit)] public struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; }
 [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type;public UNION data; }
 [DllImport("user32.dll",SetLastError=true)] public static extern uint SendInput(uint count,INPUT[] inputs,int size);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT point);
 [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint flags);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder text,int count);
 public static int InputSize(){return Marshal.SizeOf(typeof(INPUT));}
 public static int Absolute(int position,int origin,int length){if(length<2 || position<origin || (long)position>=(long)origin+length)throw new ArgumentOutOfRangeException();return (int)Math.Round(((double)position-origin)*65535.0/(length-1));}
 public sealed class ClickResult {
  public bool guard_ok;public string refusal="not_attempted";public int x,y,virtual_x,virtual_y,virtual_width,virtual_height,absolute_x,absolute_y,input_size,error,monitor_count;public uint root_dpi;public bool root_class_matches;
  public long hit_hwnd,hit_root_hwnd;public uint root_pid,root_tid,hit_pid,hit_tid,sent_count;public uint planned_count=3;public int[] held_keys=new int[0];
 }
 public static ClickResult ClickOwned(IntPtr expectedRoot,uint expectedPid,int x,int y){
  var r=new ClickResult{x=x,y=y,input_size=InputSize()};
  if(expectedRoot==IntPtr.Zero || expectedPid==0){r.refusal="missing_owned_root";return r;}
  r.root_tid=GetWindowThreadProcessId(expectedRoot,out r.root_pid);
  if(r.root_tid==0 || r.root_pid!=expectedPid || GetAncestor(expectedRoot,2)!=expectedRoot){r.refusal="root_ownership_unknown";return r;}
  var rootClass=new StringBuilder(128);r.root_class_matches=GetClassName(expectedRoot,rootClass,rootClass.Capacity)>0 && rootClass.ToString()=="BowserNativeWindow";
  r.monitor_count=GetSystemMetrics(80);r.root_dpi=GetDpiForWindow(expectedRoot);
  if(!r.root_class_matches || r.monitor_count!=1 || r.root_dpi!=96){r.refusal="unverified_root_class_or_coordinate_space";return r;}
  r.virtual_x=GetSystemMetrics(76);r.virtual_y=GetSystemMetrics(77);r.virtual_width=GetSystemMetrics(78);r.virtual_height=GetSystemMetrics(79);
  try{r.absolute_x=Absolute(x,r.virtual_x,r.virtual_width);r.absolute_y=Absolute(y,r.virtual_y,r.virtual_height);}catch(ArgumentOutOfRangeException){r.refusal="outside_physical_virtual_desktop";return r;}
  var held=new System.Collections.Generic.List<int>();foreach(int key in new[]{1,2,4,5,6,16,17,18,91,92})if((GetAsyncKeyState(key)&0x8000)!=0)held.Add(key);
  r.held_keys=held.ToArray();if(held.Count!=0){r.refusal="nonneutral_mouse_or_modifiers";return r;}
  var hit=WindowFromPoint(new POINT(x,y));r.hit_hwnd=hit.ToInt64();r.hit_root_hwnd=GetAncestor(hit,2).ToInt64();r.hit_tid=GetWindowThreadProcessId(hit,out r.hit_pid);
  uint checkRootPid,checkHitPid;
  if(hit==IntPtr.Zero || r.hit_tid==0 || r.hit_root_hwnd!=expectedRoot.ToInt64() || GetWindowThreadProcessId(expectedRoot,out checkRootPid)!=r.root_tid || checkRootPid!=expectedPid || GetWindowThreadProcessId(hit,out checkHitPid)!=r.hit_tid || checkHitPid!=r.hit_pid || GetAncestor(hit,2)!=expectedRoot){r.refusal="occluded_unmapped_or_changed_point_owner";return r;}
  var inputs=new[]{new INPUT{data=new UNION{mi=new MOUSEINPUT{dx=r.absolute_x,dy=r.absolute_y,dwFlags=0x8000|0x4000|0x0001}}},new INPUT{data=new UNION{mi=new MOUSEINPUT{dwFlags=0x0002}}},new INPUT{data=new UNION{mi=new MOUSEINPUT{dwFlags=0x0004}}}};
  r.guard_ok=true;r.refusal=null;
  r.sent_count=SendInput(3,inputs,r.input_size);r.error=r.sent_count==3?0:Marshal.GetLastWin32Error();return r;
 }
}
'@
}
function Invoke-ProductionSidebarActivation {
 param([uint32]$RootPid,[Parameter(Mandatory)][string]$InitialToken,[Parameter(Mandatory)][AllowEmptyCollection()][Collections.Generic.List[object]]$Trace)
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
 $start=[Diagnostics.Stopwatch]::GetTimestamp();$root=[BowserProductionNative]::FindHost($RootPid)
 $record=[ordered]@{kind='sidebar-owned-activation-click';start_qpc=$start;end_qpc=$null;document_count=$null;document_bounds=$null;heading=$null;click=$null;foreground_confirmed=$false;foreground_wait_ms=$null;scope='Exactly one ordinary click on the noninteractive heading in the uniquely identified local light fixture; physical screen coordinates, non-atomic owner/neutral-state checks; no retries, focus setter or performance claim'}
 $Trace.Add($record)
 try {
  $rootElement=[Windows.Automation.AutomationElement]::FromHandle($root)
  $condition=[Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,"BOWSER_PERF_READY_$InitialToken"),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Document))
  $docs=@($rootElement.FindAll([Windows.Automation.TreeScope]::Descendants,$condition));$record.document_count=$docs.Count
  if($docs.Count -ne 1){throw 'Activation requires one exact synthetic fixture document'}
  $current=$docs[0].Current;$r=$current.BoundingRectangle
  $record.document_bounds=[ordered]@{left=$r.Left;top=$r.Top;width=$r.Width;height=$r.Height;provider_pid=$current.ProcessId;offscreen=$current.IsOffscreen}
  $headingCondition=[Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Production performance fixture'),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Text))
  $headings=@($docs[0].FindAll([Windows.Automation.TreeScope]::Descendants,$headingCondition))
  if($headings.Count -ne 1){throw 'One exact noninteractive synthetic heading required; no alternative click target'}
  $h=$headings[0].Current;$hr=$h.BoundingRectangle
  $record.heading=[ordered]@{name='Production performance fixture';control_type=$h.ControlType.ProgrammaticName;provider_pid=$h.ProcessId;offscreen=$h.IsOffscreen;keyboard_focusable=$h.IsKeyboardFocusable;bounds=@{left=$hr.Left;top=$hr.Top;width=$hr.Width;height=$hr.Height}}
  if($current.IsOffscreen -or -not $current.IsEnabled -or $h.IsOffscreen -or -not $h.IsEnabled -or $h.IsKeyboardFocusable -or $hr.Width -le 10 -or $hr.Height -le 10 -or $hr.Left -lt $r.Left -or $hr.Top -lt $r.Top -or $hr.Right -gt $r.Right -or $hr.Bottom -gt $r.Bottom){throw 'Source-bound heading is not a visible noninteractive region in the fixture document'}
  $x=[int][Math]::Floor($hr.Left+$hr.Width/2);$y=[int][Math]::Floor($hr.Top+$hr.Height/2)
  $record.click=[BowserSidebarInput]::ClickOwned($root,$RootPid,$x,$y)
  if(-not $record.click.guard_ok -or $record.click.sent_count -ne 3){throw 'Single owned activation click refused or incomplete; no retry'}
  $wait=[Diagnostics.Stopwatch]::StartNew()
  do {if([BowserProductionNative]::GetForegroundWindow() -eq $root){$record.foreground_confirmed=$true;break};Start-Sleep -Milliseconds 20}while($wait.Elapsed.TotalSeconds -lt 1)
  $record.foreground_wait_ms=$wait.Elapsed.TotalMilliseconds
  if(-not $record.foreground_confirmed -or [BowserProductionNative]::FindHost($RootPid) -ne $root){throw 'Owned app did not become foreground after its one ordinary click'}
 } finally {$record.end_qpc=[Diagnostics.Stopwatch]::GetTimestamp()}
}
Export-ModuleMember -Function Invoke-ProductionSidebarActivation
