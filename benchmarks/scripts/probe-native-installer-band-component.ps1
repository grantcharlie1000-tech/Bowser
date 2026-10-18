# Actual native component fixture, not Google/Bowser installation or a frontend mockup.
[CmdletBinding()]
param([Parameter(Mandatory)][uint32]$OwnerPid,[Parameter(Mandatory)][long]$OwnerHandle,[Parameter(Mandatory)][string]$OwnerExe,
 [Parameter(Mandatory)][long]$NoteHandle,[Parameter(Mandatory)][long]$ButtonHandle,[Parameter(Mandatory)][long]$PageHandle,
 [int]$NoteLeft,[int]$NoteTop,[int]$NoteWidth,[int]$NoteHeight,[int]$ButtonLeft,[int]$ButtonTop,[int]$ButtonWidth,[int]$ButtonHeight,[int]$PageTop,
 [int]$FontScale,[int]$Dpi,[Parameter(Mandatory)][string]$Sample,[Parameter(Mandatory)][string]$Out,[switch]$Click)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class InstallerBandCapture {
 [StructLayout(LayoutKind.Sequential)]public struct RECT{public int Left,Top,Right,Bottom;}
 [StructLayout(LayoutKind.Sequential)]public struct POINT{public int X,Y;}
 [StructLayout(LayoutKind.Sequential)]public struct GUI{public uint cbSize,flags;public IntPtr Active,Focus,Capture,MenuOwner,MoveSize,Caret;public RECT CaretRect;}
 [StructLayout(LayoutKind.Sequential)]public struct TIME{public uint Low,High;public ulong Bits{get{return ((ulong)High<<32)|Low;}}}
 [DllImport("user32.dll")]public static extern bool IsWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")]public static extern bool IsWindowEnabled(IntPtr h);
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")]public static extern IntPtr GetParent(IntPtr h);
 [DllImport("user32.dll")]public static extern int GetDlgCtrlID(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int GetClassName(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern IntPtr SendMessageTimeout(IntPtr h,uint m,UIntPtr n,StringBuilder b,uint flags,uint timeout,out UIntPtr copied);
 [DllImport("user32.dll")]public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")]public static extern bool GetClientRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")]public static extern bool ClientToScreen(IntPtr h,ref POINT p);
 [DllImport("user32.dll")]public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")]static extern bool GetGUIThreadInfo(uint tid,ref GUI info);
 public static bool InputReady(IntPtr owner,uint tid){var gui=new GUI{cbSize=(uint)Marshal.SizeOf(typeof(GUI))};return GetForegroundWindow()==owner&&GetGUIThreadInfo(tid,ref gui)&&gui.Active==owner&&gui.Capture==IntPtr.Zero&&GetForegroundWindow()==owner;}
 [DllImport("user32.dll")]public static extern IntPtr WindowFromPoint(POINT p);
 [DllImport("user32.dll")]public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")]public static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll")]public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 [DllImport("user32.dll")]public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("kernel32.dll")]static extern IntPtr OpenProcess(uint rights,bool inherit,uint pid);
 [DllImport("kernel32.dll")]static extern bool CloseHandle(IntPtr h);
 [DllImport("kernel32.dll")]static extern uint GetProcessId(IntPtr h);
 [DllImport("kernel32.dll")]static extern uint WaitForSingleObject(IntPtr h,uint ms);
 [DllImport("kernel32.dll")]static extern bool GetProcessTimes(IntPtr h,out TIME a,out TIME b,out TIME c,out TIME d);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)]static extern bool QueryFullProcessImageName(IntPtr h,uint flags,StringBuilder b,ref uint n);
 public static bool Owned(IntPtr h,uint pid,uint tid,string cls){uint p;var b=new StringBuilder(64);return IsWindow(h)&&GetWindowThreadProcessId(h,out p)==tid&&p==pid&&GetClassName(h,b,64)>0&&String.Equals(b.ToString(),cls,StringComparison.Ordinal);}
 public static bool Text(IntPtr h,string expected){var b=new StringBuilder(512);UIntPtr copied;return expected.Length<512&&SendMessageTimeout(h,0x000D,(UIntPtr)512,b,0x23,250,out copied)!=IntPtr.Zero&&copied.ToUInt64()==(ulong)expected.Length&&String.Equals(b.ToString(),expected,StringComparison.Ordinal);}
 public sealed class ProcessIdentity:IDisposable{
  IntPtr h;uint pid;ulong created;string image;
  public ProcessIdentity(uint expected,string path){if(!System.Text.RegularExpressions.Regex.IsMatch(System.IO.Path.GetFileName(path),"^bowser-[0-9a-f]+\\.exe$")||!System.IO.Path.GetDirectoryName(path).EndsWith("\\target\\debug\\deps",StringComparison.OrdinalIgnoreCase))throw new InvalidOperationException("Not an isolated test executable.");h=OpenProcess(0x00101000,false,expected);if(h==IntPtr.Zero)throw new InvalidOperationException("Read-only test process handle unavailable.");try{pid=expected;image=System.IO.Path.GetFullPath(path);created=Creation();if(!SameAlive())throw new InvalidOperationException("Exact test executable identity unavailable.");}catch{Dispose();throw;}}
  ulong Creation(){TIME a,b,c,d;if(!GetProcessTimes(h,out a,out b,out c,out d))throw new InvalidOperationException("Test creation identity unavailable.");return a.Bits;}
  public bool SameAlive(){var b=new StringBuilder(32768);uint n=32768;return h!=IntPtr.Zero&&GetProcessId(h)==pid&&WaitForSingleObject(h,0)==258&&Creation()==created&&QueryFullProcessImageName(h,0,b,ref n)&&String.Equals(System.IO.Path.GetFullPath(b.ToString()),image,StringComparison.OrdinalIgnoreCase);}
  public void Dispose(){if(h!=IntPtr.Zero){CloseHandle(h);h=IntPtr.Zero;}}
 }
}
'@
if($Sample -cnotmatch '^font-(100|150|200)-width-(1044|320)$'){throw 'Unexpected fixed sample name.'}
$h=[IntPtr]$OwnerHandle;$note=[IntPtr]$NoteHandle;$button=[IntPtr]$ButtonHandle;$page=[IntPtr]$PageHandle
$p=[uint32]0;$tid=[InstallerBandCapture]::GetWindowThreadProcessId($h,[ref]$p);$guard=$null
$report=[ordered]@{scope='native-component-only-no-App-Store-or-Google';status='failed';font_scale=$FontScale;dpi=$Dpi;sample=$Sample;click_called=$false;visual_verdict='pending-independent-screenshot-review';error=$null}
function Neutral{foreach($k in @(1,2,16,17,18,27,91,92)){if([InstallerBandCapture]::GetAsyncKeyState($k) -lt 0){throw 'Held input; no repair.'}}}
function Children {
 foreach($entry in @(@($note,'Static',0x5b10),@($button,'Button',0x5b11),@($page,'Static',0x5b13))){$window=[IntPtr]$entry[0];if(![InstallerBandCapture]::Owned($window,$OwnerPid,$tid,$entry[1]) -or [InstallerBandCapture]::GetParent($window) -ne $h -or [InstallerBandCapture]::GetDlgCtrlID($window) -ne $entry[2] -or ![InstallerBandCapture]::IsWindowVisible($window)){throw 'Exact direct child identity unavailable.'}}
}
function Identity {
 if(!$guard.SameAlive() -or [InstallerBandCapture]::GetDpiForWindow($h) -ne $Dpi -or ![InstallerBandCapture]::Owned($h,$OwnerPid,$tid,'Static') -or ![InstallerBandCapture]::IsWindowVisible($h) -or ![InstallerBandCapture]::Text($h,'Bowser installer band component - no install authority')){throw 'Exact native component host unavailable.'}
 Children
 if(![InstallerBandCapture]::IsWindowEnabled($button) -or ![InstallerBandCapture]::Text($button,'Add to Bowser') -or ![InstallerBandCapture]::Text($note,'Google installs into Chrome. Bowser uses a separate verified package and permission review. Extension compatibility varies.')){throw 'Full native component copy/state unavailable.'}
 Children
 if(!$guard.SameAlive() -or [InstallerBandCapture]::GetDpiForWindow($h) -ne $Dpi -or ![InstallerBandCapture]::Owned($h,$OwnerPid,$tid,'Static')){throw 'Host/children changed during text reads.'}
}
function Bounds([IntPtr]$Window,[int]$Left,[int]$Top,[int]$Width,[int]$Height){$r=[InstallerBandCapture+RECT]::new();$p=[InstallerBandCapture+POINT]::new();$p.X=$Left;$p.Y=$Top;if(![InstallerBandCapture]::ClientToScreen($h,[ref]$p) -or ![InstallerBandCapture]::GetWindowRect($Window,[ref]$r) -or $r.Left -ne $p.X -or $r.Top -ne $p.Y -or $r.Right-$r.Left -ne $Width -or $r.Bottom-$r.Top -ne $Height -or $Width -le 0 -or $Height -le 0){throw 'Native control geometry differs from measured reservation.'};return $r}
try {
 $guard=[InstallerBandCapture+ProcessIdentity]::new($OwnerPid,$OwnerExe);Identity
 $nr=Bounds $note $NoteLeft $NoteTop $NoteWidth $NoteHeight;$br=Bounds $button $ButtonLeft $ButtonTop $ButtonWidth $ButtonHeight
 $client=[InstallerBandCapture+RECT]::new();if(![InstallerBandCapture]::GetClientRect($h,[ref]$client)){throw 'Native client bounds unavailable.'}
 $null=Bounds $page 0 $PageTop $client.Right ($client.Bottom-$PageTop)
 if($PageTop -le 0 -or $NoteTop+$NoteHeight -gt $PageTop -or $ButtonTop+$ButtonHeight -gt $PageTop -or $NoteLeft -lt 0 -or $ButtonLeft -lt 0 -or $NoteLeft+$NoteWidth -gt $client.Right -or $ButtonLeft+$ButtonWidth -gt $client.Right){throw 'Native band/page overlap or clipping.'}
 if($Click){
  Neutral;Identity;if(![InstallerBandCapture]::InputReady($h,$tid)){throw 'Exact foreground unavailable; no focus repair.'}
  $point=[InstallerBandCapture+POINT]::new();$point.X=$br.Left+[int](($br.Right-$br.Left)/2);$point.Y=$br.Top+[int](($br.Bottom-$br.Top)/2)
  if([InstallerBandCapture]::WindowFromPoint($point) -ne $button -or ![InstallerBandCapture]::SetCursorPos($point.X,$point.Y)){throw 'Exact button hit/cursor unavailable.'}
  Neutral;Identity;$fresh=Bounds $button $ButtonLeft $ButtonTop $ButtonWidth $ButtonHeight
  if($fresh.Left -ne $br.Left -or $fresh.Top -ne $br.Top -or ![InstallerBandCapture]::InputReady($h,$tid) -or [InstallerBandCapture]::WindowFromPoint($point) -ne $button){throw 'Button changed before real input.'}
  try{[InstallerBandCapture]::mouse_event(2,0,0,0,[UIntPtr]::Zero);$report.click_called=$true}finally{[InstallerBandCapture]::mouse_event(4,0,0,0,[UIntPtr]::Zero)}
 }
 Identity;$r=[InstallerBandCapture+RECT]::new();if(![InstallerBandCapture]::GetWindowRect($h,[ref]$r)){throw 'Capture root geometry unavailable.'}
 $width=$r.Right-$r.Left;$height=$r.Bottom-$r.Top;if($width -le 0 -or $height -le 0 -or $width -gt 4096 -or $height -gt 4096){throw 'Capture size outside bound.'}
 $bitmap=[Drawing.Bitmap]::new($width,$height)
 try{$graphics=[Drawing.Graphics]::FromImage($bitmap);try{$dc=$graphics.GetHdc();try{$ok=[InstallerBandCapture]::PrintWindow($h,$dc,2)}finally{$graphics.ReleaseHdc($dc)}}finally{$graphics.Dispose()};if(!$ok){throw 'Owned PrintWindow failed.'};Identity;$null=Bounds $note $NoteLeft $NoteTop $NoteWidth $NoteHeight;$null=Bounds $button $ButtonLeft $ButtonTop $ButtonWidth $ButtonHeight;$null=Bounds $page 0 $PageTop $client.Right ($client.Bottom-$PageTop);$file=Join-Path $Out "$Sample.png";if(Test-Path $file){throw 'Existing sample file refused.'};$bitmap.Save($file,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
 $report.screenshot="$Sample.png";$report.sha256=(Get-FileHash $file -Algorithm SHA256).Hash.ToLowerInvariant();$report.status='captured-native-component-visual-review-pending'
} catch {$report.error=$_.Exception.Message} finally {if($guard){$guard.Dispose()};[IO.File]::WriteAllText((Join-Path $Out "$Sample.json"),($report|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))}
if($report.status -cne 'captured-native-component-visual-review-pending'){throw 'Native component failed; not accepted.'}
