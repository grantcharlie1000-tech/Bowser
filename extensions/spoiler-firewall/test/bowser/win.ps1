# Window helpers for the live run. Only ever acts on the process id it is given (a Bowser this run launched).
#   win.ps1 -Action shot -ProcessId N -Out file.png
#   win.ps1 -Action keys -ProcessId N -Keys '^+n'   (SendKeys syntax; refuses unless that window is foreground)
param(
  [Parameter(Mandatory)][ValidateSet('shot','keys','focus','info')][string]$Action,
  [Parameter(Mandatory)][int]$ProcessId,
  [string]$Out,
  [string]$Keys
)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class W {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  public struct RECT { public int L, T, R, B; }
}
"@
$p = Get-Process -Id $ProcessId -ErrorAction Stop
$h = $p.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { Write-Output "no-window"; exit 2 }
switch ($Action) {
  'info' { $r = New-Object W+RECT; [void][W]::GetWindowRect($h, [ref]$r); "hwnd=$($h.ToInt64()) rect=$($r.L),$($r.T),$($r.R),$($r.B) title=$($p.MainWindowTitle) fg=$([W]::GetForegroundWindow().ToInt64())" }
  'focus' { [void][W]::ShowWindow($h, 9); [void][W]::SetForegroundWindow($h); Start-Sleep -Milliseconds 300; "fg=$([W]::GetForegroundWindow().ToInt64()) hwnd=$($h.ToInt64())" }
  'shot' {
    $r = New-Object W+RECT; [void][W]::GetWindowRect($h, [ref]$r)
    $w = $r.R - $r.L; $ht = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap $w, $ht
    $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc()
    $ok = [W]::PrintWindow($h, $dc, 2)   # PW_RENDERFULLCONTENT
    $g.ReleaseHdc($dc); $g.Dispose()
    $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    "shot ok=$ok ${w}x${ht} -> $Out"
  }
  'keys' {
    [void][W]::ShowWindow($h, 9); [void][W]::SetForegroundWindow($h); Start-Sleep -Milliseconds 400
    if ([W]::GetForegroundWindow() -ne $h) { Write-Output "REFUSED: foreground window is not the Bowser this run launched"; exit 3 }
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
    "sent"
  }
}
