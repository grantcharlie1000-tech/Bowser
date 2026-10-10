[CmdletBinding()]
param(
    [int] $TimeoutSeconds = 30,
    [switch] $IncludeWebSearch,
    [switch] $IncludePerformancePolicies,
    [string] $OutputDirectory = (Join-Path $PSScriptRoot 'results'),
    [string] $BowserExecutable
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'
$script:clickDiagnostics = New-Object 'System.Collections.Generic.List[object]'
$script:editHandle = [IntPtr]::Zero
$script:lastAddressEntryMethod = ''
$script:lastUiaVisibleDetails = @()
$script:lastUiaVisibleNames = @()
$script:lastVisibleUiaHyperlinkNames = @()
$script:pageArea = 0L
$script:pageView = [IntPtr]::Zero
$script:uiaChildHandles = New-Object 'System.Collections.Generic.List[object]'
$resultDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$bowserPath = if (-not [string]::IsNullOrWhiteSpace($BowserExecutable)) {
    $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($BowserExecutable)
} else {
    Join-Path $repoRoot 'target\release\bowser.exe'
}
if ([string]::IsNullOrWhiteSpace($BowserExecutable) -and -not (Test-Path -LiteralPath $bowserPath)) {
    $bowserPath = Join-Path $repoRoot 'target\x86_64-pc-windows-msvc\release\bowser.exe'
}
if (-not (Test-Path -LiteralPath $bowserPath -PathType Leaf)) { throw "Bowser executable not found: $bowserPath. Build it with cargo build --locked --release --target x86_64-pc-windows-msvc, or supply -BowserExecutable." }
$bowserHashBefore = (Get-FileHash -LiteralPath $bowserPath -Algorithm SHA256).Hash.ToLowerInvariant()

if (-not ('BowserSmoke.Native' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Runtime.InteropServices;
namespace BowserSmoke {
  public static class Native {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION U; }
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int maxCount);
    [DllImport("user32.dll", EntryPoint="SendMessageW", SetLastError=true)] public static extern IntPtr SetEditSelection(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", EntryPoint="SendMessageW", SetLastError=true)] public static extern IntPtr SendCharacter(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool SetWindowText(IntPtr hWnd, string text);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder text, int maxCount);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT point);
    [DllImport("user32.dll")] public static extern bool IsChild(IntPtr parent, IntPtr child);
    [DllImport("user32.dll", EntryPoint="SendMessageTimeoutW", SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int command);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern IntPtr SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetFocus();
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", SetLastError=true)] public static extern bool AttachThreadInput(uint attach, uint attachTo, bool value);
    [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint count, INPUT[] inputs, int size);
    [DllImport("user32.dll", EntryPoint="VkKeyScanW", CharSet=CharSet.Unicode)] private static extern short VkKeyScan(char character);
    public static short MapCharacter(char character) { return VkKeyScan(character); }
    private static void ValidateInput(INPUT input, uint type, ushort virtualKey, ushort scan, uint flags) {
      IntPtr memory = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(INPUT)));
      try {
        Marshal.StructureToPtr(input, memory, false);
        INPUT roundTrip = (INPUT)Marshal.PtrToStructure(memory, typeof(INPUT));
        if (roundTrip.type != type || (type == 1 && (roundTrip.U.ki.wVk != virtualKey || roundTrip.U.ki.wScan != scan || roundTrip.U.ki.dwFlags != flags)) || (type == 0 && roundTrip.U.mi.dwFlags != flags))
          throw new InvalidOperationException("Marshalled Win32 INPUT fields did not match the requested keyboard/mouse event.");
      } finally { Marshal.FreeHGlobal(memory); }
    }
    public static uint SendKeyInput(ushort key, uint flags) {
      INPUT input = new INPUT(); input.type = 1; input.U.ki.wVk = key; input.U.ki.dwFlags = flags;
      ValidateInput(input, 1, key, 0, flags);
      return SendInput(1, new INPUT[] { input }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint SendUnicodeInput(ushort character, uint flags) {
      INPUT input = new INPUT(); input.type = 1; input.U.ki.wScan = character; input.U.ki.dwFlags = flags;
      ValidateInput(input, 1, 0, character, flags);
      return SendInput(1, new INPUT[] { input }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint SendMouseInput(uint flags) {
      INPUT input = new INPUT(); input.type = 0; input.U.mi.dwFlags = flags;
      ValidateInput(input, 0, 0, 0, flags);
      return SendInput(1, new INPUT[] { input }, Marshal.SizeOf(typeof(INPUT)));
    }
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
  }
  public sealed class LocalHttpServer : IDisposable {
    private static readonly object requestLogLock = new object();
    private readonly TcpListener listener;
    private readonly Thread thread;
    private readonly string requestLog;
    private readonly string externalScheme;
    private readonly string fixtureDirectory;
    private volatile bool stopping;
    public int Port { get; private set; }
    public LocalHttpServer(string log, string scheme, string fixtures) {
      requestLog = log;
      externalScheme = scheme;
      fixtureDirectory = fixtures;
      listener = new TcpListener(IPAddress.Loopback, 0);
      listener.Start();
      Port = ((IPEndPoint)listener.LocalEndpoint).Port;
      thread = new Thread(Run); thread.IsBackground = true; thread.Start();
    }
    private void Run() {
      while (!stopping) {
        try {
          TcpClient client = listener.AcceptTcpClient();
          ThreadPool.QueueUserWorkItem(delegate(object state) { Handle((TcpClient)state); }, client);
        } catch { if (!stopping) Thread.Sleep(50); }
      }
    }
    private void Handle(TcpClient client) {
      try {
        using (client) {
          NetworkStream stream = client.GetStream();
          StreamReader reader = new StreamReader(stream, Encoding.ASCII, false, 1024, true);
          string first = reader.ReadLine() ?? "";
          string path = "/";
          string[] parts = first.Split(' ');
          string requestTarget = "/";
          if (parts.Length > 1) { requestTarget = parts[1]; path = requestTarget.Split('?')[0]; }
          lock (requestLogLock) { File.AppendAllText(requestLog, requestTarget + Environment.NewLine, Encoding.UTF8); }
          string title; string content;
          if (path == "/tracking-policy.html" || path == "/minimized-lifecycle.html" || path == "/essential.js") {
            string fileName = path.Substring(1);
            byte[] fixtureBytes;
            if (path == "/essential.js") {
              fixtureBytes = Encoding.UTF8.GetBytes("window.bowserSmoke = window.bowserSmoke || {}; window.bowserSmoke.script = true; window.dispatchEvent(new Event('bowser-essential-script-ready'));");
            } else {
              string fullPath = Path.GetFullPath(Path.Combine(fixtureDirectory, fileName));
              string root = Path.GetFullPath(fixtureDirectory).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
              if (!fullPath.StartsWith(root, StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("Fixture path escaped root.");
              fixtureBytes = File.ReadAllBytes(fullPath);
            }
            string mime = path.EndsWith(".js", StringComparison.OrdinalIgnoreCase) ? "application/javascript" : "text/html";
            string fixtureHeader = "HTTP/1.1 200 OK\r\nContent-Type: " + mime + "; charset=utf-8\r\nContent-Length: " + fixtureBytes.Length + "\r\nConnection: close\r\n\r\n";
            byte[] fixtureHead = Encoding.ASCII.GetBytes(fixtureHeader);
            stream.Write(fixtureHead, 0, fixtureHead.Length); stream.Write(fixtureBytes, 0, fixtureBytes.Length); stream.Flush();
            return;
          } else if (path == "/essential.png") {
            byte[] png = Convert.FromBase64String("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/pXcAAAAASUVORK5CYII=");
            byte[] imageHead = Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: " + png.Length + "\r\nConnection: close\r\n\r\n");
            stream.Write(imageHead, 0, imageHead.Length); stream.Write(png, 0, png.Length); stream.Flush(); return;
          } else if (path == "/heartbeat") {
            byte[] heartbeat = Encoding.UTF8.GetBytes("ok");
            byte[] heartbeatHead = Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: " + heartbeat.Length + "\r\nConnection: close\r\n\r\n");
            stream.Write(heartbeatHead, 0, heartbeatHead.Length); stream.Write(heartbeat, 0, heartbeat.Length); stream.Flush(); return;
          } else if (path == "/slow") {
            Thread.Sleep(120000);
            title = "BOWSER_SMOKE_SLOW"; content = "<p>slow response</p>";
          } else if (path == "/download") {
            byte[] download = Encoding.UTF8.GetBytes("Bowser smoke download payload");
            string downloadHeader = "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=bowser-smoke.txt\r\nContent-Length: " + download.Length + "\r\nConnection: close\r\n\r\n";
            byte[] downloadHead = Encoding.ASCII.GetBytes(downloadHeader);
            stream.Write(downloadHead, 0, downloadHead.Length); stream.Write(download, 0, download.Length); stream.Flush();
            return;
          } else if (path == "/popup-user") {
            title = "BOWSER_SMOKE_POPUP_USER"; content = "<p>User initiated popup destination.</p>";
          } else if (path == "/two") {
            title = "BOWSER_SMOKE_TWO"; content = "<p>Second local page. Use Alt+Left and Alt+Right to check history.</p>";
          } else {
            title = "BOWSER_SMOKE_ONE";
            content = "<main style='font:16px Segoe UI,sans-serif'><a id='result' style='display:block;position:absolute;top:12px;left:12px;height:42px;width:400px' href='/two'>Open local search result / second page</a>" +
              "<a id='userPopup' style='display:block;position:absolute;top:64px;left:12px;height:42px;width:400px' target='_blank' href='/popup-user'>Open user initiated popup</a>" +
              "<button style='position:absolute;top:116px;left:12px;height:42px;width:400px' onclick=\"setTimeout(function(){window.open('/popup-auto')},6500)\">Open delayed non-user popup</button>" +
              "<button style='position:absolute;top:168px;left:12px;height:42px;width:400px' onclick=\"navigator.geolocation.getCurrentPosition(function(){document.title='BOWSER_SMOKE_PERMISSION_UNEXPECTED'},function(){})\">Request geolocation permission</button>" +
              "<a style='display:block;position:absolute;top:220px;left:12px;height:42px;width:400px' href='/download' download='bowser-smoke.txt'>Start test download</a>" +
              "<a style='display:block;position:absolute;top:272px;left:12px;height:42px;width:400px' href='" + externalScheme + ":test'>Open blocked external protocol</a></main>";
          }
          string body = "<!doctype html><meta charset='utf-8'><title>" + title + "</title><h1>" + title + "</h1>" + content;
          byte[] bytes = Encoding.UTF8.GetBytes(body);
          string header = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: " + bytes.Length + "\r\nConnection: close\r\n\r\n";
          byte[] head = Encoding.ASCII.GetBytes(header);
          stream.Write(head, 0, head.Length); stream.Write(bytes, 0, bytes.Length); stream.Flush();
        }
      } catch { }
    }
    public void Dispose() { stopping = true; try { listener.Stop(); } catch { } }
  }
}
'@
}

function Get-WindowText([IntPtr] $Handle) {
    $buffer = New-Object System.Text.StringBuilder 1024
    [void][BowserSmoke.Native]::GetWindowText($Handle, $buffer, $buffer.Capacity)
    return $buffer.ToString()
}
function Get-EditText([IntPtr] $Handle) {
    # GetWindowText cannot retrieve another process's EDIT control contents.
    # Use WM_GETTEXT via SendMessageTimeout so a hung target cannot stall smoke.
    $lengthResult = [IntPtr]::Zero
    $sent = [BowserSmoke.Native]::SendMessageTimeout($Handle, 0x000E, [IntPtr]::Zero, [IntPtr]::Zero, 0x0002, 1000, [ref]$lengthResult)
    if ($sent -eq [IntPtr]::Zero) { throw 'Timed out reading native EDIT length with WM_GETTEXTLENGTH.' }
    $length = $lengthResult.ToInt64()
    if ($length -lt 0 -or $length -gt 32767) { throw "Native EDIT returned invalid text length: $length." }
    $capacity = [int]$length + 1
    $buffer = [Runtime.InteropServices.Marshal]::AllocHGlobal($capacity * 2)
    try {
        $textResult = [IntPtr]::Zero
        $sent = [BowserSmoke.Native]::SendMessageTimeout($Handle, 0x000D, [IntPtr]$capacity, $buffer, 0x0002, 1000, [ref]$textResult)
        if ($sent -eq [IntPtr]::Zero) { throw 'Timed out reading native EDIT text with WM_GETTEXT.' }
        return [Runtime.InteropServices.Marshal]::PtrToStringUni($buffer)
    } finally {
        [Runtime.InteropServices.Marshal]::FreeHGlobal($buffer)
    }
}
function Get-WindowClass([IntPtr] $Handle) {
    $buffer = New-Object System.Text.StringBuilder 256
    [void][BowserSmoke.Native]::GetClassName($Handle, $buffer, $buffer.Capacity)
    return $buffer.ToString()
}
function Find-Window([int] $ProcessId, [string] $Marker = '') {
    $windows = New-Object 'System.Collections.Generic.List[object]'
    $callback = [BowserSmoke.Native+EnumWindowsProc] {
        param([IntPtr] $hWnd, [IntPtr] $unused)
        [uint32] $windowProcessId = 0
        [void][BowserSmoke.Native]::GetWindowThreadProcessId($hWnd, [ref] $windowProcessId)
        if ($windowProcessId -eq $ProcessId -and [BowserSmoke.Native]::IsWindowVisible($hWnd)) {
            $title = Get-WindowText $hWnd
            if ($title) { $windows.Add([pscustomobject]@{ Handle=$hWnd; Title=$title }) }
        }
        return $true
    }
    [void][BowserSmoke.Native]::EnumWindows($callback, [IntPtr]::Zero)
    if ($Marker) { return $windows | Where-Object { $_.Title -like "*$Marker*" } | Select-Object -First 1 }
    return $windows | Select-Object -First 1
}
function Find-Dialogs([int] $ProcessId) {
    $dialogs = New-Object 'System.Collections.Generic.List[object]'
    $topCallback = [BowserSmoke.Native+EnumWindowsProc] {
        param([IntPtr] $hWnd, [IntPtr] $unused)
        [uint32] $windowProcessId = 0
        [void][BowserSmoke.Native]::GetWindowThreadProcessId($hWnd, [ref] $windowProcessId)
        if ($windowProcessId -eq $ProcessId -and [BowserSmoke.Native]::IsWindowVisible($hWnd) -and (Get-WindowClass $hWnd) -eq '#32770') {
            $labels = New-Object 'System.Collections.Generic.List[string]'
            $childCallback = [BowserSmoke.Native+EnumWindowsProc] {
                param([IntPtr] $child, [IntPtr] $childUnused)
                if ((Get-WindowClass $child) -eq 'Static') {
                    $text = Get-WindowText $child
                    if ($text) { $labels.Add($text) }
                }
                return $true
            }
            [void][BowserSmoke.Native]::EnumChildWindows($hWnd, $childCallback, [IntPtr]::Zero)
            $dialogs.Add([pscustomobject]@{ Handle=$hWnd; Title=(Get-WindowText $hWnd); Text=@($labels) })
        }
        return $true
    }
    [void][BowserSmoke.Native]::EnumWindows($topCallback, [IntPtr]::Zero)
    return $dialogs.ToArray()
}
function Wait-Title([int] $ProcessId, [string] $Marker, [int] $Seconds) {
    $end = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $end) {
        $window = Find-Window $ProcessId $Marker
        if ($window) {
            $result = [IntPtr]::Zero
            if ([BowserSmoke.Native]::SendMessageTimeout($window.Handle, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 250, [ref] $result) -ne [IntPtr]::Zero) { return $window }
        }
        Start-Sleep -Milliseconds 100
    }
    return $null
}
function Send-Key([uint16] $Key, [bool] $Up = $false) {
    $flags = if ($Up) { [uint32]2 } else { [uint32]0 }
    if ([BowserSmoke.Native]::SendKeyInput($Key, $flags) -ne 1) {
        $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw (New-Object ComponentModel.Win32Exception($errorCode, 'SendInput failed while sending a navigation key.'))
    }
}
function Reset-InjectedKeyState {
    foreach ($key in @(0x0D, 0x10, 0x11, 0x12)) {
        try { Send-Key ([uint16]$key) $true } catch { }
    }
}
function Send-Text([string] $Text) {
    foreach ($character in $Text.ToCharArray()) {
        $mapped = [BowserSmoke.Native]::MapCharacter([char]$character)
        if ($mapped -lt 0) { throw "The active keyboard layout cannot type '$character'." }
        $virtualKey = [uint16]($mapped -band 0xff); $modifiers = ($mapped -shr 8) -band 0xff
        if ($modifiers -band 1) { Send-Key 0x10 }
        if ($modifiers -band 2) { Send-Key 0x11 }
        if ($modifiers -band 4) { Send-Key 0x12 }
        Send-Key $virtualKey; Send-Key $virtualKey $true
        if ($modifiers -band 4) { Send-Key 0x12 $true }
        if ($modifiers -band 2) { Send-Key 0x11 $true }
        if ($modifiers -band 1) { Send-Key 0x10 $true }
    }
}
function Send-TextToEdit([IntPtr] $Edit, [string] $Text) {
    [void][BowserSmoke.Native]::SetEditSelection($Edit, 0x00B1, [IntPtr]::Zero, [IntPtr]::new(-1))
    foreach ($character in $Text.ToCharArray()) {
        [void][BowserSmoke.Native]::SendCharacter($Edit, 0x0102, [IntPtr][int][char]$character, [IntPtr]::Zero)
    }
}
function Find-Edit([IntPtr] $Window) {
    $script:editHandle = [IntPtr]::Zero
    $callback = [BowserSmoke.Native+EnumWindowsProc] {
        param([IntPtr] $hWnd, [IntPtr] $unused)
        if ((Get-WindowClass $hWnd) -eq 'Edit') { $script:editHandle = $hWnd; return $false }
        return $true
    }
    [void][BowserSmoke.Native]::EnumChildWindows($Window, $callback, [IntPtr]::Zero)
    return $script:editHandle
}
function Find-PageView([IntPtr] $Window) {
    $script:pageView = [IntPtr]::Zero; $script:pageArea = 0L
    $callback = [BowserSmoke.Native+EnumWindowsProc] {
        param([IntPtr] $hWnd, [IntPtr] $unused)
        if ([BowserSmoke.Native]::IsWindowVisible($hWnd) -and (Get-WindowClass $hWnd) -ne 'Edit') {
            $rect = New-Object BowserSmoke.Native+RECT
            [void][BowserSmoke.Native]::GetWindowRect($hWnd, [ref] $rect)
            $area = [Math]::Max(0, $rect.Right - $rect.Left) * [Math]::Max(0, $rect.Bottom - $rect.Top)
            if ($area -gt $script:pageArea) { $script:pageArea = $area; $script:pageView = $hWnd }
        }
        return $true
    }
    [void][BowserSmoke.Native]::EnumChildWindows($Window, $callback, [IntPtr]::Zero)
    return $script:pageView
}
function Click-PageAt([IntPtr] $Window, [int] $X, [int] $Y) {
    $view = Find-PageView $Window
    if ($view -eq [IntPtr]::Zero) { throw 'The WebView2 page surface was not found.' }
    $rect = New-Object BowserSmoke.Native+RECT
    [void][BowserSmoke.Native]::GetWindowRect($view, [ref] $rect)
    $dpi = [BowserSmoke.Native]::GetDpiForWindow($view)
    if ($dpi -eq 0) { $dpi = 96 }
    $physicalX = [int][Math]::Round($X * $dpi / 96.0)
    $physicalY = [int][Math]::Round($Y * $dpi / 96.0)
    $mainRect = New-Object BowserSmoke.Native+RECT
    [void][BowserSmoke.Native]::GetWindowRect($Window, [ref] $mainRect)
    if ($script:clickDiagnostics -is [System.Collections.Generic.List[object]]) {
        $script:clickDiagnostics.Add([pscustomobject]@{ x_css=$X; y_css=$Y; dpi=[int]$dpi; child_class=(Get-WindowClass $view); child_left=$rect.Left; child_top=$rect.Top; child_right=$rect.Right; child_bottom=$rect.Bottom; click_x=$rect.Left+$physicalX; click_y=$rect.Top+$physicalY; main_left=$mainRect.Left; main_top=$mainRect.Top; main_right=$mainRect.Right; main_bottom=$mainRect.Bottom })
    }
    if (($rect.Right - $rect.Left) -lt ($physicalX + 4) -or ($rect.Bottom - $rect.Top) -lt ($physicalY + 4)) { throw "Page control coordinate ($X,$Y) is outside the WebView2 surface." }
    $activated = [BowserSmoke.Native]::SetForegroundWindow($Window)
    Start-Sleep -Milliseconds 50
    $foreground = [BowserSmoke.Native]::GetForegroundWindow()
    if ($foreground -ne $Window) { throw "Bowser is not foreground before a page click (SetForegroundWindow=$activated; foreground HWND=$foreground)." }
    if (-not [BowserSmoke.Native]::SetCursorPos($rect.Left + $physicalX, $rect.Top + $physicalY)) { throw 'Windows rejected the page-click cursor position.' }
    if ([BowserSmoke.Native]::SendMouseInput(0x0002) -ne 1) { throw 'SendInput failed on page mouse-down.' }
    if ([BowserSmoke.Native]::SendMouseInput(0x0004) -ne 1) { throw 'SendInput failed on page mouse-up.' }
    Start-Sleep -Milliseconds 200
}
function Wait-EventName([string] $Path, [string] $Name, [int] $Seconds = 8) {
    $end = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $end) {
        if (Test-Path -LiteralPath $Path) {
            foreach ($record in Import-Csv -LiteralPath $Path -ErrorAction SilentlyContinue) {
                if ($record.event -eq $Name) { return $true }
            }
        }
        Start-Sleep -Milliseconds 100
    }
    return $false
}
function Wait-EventAfterCount([string] $Path, [string] $Name, [int] $Count, [int] $Seconds = 8) {
    $end = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $end) {
        $rows = @(Get-LoggedEvents $Path)
        if ($rows.Count -gt $Count -and @($rows | Select-Object -Skip $Count | Where-Object event -eq $Name).Count -gt 0) { return $true }
        Start-Sleep -Milliseconds 100
    }
    return $false
}
function Focus-Address([int] $ProcessId, [IntPtr] $Window) {
    $activated = [BowserSmoke.Native]::SetForegroundWindow($Window)
    Start-Sleep -Milliseconds 100
    $foreground = [BowserSmoke.Native]::GetForegroundWindow()
    $edit = Find-Edit $Window
    if ($edit -eq [IntPtr]::Zero) { throw 'The native address EDIT control was not found.' }
    if ($foreground -ne $Window) {
        # Windows may deny foreground activation after restore. A physical click gives the same
        # activation path as a user and lets the smoke verify real address focus.
        $editRect = New-Object BowserSmoke.Native+RECT
        if (-not [BowserSmoke.Native]::GetWindowRect($edit, [ref]$editRect)) { throw "Could not inspect address EDIT bounds after SetForegroundWindow=$activated." }
        $centerX = [int](($editRect.Left + $editRect.Right) / 2)
        $centerY = [int](($editRect.Top + $editRect.Bottom) / 2)
        $point = New-Object BowserSmoke.Native+POINT
        $point.X = $centerX; $point.Y = $centerY
        $hit = [BowserSmoke.Native]::WindowFromPoint($point)
        [uint32]$hitPid = 0
        if ($hit -ne [IntPtr]::Zero) { [void][BowserSmoke.Native]::GetWindowThreadProcessId($hit, [ref]$hitPid) }
        if ($hitPid -ne $ProcessId -or ($hit -ne $Window -and -not [BowserSmoke.Native]::IsChild($Window, $hit))) { throw "Address activation blocked safely: click point is occluded by HWND=$hit process=$hitPid, target process=$ProcessId." }
        if (-not [BowserSmoke.Native]::SetCursorPos($centerX, $centerY) -or [BowserSmoke.Native]::SendMouseInput(0x0002) -ne 1 -or [BowserSmoke.Native]::SendMouseInput(0x0004) -ne 1) { throw "Could not click address EDIT after SetForegroundWindow=$activated." }
        Start-Sleep -Milliseconds 100
        $foreground = [BowserSmoke.Native]::GetForegroundWindow()
    }
    if ($foreground -ne $Window) { throw "Bowser is not the foreground smoke-test target after activation and address click (SetForegroundWindow=$activated; foreground HWND=$foreground)." }
    [uint32] $editPid = 0
    $editThread = [BowserSmoke.Native]::GetWindowThreadProcessId($edit, [ref] $editPid)
    $currentThread = [BowserSmoke.Native]::GetCurrentThreadId()
    [void][BowserSmoke.Native]::AttachThreadInput($currentThread, $editThread, $true)
    [void][BowserSmoke.Native]::SetFocus($edit)
    $focusedWindow = [BowserSmoke.Native]::GetFocus()
    [void][BowserSmoke.Native]::AttachThreadInput($currentThread, $editThread, $false)
    if ($focusedWindow -ne $edit) { throw 'The Bowser native address EDIT control did not become focused.' }
}
function Get-AddressValue([IntPtr] $Window) {
    $edit = Find-Edit $Window
    if ($edit -eq [IntPtr]::Zero) { return '' }
    return Get-EditText $edit
}
function Get-VisibleResultLinks([IntPtr] $Window) {
    $root = [System.Windows.Automation.AutomationElement]::FromHandle($Window)
    $condition = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Hyperlink)
    $elements = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    $links = New-Object 'System.Collections.Generic.List[object]'
    $allNames = New-Object 'System.Collections.Generic.List[string]'
    for ($i = 0; $i -lt $elements.Count; $i++) {
        $element = $elements.Item($i)
        try {
            $current = $element.Current
            if ($current.Name) { $allNames.Add([string]$current.Name) }
            if (-not $current.IsOffscreen -and $current.BoundingRectangle.Width -gt 0 -and $current.BoundingRectangle.Height -gt 0 -and $current.Name) {
                $links.Add([pscustomobject]@{ Element=$element; Name=[string]$current.Name; Bounds=$current.BoundingRectangle })
            }
        } catch { }
    }
    $script:lastVisibleUiaHyperlinkNames = @($allNames.ToArray())
    return @($links.ToArray())
}
function Find-UiaTarget([IntPtr] $Window, [string] $NamePattern) {
    $handles = New-Object 'System.Collections.Generic.List[IntPtr]'
    $handles.Add($Window)
    $script:uiaChildHandles = New-Object 'System.Collections.Generic.List[object]'
    $childCallback = [BowserSmoke.Native+EnumWindowsProc] {
        param([IntPtr] $hWnd, [IntPtr] $unused)
        if ([BowserSmoke.Native]::IsWindowVisible($hWnd)) {
            $script:uiaChildHandles.Add([pscustomobject]@{ Handle=$hWnd; Class=(Get-WindowClass $hWnd) })
            $handles.Add($hWnd)
        }
        return $true
    }
    [void][BowserSmoke.Native]::EnumChildWindows($Window, $childCallback, [IntPtr]::Zero)
    $targetMatches = New-Object 'System.Collections.Generic.List[object]'
    $visibleNames = New-Object 'System.Collections.Generic.List[string]'
    $visibleDetails = New-Object 'System.Collections.Generic.List[object]'
    foreach ($candidateHandle in $handles) {
        try {
            $root = [System.Windows.Automation.AutomationElement]::FromHandle($candidateHandle)
            $elements = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
            for ($i = 0; $i -lt $elements.Count; $i++) {
                $element = $elements.Item($i)
                try {
                    $current = $element.Current
                    $bounds = $current.BoundingRectangle
                    if ($current.Name) {
                        $visibleNames.Add(('{0}: {1}' -f $current.ControlType.ProgrammaticName, $current.Name))
                        $visibleDetails.Add([pscustomobject]@{ control_type=[string]$current.ControlType.ProgrammaticName; name=[string]$current.Name; offscreen=[bool]$current.IsOffscreen; left=[double]$bounds.Left; top=[double]$bounds.Top; width=[double]$bounds.Width; height=[double]$bounds.Height })
                    }
                    if (-not $current.IsOffscreen -and $bounds.Width -gt 2 -and $bounds.Height -gt 2 -and $current.Name -match $NamePattern) {
                        $targetMatches.Add([pscustomobject]@{ Element=$element; Name=[string]$current.Name; ControlType=[string]$current.ControlType.ProgrammaticName; Bounds=$bounds })
                    }
                } catch { }
            }
        } catch { }
    }
    $script:lastUiaVisibleNames = @($visibleNames.ToArray())
    $script:lastUiaVisibleDetails = @($visibleDetails.ToArray())
    return @($targetMatches.ToArray())
}
function Click-UiaTarget([IntPtr] $Window, [string] $NamePattern) {
    $foundTargets = @(Find-UiaTarget $Window $NamePattern)
    if ($foundTargets.Count -eq 0) { throw "No visible UIA target matched '$NamePattern'; visible UIA controls: $($script:lastUiaVisibleNames -join ' | ')" }
    return Click-UiaElement $Window $foundTargets[0]
}
function Click-UiaElement([IntPtr] $Window, $item) {
    $mainRect = New-Object BowserSmoke.Native+RECT
    [void][BowserSmoke.Native]::GetWindowRect($Window, [ref] $mainRect)
    $x = [int][Math]::Round($item.Bounds.Left + $item.Bounds.Width / 2)
    $y = [int][Math]::Round($item.Bounds.Top + $item.Bounds.Height / 2)
    if ($x -lt $mainRect.Left -or $x -ge $mainRect.Right -or $y -lt $mainRect.Top -or $y -ge $mainRect.Bottom) { throw "UIA target bounds lie outside Bowser HWND: $($item.Name) at ($x,$y), main=($($mainRect.Left),$($mainRect.Top),$($mainRect.Right),$($mainRect.Bottom))." }
    $script:clickDiagnostics.Add([pscustomobject]@{ name=$item.Name; control_type=$item.ControlType; left=[double]$item.Bounds.Left; top=[double]$item.Bounds.Top; width=[double]$item.Bounds.Width; height=[double]$item.Bounds.Height; click_x=$x; click_y=$y; main_left=$mainRect.Left; main_top=$mainRect.Top; main_right=$mainRect.Right; main_bottom=$mainRect.Bottom })
    $activated = [BowserSmoke.Native]::SetForegroundWindow($Window)
    Start-Sleep -Milliseconds 50
    if ([BowserSmoke.Native]::GetForegroundWindow() -ne $Window) { throw "Bowser is not foreground before UIA element click (SetForegroundWindow=$activated)." }
    if (-not [BowserSmoke.Native]::SetCursorPos($x, $y)) { throw "SetCursorPos failed for UIA target '$($item.Name)'." }
    if ([BowserSmoke.Native]::SendMouseInput(0x0002) -ne 1) { throw "SendInput mouse-down failed for UIA target '$($item.Name)'." }
    if ([BowserSmoke.Native]::SendMouseInput(0x0004) -ne 1) { throw "SendInput mouse-up failed for UIA target '$($item.Name)'." }
    Start-Sleep -Milliseconds 200
    return $item
}
function Get-CurrentPageUrl([IntPtr] $Window) {
    $urlPanes = @(Find-UiaTarget $Window '^https?://' | Where-Object { $_.ControlType -eq 'ControlType.Pane' -and $_.Name -match '^https?://' })
    if ($urlPanes.Count -gt 0) { return [string]$urlPanes[0].Name }
    return ''
}
function Invoke-VisibleLink([IntPtr] $Window, [string] $NamePattern) {
    $matchingLinks = @(Get-VisibleResultLinks $Window | Where-Object { $_.Name -match $NamePattern })
    if ($matchingLinks.Count -eq 0) { throw "No visible UIA hyperlink matched '$NamePattern'; visible UIA Hyperlink names: $($script:lastVisibleUiaHyperlinkNames -join ' | ')" }
    $item = $matchingLinks[0]
    $invokePattern = $null
    if (-not $item.Element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invokePattern)) {
        throw "Visible link '$($item.Name)' did not expose UI Automation InvokePattern."
    }
    $invokePattern.Invoke()
    return $item.Name
}
function Navigate([int] $ProcessId, [IntPtr] $Window, [string] $Url) {
    Reset-InjectedKeyState
    Focus-Address $ProcessId $Window
    Send-Key 0x11; Send-Key 0x41; Send-Key 0x41 $true; Send-Key 0x11 $true
    $edit = Find-Edit $Window
    Send-Text $Url
    $enteredText = Get-EditText $edit
    $entryMethod = 'SendInput'
    if ($enteredText -cne $Url) {
        Send-Key 0x11; Send-Key 0x41; Send-Key 0x41 $true; Send-Key 0x11 $true
        [System.Windows.Forms.SendKeys]::SendWait($Url)
        $enteredText = Get-EditText $edit
        $entryMethod = 'Windows Forms SendKeys after virtual-key SendInput'
    }
    if ($enteredText -cne $Url) {
        Send-TextToEdit $edit $Url
        $enteredText = Get-EditText $edit
        $entryMethod = 'WM_CHAR messages to native EDIT after SendInput and SendKeys did not populate the field'
    }
    if ($enteredText -cne $Url) {
        if (-not [BowserSmoke.Native]::SetWindowText($edit, $Url)) { throw "Address input text mismatch before Enter; SetWindowText fallback failed. Expected '$Url'; actual '$enteredText'." }
        $enteredText = Get-EditText $edit
        $entryMethod = 'native EDIT SetWindowText fallback after SendInput, SendKeys, and WM_CHAR'
    }
    if ($enteredText -cne $Url) { throw "Address input text mismatch before Enter. Expected '$Url'; actual '$enteredText'." }
    Send-Key 0x0D
    Send-Key 0x0D $true
    $script:lastAddressEntryMethod = $entryMethod
}
function Get-Descendants([int] $RootPid, [string] $UserDataProfile) {
    $all = @(Get-CimInstance -ClassName Win32_Process -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject]@{ Pid=[int]$_.ProcessId; Parent=[int]$_.ParentProcessId; Name=[string]$_.Name; CommandLine=[string]$_.CommandLine } })
    $ids = New-Object 'System.Collections.Generic.HashSet[int]'; [void]$ids.Add($RootPid)
    $profileVariant = $UserDataProfile.Replace('\', '/')
    foreach ($processEntry in $all) {
        if ($processEntry.CommandLine -and ($processEntry.CommandLine.IndexOf($UserDataProfile, [StringComparison]::OrdinalIgnoreCase) -ge 0 -or $processEntry.CommandLine.IndexOf($profileVariant, [StringComparison]::OrdinalIgnoreCase) -ge 0)) { [void]$ids.Add($processEntry.Pid) }
    }
    $changed = $true
    while ($changed) { $changed = $false; foreach ($p in $all) { if ($ids.Contains($p.Parent) -and -not $ids.Contains($p.Pid)) { [void]$ids.Add($p.Pid); $changed = $true } } }
    return @($all | Where-Object { $ids.Contains($_.Pid) })
}
function Wait-GroupExit([int] $RootPid, [string] $UserDataProfile, [int] $Seconds = 60) {
    $end = [DateTime]::UtcNow.AddSeconds($Seconds)
    do { $remaining = @(Get-Descendants $RootPid $UserDataProfile); if ($remaining.Count -eq 0) { return $true }; Start-Sleep -Milliseconds 250 } while ([DateTime]::UtcNow -lt $end)
    return $false
}
function Save-WindowScreenshot([IntPtr] $Window, [string] $Path) {
    $rect = New-Object BowserSmoke.Native+RECT
    if (-not [BowserSmoke.Native]::GetWindowRect($Window, [ref] $rect)) { return $false }
    $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
    if ($width -le 0 -or $height -le 0) { return $false }
    $bitmap = New-Object System.Drawing.Bitmap($width, $height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($width, $height)))
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        return $true
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}

function Get-LoggedEvents([string] $Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return @() }
    return @(Import-Csv -LiteralPath $Path -ErrorAction SilentlyContinue)
}
function Get-LifecycleSamples([string] $Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return @() }
    $samples = New-Object 'System.Collections.Generic.List[object]'
    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        $match = [regex]::Match($line, '^/heartbeat\?tick=(\d+)&media=([^&]+)&time=([0-9.]+)')
        if ($match.Success) { $samples.Add([pscustomobject]@{ ticks=[int]$match.Groups[1].Value; media=$match.Groups[2].Value; time=[double]$match.Groups[3].Value }) }
    }
    return @($samples.ToArray())
}

function Invoke-PerformancePolicySmoke {
    if ([string]::IsNullOrWhiteSpace($BowserExecutable)) { throw '-IncludePerformancePolicies requires an explicit -BowserExecutable path.' }
    if (-not (Test-Path -LiteralPath $bowserPath -PathType Leaf)) { throw "Bowser executable not found: $bowserPath" }

    $runRoot = Join-Path ([IO.Path]::GetTempPath()) ('bowser-policy-smoke-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $runRoot | Out-Null
    $requestLog = Join-Path $runRoot 'requests.txt'
    $server = [BowserSmoke.LocalHttpServer]::new($requestLog, 'unused', (Join-Path $PSScriptRoot 'fixtures'))
    $results = New-Object 'System.Collections.Generic.List[object]'
    $hashBefore = (Get-FileHash -LiteralPath $bowserPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $profileRoots = New-Object 'System.Collections.Generic.List[string]'
    $allGroupsClosed = $true
    $policyOutcomes = New-Object 'System.Collections.Generic.List[object]'
    $activeProcessId = 0
    $activeProfile = $null
    try {
        foreach ($policy in @('balanced','strict')) {
            $profile = Join-Path $runRoot ('profile-' + $policy)
            [void]$profileRoots.Add($profile)
            $profileSafeToReuse = $true
            foreach ($pass in @(1,2)) {
                if ($pass -eq 2 -and -not $profileSafeToReuse) {
                    $results.Add([pscustomobject]@{ test="${policy}_restart_readback"; result='BLOCKED_PROFILE_UNKNOWN'; detail='Initial policy readback rollback left the profile state unknown, so this root was not reused.' })
                    break
                }
                $label = if ($pass -eq 1) { 'fresh' } else { 'restart' }
                $eventLog = Join-Path $runRoot ($policy + '-' + $label + '.csv')
                $psi = New-Object Diagnostics.ProcessStartInfo
                $psi.FileName = $bowserPath
                $psi.WorkingDirectory = $repoRoot
                $psi.UseShellExecute = $false
                $psi.Arguments = '--benchmark-log "' + $eventLog + '" --benchmark-user-data-dir "' + $profile + '" --benchmark-tracking-prevention ' + $policy
                $hashAtLaunch = (Get-FileHash -LiteralPath $bowserPath -Algorithm SHA256).Hash.ToLowerInvariant()
                if ($hashAtLaunch -ne $hashBefore) { throw 'Pinned Bowser executable changed before a policy launch.' }
                $process = [Diagnostics.Process]::Start($psi)
                $rootPid = $process.Id
                $activeProcessId = $rootPid
                $activeProfile = $profile
                $window = $null
                $windowDeadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
                while ([DateTime]::UtcNow -lt $windowDeadline -and -not $window) { $window = Find-Window $rootPid; Start-Sleep -Milliseconds 100 }
                if (-not $window) { throw "$policy/${label}: Bowser did not create a visible window." }

                # For the first policy pass, minimize before submitting navigation so engine-ready
                # reconciliation observes the live minimized state while asynchronous creation runs.
                $raceRequested = ($policy -eq 'balanced' -and $pass -eq 1)
                $base = 'http://127.0.0.1:' + $server.Port
                Navigate $rootPid $window.Handle ($base + '/tracking-policy.html')
                if ($raceRequested) {
                    if (-not (Wait-EventName $eventLog 'webview_environment_requested' $TimeoutSeconds)) { throw 'Creation-race smoke did not observe asynchronous environment creation.' }
                    if (@((Get-LoggedEvents $eventLog) | Where-Object event -eq 'engine_ready').Count -gt 0) { throw 'Creation-race smoke reached engine_ready before minimize could be injected.' }
                    [void][BowserSmoke.Native]::ShowWindow($window.Handle, 6)
                }
                $ready = Wait-Title $rootPid 'TRACKING_POLICY_READY' $TimeoutSeconds
                if (-not $ready) { throw "$policy/${label}: essential script/image page did not become ready." }
                $title = Get-WindowText $ready.Handle
                if ($title -notmatch 'script=True\|image=True') { throw "$policy/${label}: essential first-party content failed: $title" }
                $eventRows = @(Get-LoggedEvents $eventLog)
                $requestedName = 'tracking_prevention_requested_' + $policy
                $readbackName = 'tracking_prevention_readback_' + $policy
                $requested = @($eventRows | Where-Object event -eq $requestedName).Count -gt 0
                $applied = @($eventRows | Where-Object event -eq 'tracking_prevention_applied').Count -gt 0
                $readback = @($eventRows | Where-Object event -eq $readbackName).Count -gt 0
                $profileUnknown = @($eventRows | Where-Object event -eq 'tracking_prevention_profile_unknown').Count -gt 0
                $restored = @($eventRows | Where-Object event -eq 'tracking_prevention_restored').Count -gt 0
                $unsupported = @($eventRows | Where-Object event -eq 'tracking_prevention_unsupported').Count -gt 0
                $policyError = @($eventRows | Where-Object event -in @('tracking_prevention_error','tracking_prevention_mismatch','tracking_prevention_readback_other')).Count -gt 0
                $firstNavigation = @($eventRows | Where-Object event -eq 'navigation_started' | Select-Object -First 1)
                $policyReadyIndex = -1; $navigationIndex = -1
                for ($i=0; $i -lt $eventRows.Count; $i++) {
                    if ($eventRows[$i].event -eq $readbackName) { $policyReadyIndex = $i }
                    if ($eventRows[$i].event -eq 'navigation_started' -and $navigationIndex -lt 0) { $navigationIndex = $i }
                }
                if ($profileUnknown) {
                    $policyState = 'FAIL'
                    $profileSafeToReuse = $false
                    $policyDetail = 'Policy readback failed and the prior profile value could not be safely restored.'
                } elseif ($restored) {
                    $policyState = 'BLOCKED_RESTORED'
                    $policyDetail = 'Requested policy was not verified; prior profile setting was restored and read back successfully.'
                } elseif ($unsupported) {
                    $policyState = 'BLOCKED_UNSUPPORTED'
                    $policyDetail = 'Runtime did not confirm requested policy; browsing and first-party essential content remained usable.'
                } elseif ($policyError) {
                    $policyState = 'BLOCKED_POLICY_ERROR'
                    $policyDetail = 'Policy setter/readback did not verify the requested level; browsing and first-party essential content remained usable.'
                } elseif (-not ($requested -and $applied -and $readback -and $policyReadyIndex -ge 0 -and $navigationIndex -gt $policyReadyIndex)) {
                    throw "$policy/${label}: requested/effective/readback events were incomplete or occurred after navigation."
                } else {
                    $policyState = 'PASS'
                    $policyDetail = "Requested, applied and read back $policy before first navigation."
                }
                if ($raceRequested) {
                    $lowIndex = -1; $firstNavigationIndex = -1
                    for ($i=0; $i -lt $eventRows.Count; $i++) {
                        if ($eventRows[$i].event -in @('memory_target_applied_low','memory_target_unsupported') -and $lowIndex -lt 0) { $lowIndex = $i }
                        if ($eventRows[$i].event -eq 'navigation_started' -and $firstNavigationIndex -lt 0) { $firstNavigationIndex = $i }
                    }
                    if ($lowIndex -lt 0 -or $firstNavigationIndex -le $lowIndex) { throw 'Minimized engine-ready reconciliation did not report Low or unsupported before pending navigation.' }
                    $results.Add([pscustomobject]@{ test='minimize_during_engine_creation_order'; result='PASS'; detail='Current minimized target outcome was logged before the pending first navigation.' })
                }
                $results.Add([pscustomobject]@{ test="${policy}_${label}_policy_readback"; result=$policyState; detail=$policyDetail })
                $candidate = if ($title -match 'candidate=([^|]+)') { $Matches[1] } else { 'unobserved' }
                $results.Add([pscustomobject]@{ test="${policy}_${label}_essential_content"; result='PASS'; detail="Same-origin script and image loaded; tracking candidate result=$candidate (classification is inconclusive unless runtime behavior is independently identified)." })

                if ($raceRequested) {
                    $minimized = Wait-EventName $eventLog 'window_minimized' $TimeoutSeconds
                    if (-not $minimized) { throw 'Creation-race smoke did not log window_minimized.' }
                    $lowApplied = Wait-EventName $eventLog 'memory_target_applied_low' 2
                    if (-not $lowApplied) {
                        $unsupportedLow = Wait-EventName $eventLog 'memory_target_unsupported' 1
                        if (-not $unsupportedLow) { throw 'Creation-race smoke neither applied Low nor reported the target unsupported.' }
                        $results.Add([pscustomobject]@{ test='minimized_memory_target'; result='BLOCKED_UNSUPPORTED'; detail='WebView2 _19 memory target API unavailable; lifecycle continuity checks still ran.' })
                    } else {
                        $results.Add([pscustomobject]@{ test='minimized_memory_target'; result='PASS'; detail='Low memory target accepted after minimized transition.' })
                    }
                    $raceRestoreCount = @(Get-LoggedEvents $eventLog).Count
                    [void][BowserSmoke.Native]::ShowWindow($ready.Handle, 9)
                    if (-not (Wait-EventAfterCount $eventLog 'window_restored' $raceRestoreCount 5)) { throw 'Creation-race window did not restore after engine became ready.' }
                    Start-Sleep -Milliseconds 250
                    $lifeUrl = $base + '/minimized-lifecycle.html'
                    Navigate $rootPid $ready.Handle $lifeUrl
                    $life = Find-Window $rootPid
                    $initialLifeDeadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
                    $samplesBefore = @(Get-LifecycleSamples $requestLog)
                    while ([DateTime]::UtcNow -lt $initialLifeDeadline -and $samplesBefore.Count -eq 0) { Start-Sleep -Milliseconds 100; $samplesBefore = @(Get-LifecycleSamples $requestLog) }
                    if ($samplesBefore.Count -eq 0) { throw 'Lifecycle fixture did not issue its initial loopback telemetry request.' }
                    $lifecycleMinimizeCount = @(Get-LoggedEvents $eventLog).Count
                    [void][BowserSmoke.Native]::ShowWindow($life.Handle, 6)
                    if (-not (Wait-EventAfterCount $eventLog 'window_minimized' $lifecycleMinimizeCount 5)) { throw 'Lifecycle page did not log its minimized transition.' }
                    Start-Sleep -Seconds 3
                    $samplesDuring = @(Get-LifecycleSamples $requestLog)
                    Start-Sleep -Seconds 2
                    $samplesAfter = @(Get-LifecycleSamples $requestLog)
                    if ($samplesAfter.Count -le $samplesBefore.Count) { throw 'Loopback heartbeat stopped advancing while minimized.' }
                    $sampleBefore = $samplesBefore[$samplesBefore.Count - 1]
                    $sampleDuring = if ($samplesAfter.Count -gt 0) { $samplesAfter[$samplesAfter.Count - 1] } else { $null }
                    if (-not $sampleDuring) { throw 'No lifecycle telemetry arrived while minimized.' }
                    $tickBefore = $sampleBefore.ticks
                    $tickDuring = $sampleDuring.ticks
                    if ($tickDuring -le $tickBefore) { throw 'JavaScript timer did not advance while minimized.' }
                    $mediaState = $sampleDuring.media
                    $mediaStatus = if ($mediaState -eq 'playing') { 'PASS' } else { 'BLOCKED_UNSUPPORTED' }
                    if ($mediaState -eq 'playing' -and $sampleDuring.time -le $sampleBefore.time) { throw 'Local media playback reported playing but its timeline did not advance while minimized.' }
                    $results.Add([pscustomobject]@{ test='minimized_page_continuity'; result='PASS'; detail="JavaScript ticks advanced $tickBefore->$tickDuring across $($samplesAfter.Count - $samplesBefore.Count) loopback heartbeats while minimized." })
                    $results.Add([pscustomobject]@{ test='minimized_local_media'; result=$mediaStatus; detail="Muted local playback state=$mediaState; unsupported decoders/autoplay are reported separately." })
                    $eventsBeforeRestore = @(Get-LoggedEvents $eventLog).Count
                    [void][BowserSmoke.Native]::ShowWindow($life.Handle, 9)
                    Start-Sleep -Milliseconds 400
                    $restoredRect = New-Object BowserSmoke.Native+RECT
                    if (-not [BowserSmoke.Native]::GetWindowRect($life.Handle, [ref]$restoredRect) -or $restoredRect.Right -le $restoredRect.Left -or $restoredRect.Bottom -le $restoredRect.Top) { throw 'Restored window did not have nonzero bounds.' }
                    $restored = Wait-EventAfterCount $eventLog 'window_restored' $eventsBeforeRestore 5
                    if (-not $restored) { throw 'Restore transition event was not observed.' }
                    $memoryResultDeadline = [DateTime]::UtcNow.AddSeconds(3)
                    $restoreMemoryResult = $null
                    while ([DateTime]::UtcNow -lt $memoryResultDeadline -and -not $restoreMemoryResult) {
                        $restoreEvents = @(Get-LoggedEvents $eventLog | Select-Object -Skip $eventsBeforeRestore)
                        $restoreMemoryResult = $restoreEvents | Where-Object event -in @('memory_target_applied_normal','memory_target_unsupported','memory_target_failed_normal') | Select-Object -First 1
                        Start-Sleep -Milliseconds 100
                    }
                    $normalApplied = $restoreMemoryResult -and $restoreMemoryResult.event -eq 'memory_target_applied_normal'
                    $normalUnsupported = $restoreMemoryResult -and $restoreMemoryResult.event -eq 'memory_target_unsupported'
                    $normalFailed = $restoreMemoryResult -and $restoreMemoryResult.event -eq 'memory_target_failed_normal'
                    if (-not ($normalApplied -or $normalUnsupported -or $normalFailed)) { throw 'Restore did not report Normal target accepted, unsupported, or failed.' }
                    if ($normalUnsupported) { $results.Add([pscustomobject]@{ test='restored_memory_target'; result='BLOCKED_UNSUPPORTED'; detail='Memory target API unavailable; restore and navigation remained usable.' }) }
                    elseif ($normalFailed) {
                        if (@($restoreEvents | Where-Object event -eq 'memory_target_applied_normal').Count -gt 0) { throw 'Normal target was reported accepted after its setter failed.' }
                        $results.Add([pscustomobject]@{ test='restored_memory_target'; result='BLOCKED_SETTER_ERROR'; detail='Normal setter failed; the smoke checks that it was not claimed effective and navigation remained available.' })
                    } else { $results.Add([pscustomobject]@{ test='restored_memory_target'; result='PASS'; detail='Normal memory target accepted after restore.' }) }
                    $targetEventsBeforeDuplicate = @((Get-LoggedEvents $eventLog | Where-Object event -in @('memory_target_applied_low','memory_target_applied_normal','memory_target_unsupported','memory_target_failed_low','memory_target_failed_normal'))).Count
                    $sizePayload = [IntPtr]((600 -shl 16) -bor 800)
                    [void][BowserSmoke.Native]::PostMessage($life.Handle, 0x0005, [IntPtr]::Zero, $sizePayload)
                    Start-Sleep -Milliseconds 400
                    $targetEventsAfterDuplicate = @((Get-LoggedEvents $eventLog | Where-Object event -in @('memory_target_applied_low','memory_target_applied_normal','memory_target_unsupported','memory_target_failed_low','memory_target_failed_normal'))).Count
                    if ($targetEventsAfterDuplicate -ne $targetEventsBeforeDuplicate) { throw 'Duplicate ordinary WM_SIZE retried the memory target setter.' }
                    $results.Add([pscustomobject]@{ test='duplicate_resize_no_target_retry'; result='PASS'; detail='An ordinary WM_SIZE after restore did not retry the target setter.' })
                    Focus-Address $rootPid $life.Handle
                    $address = Get-EditText (Find-Edit $life.Handle)
                    if ($address -notmatch '/minimized-lifecycle\.html$') { throw "Address field lost the active page after restore: $address" }
                    $responsiveness = [Diagnostics.Stopwatch]::StartNew()
                    Navigate $rootPid $life.Handle ($base + '/two')
                    if (-not (Wait-Title $rootPid 'BOWSER_SMOKE_TWO' $TimeoutSeconds)) { throw 'Navigation after restore did not remain usable.' }
                    $responsiveness.Stop()
                    if ($responsiveness.Elapsed.TotalSeconds -gt 5) { throw "Local navigation after restore took $([Math]::Round($responsiveness.Elapsed.TotalSeconds,2)) seconds, exceeding the 5 second responsiveness bound." }
                    $results.Add([pscustomobject]@{ test='restored_navigation'; result='PASS'; detail="Address field retained the original page; navigation remained responsive in $([Math]::Round($responsiveness.Elapsed.TotalMilliseconds)) ms." })
                }

                $closeWindow = Find-Window $rootPid
                if ($closeWindow) { [void][BowserSmoke.Native]::PostMessage($closeWindow.Handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
                $closed = Wait-GroupExit $rootPid $profile 30
                if (-not $closed) { $allGroupsClosed = $false; throw "$policy/${label}: process group did not exit; retaining owned profile." }
                $activeProcessId = 0
                $activeProfile = $null
                $results.Add([pscustomobject]@{ test="${policy}_${label}_process_group_shutdown"; result='PASS'; detail='Bowser and observed profile descendants exited before profile reuse or cleanup.' })
                if ($pass -eq 1) {
                    $marker = Join-Path $profile '.bowser-benchmark-profile'
                    if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) { throw "$policy profile owner marker was not created." }
                    $markerText = [IO.File]::ReadAllText($marker)
                    if ($markerText -cne "Bowser benchmark WebView2 profile v1`n") { throw "$policy profile owner marker contents did not match the ownership contract." }
                }
            }
            $profileItem = Get-Item -LiteralPath $profile -Force
            $resolvedRoot = [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $runRoot).Path).TrimEnd('\') + '\'
            $resolvedProfile = [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $profile).Path)
            if ($allGroupsClosed -and $resolvedProfile.StartsWith($resolvedRoot, [StringComparison]::OrdinalIgnoreCase) -and -not ($profileItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                Remove-Item -LiteralPath $resolvedProfile -Recurse -Force
                $results.Add([pscustomobject]@{ test="${policy}_owned_profile_cleanup"; result='PASS'; detail='Verified owned non-reparse profile removed after both process groups exited.' })
            } else { $results.Add([pscustomobject]@{ test="${policy}_owned_profile_cleanup"; result='BLOCKED'; detail="Profile retained because shutdown/path ownership could not be fully confirmed: $resolvedProfile" }) }
        }

        # Parser misuse checks use an owned temp path, except the production alias case whose guard
        # must reject before creating or writing that profile.
        $invalidProfile = Join-Path $runRoot 'unowned-nonempty'
        New-Item -ItemType Directory -Path $invalidProfile | Out-Null
        Set-Content -LiteralPath (Join-Path $invalidProfile 'sentinel.txt') -Value 'leave intact' -NoNewline
        $productionProfile = Join-Path $env:LOCALAPPDATA 'Bowser\WebView2'
        $productionMarker = Join-Path $productionProfile '.bowser-benchmark-profile'
        $productionMarkerBefore = if (Test-Path -LiteralPath $productionMarker -PathType Leaf) { Get-FileHash -LiteralPath $productionMarker -Algorithm SHA256 } else { $null }
        $productionProfileCaseAlias = $productionProfile.ToUpperInvariant()
        $misuseCases = @(
            @{ name='policy_without_log'; expected='Benchmark profile and URL require --benchmark-log'; args='--benchmark-tracking-prevention strict' },
            @{ name='policy_without_profile'; expected='requires --benchmark-user-data-dir'; args='--benchmark-log "' + (Join-Path $runRoot 'misuse.csv') + '" --benchmark-tracking-prevention strict' },
            @{ name='unowned_nonempty_profile'; expected='Benchmark profile must be empty or owned by Bowser'; args='--benchmark-log "' + (Join-Path $runRoot 'misuse.csv') + '" --benchmark-user-data-dir "' + $invalidProfile + '" --benchmark-tracking-prevention strict' },
            @{ name='production_profile_alias'; expected='cannot use Bowser''s production WebView2 profile'; args='--benchmark-log "' + (Join-Path $runRoot 'misuse.csv') + '" --benchmark-user-data-dir "' + $productionProfile + '" --benchmark-tracking-prevention strict' },
            @{ name='production_profile_case_alias'; expected='cannot use Bowser''s production WebView2 profile'; args='--benchmark-log "' + (Join-Path $runRoot 'misuse.csv') + '" --benchmark-user-data-dir "' + $productionProfileCaseAlias + '" --benchmark-tracking-prevention strict' }
        )
        foreach ($case in $misuseCases) {
            $bad = New-Object Diagnostics.ProcessStartInfo
            $bad.FileName = $bowserPath; $bad.WorkingDirectory = $repoRoot; $bad.UseShellExecute = $false; $bad.Arguments = $case.args
            $badProcess = [Diagnostics.Process]::Start($bad)
            if (-not $badProcess.WaitForExit(3000)) {
                $dialogs = @(Find-Dialogs $badProcess.Id)
                $dialogText = (@($dialogs | ForEach-Object { @($_.Text) -join ' | ' }) -join ' || ')
                if ($dialogText -notlike "*$($case.expected)*") {
                    foreach ($dialog in $dialogs) { [void][BowserSmoke.Native]::PostMessage($dialog.Handle, 0x0111, [IntPtr]::new(1), [IntPtr]::Zero) }
                    $badWindow = Find-Window $badProcess.Id
                    if ($badWindow) { [void][BowserSmoke.Native]::PostMessage($badWindow.Handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
                    [void]$badProcess.WaitForExit(5000)
                    throw "$($case.name): expected parser rejection dialog was not observed; text='$dialogText'."
                }
                foreach ($dialog in $dialogs) { [void][BowserSmoke.Native]::PostMessage($dialog.Handle, 0x0111, [IntPtr]::new(1), [IntPtr]::Zero) }
                if (-not $badProcess.WaitForExit(5000)) { throw "$($case.name): parser rejection dialog was acknowledged but process remained alive." }
            }
            if ($badProcess.ExitCode -eq 0) { throw "$($case.name): invalid policy arguments were accepted." }
            $results.Add([pscustomobject]@{ test=$case.name; result='PASS'; detail="Rejected with exit code $($badProcess.ExitCode) before navigation. Parser dialog: $($case.expected)." })
        }
        if ([IO.File]::ReadAllText((Join-Path $invalidProfile 'sentinel.txt')) -cne 'leave intact') { throw 'Unowned profile sentinel changed during CLI rejection.' }
        $productionMarkerAfter = if (Test-Path -LiteralPath $productionMarker -PathType Leaf) { Get-FileHash -LiteralPath $productionMarker -Algorithm SHA256 } else { $null }
        if (($null -eq $productionMarkerBefore) -ne ($null -eq $productionMarkerAfter)) { throw 'Production benchmark owner marker presence changed during parser rejection.' }
        if ($productionMarkerBefore -and $productionMarkerBefore.Hash -cne $productionMarkerAfter.Hash) { throw 'Production benchmark owner marker changed during parser rejection.' }
        $results.Add([pscustomobject]@{ test='production_profile_nonmutation'; result='PASS'; detail='Exact and case-variant production profile launches were rejected; the existing benchmark owner marker was unchanged.' })
    } catch {
        $failure = $_.Exception.Message
        if ($activeProcessId -gt 0) {
            $activeWindow = Find-Window $activeProcessId
            if ($activeWindow) { [void][BowserSmoke.Native]::PostMessage($activeWindow.Handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
            if ($activeProfile -and -not (Wait-GroupExit $activeProcessId $activeProfile 30)) { $allGroupsClosed = $false }
        }
        $results.Add([pscustomobject]@{ test='performance_policy_smoke'; result='FAIL'; detail=$failure })
    } finally {
        if ($server) { $server.Dispose() }
    }

    $hashAfter = (Get-FileHash -LiteralPath $bowserPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hashAfter -ne $hashBefore) { $results.Add([pscustomobject]@{ test='pinned_executable_hash'; result='FAIL'; detail='Executable hash changed during smoke.' }) }
    else { $results.Add([pscustomobject]@{ test='pinned_executable_hash'; result='PASS'; detail="SHA-256 stayed $hashAfter" }) }
    $requestRows = if (Test-Path -LiteralPath $requestLog) { @([IO.File]::ReadAllLines($requestLog)) } else { @() }
    $failed = @($results | Where-Object result -eq 'FAIL')
    $report = [ordered]@{ captured_utc=[DateTime]::UtcNow.ToString('o'); outcome=if ($failed.Count) { 'FAIL' } else { 'PASS' }; mode='performance_policy_correctness_smoke'; executable=$bowserPath; executable_sha256_before=$hashBefore; executable_sha256_after=$hashAfter; process_group_shutdown_confirmed=$allGroupsClosed; artifact_directory=$runRoot; event_logs=@(Get-ChildItem -LiteralPath $runRoot -Filter '*.csv' -File | ForEach-Object FullName); local_http_requests=$requestRows; tests=@($results.ToArray()) }
    New-Item -ItemType Directory -Force -Path $resultDirectory | Out-Null
    $jsonPath = Join-Path $resultDirectory 'smoke-performance-policies.json'
    $mdPath = Join-Path $resultDirectory 'smoke-performance-policies.md'
    $report | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $jsonPath -Encoding UTF8
    $markdown = New-Object 'System.Collections.Generic.List[string]'
    $markdown.Add('# Bowser performance policy smoke results'); $markdown.Add('')
    $markdown.Add(('Captured UTC: {0}; result: **{1}**; executable SHA-256: `{2}`.' -f $report.captured_utc,$report.outcome,$hashAfter)); $markdown.Add('')
    $markdown.Add(('Artifacts: `{0}`. Loopback requests: {1}.' -f $runRoot,$requestRows.Count)); $markdown.Add('')
    foreach ($item in $results) { $markdown.Add(('- **{0}: {1}** — {2}' -f $item.test,$item.result,($item.detail -replace '\s+',' '))) }
    $markdown | Set-Content -LiteralPath $mdPath -Encoding UTF8
    $results | Format-List test,result,detail
    Write-Host "Smoke artifacts: $runRoot"
    Write-Host "Smoke JSON: $jsonPath"
    Write-Host "Smoke summary: $mdPath"
    if ($failed.Count -gt 0) { exit 1 }
}

if ($IncludePerformancePolicies) { Invoke-PerformancePolicySmoke; exit 0 }

$temp = Join-Path ([IO.Path]::GetTempPath()) ('bowser-smoke-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null
$requestLog = Join-Path $temp 'requests.txt'
$eventLog = Join-Path $temp 'events.csv'
$smokeUserDataDir = Join-Path $temp 'webview-profile'
$protocolScheme = 'bowser-smoke-' + ([Guid]::NewGuid().ToString('N'))
$protocolKeyPath = 'HKCU:\Software\Classes\' + $protocolScheme
$externalMarker = Join-Path $temp 'external-handler-ran.txt'
$externalHandler = Join-Path $temp 'external-handler.ps1'
@'
Set-Content -LiteralPath '__MARKER__' -Value 'handler-launched' -Encoding ASCII
'@.Replace('__MARKER__', $externalMarker.Replace("'", "''")) | Set-Content -LiteralPath $externalHandler -Encoding UTF8
$protocolCommand = '"' + (Join-Path $PSHOME 'powershell.exe') + '" -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $externalHandler + '" "%1"'
$server = $null
$process = $null
$processGroupClosed = $false
$currentStep = 'startup_window'
$screenshotPath = Join-Path $temp 'failure-window.png'
$screenshotCaptured = $false
$externalProtocolEventObserved = $false
$externalHandlerMarkerCreated = $false
$searchSmoke = [ordered]@{ requested=[bool]$IncludeWebSearch; outcome=if ($IncludeWebSearch) { 'NOT_RUN' } else { 'NOT_REQUESTED' }; query=$null; provider_url=$null; provider_native_address=$null; provider_title=$null; visible_result_name=$null; result_url=$null; result_native_address=$null; result_title=$null; same_window=$null; detail=$null }
$results = New-Object 'System.Collections.Generic.List[object]'
$script:clickDiagnostics = New-Object 'System.Collections.Generic.List[object]'
try {
    New-Item -Path (Join-Path $protocolKeyPath 'shell\open\command') -Force | Out-Null
    Set-Item -Path (Join-Path $protocolKeyPath 'shell\open\command') -Value $protocolCommand
    New-ItemProperty -Path $protocolKeyPath -Name 'URL Protocol' -Value '' -PropertyType String -Force | Out-Null
    $server = [BowserSmoke.LocalHttpServer]::new($requestLog, $protocolScheme, (Join-Path $PSScriptRoot 'fixtures'))
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $bowserPath; $psi.WorkingDirectory = $repoRoot; $psi.UseShellExecute = $false
    $psi.Arguments = '--benchmark-log "' + $eventLog + '" --benchmark-user-data-dir "' + $smokeUserDataDir + '"'
    if ($IncludeWebSearch) {
        # Test the missing-config default without reading or changing the user's override.
        $psi.EnvironmentVariables['LOCALAPPDATA'] = Join-Path $temp 'app-data'
    }
    $process = [Diagnostics.Process]::Start($psi)
    $rootProcessId = $process.Id
    $window = $null; $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline -and -not $window) { $window = Find-Window $rootProcessId; Start-Sleep -Milliseconds 100 }
    if (-not $window) { throw 'Bowser did not show a top-level window.' }
    if ((Get-WindowClass (Find-Edit $window.Handle)) -ne 'Edit') { throw 'Native address EDIT control was not found.' }
    $results.Add([pscustomobject]@{ test='empty_launch_native_input'; result='PASS'; detail='Visible native window and EDIT control found.' })
    $webviewBefore = @(Get-Descendants $rootProcessId $smokeUserDataDir | Where-Object { $_.Name -match '^msedgewebview2\.exe$' })
    if ($webviewBefore.Count -gt 0) { throw 'WebView2 process started before first submitted navigation.' }
    $results.Add([pscustomobject]@{ test='lazy_engine_start'; result='PASS'; detail='No msedgewebview2.exe descendant before navigation.' })

    $base = 'http://127.0.0.1:' + $server.Port
    $currentStep = 'address_submission_navigation'
    Navigate $rootProcessId $window.Handle ($base + '/one')
    if (-not (Wait-Title $rootProcessId 'BOWSER_SMOKE_ONE' $TimeoutSeconds)) { throw 'First local page did not reach its title marker.' }
    $results.Add([pscustomobject]@{ test='address_submission_navigation'; result='PASS'; detail=('Address field verified before Enter using {0}; loopback HTTP navigation reached BOWSER_SMOKE_ONE.' -f $lastAddressEntryMethod) })

    $currentStep = 'result_link_and_history'
    $localResultTarget = Click-UiaTarget $window.Handle '^Open local search result / second page$'
    if (-not (Wait-Title $rootProcessId 'BOWSER_SMOKE_TWO' $TimeoutSeconds)) { throw 'Second local page did not reach its title marker.' }
    $resultWindow = Find-Window $rootProcessId 'BOWSER_SMOKE_TWO'
    if (-not $resultWindow -or $resultWindow.Handle -ne $window.Handle) { throw 'The local result link opened outside the original Bowser HWND.' }
    Send-Key 0x12; Send-Key 0x25; Send-Key 0x25 $true; Send-Key 0x12 $true
    if (-not (Wait-Title $rootProcessId 'BOWSER_SMOKE_ONE' $TimeoutSeconds)) { throw 'Alt+Left did not return to the first page.' }
    Send-Key 0x12; Send-Key 0x27; Send-Key 0x27 $true; Send-Key 0x12 $true
    if (-not (Wait-Title $rootProcessId 'BOWSER_SMOKE_TWO' $TimeoutSeconds)) { throw 'Alt+Right did not return to the second page.' }
    $results.Add([pscustomobject]@{ test='result_link_and_history'; result='PASS'; detail='Clicked the local result link; Alt+Left and Alt+Right title markers observed.' })

    $currentStep = 'user_popup_new_tab'
    Send-Key 0x12; Send-Key 0x25; Send-Key 0x25 $true; Send-Key 0x12 $true
    if (-not (Wait-Title $rootProcessId 'BOWSER_SMOKE_ONE' $TimeoutSeconds)) { throw 'Could not return to the local test control page.' }
    $window = Find-Window $rootProcessId
    [void](Click-UiaTarget $window.Handle '^Open user initiated popup$')
    if (-not (Wait-EventName $eventLog 'popup_new_tab')) { throw 'User-initiated target=_blank popup was not recorded as popup_new_tab.' }
    if (-not (Wait-Title $rootProcessId 'BOWSER_SMOKE_POPUP_USER' $TimeoutSeconds)) { throw 'User-initiated popup destination did not load in the new active tab.' }
    if (-not (Test-Path -LiteralPath $requestLog -PathType Leaf) -or -not (Select-String -Path $requestLog -Pattern '^/popup-user$' -Quiet)) { throw 'The popup destination request was not observed by the loopback server.' }
    $popupWindow = Find-Window $rootProcessId 'BOWSER_SMOKE_POPUP_USER'
    if (-not $popupWindow -or $popupWindow.Handle -ne $window.Handle) { throw 'The user-initiated popup did not open as a tab in the original Bowser HWND.' }
    # Ctrl+1 selects the first tab. Its title must still be the control page, so the popup did not replace it.
    Send-Key 0x11; Send-Key 0x31; Send-Key 0x31 $true; Send-Key 0x11 $true
    if (-not (Wait-Title $rootProcessId 'BOWSER_SMOKE_ONE' $TimeoutSeconds)) { throw 'Ctrl+1 did not return to the original control page tab after the popup opened.' }
    $results.Add([pscustomobject]@{ test='user_popup_new_tab'; result='PASS'; detail='User-initiated target=_blank produced popup_new_tab in the original HWND; /popup-user reached the loopback server and BOWSER_SMOKE_POPUP_USER became the active tab title; Ctrl+1 restored the original tab with BOWSER_SMOKE_ONE.' })

    $currentStep = 'non_user_popup_blocked'
    # Ctrl+1 left the control page active with its own history, so no Alt+Left is needed here.
    if (-not (Wait-Title $rootProcessId 'BOWSER_SMOKE_ONE' $TimeoutSeconds)) { throw 'The local test control page was not active before the non-user popup test.' }
    $window = Find-Window $rootProcessId
    [void](Click-UiaTarget $window.Handle '^Open delayed non-user popup$')
    if (-not (Wait-EventName $eventLog 'popup_blocked' 12)) { throw 'Delayed non-user popup was not recorded as blocked.' }
    if (-not (Wait-Title $rootProcessId 'BOWSER_SMOKE_ONE' 2)) { throw 'The blocked non-user popup changed the main document.' }
    $results.Add([pscustomobject]@{ test='non_user_popup_blocked'; result='PASS'; detail='Delayed popup produced popup_blocked and left the current document usable.' })

    $currentStep = 'permission_denied'
    [void](Click-UiaTarget $window.Handle '^Request geolocation permission$')
    if (-not (Wait-EventName $eventLog 'permission_denied')) { throw 'Geolocation request was not recorded as denied.' }
    $results.Add([pscustomobject]@{ test='permission_denied'; result='PASS'; detail='Local-page geolocation request produced permission_denied.' })

    $currentStep = 'download_cancelled'
    [void](Click-UiaTarget $window.Handle '^Start test download$')
    if (-not (Wait-EventName $eventLog 'download_cancelled')) { throw 'Download was not recorded as cancelled.' }
    $results.Add([pscustomobject]@{ test='download_cancelled'; result='PASS'; detail='Local attachment request produced download_cancelled.' })

    $currentStep = 'external_protocol_blocked'
    [void](Click-UiaTarget $window.Handle '^Open blocked external protocol$')
    $externalProtocolEventObserved = Wait-EventName $eventLog 'external_protocol_blocked'
    if (-not $externalProtocolEventObserved) { throw "Custom external protocol '$protocolScheme' did not produce external_protocol_blocked." }
    Start-Sleep -Milliseconds 1000
    $externalHandlerMarkerCreated = Test-Path -LiteralPath $externalMarker
    if ($externalHandlerMarkerCreated) { throw "External protocol handler for '$protocolScheme' ran despite the blocked event." }
    $results.Add([pscustomobject]@{ test='external_protocol_blocked'; result='PASS'; detail="scheme=$protocolScheme; external_protocol_blocked observed; handler marker absent after 1 second." })

    if ($IncludeWebSearch) {
        $currentStep = 'real_web_search_result'
        $window = Find-Window $rootProcessId
        $query = 'Rust WebView2'
        # Match a visible Rust/WebView result, not the query, suggestions, or sign-in links on Bing.
        $resultNamePattern = '(?i)^(?:webview2\s*-\s*Rust(?:\s*-\s*Docs\.rs)?|Rust WebView Libraries)$'
        Navigate $rootProcessId $window.Handle $query
        $searchSmoke.query = $query
        $searchDeadline = [DateTime]::UtcNow.AddSeconds([Math]::Max(45, $TimeoutSeconds))
        $links = @()
        $providerUrl = ''
        $providerTitle = ''
        while ([DateTime]::UtcNow -lt $searchDeadline) {
            $providerUrl = Get-CurrentPageUrl $window.Handle
            $providerTitle = Get-WindowText $window.Handle
            $links = @(Find-UiaTarget $window.Handle $resultNamePattern | Where-Object { $_.ControlType -eq 'ControlType.Hyperlink' })
            if ($providerUrl -match '^https://www\.bing\.com/search\?' -and $providerUrl -match '[?&]q=Rust(?:%20|\+)WebView2(?:&|$)' -and $providerTitle -match '(?i)Rust WebView2 - Search' -and $links.Count -gt 0) { break }
            Start-Sleep -Milliseconds 250
        }
        $searchSmoke.provider_url = $providerUrl
        $searchSmoke.provider_native_address = Get-AddressValue $window.Handle
        $searchSmoke.provider_title = $providerTitle
        if ($providerUrl -notmatch '^https://www\.bing\.com/search\?' -or $providerUrl -notmatch '[?&]q=Rust(?:%20|\+)WebView2(?:&|$)' -or $providerTitle -notmatch '(?i)Rust WebView2 - Search') {
            throw "Default Bing search with the submitted query and provider title was not observable in WebView UIA location pane. Current page URL='$providerUrl'; window title='$providerTitle'; native address='$((Get-AddressValue $window.Handle))'. Network/provider response may be unavailable."
        }
        if ($searchSmoke.provider_native_address -ne $providerUrl) { throw "Native address EDIT does not contain the Bing search page source URL; page='$providerUrl', edit='$($searchSmoke.provider_native_address)'." }
        if ($links.Count -eq 0) { throw ("Bing page title/URL appeared but no visible UIA result hyperlink matching '$resultNamePattern' appeared before timeout; names=" + ($script:lastUiaVisibleNames -join ' | ')) }
        $selectedLink = $links[0]
        $searchSmoke.visible_result_name = $selectedLink.Name
        [void](Click-UiaElement $window.Handle $selectedLink)
        $resultDeadline = [DateTime]::UtcNow.AddSeconds([Math]::Max(30, $TimeoutSeconds))
        $resultUrl = ''
        $resultAddress = ''
        $resultTitle = ''
        $resultUri = $null
        $resultAddressUri = $null
        $destinationTitlePattern = if ($selectedLink.Name -match '(?i)docs\.rs') { '(?i)rust|webview2|docs\.rs' } else { '(?i)rust|webview2' }
        while ([DateTime]::UtcNow -lt $resultDeadline) {
            $resultUrl = Get-CurrentPageUrl $window.Handle
            $resultAddress = Get-AddressValue $window.Handle
            $resultTitle = Get-WindowText $window.Handle
            $resultUri = $null
            $resultAddressUri = $null
            $pageUrlIsFinal = [Uri]::TryCreate($resultUrl, [UriKind]::Absolute, [ref]$resultUri) -and $resultUri.Scheme -match '^https?$' -and $resultUri.Host -notmatch '(^|\.)bing\.com$'
            $addressIsFinal = [Uri]::TryCreate($resultAddress, [UriKind]::Absolute, [ref]$resultAddressUri) -and $resultAddressUri.Scheme -match '^https?$' -and $resultAddressUri.Host -notmatch '(^|\.)bing\.com$'
            $addressMatchesPage = $pageUrlIsFinal -and $addressIsFinal -and $resultAddressUri.AbsoluteUri -eq $resultUri.AbsoluteUri
            $titleIsDestination = $resultTitle -and $resultTitle -notmatch '(?i)bing\.com|\bBing\b|^https?://' -and $resultTitle -match $destinationTitlePattern
            if ($addressMatchesPage -and $titleIsDestination) { break }
            Start-Sleep -Milliseconds 100
        }
        $searchSmoke.result_url = $resultUrl
        $searchSmoke.result_native_address = $resultAddress
        $searchSmoke.result_title = $resultTitle
        $sameWindow = $false
        $currentWindow = Find-Window $rootProcessId
        if ($currentWindow) { $sameWindow = ($currentWindow.Handle -eq $window.Handle) }
        $searchSmoke.same_window = [bool]$sameWindow
        $resultUri = $null
        $resultAddressUri = $null
        $pageUrlIsFinal = [Uri]::TryCreate($resultUrl, [UriKind]::Absolute, [ref]$resultUri) -and $resultUri.Scheme -match '^https?$' -and $resultUri.Host -notmatch '(^|\.)bing\.com$'
        $addressIsFinal = [Uri]::TryCreate($resultAddress, [UriKind]::Absolute, [ref]$resultAddressUri) -and $resultAddressUri.Scheme -match '^https?$' -and $resultAddressUri.Host -notmatch '(^|\.)bing\.com$'
        if (-not $pageUrlIsFinal) { throw "Invoked visible result did not reach a non-Bing HTTP(S) page; observed URL='$resultUrl', page title='$resultTitle'." }
        if (-not $addressIsFinal -or $resultAddressUri.AbsoluteUri -ne $resultUri.AbsoluteUri) { throw "Native address EDIT does not contain the final page source URL; page='$resultUrl', edit='$resultAddress'." }
        if (-not $resultTitle -or $resultTitle -match '(?i)bing\.com|\bBing\b|^https?://' -or $resultTitle -notmatch $destinationTitlePattern) { throw "Search result page did not expose a usable destination title matching '$destinationTitlePattern'; observed='$resultTitle'." }
        if (-not $sameWindow) { throw 'Search result did not remain in the original Bowser top-level HWND.' }
        $searchSmoke.outcome = 'PASS'
        $searchSmoke.detail = 'Visible UIA result hyperlink clicked at its physical BoundingRectangle center; WebView source and native address EDIT agree on the final non-provider HTTP(S) URL, the document title identifies the destination page, and the original Bowser HWND remains visible.'
        $results.Add([pscustomobject]@{ test='real_web_search_result'; result='PASS'; detail=('query={0}; provider={1}; title={2}; result="{3}" -> {4}; native address={5}; destination title={6}; same HWND' -f $query, $providerUrl, $providerTitle, $selectedLink.Name, $resultUrl, $resultAddress, $resultTitle) })
    }

    $currentStep = 'close_during_loading'
    $window = Find-Window $rootProcessId
    Navigate $rootProcessId $window.Handle ($base + '/slow')
    $slowEnd = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $slowEnd -and ((-not (Test-Path -LiteralPath $requestLog -PathType Leaf)) -or (-not (Select-String -Path $requestLog -Pattern '^/slow$' -Quiet)))) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path -LiteralPath $requestLog -PathType Leaf) -or -not (Select-String -Path $requestLog -Pattern '^/slow$' -Quiet)) { throw 'The local slow navigation request was not observed.' }
    [void][BowserSmoke.Native]::PostMessage($window.Handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    $processGroupClosed = Wait-GroupExit $rootProcessId $smokeUserDataDir 30
    if (-not $processGroupClosed) { throw 'Bowser/WebView2 process group did not exit after WM_CLOSE during active navigation.' }
    $results.Add([pscustomobject]@{ test='close_during_loading'; result='PASS'; detail='The browser and all observed descendants exited after WM_CLOSE while /slow was pending.' })
} catch {
    $failureDetail = $_.Exception.Message
    if ($currentStep -eq 'real_web_search_result') { $searchSmoke.outcome = 'FAIL'; $searchSmoke.detail = $_.Exception.Message }
    if ($_.ScriptStackTrace) { $failureDetail += ' | ' + $_.ScriptStackTrace }
    try {
        if (Test-Path -LiteralPath $eventLog -PathType Leaf) {
            $observedEvents = @((Import-Csv -LiteralPath $eventLog -ErrorAction SilentlyContinue | ForEach-Object { '{0}@{1}ms' -f $_.event,$_.elapsed_ms }))
            if ($observedEvents.Count -gt 0) { $failureDetail += ' | Events: ' + ($observedEvents -join ', ') }
        }
        if (Test-Path -LiteralPath $requestLog -PathType Leaf) {
            $observedRequests = @([IO.File]::ReadAllLines($requestLog))
            if ($observedRequests.Count -gt 0) { $failureDetail += ' | HTTP requests: ' + ($observedRequests -join ', ') }
        }
    } catch { $failureDetail += ' | Smoke diagnostic collection failed: ' + $_.Exception.Message }
    $dialogDetails = New-Object 'System.Collections.Generic.List[string]'
    if ($process) {
        try {
            $liveWindow = Find-Window $process.Id
            if ($liveWindow) { $screenshotCaptured = Save-WindowScreenshot $liveWindow.Handle $screenshotPath }
        } catch { $failureDetail += ' | Failure screenshot unavailable: ' + $_.Exception.Message }
        if ($script:lastUiaVisibleDetails -and $currentStep -ne 'real_web_search_result') {
            $failureDetail += ' | UIA bounds: ' + (ConvertTo-Json -InputObject @($script:lastUiaVisibleDetails) -Depth 3 -Compress)
        }
        if ($script:uiaChildHandles -and $script:uiaChildHandles.Count -gt 0) {
            $failureDetail += ' | UIA child HWNDs: ' + (ConvertTo-Json -InputObject @($script:uiaChildHandles.ToArray() | ForEach-Object { [pscustomobject]@{ hwnd=$_.Handle.ToInt64(); class=$_.Class } }) -Depth 3 -Compress)
        }
        try {
            foreach ($dialog in (Find-Dialogs $process.Id)) {
                $dialogText = (@($dialog.Text) -join ' | ')
                $dialogDetails.Add(($dialog.Title + ': ' + $dialogText))
                [void][BowserSmoke.Native]::PostMessage($dialog.Handle, 0x0111, [IntPtr]::new(1), [IntPtr]::Zero)
            }
        } catch { $dialogDetails.Add(('Dialog inspection failed: ' + $_.Exception.Message + ' | ' + $_.ScriptStackTrace)) }
        if ($dialogDetails.Count -gt 0) { $failureDetail += ' | Native error dialog: ' + ($dialogDetails -join ' || ') }
        $results.Add([pscustomobject]@{ test=$currentStep; result='FAIL'; detail=$failureDetail })
        if (-not $process.HasExited) {
            $window = Find-Window $process.Id
            if ($window) { [void][BowserSmoke.Native]::PostMessage($window.Handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
        }
        $processGroupClosed = Wait-GroupExit $process.Id $smokeUserDataDir 60
        if (-not $processGroupClosed) {
            $results.Add([pscustomobject]@{ test='shutdown_after_failure'; result='FAIL'; detail='Bowser descendants remained after WM_CLOSE or process exit; none were killed.' })
        }
    }
    if (-not $process) { $results.Add([pscustomobject]@{ test=$currentStep; result='FAIL'; detail=$failureDetail }) }
} finally {
    Reset-InjectedKeyState
    if ($server) { $server.Dispose() }
    if (Test-Path -LiteralPath $protocolKeyPath) { Remove-Item -LiteralPath $protocolKeyPath -Recurse -Force }
}

if ($processGroupClosed -and (Test-Path -LiteralPath $smokeUserDataDir -PathType Container)) {
    $resolvedTemp = [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $temp).Path).TrimEnd('\') + '\'
    $resolvedSmokeProfile = [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $smokeUserDataDir).Path)
    $profileItem = Get-Item -LiteralPath $resolvedSmokeProfile -Force
    $insideTemp = $resolvedSmokeProfile.StartsWith($resolvedTemp, [StringComparison]::OrdinalIgnoreCase)
    if ($insideTemp -and (Split-Path -Leaf $resolvedSmokeProfile) -eq 'webview-profile' -and -not ($profileItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        Remove-Item -LiteralPath $resolvedSmokeProfile -Recurse -Force
        Write-Host 'Disposable WebView2 smoke profile removed after confirmed shutdown.'
    } else {
        Write-Host "Smoke profile cleanup skipped because the resolved path did not pass safety checks: $resolvedSmokeProfile"
    }
}

$failedResults = @($results | Where-Object result -eq 'FAIL')
$bowserHashAfter = (Get-FileHash -LiteralPath $bowserPath -Algorithm SHA256).Hash.ToLowerInvariant()
$hashUnchanged = $bowserHashBefore -eq $bowserHashAfter
if (-not $hashUnchanged) { $results.Add([pscustomobject]@{ test='frozen_binary_check'; result='FAIL'; detail='Bowser release executable SHA-256 changed during smoke collection.' }) }
else { $results.Add([pscustomobject]@{ test='frozen_binary_check'; result='PASS'; detail=('Bowser release SHA-256 unchanged: ' + $bowserHashAfter) }) }
$failedResults = @($results | Where-Object result -eq 'FAIL')
$requestLogLines = @()
if (Test-Path -LiteralPath $requestLog -PathType Leaf) { $requestLogLines = @([IO.File]::ReadAllLines($requestLog)) }
$outcome = if ($failedResults.Count -eq 0) { 'PASS' } else { 'FAIL' }
$profileRemoved = -not (Test-Path -LiteralPath $smokeUserDataDir -PathType Container)
$report = [ordered]@{
    captured_utc = [DateTime]::UtcNow.ToString('o')
    outcome = $outcome
    browser = 'Bowser'
    version = [Diagnostics.FileVersionInfo]::GetVersionInfo($bowserPath).ProductVersion
    bowser_sha256_before = $bowserHashBefore
    bowser_sha256_after = $bowserHashAfter
    bowser_sha256_unchanged = $hashUnchanged
    process_id = if ($process) { $process.Id } else { $null }
    process_group_shutdown_confirmed = [bool]$processGroupClosed
    temporary_profile_removed = [bool]$profileRemoved
    artifact_directory = $temp
    event_log_csv = $eventLog
    request_log = $requestLog
    event_log_exists = Test-Path -LiteralPath $eventLog
    request_log_exists = Test-Path -LiteralPath $requestLog
    local_http_requests = $requestLogLines
    external_protocol_scheme = $protocolScheme
    external_protocol_event_observed = [bool]$externalProtocolEventObserved
    external_handler_marker_created = [bool]$externalHandlerMarkerCreated
    real_web_search = $searchSmoke
    click_diagnostics = @($script:clickDiagnostics.ToArray())
    failure_screenshot = if ($screenshotCaptured) { $screenshotPath } else { $null }
    failure_uia_controls = if ($script:lastUiaVisibleDetails) { @($script:lastUiaVisibleDetails) } else { @() }
    tests = @($results.ToArray())
}
New-Item -ItemType Directory -Force -Path $resultDirectory | Out-Null
$jsonPath = Join-Path $resultDirectory 'smoke.json'
$markdownPath = Join-Path $resultDirectory 'smoke.md'
$report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $jsonPath -Encoding UTF8
$markdown = New-Object 'System.Collections.Generic.List[string]'
$markdown.Add('# Bowser UI and policy smoke results')
$markdown.Add('')
$markdown.Add(('Captured UTC: {0}; overall result: **{1}**.' -f $report.captured_utc, $report.outcome))
$markdown.Add('')
$requestLogReference = if ($report.request_log_exists) { $report.request_log } else { 'not created; the local HTTP server received no request' }
$markdown.Add(('Event log: `{0}`; local HTTP request log: `{1}`.' -f $report.event_log_csv, $requestLogReference))
$markdown.Add(('Temporary profile removed after shutdown: {0}; process-group shutdown confirmed: {1}.' -f $profileRemoved, $processGroupClosed))
$markdown.Add(('External URI proof: scheme `{0}`, callback event observed `{1}`, benign handler marker created `{2}`.' -f $protocolScheme, $externalProtocolEventObserved, $externalHandlerMarkerCreated))
$markdown.Add(('Loopback requests observed: {0}.' -f $(if ($requestLogLines.Count) { $requestLogLines -join ', ' } else { 'none' })))
$markdown.Add('')
$markdown.Add('## Checks')
$markdown.Add('')
foreach ($item in $results) { $markdown.Add(('- **{0}: {1}** — {2}' -f $item.test, $item.result, ($item.detail -replace '\s+', ' '))) }
$markdown.Add('')
$markdown.Add('The smoke uses a disposable WebView2 profile and a loopback-only HTTP server. It checks empty startup, native address input, lazy WebView2 startup, a local result link and history, a user-initiated popup opening a new tab, a non-user popup being blocked, geolocation denial, download cancellation, external protocol blocking, and close during a deliberately held local request. `-IncludeWebSearch` additionally uses temporary LOCALAPPDATA with no search-provider override, submits `Rust WebView2` to default Bing, locates and physically clicks a visible result via UI Automation, then requires the WebView source and native address EDIT to agree on the final non-Bing HTTP(S) URL and the Bowser window title to identify the destination page in the same HWND. A Bing redirect intermediary, stale address field, unusable title, or provider/network failure is recorded as a failure.')
$markdown | Set-Content -LiteralPath $markdownPath -Encoding UTF8
$results | Format-List test,result,detail
Write-Host "Smoke artifacts: $temp"
Write-Host "Smoke JSON: $jsonPath"
Write-Host "Smoke summary: $markdownPath"
if ($failedResults.Count -gt 0) { exit 1 }
