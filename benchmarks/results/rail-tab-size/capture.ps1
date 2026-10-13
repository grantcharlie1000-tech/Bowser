param([string]$Executable, [string]$Name)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class GlassCapture {
 public delegate bool EnumProc(IntPtr h, IntPtr p);
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int ht,uint flags);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int n);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 public static IntPtr Find(uint pid) {IntPtr found=IntPtr.Zero; EnumWindows((h,p)=>{uint id; GetWindowThreadProcessId(h,out id); var b=new StringBuilder(128);GetClassName(h,b,128);if(id==pid && b.ToString()=="BowserNativeWindow"){found=h;return false;}return true;},IntPtr.Zero);return found;}
}
'@
[GlassCapture]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$root=$PSScriptRoot
$env:BOWSER_UI="html"; $p=Start-Process -FilePath $Executable -ArgumentList @('--benchmark-log',(Join-Path $root "$Name-live-events.csv"),'--benchmark-user-data-dir',(Join-Path $root "$Name-live-profile")) -WindowStyle Hidden -PassThru
try {
 Start-Sleep -Seconds 6
 $h=[GlassCapture]::Find($p.Id)
 if($h -eq [IntPtr]::Zero){throw 'No BowserWindow'}
 [GlassCapture]::ShowWindow($h,5)|Out-Null
 foreach($w in @(1100)){
  [GlassCapture]::SetWindowPos($h,[IntPtr]::Zero,20,20,$w,520,0x0040)|Out-Null
  [GlassCapture]::SetForegroundWindow($h)|Out-Null
  Start-Sleep -Milliseconds 1500
  $r=New-Object GlassCapture+Rect
  [GlassCapture]::GetWindowRect($h,[ref]$r)|Out-Null
  $bmp=New-Object Drawing.Bitmap(($r.Right-$r.Left),($r.Bottom-$r.Top))
  $g=[Drawing.Graphics]::FromImage($bmp)
  try{$g.CopyFromScreen($r.Left,$r.Top,0,0,$bmp.Size);$bmp.Save((Join-Path $root "$Name-full.png"),[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bmp.Dispose()}
 }
 "DPI=$([GlassCapture]::GetDpiForWindow($h))"
}finally{if(-not $p.HasExited){Stop-Process -Id $p.Id}}

