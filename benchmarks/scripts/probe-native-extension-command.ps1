# Genuine OS keyboard input only, restricted to an already focused exact disposable host.
[CmdletBinding()]
param([Parameter(Mandatory)][uint32]$OwnerPid,[Parameter(Mandatory)][long]$OwnerHandle,[ValidateSet('F8','CtrlShiftX')][string]$Chord)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;using System.Runtime.InteropServices;using System.Text;
public static class BowserNativeCommandInput {
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder text,int cap);
}
'@
$h=[IntPtr]$OwnerHandle;$actual=[uint32]0;[void][BowserNativeCommandInput]::GetWindowThreadProcessId($h,[ref]$actual)
$class=[Text.StringBuilder]::new(128);[void][BowserNativeCommandInput]::GetClassName($h,$class,128)
if($actual -ne $OwnerPid -or $class.ToString() -ne 'Static' -or ![BowserNativeCommandInput]::IsWindowVisible($h) -or [BowserNativeCommandInput]::GetForegroundWindow() -ne $h){throw 'Exact owned visible focused disposable host was not verified; no keyboard input sent.'}
try{[Windows.Forms.SendKeys]::SendWait($(if($Chord -eq 'F8'){'{F8}'}else{'^+x'}))}finally{foreach($key in @(0x58,0x10,0x11,0x77)){[BowserNativeCommandInput]::keybd_event([byte]$key,0,2,[UIntPtr]::Zero)}}
Write-Output "NATIVE_COMMAND_INPUT exact_owned_visible_foreground=true chord=$Chord real_os_keyboard_sent=true"
