param([string]$Exe,[string]$Profile,[int]$ProcessId=0,[string]$OutDir,[string]$StepsFile)
$ErrorActionPreference='Stop'; Add-Type -AssemblyName System.Drawing; Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class Input32 {
 [StructLayout(LayoutKind.Sequential)] public struct RECT{public int Left,Top,Right,Bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct POINT{public int X,Y;}
 [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT{public ushort wVk,wScan;public uint dwFlags,time;public UIntPtr dwExtraInfo;}
 [StructLayout(LayoutKind.Explicit,Size=40)] public struct INPUT{[FieldOffset(0)]public uint type;[FieldOffset(8)]public KEYBDINPUT ki;}
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 private static IntPtr match;
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb,IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern uint SendInput(uint n,INPUT[] inputs,int cbSize);
 [DllImport("user32.dll")] public static extern void keybd_event(byte vk,byte scan,uint flags,UIntPtr extra);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int cmd);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
 [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,IntPtr l,uint flags,uint ms,out IntPtr result);
 public static int SendKey(ushort vk,bool down){keybd_event((byte)vk,0,down?0u:2u,UIntPtr.Zero);return 1;}
 public static int TypeText(string s){int total=0;foreach(char c in s){var i=new INPUT[2];i[0].type=i[1].type=1;i[0].ki.wScan=c;i[0].ki.dwFlags=4;i[1].ki.wScan=c;i[1].ki.dwFlags=6;total+=(int)SendInput(2,i,Marshal.SizeOf(typeof(INPUT)));}return total;}
 public static IntPtr FindBowser(uint wanted){match=IntPtr.Zero;EnumWindows((h,p)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid==wanted&&IsWindowVisible(h)){var c=new StringBuilder(128);GetClassName(h,c,c.Capacity);if(c.ToString()=="BowserNativeWindow")match=h;}return true;},IntPtr.Zero);return match;}
}
"@
if($ProcessId -gt 0){$p=Get-Process -Id $ProcessId}else{$p=Start-Process -FilePath $Exe -ArgumentList @('--profile',$Profile) -PassThru; Set-Content (Join-Path $OutDir 'pid.txt') $p.Id}
for($i=0;$i -lt 80;$i++){$p.Refresh();if($p.MainWindowHandle -ne 0){break};Start-Sleep -Milliseconds 250}; if($p.MainWindowHandle -eq 0){throw 'main window not found'}
$h=[Input32]::FindBowser([uint32]$p.Id); if($h -eq [IntPtr]::Zero){throw "No visible Bowser window for pid $($p.Id)"}; [void][Input32]::ShowWindow($h,9); [void][Input32]::SetForegroundWindow($h); Start-Sleep -Milliseconds 400
function Save-Capture([string]$name){$b=[Windows.Forms.Screen]::PrimaryScreen.Bounds;$img=[Drawing.Bitmap]::new($b.Width,$b.Height);$g=[Drawing.Graphics]::FromImage($img);$g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size);$path=Join-Path $OutDir ($name+'.png');$img.Save($path,[Drawing.Imaging.ImageFormat]::Png);$g.Dispose();$img.Dispose();$p.Refresh();$title=[Text.StringBuilder]::new(512);[void][Input32]::GetWindowText($h,$title,$title.Capacity);$wr=New-Object Input32+RECT;[void][Input32]::GetWindowRect($h,[ref]$wr);$cl=New-Object Input32+RECT;[void][Input32]::GetClientRect($h,[ref]$cl);[pscustomobject]@{name=$name;pid=$p.Id;path=$p.Path;alive=(-not $p.HasExited);title=$title.ToString();mainHwnd=$h.ToInt64();foregroundHwnd=[Input32]::GetForegroundWindow().ToInt64();windowRect=@($wr.Left,$wr.Top,$wr.Right,$wr.Bottom);client=@($cl.Right,$cl.Bottom);capture=$path}}
$rows=[System.Collections.Generic.List[object]]::new(); $rows.Add((Save-Capture 'state-start'))
if($StepsFile){$steps=Get-Content -LiteralPath $StepsFile -Raw|ConvertFrom-Json;foreach($step in $steps){foreach($a in $step.actions){switch($a.kind){'clickClient'{$pt=New-Object Input32+POINT;$pt.X=[int]$a.x;$pt.Y=[int]$a.y;[void][Input32]::ClientToScreen($h,[ref]$pt);[void][Input32]::SetCursorPos($pt.X,$pt.Y);Start-Sleep -Milliseconds 100;[Input32]::mouse_event(2,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 60;[Input32]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}'clickScreen'{[void][Input32]::SetCursorPos([int]$a.x,[int]$a.y);Start-Sleep -Milliseconds 100;[Input32]::mouse_event(2,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 60;[Input32]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}'key'{[void][Input32]::SetForegroundWindow($h);[void][Input32]::SendKey([System.UInt16]$a.vk,$true);[void][Input32]::SendKey([System.UInt16]$a.vk,$false)}'type'{[void][Input32]::TypeText([string]$a.text)}'wait'{Start-Sleep -Milliseconds ([int]$a.ms)}'ctrlKey'{[void][Input32]::SendKey(0x11,$true);[void][Input32]::SendKey([System.UInt16]$a.vk,$true);[void][Input32]::SendKey([System.UInt16]$a.vk,$false);[void][Input32]::SendKey(0x11,$false)}}};Start-Sleep -Milliseconds 350;$rows.Add((Save-Capture ([string]$step.name)))} }
$rows|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $OutDir 'ui-states.json') -Encoding UTF8
$rows|ConvertTo-Json -Depth 6
