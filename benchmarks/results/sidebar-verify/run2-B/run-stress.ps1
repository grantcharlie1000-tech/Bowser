param([int]$ProcessId,[string]$Profile,[string]$OutDir,[int]$DurationSeconds=60,[int]$Actions=240)
$ErrorActionPreference='Stop'; Add-Type @"
using System;using System.Runtime.InteropServices;using System.Text;
public static class Stress32{
 [StructLayout(LayoutKind.Sequential)]public struct POINT{public int X,Y;}
 [StructLayout(LayoutKind.Sequential)]public struct RECT{public int L,T,R,B;}
 public delegate bool EnumProc(IntPtr h,IntPtr p);private static IntPtr found;
 [DllImport("user32.dll")]public static extern bool EnumWindows(EnumProc cb,IntPtr p);
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern int GetClassName(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll")]public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int cmd);
 [DllImport("user32.dll")]public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")]public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
 [DllImport("user32.dll")]public static extern bool ClientToScreen(IntPtr h,ref POINT p);
 [DllImport("user32.dll")]public static extern bool GetClientRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")]public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int hgt,uint flags);
 [DllImport("user32.dll")]public static extern void keybd_event(byte vk,byte scan,uint flags,UIntPtr extra);
 [DllImport("user32.dll")]public static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,IntPtr l,uint flags,uint ms,out IntPtr result);
 [DllImport("user32.dll")]public static extern uint GetGuiResources(IntPtr h,uint f);
 public static IntPtr Find(uint pid){found=IntPtr.Zero;EnumWindows((h,p)=>{uint id;GetWindowThreadProcessId(h,out id);if(id==pid&&IsWindowVisible(h)){var c=new StringBuilder(128);GetClassName(h,c,c.Capacity);if(c.ToString()=="BowserNativeWindow")found=h;}return true;},IntPtr.Zero);return found;}
 public static void Click(IntPtr h,int x,int y){var p=new POINT{X=x,Y=y};ClientToScreen(h,ref p);SetCursorPos(p.X,p.Y);System.Threading.Thread.Sleep(15);mouse_event(2,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(20);mouse_event(4,0,0,0,UIntPtr.Zero);}
 public static void Escape(){keybd_event(0x1b,0,0,UIntPtr.Zero);keybd_event(0x1b,0,2,UIntPtr.Zero);}
}
"@
$p=Get-Process -Id $ProcessId;$h=[Stress32]::Find([uint32]$ProcessId);if($h -eq [IntPtr]::Zero){throw 'BowserNativeWindow not found'};[void][Stress32]::ShowWindow($h,9);[void][Stress32]::SetForegroundWindow($h);Start-Sleep -Milliseconds 500
$rows=[System.Collections.Generic.List[object]]::new();$rng=[Random]::new(80317)
function Snapshot([string]$tag){$p.Refresh();$r=New-Object Stress32+RECT;[void][Stress32]::GetClientRect($h,[ref]$r);$z=[IntPtr]::Zero;$sent=[Stress32]::SendMessageTimeout($h,0,[IntPtr]::Zero,[IntPtr]::Zero,2,1000,[ref]$z);$rows.Add([pscustomobject]@{tag=$tag;time=(Get-Date -Format o);alive=(-not $p.HasExited);responded=($sent -ne [IntPtr]::Zero);hwnd=$h.ToInt64();client=@($r.R,$r.B);handles=$p.HandleCount;gdi=[Stress32]::GetGuiResources($p.Handle,0);user=[Stress32]::GetGuiResources($p.Handle,1);cpuSeconds=$p.TotalProcessorTime.TotalSeconds;privateBytes=$p.PrivateMemorySize64;workingSet=$p.WorkingSet64})}
Snapshot 'start'
# Twenty explicit WebApp panel open/close toggles, separated by enough time for each animation.
for($i=1;$i -le 20;$i++){[void][Stress32]::Click($h,22,166);Start-Sleep -Milliseconds 180;[void][Stress32]::Click($h,22,166);Start-Sleep -Milliseconds 180};Snapshot 'after-20-webapp-open-close-cycles'
$sizes=@(@(900,650),@(1120,780),@(1320,880),@(1024,700),@(1280,820));$points=@()
for($i=0;$i -lt $Actions;$i++){
  $r=New-Object Stress32+RECT;[void][Stress32]::GetClientRect($h,[ref]$r);$height=[Math]::Max(500,$r.B)
  switch($rng.Next(0,10)){
   {$_ -le 5}{$ys=@(22,56,94,128,166,200,($height-89),($height-56),($height-23));[void][Stress32]::Click($h,22,$ys[$rng.Next(0,$ys.Count)])}
   6{[Stress32]::Escape()}
   7{$s=$sizes[$rng.Next(0,$sizes.Count)];[void][Stress32]::ShowWindow($h,9);[void][Stress32]::SetWindowPos($h,[IntPtr]::Zero,120,90,$s[0],$s[1],0x0014)}
   8{$ys=@(22,56,94,166,($height-89),($height-56));[void][Stress32]::Click($h,22,$ys[$rng.Next(0,$ys.Count)])}
   9{[Stress32]::Escape()}
  }
  if(($i+1)%80 -eq 0){Snapshot "action-$($i+1)"};Start-Sleep -Milliseconds ([Math]::Max(1,[int]($DurationSeconds*1000/$Actions)-60))
}
Snapshot 'end'
$rows|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $OutDir 'stress-samples.json') -Encoding UTF8
[pscustomobject]@{profile=$Profile;actionsRequested=$Actions;durationSeconds=$DurationSeconds;cycleCount=20;samples=$rows}|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $OutDir 'stress-summary.json') -Encoding UTF8
$rows|Format-Table tag,alive,responded,handles,gdi,user,cpuSeconds,privateBytes,workingSet -AutoSize
