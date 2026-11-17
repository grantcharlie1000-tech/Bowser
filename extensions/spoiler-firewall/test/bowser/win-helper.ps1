# Helper that MUST be started on the hidden desktop (via hidden-desktop.ps1 -Action run -Exe powershell.exe ...), so every
# window message it posts goes to windows of the Bowser process started there, never to the user's desktop. It only ever
# addresses the -ProcessId it is given (the PID live-hidden.mjs started). Output goes to files (no console on that desktop).
#
# Single-shot:  -Action <a> -ProcessId <pid> [-Out f] [-Keys k] [-Text t] [-Enter]
# Server mode:  -Action server -Dir <dir> : a long-lived loop (one Add-Type compile, one PowerShell start, which matters on a
#               loaded machine) that executes JSON command files <dir>\*.json and writes <id>.out / <id>.done, and samples
#               the caption of every "caption" job it was given every ~20 ms. Stops when <dir>\stop exists.
#
# actions: windows | close | caption | keys | settext
#   windows  list the process's top-level and child windows
#   close    WM_CLOSE to the main window (graceful exit, flushes the profile)
#   caption  (server: start sampling; single-shot: sample until -StopFile) distinct main-window captions with unix-ms timestamps
#   keys     post WM_KEYDOWN/UP with modifier state set on Bowser's UI thread (AttachThreadInput + SetKeyboardState)
#   settext  native-chrome omnibox: WM_SETTEXT on the Edit control, then Enter
param(
  [Parameter(Mandatory)][string]$Action,
  [int]$ProcessId = 0,
  [string]$Out = '', [string]$StopFile = '', [string]$Keys = '', [string]$Text = '', [int]$DurationMs = 3600000, [switch]$Enter, [string]$Dir = ''
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class W {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, StringBuilder l);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, string l);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("user32.dll")] public static extern bool GetKeyboardState(byte[] s);
  [DllImport("user32.dll")] public static extern bool SetKeyboardState(byte[] s);
  [DllImport("user32.dll")] public static extern IntPtr GetFocus();
  [DllImport("user32.dll")] public static extern uint MapVirtualKey(uint c, uint t);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  public static List<IntPtr> TopWindows(uint pid) {
    var l = new List<IntPtr>();
    EnumWindows((h, p) => { uint q; GetWindowThreadProcessId(h, out q); if (q == pid) l.Add(h); return true; }, IntPtr.Zero);
    return l;
  }
  public static List<IntPtr> Children(IntPtr parent) {
    var l = new List<IntPtr>();
    EnumChildWindows(parent, (h, p) => { l.Add(h); return true; }, IntPtr.Zero);
    return l;
  }
  public static string Cls(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
  public static string Txt(IntPtr h) { var s = new StringBuilder(1024); SendMessageW(h, 0x000D, (IntPtr)1024, s); return s.ToString(); } // WM_GETTEXT
  public static IntPtr MainWin(uint pid) {
    foreach (var h in TopWindows(pid)) if (Cls(h) == "BowserNativeWindow") return h;
    return IntPtr.Zero;
  }
  // Presses a key as seen by GetKeyState on the target UI thread: attach input queues, set modifier bits, post the messages.
  public static string Press(uint pid, uint vk, bool ctrl, bool shift, bool alt) {
    IntPtr h = MainWin(pid); if (h == IntPtr.Zero) return "no main window";
    uint tp; uint tid = GetWindowThreadProcessId(h, out tp);
    uint me = GetCurrentThreadId();
    bool att = AttachThreadInput(me, tid, true);
    var ks = new byte[256]; GetKeyboardState(ks);
    var saved = (byte[])ks.Clone();
    if (ctrl) ks[0x11] = 0x80; if (shift) ks[0x10] = 0x80; if (alt) ks[0x12] = 0x80;
    SetKeyboardState(ks);
    uint scan = MapVirtualKey(vk, 0);
    IntPtr lpDown = (IntPtr)(1 | (scan << 16));
    IntPtr lpUp = (IntPtr)(unchecked((int)(1 | (scan << 16) | 0xC0000000)));
    bool a = PostMessageW(h, 0x0100, (IntPtr)vk, lpDown); // WM_KEYDOWN
    System.Threading.Thread.Sleep(250); // the target reads GetKeyState while handling the message
    bool b = PostMessageW(h, 0x0101, (IntPtr)vk, lpUp);
    System.Threading.Thread.Sleep(150);
    SetKeyboardState(saved);
    if (att) AttachThreadInput(me, tid, false);
    return "attached=" + att + " posted=" + a + "/" + b;
  }
}
"@

function Add-Line($file, $s) { if ($file) { Add-Content -Path $file -Value $s -Encoding UTF8 } }

function Invoke-Act($act, [int]$procId, $out, $keys, $text) {
  try {
    switch ($act) {
      'windows' {
        foreach ($h in [W]::TopWindows([uint32]$procId)) {
          Add-Line $out ("TOP 0x{0:X} class={1} visible={2} text='{3}'" -f [int64]$h, [W]::Cls($h), [W]::IsWindowVisible($h), [W]::Txt($h))
          foreach ($c in [W]::Children($h)) { Add-Line $out ("  CHILD 0x{0:X} class={1} visible={2} text='{3}'" -f [int64]$c, [W]::Cls($c), [W]::IsWindowVisible($c), ([W]::Txt($c) -replace "`r|`n",' ')) }
        }
      }
      'close' {
        $h = [W]::MainWin([uint32]$procId)
        if ($h -ne [IntPtr]::Zero) { [void][W]::PostMessageW($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero); Add-Line $out 'WM_CLOSE posted' } else { Add-Line $out 'no main window' }
      }
      'keys' {
        $parts = $keys.ToLower().Split('+'); $k = $parts[-1]
        $vk = if ($k.Length -eq 1) { [uint32][char]($k.ToUpper()) } elseif ($k -eq 'enter') { 13 } elseif ($k -eq 'escape') { 27 } elseif ($k -eq 'tab') { 9 } else { throw "key $k" }
        Add-Line $out ([W]::Press([uint32]$procId, [uint32]$vk, ($parts -contains 'ctrl'), ($parts -contains 'shift'), ($parts -contains 'alt')))
      }
      'settext' {
        $h = [W]::MainWin([uint32]$procId); $edit = [IntPtr]::Zero
        foreach ($c in [W]::Children($h)) { if ([W]::Cls($c) -eq 'Edit') { $edit = $c } }
        if ($edit -eq [IntPtr]::Zero) { Add-Line $out 'no Edit control' } else {
          [void][W]::SendMessageW($edit, 0x000C, [IntPtr]::Zero, $text)   # WM_SETTEXT
          Start-Sleep -Milliseconds 200
          [void][W]::PostMessageW($edit, 0x0100, [IntPtr]13, [IntPtr]0x001C0001); [void][W]::PostMessageW($edit, 0x0102, [IntPtr]13, [IntPtr]0x001C0001); [void][W]::PostMessageW($edit, 0x0101, [IntPtr]13, [IntPtr]([int]0xC01C0001))
          Add-Line $out ('settext on Edit 0x{0:X}: ' -f [int64]$edit + [W]::Txt($edit))
        }
      }
      default { Add-Line $out "unknown action $act" }
    }
  } catch { Add-Line $out ("ERR " + $_ + " @ " + $_.InvocationInfo.PositionMessage) }
}

if ($Action -eq 'server') {
  if (-not $Dir) { throw '-Dir is required' }
  New-Item -ItemType Directory -Force $Dir | Out-Null
  Add-Line ($Dir + '\ready') 'ready'
  $jobs = @{}   # caption jobs: procId -> @{ file; last }
  $t0 = Get-Date
  while (((Get-Date) - $t0).TotalMilliseconds -lt $DurationMs) {
    if (Test-Path (Join-Path $Dir 'stop')) { break }
    foreach ($procId in @($jobs.Keys)) {
      $j = $jobs[$procId]
      $h = [W]::MainWin([uint32]$procId)
      if ($h -ne [IntPtr]::Zero) {
        $s = [W]::Txt($h)
        if ($s -ne $j.last) { $j.last = $s; $ms = [int64]([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()); Add-Content -Path $j.file -Value "$ms`t$s" -Encoding UTF8 }
      }
    }
    $cmd = Get-ChildItem -Path $Dir -Filter '*.json' -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -First 1
    if ($cmd) {
      $raw = Get-Content -Raw -Path $cmd.FullName; Remove-Item -Force $cmd.FullName
      $c = $raw | ConvertFrom-Json
      if ($c.action -eq 'caption') { $jobs[[int]$c.pid] = @{ file = $c.out; last = $null }; Add-Line ($Dir + '\' + $c.id + '.done') 'started' }
      elseif ($c.action -eq 'launch') {
        # start a program on THIS (hidden) desktop without another hidden-desktop.ps1 compile; below-normal priority
        $o = $Dir + '\' + $c.id + '.out'
        try {
          $psi = New-Object System.Diagnostics.ProcessStartInfo
          $psi.FileName = $c.exe; $psi.Arguments = $c.args; $psi.UseShellExecute = $false
          if ($c.env) { foreach ($pr in $c.env.PSObject.Properties) { $psi.EnvironmentVariables[$pr.Name] = [string]$pr.Value } }
          $p = [System.Diagnostics.Process]::Start($psi)
          try { $p.PriorityClass = 'BelowNormal' } catch { }
          Add-Line $o ([string]$p.Id)
        } catch { Add-Line $o ('ERR ' + $_) }
        Add-Line ($Dir + '\' + $c.id + '.done') 'done'
      }
      elseif ($c.action -eq 'caption-stop') { $jobs.Remove([int]$c.pid); Add-Line ($Dir + '\' + $c.id + '.done') 'stopped' }
      else {
        Invoke-Act $c.action ([int]$c.pid) ($Dir + '\' + $c.id + '.out') $c.keys $c.text
        Add-Line ($Dir + '\' + $c.id + '.done') 'done'
      }
    }
    Start-Sleep -Milliseconds 20
  }
  return
}

# single-shot
if ($Action -eq 'caption') {
  $last = $null; $t0 = Get-Date
  while (((Get-Date) - $t0).TotalMilliseconds -lt $DurationMs) {
    if ($StopFile -and (Test-Path $StopFile)) { break }
    $h = [W]::MainWin([uint32]$ProcessId)
    if ($h -eq [IntPtr]::Zero) { if (-not (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) { break }; Start-Sleep -Milliseconds 100; continue }
    $s = [W]::Txt($h)
    if ($s -ne $last) { $last = $s; $ms = [int64]([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()); Add-Content -Path $Out -Value "$ms`t$s" -Encoding UTF8 }
    Start-Sleep -Milliseconds 20
  }
} else {
  Invoke-Act $Action $ProcessId $Out $Keys $Text
}
