# Runs a program on a separate, never-displayed Win32 desktop, so live Bowser tests cannot appear on, take focus
# from, or receive input from the user's screen. Windows on another desktop are invisible to the user and the
# interactive desktop's focus is never touched.
#
#   hidden-desktop.ps1 -Action run  -Desktop SfwLive -Exe <path> -Args "<args>" [-Wait]   -> prints the PID
#   hidden-desktop.ps1 -Action shot -Desktop SfwLive -ProcessId <pid> -Out <file.png>      (runs a capture helper on
#                                                                                           the same desktop)
# Safety: only starts the program it is given; never stops anything (callers stop the PIDs they started).
param(
  [Parameter(Mandatory)][ValidateSet('run','shot')][string]$Action,
  [string]$Desktop = 'SfwLive',
  [string]$Exe,
  [string]$Args = '',
  [int]$ProcessId,
  [string]$Out,
  [switch]$Wait
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class HiddenDesktop {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct STARTUPINFO {
    public int cb; public string lpReserved; public string lpDesktop; public string lpTitle;
    public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
    public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct PROCESS_INFORMATION { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
  [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern IntPtr CreateDesktop(string name, IntPtr dev, IntPtr devmode, int flags, uint access, IntPtr sa);
  [DllImport("user32.dll", SetLastError = true)] public static extern bool CloseDesktop(IntPtr h);
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern bool CreateProcess(string app, string cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags,
    IntPtr env, string cwd, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
  [DllImport("kernel32.dll")] public static extern uint WaitForSingleObject(IntPtr h, uint ms);
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
  const uint GENERIC_ALL = 0x10000000;
  const uint BELOW_NORMAL_PRIORITY_CLASS = 0x00004000;
  // Creates (or opens) the desktop and starts the command on it. The desktop handle is closed afterwards; the
  // desktop lives on while any process runs on it.
  public static int Run(string desktop, string cmdLine, bool wait) {
    IntPtr d = CreateDesktop(desktop, IntPtr.Zero, IntPtr.Zero, 0, GENERIC_ALL, IntPtr.Zero);
    if (d == IntPtr.Zero) throw new Exception("CreateDesktop failed: " + Marshal.GetLastWin32Error());
    try {
      var si = new STARTUPINFO(); si.cb = Marshal.SizeOf(si); si.lpDesktop = @"WinSta0\" + desktop;
      PROCESS_INFORMATION pi;
      if (!CreateProcess(null, cmdLine, IntPtr.Zero, IntPtr.Zero, false, BELOW_NORMAL_PRIORITY_CLASS, IntPtr.Zero, null, ref si, out pi))
        throw new Exception("CreateProcess failed: " + Marshal.GetLastWin32Error());
      if (wait) WaitForSingleObject(pi.hProcess, 120000);
      CloseHandle(pi.hThread); CloseHandle(pi.hProcess);
      return pi.dwProcessId;
    } finally { CloseDesktop(d); }
  }
}
"@
switch ($Action) {
  'run' {
    if (-not $Exe) { throw '-Exe is required' }
    $cmd = '"' + $Exe + '" ' + $Args
    [HiddenDesktop]::Run($Desktop, $cmd, [bool]$Wait)
  }
  'shot' {
    # The capture must run on the same desktop as the window (PrintWindow does not cross desktops), so start a
    # small PowerShell capture script there and wait for it.
    $helper = Join-Path $PSScriptRoot 'hidden-shot.ps1'
    $ps = (Get-Command powershell.exe).Source
    $cmd = '"' + $ps + '" -NoProfile -ExecutionPolicy Bypass -File "' + $helper + '" -ProcessId ' + $ProcessId + ' -Out "' + $Out + '"'
    [void][HiddenDesktop]::Run($Desktop, $cmd, $true)
    if (Test-Path $Out) { "shot -> $Out" } else { "shot failed (no file)" }
  }
}
