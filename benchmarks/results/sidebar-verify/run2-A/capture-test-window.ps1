param([string]$Profile = 'vA-4', [string]$OutDir = $PSScriptRoot)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class WindowCapture {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h,uint attr,out RECT r,uint cb);
}
'@
$exe=(Resolve-Path (Join-Path $PSScriptRoot 'test-bin\bowser.exe')).Path
$proc=Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -eq $exe -and $_.CommandLine -match ("--profile\s+"+[regex]::Escape($Profile))} | Select-Object -First 1
if(!$proc){throw "No owned test process for $Profile"}
$p=Get-Process -Id $proc.ProcessId
$h=[IntPtr]$p.MainWindowHandle
if($h -eq [IntPtr]::Zero){throw "Test process has no main window: $($proc.ProcessId)"}
$r=New-Object WindowCapture+RECT; [void][WindowCapture]::GetWindowRect($h,[ref]$r)
$dwm=New-Object WindowCapture+RECT; $dwmResult=[WindowCapture]::DwmGetWindowAttribute($h,9,[ref]$dwm,[Runtime.InteropServices.Marshal]::SizeOf([type][WindowCapture+RECT]))
$w=$r.Right-$r.Left; $height=$r.Bottom-$r.Top
$bmp=[Drawing.Bitmap]::new($w,$height); $g=[Drawing.Graphics]::FromImage($bmp); $dc=$g.GetHdc(); $printed=[WindowCapture]::PrintWindow($h,$dc,2); $g.ReleaseHdc($dc)
$png=Join-Path $OutDir 'test-window-printwindow.png'; $bmp.Save($png,[Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
$title=[Text.StringBuilder]::new(512); [void][WindowCapture]::GetWindowText($h,$title,$title.Capacity)
[pscustomobject]@{profile=$Profile;pid=$p.Id;executable=$proc.ExecutablePath;hwnd=$h.ToInt64();title=$title.ToString();windowRect="[$($r.Left),$($r.Top),$($r.Right),$($r.Bottom)]";dwmResult=$dwmResult;dwmRect="[$($dwm.Left),$($dwm.Top),$($dwm.Right),$($dwm.Bottom)]";printWindow=$printed;png=$png} | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $OutDir 'test-window-printwindow.json') -Encoding UTF8
Get-Content (Join-Path $OutDir 'test-window-printwindow.json')
