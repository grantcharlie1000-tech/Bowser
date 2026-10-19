# Passive provider/foreground discovery only. No activation, action or resource measurement.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if(-not ('BowserSidebarPreflight' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
public static class BowserSidebarPreflight {
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll",SetLastError=true)] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern int GetClassName(IntPtr h,StringBuilder text,int count);
 [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h,uint flags);
 [DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
 public sealed class WindowInfo {
  public long hwnd,root_hwnd,root_owner_hwnd; public uint pid,tid,pid_after,tid_after; public int class_result,class_error,owner_error_before,owner_error_after;
  public string window_class="unknown"; public bool ownership_stable; public string status="unknown";
 }
 public static WindowInfo Read(IntPtr h) {
  var r=new WindowInfo{hwnd=h.ToInt64()};if(h==IntPtr.Zero){r.status="null_handle";return r;}
  r.tid=GetWindowThreadProcessId(h,out r.pid);r.owner_error_before=r.tid==0?Marshal.GetLastWin32Error():0;
  var cls=new StringBuilder(128);r.class_result=GetClassName(h,cls,cls.Capacity);r.class_error=r.class_result==0?Marshal.GetLastWin32Error():0;
  if(r.class_result>0){switch(cls.ToString()){
   case "BowserNativeWindow":case "ConsoleWindowClass":case "CASCADIA_HOSTING_WINDOW_CLASS":case "Chrome_WidgetWin_0":case "Chrome_RenderWidgetHostHWND":case "Progman":case "WorkerW":case "Shell_TrayWnd":case "Windows.UI.Core.CoreWindow":r.window_class=cls.ToString();break;
   default:r.window_class="other";break;
  }}
  r.root_hwnd=GetAncestor(h,2).ToInt64();r.root_owner_hwnd=GetAncestor(h,3).ToInt64();
  r.tid_after=GetWindowThreadProcessId(h,out r.pid_after);r.owner_error_after=r.tid_after==0?Marshal.GetLastWin32Error():0;
  r.ownership_stable=r.pid!=0 && r.tid!=0 && r.pid==r.pid_after && r.tid==r.tid_after;
  r.status=r.ownership_stable && r.class_result>0 && r.root_hwnd!=0 && r.root_owner_hwnd!=0?"observed_nonatomic":"unknown_or_incomplete";
  return r;
 }
}
'@
}
function Get-ProductionSidebarPreflightSnapshot {
 param([uint32]$RootPid=0,[Parameter(Mandatory)][string]$Boundary)
 $start=[Diagnostics.Stopwatch]::GetTimestamp();$inventory=@(Get-ProcessInventory)
 $harness=@($inventory|Where-Object {$_.Pid -eq $PID});$parent=if($harness.Count -eq 1){$harness[0].Parent}else{$null}
 $root=if($RootPid){[BowserProductionNative]::FindHost($RootPid)}else{[IntPtr]::Zero}
 $foreground=[BowserSidebarPreflight]::Read([BowserSidebarPreflight]::GetForegroundWindow())
 $console=[BowserSidebarPreflight]::Read([BowserSidebarPreflight]::GetConsoleWindow())
 $owned=[BowserSidebarPreflight]::Read($root)
 $foregroundIdentity=@($inventory|Where-Object {$_.Pid -eq $foreground.pid})
 $foregroundMapped=($foregroundIdentity.Count -eq 1 -and $null -ne $foregroundIdentity[0].CreationTicks -and [long]$foregroundIdentity[0].CreationTicks -gt 0)
 $role=if($foreground.status -cne 'observed_nonatomic' -or -not $foregroundMapped -or ($RootPid -and ($owned.status -cne 'observed_nonatomic' -or $owned.pid -ne $RootPid -or $owned.hwnd -eq 0))){'unknown'}elseif($RootPid -and $owned.status -ceq 'observed_nonatomic' -and $owned.pid -eq $RootPid -and $foreground.pid -eq $RootPid -and $foreground.root_owner_hwnd -eq $root.ToInt64()){'owned_app'}elseif($foreground.pid -eq $PID){'harness'}elseif($null -ne $parent -and $foreground.pid -eq $parent){'harness_parent'}elseif($console.status -ceq 'observed_nonatomic' -and $foreground.pid -eq $console.pid){'console_owner'}else{'other_process'}
 $wantedPids=@(@([uint32]$PID,$parent,$foreground.pid,$console.pid,$RootPid)|Where-Object {$_}|Sort-Object -Unique)
 $identities=foreach($wanted in $wantedPids){
  $matches=@($inventory|Where-Object {$_.Pid -eq $wanted})
  [ordered]@{pid=$wanted;inventory_count=$matches.Count;parent_pid=if($matches.Count -eq 1){$matches[0].Parent}else{$null};created_ticks=if($matches.Count -eq 1){$matches[0].CreationTicks}else{$null};scope='Numeric process identity only; no titles, process paths or command lines'}
 }
 [ordered]@{kind='sidebar-preflight-foreground';boundary=$Boundary;start_qpc=$start;end_qpc=[Diagnostics.Stopwatch]::GetTimestamp();foreground=$foreground;foreground_role=$role;owned_root=$owned;expected_root_pid=$RootPid;console=$console;harness_pid=$PID;harness_parent_pid=$parent;identities=@($identities);inventory_available=($inventory.Count -gt 0);scope='Read-only non-atomic HWND/PID/TID/class/root-owner observations; other_process is not proof of contention; NULL/recycled/incomplete handles remain unknown; matching PID/TID cannot rule out same-process handle reuse; HWNDs are meaningful only within this run'}
}
function Get-ProductionSidebarCapabilities {
 param([uint32]$RootPid,[Parameter(Mandatory)][AllowEmptyCollection()][Collections.Generic.List[object]]$Trace)
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
 $Trace.Add((Get-ProductionSidebarPreflightSnapshot -RootPid $RootPid -Boundary 'post-ready-before-query'))
 $window=[BowserProductionNative]::FindHost($RootPid);$owner=[BowserSidebarPreflight]::Read($window)
 if(-not $owner.ownership_stable -or $owner.pid -ne $RootPid -or $owner.window_class -cne 'BowserNativeWindow'){throw 'Capability preflight owned native root unavailable'}
 $root=[Windows.Automation.AutomationElement]::FromHandle($window);$errors=[Collections.Generic.List[string]]::new()
 function Query($parent,[string]$name,$type) {
  $start=[Diagnostics.Stopwatch]::GetTimestamp()
  $record=[ordered]@{kind='sidebar-preflight-capabilities';name=$name;start_qpc=$start;end_qpc=$null;match_count=$null;elements=@();query_error=$null;source_binding='Exact unique owned BowserNativeWindow > Left rail > Fixture A/B; child provider PID need not equal native root'}
  $Trace.Add($record);$found=@()
  try {
   $condition=[Windows.Automation.AndCondition]::new([Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name),[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,$type))
   $found=@($parent.FindAll([Windows.Automation.TreeScope]::Descendants,$condition))
   $record.match_count=$found.Count
   $record.elements=@($found|ForEach-Object {$c=$_.Current;$r=$c.BoundingRectangle;[ordered]@{name=$name;provider_pid=$c.ProcessId;control_type=$c.ControlType.ProgrammaticName;is_enabled=$c.IsEnabled;is_offscreen=$c.IsOffscreen;bounds=@{left=$r.Left;top=$r.Top;width=$r.Width;height=$r.Height};patterns=@($_.GetSupportedPatterns()|ForEach-Object {[ordered]@{id=$_.Id;name=$_.ProgrammaticName}})}})
   if($found.Count -ne 1 -or $record.elements[0].is_offscreen -or -not $record.elements[0].is_enabled -or $record.elements[0].bounds.width -le 10 -or $record.elements[0].bounds.height -le 10){$errors.Add("unavailable_or_ambiguous:$name");$found=@()}
  } catch {$record.query_error=$_.Exception.GetType().FullName;$errors.Add("query_error:$name");$found=@()}
  finally {$record.end_qpc=[Diagnostics.Stopwatch]::GetTimestamp()}
  $found
 }
 $rail=@(Query $root 'Left rail' ([Windows.Automation.ControlType]::ToolBar))
 if($rail.Count -eq 1){foreach($name in @('Fixture A','Fixture B')){[void]@(Query $rail[0] $name ([Windows.Automation.ControlType]::Button))}}
 else {$errors.Add('unqueried_app_buttons_without_unique_rail')}
 $Trace.Add((Get-ProductionSidebarPreflightSnapshot -RootPid $RootPid -Boundary 'post-query'))
 $after=[BowserSidebarPreflight]::Read([BowserProductionNative]::FindHost($RootPid))
 if(-not $after.ownership_stable -or $after.pid -ne $RootPid -or $after.hwnd -ne $owner.hwnd){$errors.Add('owned_root_changed_during_query')}
 [ordered]@{complete=($errors.Count -eq 0);errors=@($errors.ToArray());scope='Passive source-bound supported-pattern discovery only; no activation/action, lifecycle count, resource result or action-correctness acceptance'}
}
Export-ModuleMember -Function Get-ProductionSidebarPreflightSnapshot,Get-ProductionSidebarCapabilities
