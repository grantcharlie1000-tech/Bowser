[CmdletBinding()]
param(
    [ValidateRange(1, 100)] [int] $Runs = 5,
    [ValidateRange(1, 3600)] [int] $IdleSeconds = 30,
    [string] $OutputDirectory = (Join-Path $PSScriptRoot 'results'),
    [ValidateSet('All','Bowser','Edge','Chrome')] [string] $Browser = 'All',
    [ValidateSet('blank-idle','blank-submit','light','demanding')] [string[]] $SelectedWorkloads,
    [string] $BowserExecutable = (Join-Path $PSScriptRoot '..\target\x86_64-pc-windows-msvc\release\bowser.exe'),
    # Bowser accepts --benchmark-url only for files under the fixture root it was compiled with
    # (CARGO_MANIFEST_DIR\benchmarks\fixtures). Point this at that build's fixtures for other worktrees.
    [string] $FixtureRoot = (Join-Path $PSScriptRoot 'fixtures'),
    [AllowNull()] [AllowEmptyString()] [string] $TrackingPrevention,
    [switch] $KeepProfiles
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Import-Module (Join-Path $PSScriptRoot 'measurements.psm1') -Force
$runSelection = Resolve-BowserRunSelection -OutputDirectory $OutputDirectory -SelectedWorkloads $SelectedWorkloads -TrackingPrevention $TrackingPrevention
$workloads = @($runSelection.Workloads)
$TrackingPrevention = $runSelection.TrackingPrevention
$OutputDirectory = $runSelection.OutputDirectory

# Reproducible user-mode benchmark. Windows' standby/file cache is deliberately
# left alone: cold-profile means a new browser profile, not a cold OS cache.
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$fixtureRoot = (Resolve-Path -LiteralPath $FixtureRoot).Path
$lightFixture = (Join-Path $fixtureRoot 'light.html')
$demandingFixture = (Join-Path $fixtureRoot 'demanding.html')
$bowserPath = Resolve-BowserExecutable $BowserExecutable
if ($TrackingPrevention -and $Browser -in @('Edge','Chrome')) { throw '-TrackingPrevention requires Bowser in the selected browser set.' }

function Find-BrowserExe([string] $Kind) {
    $candidates = if ($Kind -eq 'Edge') {
        @(
            (Join-Path ${env:ProgramFiles(x86)} 'Microsoft\Edge\Application\msedge.exe'),
            (Join-Path $env:ProgramFiles 'Microsoft\Edge\Application\msedge.exe'),
            (Join-Path $env:LOCALAPPDATA 'Microsoft\Edge\Application\msedge.exe')
        )
    } else {
        @(
            (Join-Path ${env:ProgramFiles(x86)} 'Google\Chrome\Application\chrome.exe'),
            (Join-Path $env:ProgramFiles 'Google\Chrome\Application\chrome.exe'),
            (Join-Path $env:LOCALAPPDATA 'Google\Chrome\Application\chrome.exe')
        )
    }
    $found = $candidates | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
    if (-not $found) { throw "$Kind executable was not found in its standard installation paths." }
    return (Resolve-Path -LiteralPath $found).Path
}

$edgePath = $null; $chromePath = $null
if (-not $TrackingPrevention -and $Browser -in @('All','Edge')) { $edgePath = Find-BrowserExe 'Edge' }
if (-not $TrackingPrevention -and $Browser -in @('All','Chrome')) { $chromePath = Find-BrowserExe 'Chrome' }

if (-not ('BowserBench.Native' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
namespace BowserBench {
  public static class Native {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION U; }
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int maxCount);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool SetWindowText(IntPtr hWnd, string text);
    [DllImport("user32.dll", EntryPoint="SendMessageW", SetLastError=true)] public static extern IntPtr SetEditSelection(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", EntryPoint="SendMessageW", SetLastError=true)] public static extern IntPtr SendCharacter(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder text, int maxCount);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll", EntryPoint="SendMessageTimeoutW", SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int command);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr SetFocus(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetFocus();
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", SetLastError=true)] public static extern bool AttachThreadInput(uint attach, uint attachTo, bool value);
    [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint count, INPUT[] inputs, int size);
    [DllImport("user32.dll", EntryPoint="VkKeyScanW", CharSet=CharSet.Unicode)] private static extern short VkKeyScan(char character);
    public static short MapCharacter(char character) { return VkKeyScan(character); }
    public static uint SendKeyInput(ushort key, uint flags) {
      INPUT input = new INPUT(); input.type = 1; input.U.ki.wVk = key; input.U.ki.dwFlags = flags;
      return SendInput(1, new INPUT[] { input }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static uint SendUnicodeInput(ushort character, uint flags) {
      INPUT input = new INPUT(); input.type = 1; input.U.ki.wScan = character; input.U.ki.dwFlags = flags;
      return SendInput(1, new INPUT[] { input }, Marshal.SizeOf(typeof(INPUT)));
    }
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
  }
}
'@
}

function Get-WindowText([IntPtr] $Handle) {
    $buffer = New-Object System.Text.StringBuilder 1024
    [void][BowserBench.Native]::GetWindowText($Handle, $buffer, $buffer.Capacity)
    return $buffer.ToString()
}

function Get-WindowClass([IntPtr] $Handle) {
    $buffer = New-Object System.Text.StringBuilder 256
    [void][BowserBench.Native]::GetClassName($Handle, $buffer, $buffer.Capacity)
    return $buffer.ToString()
}

function Get-ErrorDialogMessage([int] $ProcessId) {
    # Bowser reports startup/navigation rejections with a modal MessageBox (class #32770) owned by
    # the process. The main window never appears in that state, so the harness must read the text.
    $dialogs = @(Get-TopWindows $ProcessId | Where-Object { (Get-WindowClass $_.Handle) -eq '#32770' })
    if ($dialogs.Count -eq 0) { return $null }
    $texts = New-Object 'System.Collections.Generic.List[string]'
    $callback = [BowserBench.Native+EnumWindowsProc] {
        param([IntPtr] $hWnd, [IntPtr] $unused)
        $text = Get-WindowText $hWnd
        if ($text -and $text -ne 'OK') { $texts.Add($text) }
        return $true
    }
    [void][BowserBench.Native]::EnumChildWindows($dialogs[0].Handle, $callback, [IntPtr]::Zero)
    if ($texts.Count -gt 0) { return ($texts -join ' ') }
    return $dialogs[0].Title
}

function Assert-NoErrorDialog([int] $ProcessId) {
    $message = Get-ErrorDialogMessage $ProcessId
    if ($message) { throw "Bowser showed an error dialog instead of loading: $message" }
}

function Find-AddressEdit([IntPtr] $MainWindow) {
    $script:addressEdit = [IntPtr]::Zero
    $callback = [BowserBench.Native+EnumWindowsProc] {
        param([IntPtr] $hWnd, [IntPtr] $unused)
        if ((Get-WindowClass $hWnd) -eq 'Edit') { $script:addressEdit = $hWnd; return $false }
        return $true
    }
    [void][BowserBench.Native]::EnumChildWindows($MainWindow, $callback, [IntPtr]::Zero)
    return $script:addressEdit
}

function Get-FocusedValueElement {
    $element = [System.Windows.Automation.AutomationElement]::FocusedElement
    for ($depth = 0; $element -and $depth -lt 8; $depth++) {
        try {
            $pattern = $null
            if ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit -and $element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
                return [pscustomobject]@{ Element=$element; Pattern=$pattern }
            }
        } catch { }
        try { $element = [System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($element) } catch { $element = $null }
    }
    return $null
}

function Get-TopWindows([int] $ProcessId) {
    $found = New-Object 'System.Collections.Generic.List[object]'
    $callback = [BowserBench.Native+EnumWindowsProc] {
        param([IntPtr] $hWnd, [IntPtr] $unused)
        [uint32] $windowProcessId = 0
        [void][BowserBench.Native]::GetWindowThreadProcessId($hWnd, [ref] $windowProcessId)
        if ($windowProcessId -eq $ProcessId -and [BowserBench.Native]::IsWindowVisible($hWnd)) {
            $title = Get-WindowText $hWnd
            if ($title) {
                $rect = New-Object BowserBench.Native+RECT
                [void][BowserBench.Native]::GetWindowRect($hWnd, [ref] $rect)
                $area = [Math]::Max(0, ($rect.Right - $rect.Left)) * [Math]::Max(0, ($rect.Bottom - $rect.Top))
                $found.Add([pscustomobject]@{ Handle=$hWnd; Title=$title; Area=$area })
            }
        }
        return $true
    }
    [void][BowserBench.Native]::EnumWindows($callback, [IntPtr]::Zero)
    return @($found | Sort-Object Area -Descending)
}

function Find-MainWindow([int] $ProcessId, [string] $TitleMarker) {
    $windows = Get-TopWindows $ProcessId
    if ($TitleMarker) {
        $match = $windows | Where-Object { $_.Title -like "*$TitleMarker*" } | Select-Object -First 1
        if ($match) { return $match }
        return $null
    }
    return $windows | Select-Object -First 1
}

function Test-WindowResponsive([IntPtr] $Handle) {
    $result = [IntPtr]::Zero
    $sent = [BowserBench.Native]::SendMessageTimeout($Handle, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 250, [ref] $result)
    return ($sent -ne [IntPtr]::Zero)
}

function Wait-VisibleResponsive([int] $ProcessId, [long] $StartedTick, [int] $TimeoutSeconds = 90) {
    $limit = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $limit) {
        Assert-NoErrorDialog $ProcessId
        $window = Find-MainWindow $ProcessId ''
        if ($window -and (Test-WindowResponsive $window.Handle)) {
            return [pscustomobject]@{ Window=$window; ElapsedMs=(Get-ElapsedMs $StartedTick) }
        }
        Start-Sleep -Milliseconds 100
    }
    return $null
}

function Wait-TitleMarker([int] $ProcessId, [string] $Marker, [int] $TimeoutSeconds = 90) {
    $limit = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $limit) {
        Assert-NoErrorDialog $ProcessId
        $window = Find-MainWindow $ProcessId $Marker
        if ($window -and (Test-WindowResponsive $window.Handle)) { return [pscustomobject]@{ Window=$window; ElapsedMs=(Get-ElapsedMs $script:activeStartTick) } }
        Start-Sleep -Milliseconds 10
    }
    return $null
}

function Get-ElapsedMs([long] $StartedTick) {
    return [Math]::Round((([Diagnostics.Stopwatch]::GetTimestamp() - $StartedTick) * 1000.0) / [Diagnostics.Stopwatch]::Frequency, 3)
}

function Send-Key([uint16] $VirtualKey, [bool] $KeyUp = $false) {
    $flags = if ($KeyUp) { [uint32]2 } else { [uint32]0 }
    if ([BowserBench.Native]::SendKeyInput($VirtualKey, $flags) -ne 1) {
        $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw (New-Object ComponentModel.Win32Exception($errorCode, 'SendInput failed while entering the search phrase.'))
    }
}

function Send-Text([string] $Text) {
    foreach ($character in $Text.ToCharArray()) {
        $mapped = [BowserBench.Native]::MapCharacter([char]$character)
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
    [void][BowserBench.Native]::SetEditSelection($Edit, 0x00B1, [IntPtr]::Zero, [IntPtr]::new(-1))
    foreach ($character in $Text.ToCharArray()) {
        [void][BowserBench.Native]::SendCharacter($Edit, 0x0102, [IntPtr][int][char]$character, [IntPtr]::Zero)
    }
}

function Activate-MainWindow([IntPtr] $MainWindow) {
    [void][BowserBench.Native]::ShowWindow($MainWindow, 9)
    [uint32] $targetPid = 0
    $targetThread = [BowserBench.Native]::GetWindowThreadProcessId($MainWindow, [ref] $targetPid)
    $currentThread = [BowserBench.Native]::GetCurrentThreadId()
    $lastDetail = ''
    # Windows refuses SetForegroundWindow from a process that did not receive the last input event.
    # Shell surfaces such as the Start/Search flyout can keep that right. Attaching to the current
    # foreground thread and tapping Alt (which Windows does not treat as a menu command here) gives
    # this process the foreground right; the attempt is retried a few times and then reported.
    for ($attempt = 1; $attempt -le 5; $attempt++) {
        $foregroundBefore = [BowserBench.Native]::GetForegroundWindow()
        [uint32] $foregroundPid = 0
        $foregroundThread = if ($foregroundBefore -ne [IntPtr]::Zero) { [BowserBench.Native]::GetWindowThreadProcessId($foregroundBefore, [ref] $foregroundPid) } else { [uint32]0 }
        $attachedForeground = $false; $attachedTarget = $false
        try {
            if ($foregroundThread -ne 0 -and $foregroundThread -ne $currentThread) {
                $attachedForeground = [BowserBench.Native]::AttachThreadInput($currentThread, $foregroundThread, $true)
            }
            if ($targetThread -ne 0 -and $targetThread -ne $currentThread -and $targetThread -ne $foregroundThread) {
                $attachedTarget = [BowserBench.Native]::AttachThreadInput($currentThread, $targetThread, $true)
            }
            [void][BowserBench.Native]::BringWindowToTop($MainWindow)
            $activated = [BowserBench.Native]::SetForegroundWindow($MainWindow)
            Start-Sleep -Milliseconds 100
            $foreground = [BowserBench.Native]::GetForegroundWindow()
            if ($foreground -eq $MainWindow) { return }
            [uint32] $actualForegroundPid = 0
            if ($foreground -ne [IntPtr]::Zero) { [void][BowserBench.Native]::GetWindowThreadProcessId($foreground, [ref] $actualForegroundPid) }
            $foregroundTitle = Get-WindowText $foreground
            $lastDetail = "attempt $attempt; SetForegroundWindow=$activated; target PID=$targetPid; foreground HWND=$foreground PID=$actualForegroundPid title='$foregroundTitle'"
        } finally {
            if ($attachedTarget) { [void][BowserBench.Native]::AttachThreadInput($currentThread, $targetThread, $false) }
            if ($attachedForeground) { [void][BowserBench.Native]::AttachThreadInput($currentThread, $foregroundThread, $false) }
        }
        if ($attempt -lt 5) {
            Send-Key 0x12; Send-Key 0x12 $true
            Start-Sleep -Milliseconds 150
        }
    }
    throw "The benchmark browser window is not foreground after 5 activation attempts ($lastDetail)."
}

function Focus-AddressInput([IntPtr] $MainWindow, [string] $Browser) {
    Activate-MainWindow $MainWindow
    if ($Browser -ne 'Bowser') {
        Send-Key 0x11; Send-Key 0x4C; Send-Key 0x4C $true; Send-Key 0x11 $true
    } else {
        # Bowser focuses its one native EDIT control at launch. Find it explicitly
        # and attach input queues so SetFocus remains safe across process threads.
        $edit = [IntPtr]::Zero
        $callback = [BowserBench.Native+EnumWindowsProc] {
            param([IntPtr] $hWnd, [IntPtr] $unused)
            if ((Get-WindowClass $hWnd) -eq 'Edit') { $script:editCandidate = $hWnd; return $false }
            return $true
        }
        $script:editCandidate = [IntPtr]::Zero
        [void][BowserBench.Native]::EnumChildWindows($MainWindow, $callback, [IntPtr]::Zero)
        $edit = $script:editCandidate
        if ($edit -eq [IntPtr]::Zero) { throw 'Bowser native address EDIT control was not found.' }
        [uint32] $targetThread = 0; [uint32] $windowProcessId = 0
        $targetThread = [BowserBench.Native]::GetWindowThreadProcessId($edit, [ref] $windowProcessId)
        $currentThread = [BowserBench.Native]::GetCurrentThreadId()
        [void][BowserBench.Native]::AttachThreadInput($currentThread, $targetThread, $true)
        [void][BowserBench.Native]::SetFocus($edit)
        $focusedWindow = [BowserBench.Native]::GetFocus()
        [void][BowserBench.Native]::AttachThreadInput($currentThread, $targetThread, $false)
        if ($focusedWindow -ne $edit) { throw 'The Bowser address EDIT control did not become the focused input.' }
    }
}

function Get-ProcessGroup([int] $RootPid, [string] $UserDataProfile) {
    $inventory = Get-ProcessInventory
    return @(Get-ProcessGroupMembers -RootPid $RootPid -Profile $UserDataProfile -Inventory $inventory)
}

function Get-GroupSnapshot([int] $RootPid, [string] $UserDataProfile) {
    return Get-ProcessGroupSnapshot -RootPid $RootPid -Profile $UserDataProfile
}

function Measure-IdleGroup([int] $RootPid, [string] $UserDataProfile, [int] $Seconds) {
    $previous = Get-GroupSnapshot $RootPid $UserDataProfile
    $cpuDelta = 0.0
    $startTick = [Diagnostics.Stopwatch]::GetTimestamp()
    $endTime = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $endTime) {
        Start-Sleep -Milliseconds 1000
        $current = Get-GroupSnapshot $RootPid $UserDataProfile
        foreach ($processId in $current.CpuByPid.Keys) {
            if ($previous.CpuByPid.ContainsKey($processId)) {
                $cpuDelta += [Math]::Max(0.0, ([double]$current.CpuByPid[$processId] - [double]$previous.CpuByPid[$processId]))
            }
        }
        $previous = $current
    }
    $sampleSeconds = ([Diagnostics.Stopwatch]::GetTimestamp() - $startTick) / [double][Diagnostics.Stopwatch]::Frequency
    $cores = [Math]::Max(1, [Environment]::ProcessorCount)
    return [pscustomobject]@{
        CpuPercent = [Math]::Round(($cpuDelta / [Math]::Max(0.001, $sampleSeconds) / $cores) * 100.0, 4)
        Snapshot = $previous
        SampleSeconds = [Math]::Round($sampleSeconds, 3)
    }
}

function Wait-ProfileExit([int] $RootPid, [string] $UserDataProfile, [int] $TimeoutSeconds = 30) {
    $limit = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $members = @(Get-ProcessGroup $RootPid $UserDataProfile)
        if ($members.Count -eq 0) { return [pscustomobject]@{ Exited=$true; Remaining=@() } }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $limit)
    $remaining = @(Get-ProcessGroup $RootPid $UserDataProfile | ForEach-Object {
        [pscustomobject]@{ process_id=[int]$_.Pid; parent_process_id=[int]$_.Parent; name=[string]$_.Name; command_line=[string]$_.CommandLine }
    })
    return [pscustomobject]@{ Exited=($remaining.Count -eq 0); Remaining=$remaining }
}

function Close-Run([int] $RootPid, [string] $UserDataProfile) {
    $window = Find-MainWindow $RootPid ''
    $closePosted = $false
    if ($window) { $closePosted = [BowserBench.Native]::PostMessage($window.Handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
    $exit = Wait-ProfileExit $RootPid $UserDataProfile 30
    return [pscustomobject]@{ Exited=[bool]$exit.Exited; WindowFound=($null -ne $window); WmClosePosted=[bool]$closePosted; Remaining=@($exit.Remaining) }
}

function Get-Bytes($Bytes) {
    if ($null -eq $Bytes) { return $null }
    return [Math]::Round([double]$Bytes / 1MB, 3)
}

function Start-OneBrowser([string] $Browser, [string] $UserDataProfile, [string] $Workload, [string] $LogPath) {
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.UseShellExecute = $false
    $psi.WorkingDirectory = $repoRoot
    if ($Browser -eq 'Bowser') {
        $psi.FileName = $bowserPath
        $launchArguments = @('--benchmark-log', ('"{0}"' -f $LogPath), '--benchmark-user-data-dir', ('"{0}"' -f $UserDataProfile))
        if ($TrackingPrevention) { $launchArguments += @('--benchmark-tracking-prevention', $TrackingPrevention.ToLowerInvariant()) }
        if ($Workload -eq 'light' -or $Workload -eq 'demanding') {
            $fixture = if ($Workload -eq 'light') { $lightFixture } else { $demandingFixture }
            $launchArguments += @('--benchmark-url', ('"{0}"' -f ([Uri]$fixture).AbsoluteUri))
        }
        $psi.Arguments = $launchArguments -join ' '
    } else {
        $psi.FileName = if ($Browser -eq 'Edge') { $edgePath } else { $chromePath }
        $launchArguments = @('--user-data-dir="' + $UserDataProfile + '"', '--no-first-run', '--no-default-browser-check', '--disable-extensions')
        if ($Workload -eq 'light' -or $Workload -eq 'demanding') {
            $fixture = if ($Workload -eq 'light') { $lightFixture } else { $demandingFixture }
            $launchArguments += ('"' + ([Uri]$fixture).AbsoluteUri + '"')
        } else { $launchArguments += 'about:blank' }
        $psi.Arguments = $launchArguments -join ' '
    }
    $startedTick = [Diagnostics.Stopwatch]::GetTimestamp()
    $process = [Diagnostics.Process]::Start($psi)
    return [pscustomobject]@{ Process=$process; StartedTick=$startedTick }
}

function Assert-BowserFixtureAccepted([string] $Workload) {
    # One short launch per fixture workload: fail fast if this exe will not open the fixture URL,
    # instead of timing out 120 seconds per run on a modal error dialog.
    $probeDirectory = Join-Path $profileRoot ('fixture-preflight-' + $Workload)
    $probeProfile = Join-Path $probeDirectory 'profile'
    New-Item -ItemType Directory -Force -Path $probeProfile | Out-Null
    $probeLog = Join-Path $probeDirectory 'preflight-events.csv'
    $launch = Start-OneBrowser 'Bowser' $probeProfile $Workload $probeLog
    $rootPid = $launch.Process.Id
    $accepted = $false; $dialog = $null; $windowReady = $false
    $limit = [DateTime]::UtcNow.AddSeconds(15)
    # Wait for the main window as well as the log: the log is created before the window, and WM_CLOSE
    # posted before a window exists does nothing, which left the probe running.
    while ([DateTime]::UtcNow -lt $limit -and -not $dialog -and -not ($accepted -and $windowReady)) {
        Start-Sleep -Milliseconds 200
        $dialog = Get-ErrorDialogMessage $rootPid
        $accepted = Test-Path -LiteralPath $probeLog -PathType Leaf
        $windowReady = ($null -ne (Find-MainWindow $rootPid ''))
    }
    $close = Close-Run $rootPid $probeProfile
    if (-not $close.Exited) {
        # This probe is ours and disposable: stop its process tree so it cannot hold the GUI or profile.
        & taskkill.exe /PID $rootPid /T /F | Out-Null
        Start-Sleep -Seconds 2
        if (@(Get-ProcessGroup $rootPid $probeProfile).Count -gt 0) { throw "Fixture preflight for $Workload could not stop Bowser PID $rootPid; stop it manually and rerun." }
    }
    if ($dialog) {
        throw "Bowser rejected the $Workload fixture URL with the dialog '$dialog'. Bowser compiles its fixture root into the executable (CARGO_MANIFEST_DIR\benchmarks\fixtures). Pass -FixtureRoot pointing at benchmarks\fixtures in the source tree that built $bowserPath."
    }
    if (-not $accepted) { throw "Fixture preflight for $Workload saw neither a Bowser event log nor an error dialog within 15 seconds." }
}

function Get-FileBytes([string[]] $Paths) {
    [long]$total = 0
    foreach ($path in $Paths) { if ($path -and (Test-Path -LiteralPath $path -PathType Leaf)) { $total += (Get-Item -LiteralPath $path).Length } }
    return $total
}

function Get-BowserDiskBytes {
    $files = @($bowserPath)
    $loader = Join-Path (Split-Path -Parent $bowserPath) 'WebView2Loader.dll'
    if (Test-Path -LiteralPath $loader) { $files += $loader }
    return Get-FileBytes $files
}

function Get-WebViewRuntimeInfo {
    $roots = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Microsoft\EdgeWebView\Application'),
        (Join-Path $env:ProgramFiles 'Microsoft\EdgeWebView\Application'),
        (Join-Path $env:LOCALAPPDATA 'Microsoft\EdgeWebView\Application')
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Container) }
    $directories = @()
    foreach ($root in $roots) {
        $directories += Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue | Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' }
    }
    [long]$bytes = 0; $versions = @()
    foreach ($directory in $directories) {
        $versions += $directory.Name
        foreach ($file in Get-ChildItem -LiteralPath $directory.FullName -File -Recurse -ErrorAction SilentlyContinue) { $bytes += $file.Length }
    }
    return [pscustomobject]@{ Version=($versions | Sort-Object -Unique) -join ';'; Bytes=$bytes; Roots=($roots -join ';') }
}

function Get-ToolVersion([string] $CommandName, [string[]] $ArgumentList) {
    try {
        $null = Get-Command $CommandName -ErrorAction Stop
        $output = & $CommandName @ArgumentList 2>$null
        if ($LASTEXITCODE -eq 0) { return (@($output) -join ' ').Trim() }
    } catch { }
    return 'unavailable'
}

function Get-BenchmarkInputHashes {
    $pathMap = [ordered]@{
        bowser_executable=$bowserPath
        run_harness=(Join-Path $PSScriptRoot 'run.ps1')
        measurement_module=(Join-Path $PSScriptRoot 'measurements.psm1')
    }
    if ($edgePath) { $pathMap.edge_executable=$edgePath }
    if ($chromePath) { $pathMap.chrome_executable=$chromePath }
    $fixtureFiles = @(Get-ChildItem -LiteralPath $fixtureRoot -File -Recurse | Sort-Object FullName)
    foreach ($fixtureFile in $fixtureFiles) {
        $relative = $fixtureFile.FullName.Substring($fixtureRoot.Length).TrimStart('\')
        $pathMap[('fixture/' + $relative.Replace('\','/'))] = $fixtureFile.FullName
    }
    $raw = Get-MeasurementInputHashes -Paths @($pathMap.Values)
    $result = [ordered]@{}
    foreach ($key in $pathMap.Keys) {
        $resolved = (Resolve-Path -LiteralPath $pathMap[$key]).Path
        $result[$key] = $raw[$resolved]
    }
    return $result
}

function Get-HostMetadata {
    $os = Get-CimInstance -ClassName Win32_OperatingSystem
    $cpu = Get-CimInstance -ClassName Win32_Processor | Select-Object -First 1
    $bowserVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($bowserPath).ProductVersion
    if (-not $bowserVersion) {
        $cargo = Get-Content -LiteralPath (Join-Path $repoRoot 'Cargo.toml') -Raw -ErrorAction SilentlyContinue
        if ($cargo -match '(?m)^version\s*=\s*"([^"]+)"') { $bowserVersion = $Matches[1] }
    }
    $runtimeInfo = Get-WebViewRuntimeInfo
    $inputHashes = Get-BenchmarkInputHashes
    return [ordered]@{
        captured_utc = [DateTime]::UtcNow.ToString('o')
        computer = $env:COMPUTERNAME
        os = $os.Caption
        os_version = $os.Version
        os_build = $os.BuildNumber
        cpu = $cpu.Name
        logical_processors = [Environment]::ProcessorCount
        physical_memory_bytes = [long]$os.TotalVisibleMemorySize * 1KB
        powershell = $PSVersionTable.PSVersion.ToString()
        rustc = (Get-ToolVersion 'rustc' @('--version'))
        target = 'x86_64-pc-windows-msvc'
        stopwatch_frequency = [Diagnostics.Stopwatch]::Frequency
        bowser_executable = $bowserPath
        bowser_app_disk_bytes = Get-BowserDiskBytes
        bowser_version = $bowserVersion
        bowser_executable_sha256 = $inputHashes.bowser_executable
        edge_executable = $edgePath
        edge_version = if ($edgePath) { [Diagnostics.FileVersionInfo]::GetVersionInfo($edgePath).ProductVersion } else { $null }
        chrome_executable = $chromePath
        chrome_version = if ($chromePath) { [Diagnostics.FileVersionInfo]::GetVersionInfo($chromePath).ProductVersion } else { $null }
        measurement_harness = Join-Path $PSScriptRoot 'run.ps1'
        measurement_module = Join-Path $PSScriptRoot 'measurements.psm1'
        input_sha256_before = $inputHashes
        webview2_runtime = $runtimeInfo.Version
        webview2_runtime_versioned_directories_bytes = $runtimeInfo.Bytes
        fixtures = @((Get-ChildItem -LiteralPath $fixtureRoot -File -Recurse | ForEach-Object { $_.FullName.Substring($fixtureRoot.Length).TrimStart('\') }))
        workload_definitions = 'blank-idle: about:blank plus empty-page memory/idle CPU; blank-submit: launch, focus/read back input, Enter, with submitted_ms immediately at key-up and no later samples; light/demanding: direct local file URL, marker timing immediately after visible window, then page memory/idle CPU.'
        conditions = 'cold-profile = fresh browser profile; warm-profile = process restart with the same profile. Windows OS cache was not flushed and is uncontrolled.'
        background_activity = 'CPU/RAM group measurements include only the launched application and its descendants. Unrelated desktop processes are excluded from app totals; ambient machine activity is uncontrolled.'
        idle_seconds = $IdleSeconds
        runs_per_condition = $Runs
        browser_order = 'Browser order rotates for each workload/run/condition to balance launch-order cache bias.'
        keep_profiles = [bool]$KeepProfiles
        browser_filter = $Browser
        tracking_prevention = $TrackingPrevention
        input_phrase = 'Bowser reproducible browser benchmark'
        input_method = 'Windows Forms SendKeys.SendWait after focusing the browser address UI. Bowser EDIT text is read back; if synthetic virtual-key/SendKeys entry does not populate it, verified WM_SETTEXT fallback is recorded before Enter. Chromium address bar content is submitted through its focused UI.'
    }
}

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$timestamp = [DateTime]::Now.ToString('yyyyMMdd-HHmmss-fff') + '-' + [Guid]::NewGuid().ToString('N').Substring(0,8)
$csvPath = Join-Path $OutputDirectory "benchmark-$timestamp.csv"
$jsonPath = Join-Path $OutputDirectory "benchmark-$timestamp.json"
$summaryPath = Join-Path $OutputDirectory "benchmark-$timestamp.md"
$metadata = Get-HostMetadata
$metadata = Add-BowserSelectionMetadata -Metadata $metadata -Selection $runSelection
$metadata | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $jsonPath -Encoding UTF8

$browsers = @(
    [pscustomobject]@{ Name='Bowser'; Exe=$bowserPath },
    [pscustomobject]@{ Name='Edge'; Exe=$edgePath },
    [pscustomobject]@{ Name='Chrome'; Exe=$chromePath }
)
if ($Browser -ne 'All') { $browsers = @($browsers | Where-Object Name -eq $Browser) }
if ($TrackingPrevention) { $browsers = @($browsers | Where-Object Name -eq 'Bowser') }
$rows = New-Object 'System.Collections.Generic.List[object]'
$phaseRows = New-Object 'System.Collections.Generic.List[object]'
$profileRoot = Join-Path $OutputDirectory ("profiles-" + $timestamp + $(if($TrackingPrevention){'-policy-' + $TrackingPrevention.ToLowerInvariant()}else{''}))
New-Item -ItemType Directory -Force -Path $profileRoot | Out-Null
$allGroupsClosed = $true
$profilesSafeToCleanup = $true
$inputPhrase = 'Bowser reproducible browser benchmark'

if ($browsers.Name -contains 'Bowser') {
    foreach ($fixtureWorkload in @($workloads | Where-Object { $_ -in @('light','demanding') })) { Assert-BowserFixtureAccepted $fixtureWorkload }
}

$workloadIndex = 0
foreach ($workload in $workloads) {
    foreach ($runIndex in 1..$Runs) {
        $pairProfile = Join-Path $profileRoot ("{0}-pair-{1:D2}" -f $workload, $runIndex)
        New-Item -ItemType Directory -Force -Path $pairProfile | Out-Null
        $profileReuseByBrowser = @{}
        foreach ($profileBrowser in $browsers) {
            $profileReuseByBrowser[$profileBrowser.Name] = [pscustomobject]@{ ProfileTainted=$false; ShutdownConfirmed=$true; Reason=$null }
        }
        foreach ($condition in @('cold-profile', 'warm-profile')) {
            $conditionOffset = if ($condition -eq 'cold-profile') { 0 } else { 1 }
            $rotation = ($runIndex - 1 + $workloadIndex + $conditionOffset) % $browsers.Count
            $orderedBrowsers = @(); for ($browserIndex = 0; $browserIndex -lt $browsers.Count; $browserIndex++) { $orderedBrowsers += $browsers[($browserIndex + $rotation) % $browsers.Count] }
            foreach ($browserSpec in $orderedBrowsers) {
                $userDataProfile = Join-Path $pairProfile $browserSpec.Name
                if ($condition -eq 'cold-profile') { New-Item -ItemType Directory -Force -Path $userDataProfile | Out-Null }
                $logPath = Join-Path $OutputDirectory ("event-{0}-{1}-{2}-{3:D2}.csv" -f $browserSpec.Name, $workload, $condition, $runIndex)
                if (Test-Path -LiteralPath $logPath) { Remove-Item -LiteralPath $logPath -Force }
                $windowVisibleMs = $null; $inputReadyMs = $null; $submittedMs = $null; $pageUsableMs = $null
                $idleCpuPct = $null; $idleWorking = $null; $idlePrivate = $null; $pageWorking = $null; $pagePrivate = $null
                $idleProcessCount = $null; $pageProcessCount = $null; $idleSnapshot = $null; $pageSnapshot = $null
                $eventSampleValid = $null; $eventInvalidReasons = @(); $phaseIntervalsJson = $null; $phaseResult = $null
                $trackingPolicyValid = $null
                $closeResult = $null
                    $notes = New-Object 'System.Collections.Generic.List[string]'
                    $rootPid = 0; $closed = $true; $runSucceeded = $false
                    $skipLaunchReason = $null
                    $reuseState = $profileReuseByBrowser[$browserSpec.Name]
                    if ($condition -eq 'warm-profile' -and -not (Test-BowserProfileReuseAllowed -ProfileTainted $reuseState.ProfileTainted -ShutdownConfirmed $reuseState.ShutdownConfirmed)) {
                        $skipLaunchReason = if ($reuseState.ProfileTainted) { 'profile_state_unknown_after_policy_attempt' } else { 'process_group_shutdown_unconfirmed' }
                        $notes.Add(('profile_reuse_skipped: ' + $skipLaunchReason))
                        $eventSampleValid = $false
                        $eventInvalidReasons = @('profile_reuse_skipped:' + $skipLaunchReason)
                        if ($TrackingPrevention) { $trackingPolicyValid = $false }
                        [void]$phaseRows.Add([pscustomobject]@{ browser=$browserSpec.Name; workload=$workload; launch_condition=$condition; run=$runIndex; phase='profile_reuse'; start_event=$null; end_event=$null; start_qpc=$null; end_qpc=$null; duration_ms=$null; app_elapsed_crosscheck_delta_ms=$null; valid=$false; invalid_reason=$eventInvalidReasons[0]; tracking_prevention_requested=$TrackingPrevention; tracking_policy_valid=if($TrackingPrevention){$false}else{$null} })
                    }
                    if ($workload -eq 'blank-idle') { $notes.Add('input_ready_ms, submitted_ms, and page_usable_ms not applicable: empty about:blank idle baseline') }
                    if ($workload -eq 'blank-submit') { $notes.Add('page_usable_ms and all idle/page memory metrics not applicable: submit-only timing ends at Enter') }
                    if ($workload -eq 'light' -or $workload -eq 'demanding') { $notes.Add('input_ready_ms and submitted_ms not applicable: workload opens the local fixture URL directly') }
                if (-not $skipLaunchReason) {
                try {
                    $launch = Start-OneBrowser $browserSpec.Name $userDataProfile $workload $logPath
                    $rootPid = $launch.Process.Id
                    $script:activeStartTick = $launch.StartedTick
                    $visible = Wait-VisibleResponsive $rootPid $launch.StartedTick 90
                    if (-not $visible) { throw 'No visible, WM_NULL-responsive window appeared within 90 seconds.' }
                    $windowVisibleMs = $visible.ElapsedMs
                    if ($workload -eq 'blank-submit') {
                        Focus-AddressInput $visible.Window.Handle $browserSpec.Name
                        $inputReadyMs = Get-ElapsedMs $launch.StartedTick
                    }

                    if ($workload -eq 'light' -or $workload -eq 'demanding') {
                        $marker = if ($workload -eq 'light') { 'BOWSER_BENCH_READY_LIGHT' } else { 'BOWSER_BENCH_READY_DEMANDING' }
                        Activate-MainWindow $visible.Window.Handle
                        $ready = Wait-TitleMarker $rootPid $marker 120
                        if (-not $ready) { throw "Fixture title marker $marker was not observed within 120 seconds." }
                        $pageUsableMs = $ready.ElapsedMs
                        Start-Sleep -Seconds 10
                        $stable = Get-GroupSnapshot $rootPid $userDataProfile
                        $pageSnapshot = $stable
                        $pageProcessCount = $stable.ProcessCount
                        $pageWorking = Get-Bytes $stable.WorkingBytes
                        $pagePrivate = Get-Bytes $stable.PrivateBytes
                    }

                    if ($workload -eq 'blank-idle') {
                        # Let the about:blank engine settle, then sample the empty-page footprint and idle process group.
                        Start-Sleep -Seconds 2
                        $blank = Get-GroupSnapshot $rootPid $userDataProfile
                        $pageSnapshot = $blank
                        $pageProcessCount = $blank.ProcessCount
                        $pageWorking = Get-Bytes $blank.WorkingBytes
                        $pagePrivate = Get-Bytes $blank.PrivateBytes
                    }

                    if ($workload -ne 'blank-submit') {
                        $idleSample = Measure-IdleGroup $rootPid $userDataProfile $IdleSeconds
                        $idleCpuPct = $idleSample.CpuPercent
                        $idleSnapshot = $idleSample.Snapshot
                        $idleProcessCount = $idleSample.Snapshot.ProcessCount
                        $idleWorking = Get-Bytes $idleSample.Snapshot.WorkingBytes
                        $idlePrivate = Get-Bytes $idleSample.Snapshot.PrivateBytes
                    } else {
                        $notes.Add('idle_cpu_percent, idle_working_set_mb, idle_private_mb, page_working_set_mb, and page_private_mb are not applicable: submit timing ends at Enter with no post-submit delay or sampling')
                    }

                    if ($workload -eq 'blank-submit') {
                        if ($browserSpec.Name -eq 'Bowser') {
                            Send-Text $inputPhrase
                            $edit = Find-AddressEdit $visible.Window.Handle
                            if ($edit -eq [IntPtr]::Zero) { throw 'Bowser address EDIT disappeared before submission.' }
                            $enteredText = Get-WindowText $edit
                            if ($enteredText -cne $inputPhrase) {
                                Send-Key 0x11; Send-Key 0x41; Send-Key 0x41 $true; Send-Key 0x11 $true
                                [System.Windows.Forms.SendKeys]::SendWait($inputPhrase)
                                $enteredText = Get-WindowText $edit
                                $notes.Add('address_input_method=Windows Forms SendKeys after virtual-key SendInput failed native EDIT readback')
                            }
                            if ($enteredText -cne $inputPhrase) {
                                Send-TextToEdit $edit $inputPhrase
                                $enteredText = Get-WindowText $edit
                                $notes.Add('address_input_method=WM_CHAR to native EDIT after SendInput and SendKeys; readback checked')
                            }
                            if ($enteredText -cne $inputPhrase) {
                                if (-not [BowserBench.Native]::SetWindowText($edit, $inputPhrase)) { throw "Bowser address input mismatch and native EDIT fallback failed; actual='$enteredText'." }
                                $enteredText = Get-WindowText $edit
                                $notes.Add('address_input_fallback=WM_SETTEXT to native EDIT after SendInput, SendKeys, and WM_CHAR; readback verified')
                            }
                            if ($enteredText -cne $inputPhrase) { throw "Bowser address input text did not match the expected phrase; actual='$enteredText'." }
                        } else {
                            [System.Windows.Forms.SendKeys]::SendWait($inputPhrase)
                            $focusedValue = Get-FocusedValueElement
                            $enteredText = if ($focusedValue) { [string]$focusedValue.Pattern.Current.Value } else { '' }
                            if ($enteredText -cne $inputPhrase) {
                                if (-not $focusedValue) { throw 'Focused Chromium address UI did not expose a UI Automation ValuePattern for readback.' }
                                $focusedValue.Pattern.SetValue($inputPhrase)
                                $enteredText = [string]$focusedValue.Pattern.Current.Value
                                $notes.Add('address_input_fallback=UI Automation ValuePattern.SetValue after SendKeys; readback verified')
                            }
                            if ($enteredText -cne $inputPhrase) { throw "Chromium address input text did not match expected phrase; actual='$enteredText'." }
                            $notes.Add('address_input_method=Windows Forms SendKeys into focused Chromium UI with UI Automation readback')
                        }
                        Send-Key 0x0D
                        Send-Key 0x0D $true
                        $submittedMs = Get-ElapsedMs $launch.StartedTick
                        $pageUsableMs = $null
                        $notes.Add('page_usable_ms unavailable by design: timing stops immediately at Enter; search-provider response is excluded')
                    }
                    $notes.Add('Outer Stopwatch/QPC timings are measured uniformly by the harness; app event log is preserved for diagnostics. CPU and memory sum the launched app and descendants; no OS standby-cache flush.')
                    $runSucceeded = $true
                } catch {
                    $notes.Add(('run_failed: ' + $_.Exception.Message))
                } finally {
                    if ($rootPid -gt 0) {
                        $closeResult = Close-Run $rootPid $userDataProfile
                        $closed = [bool]$closeResult.Exited
                        $reuseState.ShutdownConfirmed = $closed
                        if (-not $closed) {
                            $allGroupsClosed = $false
                            $remainingText = @($closeResult.Remaining | ForEach-Object { 'PID {0} {1}' -f $_.process_id, $_.name }) -join ', '
                            $notes.Add(('shutdown_unconfirmed: process group did not exit within 30 seconds; WM_CLOSE posted={0}; remaining={1}' -f $closeResult.WmClosePosted, $remainingText))
                            $reuseState.Reason = 'process_group_shutdown_unconfirmed'
                            $eventSampleValid = $false
                            $eventInvalidReasons += 'shutdown_unconfirmed'
                            if ($TrackingPrevention) { $trackingPolicyValid = $false }
                        }
                    }
                }
                }

                if ($browserSpec.Name -eq 'Bowser' -and -not $skipLaunchReason) {
                    if (Test-Path -LiteralPath $logPath -PathType Leaf) {
                        try {
                            $eventRows = @(Import-Csv -LiteralPath $logPath)
                            $eventWorkload = if ($workload -eq 'light' -or $workload -eq 'demanding') { 'direct' } else { $workload }
                            $phaseArgs = @{ Rows=$eventRows; QpcFrequency=[Diagnostics.Stopwatch]::Frequency; Workload=$eventWorkload }
                            if ($TrackingPrevention -and $eventWorkload -eq 'direct') { $phaseArgs.TrackingPrevention=$TrackingPrevention }
                            $phaseResult = Get-BowserEventIntervals @phaseArgs
                            $eventSampleValid = [bool]$phaseResult.Valid
                            $eventInvalidReasons = @($phaseResult.InvalidReasons)
                            if ($closeResult -and -not $closeResult.Exited) {
                                $eventSampleValid = $false
                                $eventInvalidReasons += 'shutdown_unconfirmed'
                                if ($TrackingPrevention) { $trackingPolicyValid = $false }
                            }
                            if ($TrackingPrevention) { $trackingPolicyValid = [bool]$phaseResult.Policy.Valid }
                            $trackingPolicyAllowsPhase = (-not $TrackingPrevention -or [bool]$phaseResult.Policy.Valid)
                            $shutdownAllowsPhase = (-not $closeResult -or [bool]$closeResult.Exited)
                            if ($phaseResult.Policy.ProfileTainted) {
                                $reuseState.ProfileTainted = $true
                                $reuseState.Reason = 'tracking_prevention_profile_unknown'
                                $profilesSafeToCleanup = $false
                                $notes.Add('profile_state_tainted: tracking_prevention_profile_unknown; retained for diagnosis')
                            }
                            foreach ($intervalProperty in $phaseResult.Intervals.PSObject.Properties) {
                                $interval = $intervalProperty.Value
                                $phaseInvalidReason = $interval.InvalidReason
                                if ($closeResult -and -not $closeResult.Exited) {
                                    if ($phaseInvalidReason) { $phaseInvalidReason += ';shutdown_unconfirmed' } else { $phaseInvalidReason = 'shutdown_unconfirmed' }
                                }
                                if ($TrackingPrevention -and -not $phaseResult.Policy.Valid) {
                                    $policyReasonText = 'tracking_policy_invalid:' + ($phaseResult.Policy.InvalidReasons -join ',')
                                    if ($phaseInvalidReason) { $phaseInvalidReason += ';' + $policyReasonText } else { $phaseInvalidReason = $policyReasonText }
                                }
                                [void]$phaseRows.Add([pscustomobject]@{
                                    browser='Bowser'; workload=$workload; launch_condition=$condition; run=$runIndex
                                    phase=$interval.Name; start_event=$interval.StartEvent; end_event=$interval.EndEvent
                                    start_qpc=$interval.StartQpc; end_qpc=$interval.EndQpc; duration_ms=$interval.DurationMs
                                    app_elapsed_crosscheck_delta_ms=$interval.AppElapsedCrossCheckDeltaMs
                                    valid=([bool]$interval.Valid -and $shutdownAllowsPhase -and $trackingPolicyAllowsPhase); invalid_reason=$phaseInvalidReason
                                    tracking_prevention_requested=$TrackingPrevention
                                    tracking_policy_valid=if($TrackingPrevention){$phaseResult.Policy.Valid}else{$null}
                                })
                            }
                            foreach ($repeatInterval in $phaseResult.RepeatNavigations) {
                                $repeatInvalidReason = $repeatInterval.InvalidReason
                                if ($closeResult -and -not $closeResult.Exited) {
                                    if ($repeatInvalidReason) { $repeatInvalidReason += ';shutdown_unconfirmed' } else { $repeatInvalidReason = 'shutdown_unconfirmed' }
                                }
                                if ($TrackingPrevention -and -not $phaseResult.Policy.Valid) {
                                    $policyReasonText = 'tracking_policy_invalid:' + ($phaseResult.Policy.InvalidReasons -join ',')
                                    if ($repeatInvalidReason) { $repeatInvalidReason += ';' + $policyReasonText } else { $repeatInvalidReason = $policyReasonText }
                                }
                                [void]$phaseRows.Add([pscustomobject]@{
                                    browser='Bowser'; workload=$workload; launch_condition=$condition; run=$runIndex
                                    phase='repeat_navigation_to_page_usable'; start_event='repeat_navigation_boundary'; end_event='page_usable'
                                    start_qpc=$repeatInterval.BoundaryQpc; end_qpc=$null; duration_ms=$repeatInterval.NavigationToUsableMs
                                    app_elapsed_crosscheck_delta_ms=$null; valid=([bool]$repeatInterval.Valid -and $shutdownAllowsPhase -and $trackingPolicyAllowsPhase); invalid_reason=$repeatInvalidReason
                                    tracking_prevention_requested=$TrackingPrevention
                                    tracking_policy_valid=if($TrackingPrevention){$phaseResult.Policy.Valid}else{$null}
                                })
                            }
                            $phaseIntervalsJson = ConvertTo-Json -InputObject ([pscustomobject]@{ intervals=$phaseResult.Intervals; repeat_navigations=@($phaseResult.RepeatNavigations); policy=$phaseResult.Policy }) -Depth 6 -Compress
                            if (-not $phaseResult.Valid) { $notes.Add(('event_missing_or_invalid: ' + ($phaseResult.InvalidReasons -join ', '))) }
                            if ($TrackingPrevention -and -not $phaseResult.Policy.Valid) { $notes.Add(('tracking_policy_invalid: ' + ($phaseResult.Policy.InvalidReasons -join ', '))) }
                        } catch {
                            $eventSampleValid = $false
                            $eventInvalidReasons = @('event_parse_failed:' + $_.Exception.Message)
                            $notes.Add(('event_missing_or_invalid: ' + $_.Exception.Message))
                            if ($TrackingPrevention) {
                                $trackingPolicyValid = $false
                                $reuseState.ProfileTainted = $true
                                $reuseState.Reason = 'policy_event_log_unreadable'
                                $profilesSafeToCleanup = $false
                                $notes.Add('profile_state_tainted: policy event log could not be verified; retained for diagnosis')
                            }
                            [void]$phaseRows.Add([pscustomobject]@{ browser='Bowser'; workload=$workload; launch_condition=$condition; run=$runIndex; phase='event_log_parse'; start_event=$null; end_event=$null; start_qpc=$null; end_qpc=$null; duration_ms=$null; app_elapsed_crosscheck_delta_ms=$null; valid=$false; invalid_reason=$eventInvalidReasons[0]; tracking_prevention_requested=$TrackingPrevention; tracking_policy_valid=$false })
                        }
                    } else {
                        $eventSampleValid = $false
                        $eventInvalidReasons = @('event_log_missing')
                        $notes.Add('event_missing_or_invalid: event log was not created')
                        if ($TrackingPrevention) {
                            $trackingPolicyValid = $false
                            $reuseState.ProfileTainted = $true
                            $reuseState.Reason = 'policy_event_log_missing'
                            $profilesSafeToCleanup = $false
                            $notes.Add('profile_state_tainted: policy event log missing; retained for diagnosis')
                        }
                        [void]$phaseRows.Add([pscustomobject]@{ browser='Bowser'; workload=$workload; launch_condition=$condition; run=$runIndex; phase='event_log_parse'; start_event=$null; end_event=$null; start_qpc=$null; end_qpc=$null; duration_ms=$null; app_elapsed_crosscheck_delta_ms=$null; valid=$false; invalid_reason='event_log_missing'; tracking_prevention_requested=$TrackingPrevention; tracking_policy_valid=$false })
                    }
                }

                $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($browserSpec.Exe).ProductVersion
                if ($browserSpec.Name -eq 'Bowser' -and -not $version) { $version = $metadata.bowser_version }
                $runtime = if ($browserSpec.Name -eq 'Bowser') { Get-WebViewRuntimeInfo } else { [pscustomobject]@{ Bytes=$null } }
                $benchmarkSampleValid = [bool]$runSucceeded -and [bool]$closed -and -not [bool]$skipLaunchReason
                if ($browserSpec.Name -eq 'Bowser') { $benchmarkSampleValid = $benchmarkSampleValid -and ($eventSampleValid -eq $true) }
                if ($TrackingPrevention) { $benchmarkSampleValid = $benchmarkSampleValid -and ($trackingPolicyValid -eq $true) }
                $sampleInvalidNotes = @($notes | Where-Object { $_ -match 'run_failed|shutdown_unconfirmed|event_missing|tracking_policy_invalid|profile_reuse_skipped|profile_state_tainted' })
                $rows.Add([pscustomobject][ordered]@{
                    browser = $browserSpec.Name
                    version = $version
                    workload = $workload
                    launch_condition = $condition
                    run = $runIndex
                    window_visible_ms = $windowVisibleMs
                    input_ready_ms = $inputReadyMs
                    submitted_ms = $submittedMs
                    page_usable_ms = $pageUsableMs
                    idle_cpu_percent = $idleCpuPct
                    idle_working_set_mb = $idleWorking
                    idle_working_set_source = if ($idleSnapshot) { $idleSnapshot.WorkingSource } else { $null }
                    idle_private_mb = $idlePrivate
                    idle_private_source = if ($idleSnapshot) { $idleSnapshot.PrivateSource } else { $null }
                    idle_native_working_set_mb = if ($idleSnapshot -and $null -ne $idleSnapshot.NativeWorkingBytes) { Get-Bytes $idleSnapshot.NativeWorkingBytes } else { $null }
                    idle_wmi_working_set_mb = if ($idleSnapshot) { Get-Bytes $idleSnapshot.WmiWorkingBytes } else { $null }
                    idle_native_private_mb = if ($idleSnapshot -and $null -ne $idleSnapshot.NativePrivateBytes) { Get-Bytes $idleSnapshot.NativePrivateBytes } else { $null }
                    idle_wmi_pagefileusage_proxy_mb = if ($idleSnapshot -and $null -ne $idleSnapshot.WmiProxyPrivateBytes) { Get-Bytes $idleSnapshot.WmiProxyPrivateBytes } else { $null }
                    idle_native_page_fault_count = if ($idleSnapshot) { $idleSnapshot.NativePageFaultCount } else { $null }
                    idle_hard_fault_count = $null
                    idle_page_in_bytes = $null
                    idle_process_count = $idleProcessCount
                    idle_raced_pid_count = if ($idleSnapshot) { $idleSnapshot.RacedExitCount } else { $null }
                    idle_process_details_json = if ($idleSnapshot) { ConvertTo-Json -InputObject ([pscustomobject]@{ sampled_at_utc=$idleSnapshot.SampledAtUtc; approximate=$idleSnapshot.Approximate; coverage_errors=@($idleSnapshot.CoverageErrors); raced_pid_count=$idleSnapshot.RacedExitCount; native_working_set_bytes=$idleSnapshot.NativeWorkingBytes; wmi_working_set_bytes=$idleSnapshot.WmiWorkingBytes; native_private_bytes=$idleSnapshot.NativePrivateBytes; wmi_pagefileusage_proxy_bytes=$idleSnapshot.WmiProxyPrivateBytes; working_source=$idleSnapshot.WorkingSource; private_source=$idleSnapshot.PrivateSource; hard_fault_count=$null; page_in_bytes=$null; processes=@($idleSnapshot.ProcessDetails) }) -Depth 5 -Compress } else { $null }
                    page_working_set_mb = $pageWorking
                    page_working_set_source = if ($pageSnapshot) { $pageSnapshot.WorkingSource } else { $null }
                    page_private_mb = $pagePrivate
                    page_private_source = if ($pageSnapshot) { $pageSnapshot.PrivateSource } else { $null }
                    page_native_working_set_mb = if ($pageSnapshot -and $null -ne $pageSnapshot.NativeWorkingBytes) { Get-Bytes $pageSnapshot.NativeWorkingBytes } else { $null }
                    page_wmi_working_set_mb = if ($pageSnapshot) { Get-Bytes $pageSnapshot.WmiWorkingBytes } else { $null }
                    page_native_private_mb = if ($pageSnapshot -and $null -ne $pageSnapshot.NativePrivateBytes) { Get-Bytes $pageSnapshot.NativePrivateBytes } else { $null }
                    page_wmi_pagefileusage_proxy_mb = if ($pageSnapshot -and $null -ne $pageSnapshot.WmiProxyPrivateBytes) { Get-Bytes $pageSnapshot.WmiProxyPrivateBytes } else { $null }
                    page_native_page_fault_count = if ($pageSnapshot) { $pageSnapshot.NativePageFaultCount } else { $null }
                    page_hard_fault_count = $null
                    page_in_bytes = $null
                    page_process_count = $pageProcessCount
                    page_raced_pid_count = if ($pageSnapshot) { $pageSnapshot.RacedExitCount } else { $null }
                    page_process_details_json = if ($pageSnapshot) { ConvertTo-Json -InputObject ([pscustomobject]@{ sampled_at_utc=$pageSnapshot.SampledAtUtc; approximate=$pageSnapshot.Approximate; coverage_errors=@($pageSnapshot.CoverageErrors); raced_pid_count=$pageSnapshot.RacedExitCount; native_working_set_bytes=$pageSnapshot.NativeWorkingBytes; wmi_working_set_bytes=$pageSnapshot.WmiWorkingBytes; native_private_bytes=$pageSnapshot.NativePrivateBytes; wmi_pagefileusage_proxy_bytes=$pageSnapshot.WmiProxyPrivateBytes; working_source=$pageSnapshot.WorkingSource; private_source=$pageSnapshot.PrivateSource; hard_fault_count=$null; page_in_bytes=$null; processes=@($pageSnapshot.ProcessDetails) }) -Depth 5 -Compress } else { $null }
                    event_sample_valid = $eventSampleValid
                    event_invalid_reasons = ($eventInvalidReasons -join '; ')
                    event_intervals_json = $phaseIntervalsJson
                    sample_valid = $benchmarkSampleValid
                    sample_invalid_reason = ($sampleInvalidNotes -join '; ')
                    tracking_prevention_requested = $TrackingPrevention
                    tracking_policy_valid = $trackingPolicyValid
                    profile_reuse_skipped = [bool]$skipLaunchReason
                    profile_reuse_skip_reason = $skipLaunchReason
                    shutdown_window_found = if ($closeResult) { $closeResult.WindowFound } else { $null }
                    shutdown_wm_close_posted = if ($closeResult) { $closeResult.WmClosePosted } else { $null }
                    shutdown_confirmed = if ($closeResult) { $closeResult.Exited } else { $null }
                    shutdown_remaining_process_count = if ($closeResult) { @($closeResult.Remaining).Count } else { $null }
                    shutdown_remaining_processes_json = if ($closeResult -and @($closeResult.Remaining).Count -gt 0) { ConvertTo-Json -InputObject @($closeResult.Remaining) -Depth 4 -Compress } elseif ($closeResult) { '[]' } else { $null }
                    app_disk_bytes = if ($browserSpec.Name -eq 'Bowser') { Get-BowserDiskBytes } else { $null }
                    runtime_disk_bytes = if ($browserSpec.Name -eq 'Bowser') { $runtime.Bytes } else { $null }
                    notes = ($notes -join '; ')
                })
                Write-Host ("{0} {1} {2} run {3}/{4}: visible={5}ms page={6}ms CPU={7}%" -f $browserSpec.Name, $workload, $condition, $runIndex, $Runs, $windowVisibleMs, $pageUsableMs, $idleCpuPct)
            }
        }
    }
    $workloadIndex++
}

$inputHashesAfter = Get-BenchmarkInputHashes
$hashKeys = @($metadata.input_sha256_before.Keys) + @($inputHashesAfter.Keys) | Sort-Object -Unique
$hashChanges = @($hashKeys | Where-Object {
    -not $metadata.input_sha256_before.Contains($_) -or -not $inputHashesAfter.Contains($_) -or $metadata.input_sha256_before[$_] -ne $inputHashesAfter[$_]
})
$metadata.input_sha256_after = $inputHashesAfter
$metadata.inputs_unchanged = ($hashChanges.Count -eq 0)
$metadata.changed_inputs = $hashChanges
$metadata | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $jsonPath -Encoding UTF8

$rows | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Encoding UTF8
$phaseCsvPath = Join-Path $OutputDirectory "phase-$timestamp.csv"
if ($phaseRows.Count -gt 0) { $phaseRows | Export-Csv -LiteralPath $phaseCsvPath -NoTypeInformation -Encoding UTF8 }
else { 'browser,workload,launch_condition,run,phase,start_event,end_event,start_qpc,end_qpc,duration_ms,valid,invalid_reason' | Set-Content -LiteralPath $phaseCsvPath -Encoding UTF8 }

$summary = New-Object 'System.Collections.Generic.List[string]'
$summary.Add('# Bowser benchmark results')
$summary.Add('')
$summary.Add(('Captured: {0}; CSV: `{1}`; metadata: `{2}`.' -f [DateTime]::Now.ToString('yyyy-MM-dd HH:mm:ss zzz'), (Split-Path -Leaf $csvPath), (Split-Path -Leaf $jsonPath)))
$summary.Add('')
$summary.Add('`cold-profile` uses a new isolated browser profile. `warm-profile` restarts the process with the same profile from its paired cold run. Windows standby/file cache was not flushed; OS cache state is uncontrolled. No browser security features were disabled. Each reported process metric sums the launched browser and its descendants; CPU is normalized by logical processor count.')
$summary.Add('Unrelated applications are excluded from each process-group total, while their background CPU, memory pressure, and disk activity remain uncontrolled host noise.')
$summary.Add('Process-group snapshots are approximate. Native `GetProcessMemoryInfo(PROCESS_MEMORY_COUNTERS_EX)` provides per-PID working set and private usage where access permits; the same inventory records `Win32_Process.PageFileUsage` separately as `wmi_pagefileusage_proxy`. Private totals identify their source and use the proxy only when native coverage is incomplete. PID plus process-creation time detects reuse; parent edges exclude children older than the current parent. Hard-fault and page-in metrics are unavailable. One-second CPU sampling can miss short-lived children.')
$summary.Add('Raw CSV includes native and WMI proxy values, their source labels, process counts, creation-time identity, role/command-line type where identifiable, sample time, and coverage errors for each RAM snapshot.')
$summary.Add('Browser launch order rotates across workload, run, and condition to reduce systematic cache-order bias. Page title markers are polled every 10 ms and the recorded outer stopwatch timestamp is the first responsive observation.')
$summary.Add('')
$summary.Add(('Host: {0}; {1} ({2}, build {3}); CPU: {4} ({5} logical processors); RAM: {6:N1} GiB.' -f $metadata.computer, $metadata.os, $metadata.os_version, $metadata.os_build, $metadata.cpu, $metadata.logical_processors, ($metadata.physical_memory_bytes / 1GB)))
$summary.Add(('Versions: Bowser {0}; Edge {1}; Chrome {2}; Evergreen WebView2 {3}.' -f $metadata.bowser_version, $metadata.edge_version, $metadata.chrome_version, $metadata.webview2_runtime))
$summary.Add(('Disk footprint: Bowser executable plus adjacent loader is {0:N0} bytes ({1:N2} MiB); shared installed Evergreen WebView2 version directories total {2:N0} bytes ({3:N2} GiB), separate from Bowser and user profiles.' -f $metadata.bowser_app_disk_bytes, ($metadata.bowser_app_disk_bytes / 1MB), $metadata.webview2_runtime_versioned_directories_bytes, ($metadata.webview2_runtime_versioned_directories_bytes / 1GB)))
$summary.Add(('SHA-256 frozen input verification: {0}; metadata contains pre/post hashes for the exact Bowser executable, runner, measurement module, selected browser executables, and every fixture.' -f $metadata.inputs_unchanged))
$summary.Add(('Rust build toolchain: {0}; target: {1}.' -f $metadata.rustc, $metadata.target))
$summary.Add(('Run settings: {0} paired profiles per selected workload; {1} seconds CPU sampling window; selected workloads: {2}; fixtures: {3}' -f $Runs, $IdleSeconds, ($workloads -join ', '), $metadata.fixtures))
$summary.Add('')
$summary.Add(('Phase detail CSV: `{0}`. Raw event CSVs remain beside the result files.' -f (Split-Path -Leaf $phaseCsvPath)))
$summary.Add('')
$summary.Add('## Median and range')
$summary.Add('')
$summary.Add('| Browser | Workload | Condition | Valid samples | Invalid samples | Policy rejected | Visible ms | Input ready ms | Submitted ms | Page usable ms | Idle CPU % | Idle WS MiB | Idle private MiB | Idle private source | Page WS MiB | Page private MiB | Page private source |')
$summary.Add('|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|---|')
foreach ($group in ($rows | Group-Object browser, workload, launch_condition | Sort-Object Name)) {
    $first = $group.Group[0]
    $eligibleRows = @($group.Group | Where-Object { $_.sample_valid })
    $invalidCount = $group.Group.Count - $eligibleRows.Count
    $policyRejectedCount = @($group.Group | Where-Object { $_.tracking_prevention_requested -and $_.tracking_policy_valid -eq $false }).Count
    $cells = New-Object 'System.Collections.Generic.List[string]'
    foreach ($field in @('window_visible_ms','input_ready_ms','submitted_ms','page_usable_ms','idle_cpu_percent','idle_working_set_mb','idle_private_mb','page_working_set_mb','page_private_mb')) {
        $values = @($eligibleRows | ForEach-Object { $value = $_.$field; if ($null -ne $value -and "$value" -ne '') { [double]$value } })
        if ($values.Count -eq 0) { $cells.Add('n/a') }
        else {
            $sorted = @($values | Sort-Object)
            if (($sorted.Count % 2) -eq 1) { $median = $sorted[[int][Math]::Floor($sorted.Count / 2)] }
            else { $median = ($sorted[($sorted.Count / 2) - 1] + $sorted[$sorted.Count / 2]) / 2.0 }
            $min = $sorted[0]; $max = $sorted[$sorted.Count - 1]
            $cells.Add(('{0:N2} ({1:N2}–{2:N2})' -f $median, $min, $max))
        }
    }
    $idleSources = @($eligibleRows | ForEach-Object { $_.idle_private_source } | Where-Object { $_ } | Sort-Object -Unique) -join '/'
    $pageSources = @($eligibleRows | ForEach-Object { $_.page_private_source } | Where-Object { $_ } | Sort-Object -Unique) -join '/'
    $summary.Add(('| {0} | {1} | {2} | {3} | {4} | {5} | {6} | {7} | {8} | {9} | {10} | {11} | {12} | {13} | {14} | {15} | {16} |' -f $first.browser, $first.workload, $first.launch_condition, $eligibleRows.Count, $invalidCount, $policyRejectedCount, $cells[0], $cells[1], $cells[2], $cells[3], $cells[4], $cells[5], $cells[6], $idleSources, $cells[7], $cells[8], $pageSources))
}
$summary.Add('')
$summary.Add('## Bowser event phase intervals')
$summary.Add('')
$summary.Add('Each row is calculated from QPC ticks within one Bowser event log. The phase CSV contains every per-run duration or its specific invalid reason. App elapsed deltas are cross-checks; harness Stopwatch timings remain the user-visible launch timings. `blank-submit` ends at Enter and has no page-ready interval.')
$summary.Add('')
$summary.Add('| Workload | Condition | Phase | Valid samples | Invalid samples | Median ms | Min ms | Max ms |')
$summary.Add('|---|---|---|---:|---:|---:|---:|---:|')
# $phaseRows is a generic List; @($list) throws "Argument types do not match" on Windows PowerShell 5.1.
foreach ($phaseSummary in (Get-BowserPhaseSummary -Rows $phaseRows.ToArray() | Sort-Object workload,launch_condition,phase)) {
    if ($phaseSummary.MedianMs -eq $null) { $medianText='n/a'; $minText='n/a'; $maxText='n/a' }
    else { $medianText='{0:N3}' -f $phaseSummary.MedianMs; $minText='{0:N3}' -f $phaseSummary.MinMs; $maxText='{0:N3}' -f $phaseSummary.MaxMs }
    $summary.Add(('| {0} | {1} | {2} | {3} | {4} | {5} | {6} | {7} |' -f $phaseSummary.workload,$phaseSummary.launch_condition,$phaseSummary.phase,$phaseSummary.ValidCount,$phaseSummary.InvalidCount,$medianText,$minText,$maxText))
    $perRun=@($phaseSummary.PerRun | ForEach-Object { if($_.valid){'run {0}: {1:N3} ms' -f $_.run,[double]$_.duration_ms}else{'run {0}: invalid ({1})' -f $_.run,$_.invalid_reason} }) -join '; '
    $summary.Add(('  - {0}' -f $perRun))
    if ($phaseSummary.InvalidReasons.Count -gt 0) { $summary.Add(('  - invalid reasons: {0}' -f ($phaseSummary.InvalidReasons -join ', '))) }
}
$summary.Add('')
$summary.Add('`blank-idle` samples about:blank memory after 2 seconds and idle CPU/RAM over the configured window, without submission. `blank-submit` measures launch, focused input readiness, verified phrase entry, and Enter; `submitted_ms` is captured immediately after Enter key-up, with no later sampling or provider response wait. Its page, idle, and memory metrics are n/a by design. `light` and `demanding` launch their local fixture directly; the title marker is polled immediately after visible/responsive, without an address-focus step before readiness. Their input-ready and submitted metrics are n/a. Harness UI timings use Stopwatch/QPC for all browsers; Bowser app event logs are retained as diagnostics. Missing values remain blank in CSV and appear as `n/a` here.')
$summary.Add('')
$summary.Add('Battery use is unmeasured. Runtime disk size counts versioned Evergreen WebView2 application directories only and is separate from Bowser files and user profiles. Bowser app disk size counts the release executable and an adjacent `WebView2Loader.dll`, if present.')
$summary.Add('')
$summary.Add($(if ($KeepProfiles) { 'Disposable browser profiles retained because `-KeepProfiles` was requested.' } elseif (-not $allGroupsClosed) { 'Disposable browser profiles are retained because at least one process group did not confirm shutdown.' } elseif (-not $profilesSafeToCleanup) { 'Disposable browser profiles are retained because at least one requested policy left profile state unknown.' } else { 'Disposable profiles are scheduled for removal after summary generation; every process group confirmed shutdown and profile state remained known.' }))
$summary.Add('')
$summary.Add('## Failures and missing metrics')
$summary.Add('')
$failures = @($rows | Where-Object { $_.notes -match 'run_failed|shutdown_unconfirmed|event_missing|tracking_policy_invalid|profile_reuse_skipped|profile_state_tainted' })
if ($failures.Count -eq 0) { $summary.Add('No launch, fixture, shutdown, policy, profile-reuse, or instrumented-event failures were recorded.') }
else {
    foreach ($failure in $failures) { $summary.Add(('- {0}, {1}, {2}, run {3}: {4}' -f $failure.browser, $failure.workload, $failure.launch_condition, $failure.run, $failure.notes)) }
}
if ($hashChanges.Count -gt 0) { $summary.Add(('Frozen input hash mismatch during collection: {0}. Do not compare these results.' -f ($hashChanges -join ', '))) }
$summary | Set-Content -LiteralPath $summaryPath -Encoding UTF8

if (-not $KeepProfiles -and $allGroupsClosed -and $profilesSafeToCleanup) {
    $resolvedOutputDirectory = [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $OutputDirectory).Path).TrimEnd('\') + '\'
    $resolvedProfileRoot = [IO.Path]::GetFullPath((Resolve-Path -LiteralPath $profileRoot).Path)
    $profileItem = Get-Item -LiteralPath $resolvedProfileRoot -Force
    $insideOutput = $resolvedProfileRoot.StartsWith($resolvedOutputDirectory, [StringComparison]::OrdinalIgnoreCase)
    $expectedName = (Split-Path -Leaf $resolvedProfileRoot) -match '^profiles-\d{8}-\d{6}-\d{3}-[0-9a-f]{8}(?:-policy-(?:balanced|strict))?$'
    if ($insideOutput -and $expectedName -and -not ($profileItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        Remove-Item -LiteralPath $resolvedProfileRoot -Recurse -Force
        Write-Host 'Disposable profiles removed after every launched process group exited.'
    } else {
        Write-Host "Profile cleanup skipped because the resolved path did not pass its safety checks: $resolvedProfileRoot"
    }
} elseif ($KeepProfiles) {
    Write-Host 'Profiles retained because -KeepProfiles was specified.'
} elseif (-not $profilesSafeToCleanup) {
    Write-Host 'Profiles retained because at least one requested policy left profile state unknown.'
} else {
    Write-Host 'Profiles retained because at least one process group did not confirm shutdown.'
}

Write-Host "CSV: $csvPath"
Write-Host "Metadata: $jsonPath"
Write-Host "Summary: $summaryPath"
if ($KeepProfiles -or -not $allGroupsClosed) { Write-Host "Profile data retained for reproducibility/evidence at: $profileRoot" }
if ($hashChanges.Count -gt 0) { throw ('Input file hashes changed during the benchmark: ' + ($hashChanges -join ', ')) }
