param(
    [Parameter(Mandatory)] [string]$Executable,
    [Parameter(Mandatory)] [string]$Name,
    [string]$ProfileRoot = (Join-Path $PSScriptRoot '..\..\..\target-foundation\profiles'),
    # Must be inside benchmarks/fixtures of the checkout the exe was built from (CARGO_MANIFEST_DIR).
    [string]$Fixture = (Join-Path $PSScriptRoot '..\..\fixtures\light.html')
)
# Captures the Bowser window at a normal and a narrow size. PrintWindow(PW_RENDERFULLCONTENT) is
# tried first (saved as *-printwindow.png) but returns black for the WebView2 chrome and page, so the
# comparison images (*-normal.png, *-narrow.png) are screen copies of the window, made topmost.
# Runs in benchmark mode with a throwaway user-data folder, so the real profile is never touched.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class FoundationCapture {
 public delegate bool EnumProc(IntPtr h, IntPtr p);
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
 [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder b, int n);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int ht, uint flags);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 // The visible BowserNativeWindow frame of the process (an owned "Static" cover window has the same rect).
 public static IntPtr Find(uint pid) {
  IntPtr found = IntPtr.Zero; long best = 0;
  EnumWindows((h, p) => {
   uint id; GetWindowThreadProcessId(h, out id);
   if (id != pid || !IsWindowVisible(h)) return true;
   var cls = new StringBuilder(128); GetClassName(h, cls, 128);
   if (cls.ToString() != "BowserNativeWindow") return true;
   Rect r; GetWindowRect(h, out r);
   long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
   if (area > best) { best = area; found = h; }
   return true;
  }, IntPtr.Zero);
  return found;
 }
}
'@
[FoundationCapture]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null

$profile = Join-Path $ProfileRoot "$Name-profile"
if (Test-Path $profile) { Remove-Item -Recurse -Force $profile }
New-Item -ItemType Directory -Force $profile | Out-Null
$profile = (Resolve-Path $profile).Path
$log = Join-Path $ProfileRoot "$Name-events.csv"
$page = 'file:///' + ((Resolve-Path $Fixture).Path -replace '\\', '/')

$env:BOWSER_UI = 'html'
$p = Start-Process -FilePath $Executable -PassThru -ArgumentList @(
    '--benchmark-log', $log, '--benchmark-user-data-dir', $profile, '--benchmark-url', $page)
try {
    Start-Sleep -Seconds 6
    $h = [FoundationCapture]::Find($p.Id)
    if ($h -eq [IntPtr]::Zero) { throw 'No Bowser main window' }
    [FoundationCapture]::ShowWindow($h, 9) | Out-Null
    foreach ($size in @(@{ tag = 'normal'; w = 1280; h = 720 }, @{ tag = 'narrow'; w = 700; h = 560 })) {
        [FoundationCapture]::SetWindowPos($h, [IntPtr](-1), 40, 40, $size.w, $size.h, 0x0040) | Out-Null
        [FoundationCapture]::SetForegroundWindow($h) | Out-Null
        Start-Sleep -Milliseconds 1800
        $r = New-Object FoundationCapture+Rect
        [FoundationCapture]::GetWindowRect($h, [ref]$r) | Out-Null
        if ($r.Right -le $r.Left) { throw "Empty window rect (process exited: $($p.HasExited))" }
        $bmp = New-Object Drawing.Bitmap(($r.Right - $r.Left), ($r.Bottom - $r.Top))
        $g = [Drawing.Graphics]::FromImage($bmp)
        try {
            $dc = $g.GetHdc()
            try { [FoundationCapture]::PrintWindow($h, $dc, 2) | Out-Null } finally { $g.ReleaseHdc($dc) }
            if ($size.tag -eq 'normal') {
                $bmp.Save((Join-Path $PSScriptRoot "$Name-printwindow.png"), [Drawing.Imaging.ImageFormat]::Png)
            }
            $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
            $bmp.Save((Join-Path $PSScriptRoot "$Name-$($size.tag).png"), [Drawing.Imaging.ImageFormat]::Png)
        } finally { $g.Dispose(); $bmp.Dispose() }
    }
    "DPI=$([FoundationCapture]::GetDpiForWindow($h))"
} finally {
    if (-not $p.HasExited) { Stop-Process -Id $p.Id }
}
