param(
    [Parameter(Mandatory=$true)][string]$Exe,
    [Parameter(Mandatory=$true)][string]$Profile,
    [string]$OutDir = $PSScriptRoot,
    [int]$ProcessId = 0
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class U32 {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
 public delegate bool EnumProc(IntPtr h, IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
 [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc cb, IntPtr p);
 [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder b, int n);
 [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder b, int n);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
 [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,IntPtr l,uint flags,uint ms,out IntPtr result);
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
 [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h,IntPtr dc);
 [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr h,uint f);
 [DllImport("user32.dll")] public static extern IntPtr PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern void keybd_event(byte vk,byte scan,uint flags,UIntPtr extra);
}
'@
$script:rows = [System.Collections.Generic.List[object]]::new()
function Save-Screen([string]$Name) {
    Start-Sleep -Milliseconds 500
    $b=[Windows.Forms.Screen]::PrimaryScreen.Bounds
    $img=[Drawing.Bitmap]::new($b.Width,$b.Height)
    $g=[Drawing.Graphics]::FromImage($img); $g.CopyFromScreen($b.Location,[Drawing.Point]::Empty,$b.Size)
    $path=Join-Path $OutDir ($Name+'.png'); $img.Save($path,[Drawing.Imaging.ImageFormat]::Png); $g.Dispose();$img.Dispose(); return $path
}
function Get-MainWindow($Process) {
    for($i=0;$i -lt 80;$i++) {
        $Process.Refresh()
        if($Process.MainWindowHandle -ne 0) { return $Process.MainWindowHandle }
        Start-Sleep -Milliseconds 250
    }
    throw 'Timed out waiting for Bowser main window'
}
function Click-Client([IntPtr]$Hwnd,[int]$X,[int]$Y) {
    $p=New-Object U32+POINT; $p.X=$X; $p.Y=$Y; [void][U32]::ClientToScreen($Hwnd,[ref]$p)
    [void][U32]::SetCursorPos($p.X,$p.Y); Start-Sleep -Milliseconds 100
    [U32]::mouse_event(2,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 80; [U32]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
}
function Click-Screen([int]$X,[int]$Y) {
    [void][U32]::SetCursorPos($X,$Y); Start-Sleep -Milliseconds 100
    [U32]::mouse_event(2,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 80; [U32]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
}
function Key([IntPtr]$Hwnd,[int]$Vk) { [void][U32]::SetForegroundWindow($Hwnd);[U32]::keybd_event([byte]$Vk,0,0,[UIntPtr]::Zero);[U32]::keybd_event([byte]$Vk,0,2,[UIntPtr]::Zero) }
function Panel([IntPtr]$Hwnd) {
    $found=@(); $cb=[U32+EnumProc]{ param($c,$p) $n=[Text.StringBuilder]::new(128);[void][U32]::GetClassName($c,$n,$n.Capacity);if($n.ToString() -eq 'BowserSidePanelHost'){$found+=,$c};return $true }.GetNewClosure()
    [void][U32]::EnumChildWindows($Hwnd,$cb,[IntPtr]::Zero)
    if($found.Count -eq 0) { return $null }; return $found[0]
}
function Capture-State([string]$Name,[IntPtr]$Hwnd,$Process) {
    $null=Save-Screen $Name
    $client=New-Object U32+RECT;[void][U32]::GetClientRect($Hwnd,[ref]$client)
    $wr=New-Object U32+RECT;[void][U32]::GetWindowRect($Hwnd,[ref]$wr);$origin=New-Object U32+POINT;[void][U32]::ClientToScreen($Hwnd,[ref]$origin)
    $panel=Panel $Hwnd;$pw=0;if($panel){$r=New-Object U32+RECT;[void][U32]::GetWindowRect($panel,[ref]$r);$pw=$r.Right-$r.Left}
    $Process.Refresh();$gdi=[U32]::GetGuiResources($Process.Handle,0);$user=[U32]::GetGuiResources($Process.Handle,1)
    return [pscustomobject]@{step=$Name;clientWidth=$client.Right;clientHeight=$client.Bottom;windowRect="[$($wr.Left),$($wr.Top),$($wr.Right),$($wr.Bottom)]";clientScreenOrigin="[$($origin.X),$($origin.Y)]";panelWidth=$pw;processId=$Process.Id;title=$Process.MainWindowTitle;foregroundHwnd=[U32]::GetForegroundWindow().ToInt64();dpi=[U32]::GetDpiForWindow($Hwnd);handles=$Process.HandleCount;gdi=$gdi;user=$user;screenshot=(Join-Path $OutDir ($Name+'.png'))}
}
if(!(Test-Path -LiteralPath $Exe)){throw "Missing executable $Exe"}
if($ProcessId -gt 0){$p=Get-Process -Id $ProcessId}else{$p=Start-Process -FilePath $Exe -ArgumentList @('--profile',$Profile) -PassThru}
$h=Get-MainWindow $p;[void][U32]::SetForegroundWindow($h)
$script:rows.Add((Capture-State '00-start' $h $p))
# At 96 DPI the full-height rail starts at y=2; 30px buttons, 3px gaps, 6px padding.
$p.Refresh();$dpi=[U32]::GetDpiForWindow($h);$scale=$dpi/96.0
$origin=New-Object U32+POINT;[void][U32]::ClientToScreen($h,[ref]$origin)
$wr=New-Object U32+RECT;[void][U32]::GetWindowRect($h,[ref]$wr)
$x=$origin.X+[int][Math]::Round(22*$scale)
$topHome=$origin.Y+[int][Math]::Round(23*$scale);$topCustomize=$origin.Y+[int][Math]::Round(56*$scale)
# Use the visible window bottom as the rail's physical anchor. This also handles DPI-unaware
# client-coordinate virtualization, while the button offsets still come from rail.rs constants.
$book=$wr.Bottom-[int][Math]::Round(89*$scale)
$hist=$wr.Bottom-[int][Math]::Round(56*$scale)
$cases=@(@{n='bookmarks';y=$book},@{n='history';y=$hist},@{n='customize';y=$topCustomize})
foreach($c in $cases){
    Click-Screen $x $c.y;Start-Sleep -Milliseconds 350
    $s=Capture-State ("10-"+$c.n+'-open') $h $p;$script:rows.Add($s)
    Click-Screen $x $c.y;Start-Sleep -Milliseconds 350
    $script:rows.Add((Capture-State ("11-"+$c.n+'-toggle-close') $h $p))
}
# Home opens a fresh tab and closes any current panel.
Click-Screen $x $topHome;Start-Sleep -Seconds 2
$script:rows.Add((Capture-State '20-home' $h $p))
# Open bookmarks then close via host header close affordance and Escape.
Click-Screen $x $book;Start-Sleep -Milliseconds 450;$ph=Panel $h
if($ph){$r=New-Object U32+RECT;[void][U32]::GetWindowRect($ph,[ref]$r);[void][U32]::SetCursorPos(($r.Right-18),($r.Top+20));[U32]::mouse_event(2,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 50;[U32]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 350}
$script:rows.Add((Capture-State '21-close-button' $h $p))
Click-Screen $x $book;Start-Sleep -Milliseconds 400;Key $h 0x1B;Start-Sleep -Milliseconds 450
$script:rows.Add((Capture-State '22-escape-close' $h $p))
# Direct switching from Bookmarks to History should retain one host and change panel content.
Click-Screen $x $book;Start-Sleep -Milliseconds 400;Click-Screen $x $hist;Start-Sleep -Milliseconds 450
$script:rows.Add((Capture-State '23-direct-switch-to-history' $h $p))
# Clean close through WM_CLOSE, then inspect persisted files before deleting only this named profile.
$z=[IntPtr]::Zero;[void][U32]::SendMessageTimeout($h,0x10,[IntPtr]::Zero,[IntPtr]::Zero,2,5000,[ref]$z)
if(!$z){$p.CloseMainWindow()|Out-Null};if(!$p.WaitForExit(8000)){$script:rows.Add([pscustomobject]@{step='close';result='WM_CLOSE did not exit within 8s'})}
$root=Join-Path $env:LOCALAPPDATA "Bowser\Profiles\$Profile"
$files=Get-ChildItem -LiteralPath $root -File -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name
$script:rows | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutDir 'panel-run.json') -Encoding UTF8
[pscustomobject]@{profileRoot=$root;profileFiles=$files;testRows=$script:rows} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $OutDir 'panel-summary.json') -Encoding UTF8
Get-Content (Join-Path $OutDir 'panel-summary.json')

