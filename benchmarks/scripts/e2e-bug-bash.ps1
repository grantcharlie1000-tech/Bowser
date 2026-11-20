# End-to-end check of the fix/bug-bash user flows on a real Windows desktop (the real bowser.exe,
# HTML chrome, real mouse and keyboard input, UI Automation to find things, screenshots as evidence).
#
# Flows (-Flows, comma separated):
#   groups   1. manual tab group from two tabs of different sites: tab menu > Tab group > "Add tab to
#               new group", rename it and pick a colour in the group editor, then "Add tab to group >"
#            2. Ctrl+T on a grouped tab opens the new tab in that group; close and restart Bowser and
#               the group (name, colour, members) comes back from session.txt
#   sw       3. a service-worker-heavy page (local stand-in for Gmail: a worker controls its scope and the
#               page fetches -Fetches subresources through it) timed alone and with 15 other live tabs;
#               then https://mail.google.com unauthenticated, timed until it stops loading
#   toolbar  4. toolbar_buttons with 20 items: the address field keeps a usable width at 1280 wide
#   strip    5. 40 tabs: no overflow drop-down, titles visible, a plain mouse wheel scrolls the strip
#
# Every launch uses its own throwaway --data-dir and a throwaway LOCALAPPDATA. Results go to
# <Out>/results.json and <Out>/results.md, screenshots to <Out>/*.png.
#
# Usage: powershell -File e2e-bug-bash.ps1 -Exe target\release\bowser.exe -Out results [-Label fix]
param(
  [string]$Exe = "$PSScriptRoot\..\..\target\release\bowser.exe",
  [string]$Out = "$PSScriptRoot\..\results\e2e-bug-bash",
  [string]$Flows = 'groups,toolbar,strip,sw',
  [string]$Label = 'fix',
  [int]$Port = 8411,
  [int]$Fetches = 200,
  [int]$Runs = 5
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
if (-not ('E2E' -as [type])) {
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices; using System.Collections.Generic;
public static class E2E {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
  public delegate bool EP(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int ht, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, int d, UIntPtr e);
  [DllImport("user32.dll")] static extern bool EnumWindows(EP p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool f);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  public static string Cls(IntPtr h) { var s = new StringBuilder(128); GetClassName(h, s, 128); return s.ToString(); }
  public static string Text(IntPtr h) { var s = new StringBuilder(512); GetWindowText(h, s, 512); return s.ToString(); }
  public static IntPtr Find(uint pid, string cls) { IntPtr found = IntPtr.Zero; EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h) && Cls(h) == cls) { found = h; return false; } return true; }, IntPtr.Zero); return found; }
  public static List<string> Visible(uint pid) { var list = new List<string>(); EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { RECT r; GetWindowRect(h, out r); list.Add(Cls(h) + " " + r.L + "," + r.T + " " + (r.R - r.L) + "x" + (r.B - r.T)); } return true; }, IntPtr.Zero); return list; }
  public static void Force(IntPtr h) { IntPtr fg = GetForegroundWindow(); uint p; uint ft = GetWindowThreadProcessId(fg, out p); uint me = GetCurrentThreadId(); AttachThreadInput(me, ft, true); ShowWindow(h, 9); SetForegroundWindow(h); AttachThreadInput(me, ft, false); }
}
"@ }
[E2E]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path; $Exe = (Resolve-Path $Exe).Path
$log = Join-Path $Out 'e2e.log'
$tmpRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
function Note([string]$text) { $line = "{0:HH:mm:ss.fff} [{1}] {2}" -f (Get-Date), $Label, $text; Write-Host $line; Add-Content -Path $log -Value $line }

$results = [ordered]@{ label = $Label; exe = $Exe; started = (Get-Date).ToString('o'); flows = [ordered]@{} }
function Save-Results {
  $results.finished = (Get-Date).ToString('o')
  $results | ConvertTo-Json -Depth 10 | Set-Content -Path (Join-Path $Out 'results.json') -Encoding UTF8
  $md = @("# e2e bug-bash flows ($Label)", '', "exe: ``$Exe``", '')
  foreach ($k in $results.flows.Keys) {
    $f = $results.flows[$k]
    $md += "## $k : $($f.status)"
    foreach ($c in $f.checks) { $md += "- [$(if ($c.ok) { 'x' } else { ' ' })] $($c.name): $($c.detail)" }
    if ($f.data) { $md += ''; $md += '```'; $md += ($f.data | ConvertTo-Json -Depth 6); $md += '```' }
    if ($f.error) { $md += "error: $($f.error)" }
    $md += ''
  }
  $md -join "`n" | Set-Content -Path (Join-Path $Out 'results.md') -Encoding UTF8
}
$current = $null
function Begin-Flow([string]$name) { $script:current = [ordered]@{ status = 'RUNNING'; checks = @(); data = [ordered]@{}; error = $null }; $results.flows[$name] = $script:current; Note "==== flow $name" }
function Check([string]$name, $ok, $detail) { $ok = [bool]$ok; $script:current.checks += [ordered]@{ name = $name; ok = $ok; detail = "$detail" }; Note ("check {0}: {1} ({2})" -f $name, $(if ($ok) { 'ok' } else { 'FAIL' }), $detail) }
function End-Flow { if ($script:current.status -eq 'RUNNING') { $bad = @($script:current.checks | Where-Object { -not $_.ok }); $script:current.status = $(if ($bad.Count -eq 0 -and $script:current.checks.Count -gt 0) { 'PASS' } else { 'FAIL' }) }; Save-Results }

# ---------------------------------------------------------------- desktop
try { Set-DisplayResolution -Width 1600 -Height 900 -Force -ErrorAction Stop; Note 'display 1600x900' } catch { Note "display unchanged: $($_.Exception.Message)" }
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Note ("screen {0}x{1}" -f $screen.Width, $screen.Height)

# ---------------------------------------------------------------- fixture server
$reports = Join-Path $Out 'sw-reports.jsonl'
Remove-Item $reports -ErrorAction SilentlyContinue
$python = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $python) { $python = (Get-Command py -ErrorAction Stop).Source }
$server = Start-Process $python -ArgumentList @("`"$PSScriptRoot\e2e-bug-bash-server.py`"", '--port', $Port, '--reports', "`"$reports`"") -PassThru -RedirectStandardOutput (Join-Path $Out 'server.out.txt') -RedirectStandardError (Join-Path $Out 'server.err.txt')
$up = $false
for ($i = 0; $i -lt 50 -and -not $up; $i++) { Start-Sleep -Milliseconds 200; try { $up = (Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$Port/health" -TimeoutSec 2).Content -eq 'ok' } catch { } }
if (-not $up) { throw 'fixture server did not start' }
Note "fixture server pid $($server.Id) on $Port"
$A = "http://127.0.0.1:$Port"      # one site
$B = "http://localhost:$Port"      # another site (a different host)

# ---------------------------------------------------------------- Bowser processes
$bowser = $null; $h = [IntPtr]::Zero; $data = $null
function New-DataDir([string]$name, [string[]]$settings) {
  $dir = Join-Path $tmpRoot ("e2e-$Label-$name-" + (Get-Random))
  New-Item -ItemType Directory -Force $dir | Out-Null
  # No first-run welcome on a later plain launch: the offer counts as answered.
  Set-Content -Path (Join-Path $dir 'import-offer.txt') -Value 'answered' -Encoding ASCII
  if ($settings) { [IO.File]::WriteAllText((Join-Path $dir 'settings.txt'), (($settings -join "`r`n") + "`r`n"), (New-Object Text.UTF8Encoding $false)) }
  $local = Join-Path $dir 'local-appdata'
  New-Item -ItemType Directory -Force $local | Out-Null
  return $dir
}
function Start-Bowser([string]$dir, [string[]]$urls, [string]$tag) {
  $env:BOWSER_UI = 'html'
  $env:LOCALAPPDATA = Join-Path $dir 'local-appdata'
  $argv = @('--data-dir', $dir, '--diagnostic-log', (Join-Path $Out "diag-$tag.log"))
  foreach ($u in $urls) { $argv += @('--open-url', $u) }
  $script:data = $dir
  $script:bowser = Start-Process $Exe -ArgumentList $argv -PassThru
  $script:h = [IntPtr]::Zero
  for ($i = 0; $i -lt 600 -and $script:h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 50; $script:h = [E2E]::Find([uint32]$script:bowser.Id, 'BowserNativeWindow') }
  if ($script:h -eq [IntPtr]::Zero) { throw "no Bowser window ($tag)" }
  Note "launched $tag pid $($script:bowser.Id)"
  Place 1280 800
  $script:root = $null
}
function Place([int]$w, [int]$ht) { [E2E]::ShowWindow($script:h, 9) | Out-Null; [E2E]::MoveWindow($script:h, 40, 30, $w, $ht, $true) | Out-Null; Start-Sleep -Milliseconds 500 }
# Hands addresses to the running window, the way Windows opens a link: a second process that forwards
# them and exits. Each one opens as a new, live foreground tab.
function Forward([string[]]$urls) {
  $p = Start-Process $Exe -ArgumentList (@('--data-dir', $script:data) + $urls) -PassThru
  if (-not $p.WaitForExit(20000)) { Note 'handoff did not exit' }
}
function Stop-Bowser([string]$why = 'close') {
  if (-not $script:bowser) { return }
  $clean = $false
  try { $script:bowser.CloseMainWindow() | Out-Null; $clean = $script:bowser.WaitForExit(20000) } catch { }
  if (-not $clean) { Note "no clean exit ($why), killing"; Stop-Process -Id $script:bowser.Id -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 800
  $d = $script:data
  Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like "*$d*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  $script:bowser = $null
  return $clean
}

# ---------------------------------------------------------------- input and evidence
function IsFg { return ([E2E]::GetForegroundWindow() -eq $script:h) }
function Front { for ($i = 0; $i -lt 10; $i++) { if (IsFg) { return $true }; [E2E]::Force($script:h); Start-Sleep -Milliseconds 200 }; return (IsFg) }
function Click([int]$x, [int]$y, [switch]$Right) {
  [E2E]::SetCursorPos($x, $y) | Out-Null; Start-Sleep -Milliseconds 150
  if ($Right) { [E2E]::mouse_event(0x8, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [E2E]::mouse_event(0x10, 0, 0, 0, [UIntPtr]::Zero) }
  else { [E2E]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [E2E]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero) }
}
function Wheel([int]$x, [int]$y, [int]$notches) {
  [E2E]::SetCursorPos($x, $y) | Out-Null; Start-Sleep -Milliseconds 150
  $step = $(if ($notches -gt 0) { 120 } else { -120 })
  for ($i = 0; $i -lt [Math]::Abs($notches); $i++) { [E2E]::mouse_event(0x0800, 0, 0, $step, [UIntPtr]::Zero); Start-Sleep -Milliseconds 90 }
}
function Keys([string]$k, [int]$wait = 400) { [Windows.Forms.SendKeys]::SendWait($k); Start-Sleep -Milliseconds $wait }
function Literal([string]$text) { return ($text -replace '([\+\^%~\(\)\{\}\[\]])', '{$1}') }
function Field { return (FindAll (Root) $CT::Edit { param($n) $n -like 'Address and search bar*' } | Select-Object -First 1) }
function FieldValue { $f = Field; if (-not $f) { return $null }; try { return $f.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value } catch { return $null } }
# Types an address into the address field and goes there; the field's text is checked before Enter
# (a slow window can drop the first keys), and typed again when it is wrong.
function Go([string]$url) {
  for ($try = 0; $try -lt 3; $try++) {
    Front | Out-Null
    Keys '^l' 700; Keys '^a' 200; Keys (Literal $url) 500
    $v = FieldValue
    if ($null -eq $v -or $v.StartsWith($url) -or ($url.StartsWith($v.TrimEnd('/')) -and $v.Length -gt 8)) { Keys '{ENTER}' 300; return }
    Note "address field holds '$v', not '$url': typing it again"
    Keys '{ESC}' 300
  }
  Keys '{ENTER}' 300
}
function Shot([string]$name, [int]$x = -1, [int]$y = -1, [int]$w = -1, [int]$ht = -1, [double]$scale = 1) {
  Start-Sleep -Milliseconds 300
  $r = New-Object E2E+RECT; [E2E]::GetWindowRect($script:h, [ref]$r) | Out-Null
  if ($x -lt 0) { $x = $r.L; $y = $r.T; $w = $r.R - $r.L; $ht = $r.B - $r.T }
  $bmp = New-Object Drawing.Bitmap $w, $ht
  $g = [Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size); $g.Dispose()
  if ($scale -ne 1) {
    $big = New-Object Drawing.Bitmap ([int]($w * $scale)), ([int]($ht * $scale))
    $g2 = [Drawing.Graphics]::FromImage($big); $g2.InterpolationMode = 'NearestNeighbor'; $g2.DrawImage($bmp, 0, 0, $big.Width, $big.Height); $g2.Dispose(); $bmp.Dispose(); $bmp = $big
  }
  $path = Join-Path $Out "$Label-$name.png"
  $bmp.Save($path, [Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  Note "screenshot $Label-$name.png"
}
function WinRect { $r = New-Object E2E+RECT; [E2E]::GetWindowRect($script:h, [ref]$r) | Out-Null; return $r }

# ---------------------------------------------------------------- UI Automation
$UIA = [System.Windows.Automation.AutomationElement]
$TS = [System.Windows.Automation.TreeScope]
$CT = [System.Windows.Automation.ControlType]
function Root { return $UIA::FromHandle($script:h) }
function Cond($prop, $value) { return New-Object System.Windows.Automation.PropertyCondition($prop, $value) }
function Center($el) { $r = $el.Current.BoundingRectangle; return @([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) }
# Elements of $type under $scope whose name matches $match (a scriptblock on the name).
function FindAll($scope, $type, [scriptblock]$match) {
  $cond = $(if ($type) { Cond $UIA::ControlTypeProperty $type } else { [System.Windows.Automation.Condition]::TrueCondition })
  $out = @()
  try { foreach ($el in $scope.FindAll($TS::Descendants, $cond)) { try { $n = $el.Current.Name; if (& $match $n) { $out += $el } } catch { } } } catch { }
  return $out
}
function WaitFor([scriptblock]$probe, [int]$timeoutMs = 15000, [int]$every = 300) {
  $clock = [Diagnostics.Stopwatch]::StartNew()
  while ($clock.ElapsedMilliseconds -lt $timeoutMs) { $v = $null; try { $v = & $probe } catch { }; if ($v) { return $v }; Start-Sleep -Milliseconds $every }
  return $null
}
# The tab strip's tabs, left to right: name, x, width, offscreen.
function Tabs {
  $list = @()
  foreach ($el in (FindAll (Root) $CT::TabItem { param($n) $true })) {
    $r = $el.Current.BoundingRectangle
    $list += [pscustomobject]@{ name = $el.Current.Name; x = [int]$r.X; y = [int]$r.Y; w = [int]$r.Width; h = [int]$r.Height; off = $el.Current.IsOffscreen; el = $el }
  }
  return @($list | Sort-Object x)
}
function TabNamed([string]$prefix) { return (Tabs | Where-Object { $_.name -like "$prefix*" -and $_.w -gt 4 } | Select-Object -First 1) }
function Popup([string]$cls) { $p = [E2E]::Find([uint32]$script:bowser.Id, $cls); if ($p -eq [IntPtr]::Zero) { return $null }; return $p }
function MenuItem([string]$prefix, [int]$timeoutMs = 6000) {
  return WaitFor {
    $p = Popup 'BowserHtmlPopup'; if (-not $p) { return $null }
    $items = FindAll ($UIA::FromHandle($p)) $null { param($n) $n -and $n.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) }
    $items | Where-Object { $r = $_.Current.BoundingRectangle; $r.Width -gt 4 -and $r.Height -gt 4 -and -not $_.Current.IsOffscreen -and ($_.Current.ControlType -eq $CT::MenuItem -or $_.Current.ControlType -eq $CT::CheckBox) } | Select-Object -First 1
  } $timeoutMs 250
}
function ClickMenu([string]$prefix) {
  $item = MenuItem $prefix
  if (-not $item) { Note "menu item not found: $prefix; visible: $([E2E]::Visible([uint32]$script:bowser.Id) -join ' | ')"; return $false }
  $c = Center $item; Click $c[0] $c[1]; Note "menu: $prefix"; Start-Sleep -Milliseconds 700
  return $true
}
function TabMenu([string]$tabPrefix) {
  Front | Out-Null
  $t = WaitFor { TabNamed $tabPrefix } 15000
  if (-not $t) { Note "no tab named $tabPrefix (tabs: $((Tabs | ForEach-Object { $_.name }) -join ' | '))"; return $false }
  $c = Center $t.el; Click $c[0] $c[1] -Right
  $ok = [bool](WaitFor { Popup 'BowserHtmlPopup' } 5000 200)
  Note "tab menu on '$($t.name)': $ok"
  Start-Sleep -Milliseconds 500
  return $ok
}
function ReadSession { $f = Join-Path $script:data 'session.txt'; if (Test-Path $f) { return [IO.File]::ReadAllText($f) }; return '' }
# The tabs and manual groups of session.txt: @{tabs=@(@{url,title,group}); groups=@{id=@{colour,name}}}
function ParseSession([string]$text) {
  $tabs = @(); $groups = @{}
  foreach ($line in ($text -split "`r?`n")) {
    $f = $line -split "`t"
    if ($f[0] -eq 'tab') { $tabs += [pscustomobject]@{ url = $f[2]; title = $(if ($f.Count -gt 3) { $f[3] } else { '' }); group = $null } }
    elseif ($f[0] -eq 'group' -and $tabs.Count -gt 0) { $tabs[-1].group = $f[1] }
    elseif ($f[0] -eq 'tabgroup') { $groups[$f[1]] = [pscustomobject]@{ colour = $f[2]; collapsed = $f[3]; name = $(if ($f.Count -gt 4) { $f[4] } else { '' }) } }
  }
  return [pscustomobject]@{ tabs = $tabs; groups = $groups }
}

# ================================================================ flows 1 + 2: manual groups
function Flow-Groups {
  Begin-Flow 'groups'
  $dir = New-DataDir 'groups' @()
  # Alpha opens with the launch; Beta is handed over like a clicked link, so it is a live, loaded tab.
  Start-Bowser $dir @("$A/site/alpha?t=Alpha%20site&c=d93025") 'groups-1'
  Start-Sleep -Seconds 3
  Forward @("$B/site/beta?t=Beta%20site&c=1a73e8")
  $tabsUp = WaitFor { $t = Tabs; if ((@($t | Where-Object { $_.name -like 'Alpha*' }).Count -ge 1) -and (@($t | Where-Object { $_.name -like 'Beta*' }).Count -ge 1)) { $t } } 30000
  Check 'two tabs of two sites open' ([bool]$tabsUp) ((Tabs | ForEach-Object { $_.name }) -join ' | ')
  Start-Sleep -Seconds 2
  Shot 'g01-two-tabs'

  # 1a. Tab menu > Tab group > Add tab to new group, clicked with the mouse like a person would.
  $opened = TabMenu 'Alpha'
  Shot 'g02-tab-menu'
  $sub = $opened -and (ClickMenu 'Tab group')
  Shot 'g03-tab-group-submenu'
  $item = $(if ($sub) { MenuItem 'Add tab to new group' } else { $null })
  $made = $false
  if ($item) {
    $c = Center $item; Click $c[0] $c[1]; $made = $true; Note 'menu: Add tab to new group'
    # Watch the group editor (HTML hover layer) for 3 s from the click, the pointer left on the row.
    $clock = [Diagnostics.Stopwatch]::StartNew(); $first = $null; $last = $null
    while ($clock.ElapsedMilliseconds -lt 3000) { if (Popup 'BowserHtmlHover') { if ($null -eq $first) { $first = $clock.ElapsedMilliseconds }; $last = $clock.ElapsedMilliseconds }; Start-Sleep -Milliseconds 25 }
    $current.data.editor_after_menu_click = [ordered]@{ first_seen_ms = $first; last_seen_ms = $last; still_open_at_3s = [bool](Popup 'BowserHtmlHover') }
    Note ("group editor after the click: first seen {0} ms, last seen {1} ms, open at 3 s {2}" -f $first, $last, [bool](Popup 'BowserHtmlHover'))
  }
  Check 'menu: Tab group > Add tab to new group' $made "menu opened $opened, submenu $sub"
  Shot 'g04-group-editor'
  $editor = Popup 'BowserHtmlHover'
  Check 'the group editor opens at once and stays open (pointer still on the menu row)' ([bool]$editor) ($current.data.editor_after_menu_click | ConvertTo-Json -Compress)
  $field = $null; $focused = $false
  if ($editor) {
    $field = WaitFor { FindAll ($UIA::FromHandle($editor)) $CT::Edit { param($n) $n -eq 'Group name' } | Select-Object -First 1 } 5000
    # Opened by the command, the field should already have the keyboard: type without clicking it.
    $focused = [bool]$field
  } else {
    # The way left without it: hover the new group's ball, which opens the same editor.
    $top = (WinRect).T
    $ball0 = WaitFor { FindAll (Root) $null { param($n) $n -like 'Group *' -or $n -eq 'Tab island' } | Where-Object { $r = $_.Current.BoundingRectangle; $r.Width -gt 2 -and $r.Height -gt 2 -and $r.Y -lt $top + 80 } | Select-Object -First 1 } 5000
    if ($ball0) {
      $c = Center $ball0; [E2E]::SetCursorPos($c[0], $c[1]) | Out-Null; Note "hovering the group ball '$($ball0.Current.Name)'"
      $editor = WaitFor { Popup 'BowserHtmlHover' } 4000 100
      if ($editor) { $field = WaitFor { FindAll ($UIA::FromHandle($editor)) $CT::Edit { param($n) $n -eq 'Group name' } | Select-Object -First 1 } 5000 }
      $current.data.editor_via_ball_hover = [bool]$field
    } else { Note 'no group ball found to hover' }
  }
  if ($field) {
    if (-not $focused) { $c = Center $field; Click $c[0] $c[1]; Start-Sleep -Milliseconds 500; Keys '^a' 200 }
    Keys 'Research' 500
    $typed = $null; try { $typed = $field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value } catch { }
    $current.data.name_field_after_typing = $typed
    if ($focused) {
      Check 'typing straight away (no click in the field) names the group' ($typed -eq 'Research') "field holds '$typed', foreground $([E2E]::Cls([E2E]::GetForegroundWindow()))"
      # So the rest of the flow still runs: name it the way that always worked, a click in the field.
      if ($typed -ne 'Research') { $c = Center $field; Click $c[0] $c[1]; Start-Sleep -Milliseconds 500; Keys '^a' 200; Keys 'Research' 500 }
    }
    Shot 'g05-name-typed'
    $teal = FindAll ($UIA::FromHandle($editor)) $null { param($n) $n -eq 'Teal' } | Select-Object -First 1
    if ($teal) { $c = Center $teal; Click $c[0] $c[1]; Note 'clicked Teal'; Start-Sleep -Milliseconds 900 } else { Note 'no Teal swatch' }
    Shot 'g06-teal-picked'
    $tealNow = FindAll ($UIA::FromHandle($editor)) $null { param($n) $n -eq 'Teal' } | Select-Object -First 1
    $pressed = $null; try { $pressed = $tealNow.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState } catch { }
    $current.data.teal_toggle_after_one_click = "$pressed"
    Check 'one click on Teal, right after typing the name, selects it' ("$pressed" -eq 'On') "Teal toggle state '$pressed'"
    # No Enter here: with the focus on the swatch it would press Teal a second time.
  }
  # Move the pointer to the page so the editor closes (a click there if it is still up).
  $wr = WinRect; [E2E]::SetCursorPos($wr.L + 640, $wr.T + 500) | Out-Null; Start-Sleep -Milliseconds 1200
  if (Popup 'BowserHtmlHover') { Click ($wr.L + 640) ($wr.T + 500); Start-Sleep -Milliseconds 800 }
  $ball = WaitFor { FindAll (Root) $null { param($n) $n -eq 'Research' } | Where-Object { $_.Current.BoundingRectangle.Width -gt 2 } | Select-Object -First 1 } 6000
  Check 'strip shows the group named Research' ([bool]$ball) $(if ($ball) { "ball at $($ball.Current.BoundingRectangle)" } else { 'no element named Research' })
  Shot 'g07-group-made'

  # 1b. Beta: Tab group > Add tab to group > Research
  $opened = TabMenu 'Beta'
  $sub = $opened -and (ClickMenu 'Tab group')
  $addTo = $sub -and (ClickMenu 'Add tab to group')
  Shot 'g08-add-to-group-submenu'
  $picked = $addTo -and (ClickMenu 'Research')
  Check 'menu: Tab group > Add tab to group > Research' $picked "menu $opened, submenu $sub, add-to $addTo"
  Start-Sleep -Milliseconds 1500
  [E2E]::SetCursorPos($wr.L + 640, $wr.T + 500) | Out-Null
  Shot 'g09-two-sites-one-group'
  # Session is saved on a timer and on close: give the timer a moment, then read what is there.
  Start-Sleep -Seconds 6
  $s1 = ParseSession (ReadSession)
  $current.data.session_after_grouping = ReadSession

  # 2a. Ctrl+T on a grouped tab, then give the new tab an address so the session keeps it.
  $alpha = TabNamed 'Alpha'
  if ($alpha) { $c = Center $alpha.el; Click $c[0] $c[1]; Start-Sleep -Milliseconds 800 }
  Front | Out-Null
  $before = @(Tabs).Count
  Keys '^t' 1500
  $after = @(Tabs).Count
  Shot 'g10-ctrl-t'
  Check 'Ctrl+T opened a tab' ($after -eq $before + 1) "tabs $before -> $after"
  Go "$A/site/gamma?t=Gamma%20new%20tab&c=188038"
  $gamma = WaitFor { TabNamed 'Gamma' } 15000
  Start-Sleep -Seconds 2
  [E2E]::SetCursorPos($wr.L + 640, $wr.T + 500) | Out-Null
  Shot 'g11-new-tab-in-group'
  $order = (Tabs | ForEach-Object { $_.name }) -join ' | '
  $current.data.strip_after_ctrl_t = $order
  $ballNow = FindAll (Root) $null { param($n) $n -eq 'Research' } | Select-Object -First 1
  if ($ballNow -and $gamma) {
    $br = $ballNow.Current.BoundingRectangle
    $current.data.ball_x = [int]$br.X
  }
  Start-Sleep -Seconds 5
  $before_close = ReadSession
  $current.data.session_before_close = $before_close

  # 2b. Close Bowser normally and read what it saved.
  $clean = Stop-Bowser 'restart'
  Check 'Bowser closed cleanly' $clean ''
  $saved = ReadSession
  $current.data.session_saved_on_close = $saved
  $ps = ParseSession $saved
  $gid = @($ps.groups.Keys | Where-Object { $ps.groups[$_].name -eq 'Research' }) | Select-Object -First 1
  $members = @($ps.tabs | Where-Object { $gid -and $_.group -eq $gid } | ForEach-Object { $_.title })
  Check 'session.txt has the manual group Research' ([bool]$gid) (($ps.groups.Keys | ForEach-Object { "$_=" + ($ps.groups[$_] | ConvertTo-Json -Compress) }) -join '; ')
  $tealColour = $(if ($gid) { $ps.groups[$gid].colour } else { '' })
  Check 'saved colour is Teal (colour id 4)' ($tealColour -eq '4') "colour '$tealColour'"
  Check 'saved members: Alpha, Beta and the Ctrl+T tab' (($members -join ',') -match 'Alpha' -and ($members -join ',') -match 'Beta' -and ($members -join ',') -match 'Gamma') ($members -join ' | ')

  # 2c. Restart: a plain launch restores the session.
  Start-Bowser $dir @() 'groups-2'
  $restored = WaitFor { $t = Tabs; if (@($t | Where-Object { $_.name -like 'Gamma*' -or $_.name -like 'Alpha*' }).Count -ge 2) { $t } } 30000
  Start-Sleep -Seconds 3
  $ballBack = WaitFor { FindAll (Root) $null { param($n) $n -eq 'Research' } | Where-Object { $_.Current.BoundingRectangle.Width -gt 2 } | Select-Object -First 1 } 10000
  $current.data.strip_after_restart = (Tabs | ForEach-Object { $_.name }) -join ' | '
  Shot 'g12-after-restart'
  Shot 'g12-after-restart-strip' ($(WinRect).L) ($(WinRect).T) 900 90 2
  Check 'after restart the strip shows group Research' ([bool]$ballBack) $current.data.strip_after_restart
  Start-Sleep -Seconds 6
  $clean2 = Stop-Bowser 'end'
  $ps2 = ParseSession (ReadSession)
  $gid2 = @($ps2.groups.Keys | Where-Object { $ps2.groups[$_].name -eq 'Research' }) | Select-Object -First 1
  $members2 = @($ps2.tabs | Where-Object { $gid2 -and $_.group -eq $gid2 } | ForEach-Object { $_.title })
  $current.data.session_after_restart = ReadSession
  Check 'after restart: same group, colour and members saved again' ($gid2 -and $ps2.groups[$gid2].colour -eq '4' -and $members2.Count -eq 3) ("members " + ($members2 -join ' | '))
}

# ================================================================ flow 4: toolbar with many buttons
function Flow-Toolbar {
  Begin-Flow 'toolbar'
  $buttons = 'back,forward,reload,site-info,split,reader,bookmark,newtab,downloads,extensions,cmd:history,cmd:settings,cmd:zoom-in,cmd:zoom-out,cmd:qr-code,cmd:capture-visible,cmd:reopen-tab,cmd:privacy-dashboard,cmd:toggle-sidebar,cmd:passwords,cmd:add-bookmark,cmd:new-window'
  $current.data.toolbar_buttons = $buttons
  foreach ($variant in @('default', 'many')) {
    $settings = @(); if ($variant -eq 'many') { $settings = @("toolbar_buttons=$buttons") }
    $dir = New-DataDir "toolbar-$variant" $settings
    Start-Bowser $dir @("$A/site/toolbar?t=Toolbar%20check&c=8430ce") "toolbar-$variant"
    Start-Sleep -Seconds 6
    foreach ($w in @(1280, 1024)) {
      Place $w 800; Start-Sleep -Seconds 2; Front | Out-Null
      $field = WaitFor { FindAll (Root) $CT::Edit { param($n) $n -like 'Address and search bar*' } | Select-Object -First 1 } 10000
      $fr = $(if ($field) { $field.Current.BoundingRectangle } else { $null })
      $wr = WinRect
      # Buttons in the toolbar row (the row the field is on).
      $row = @()
      if ($fr) { $row = @(FindAll (Root) $CT::Button { param($n) $true } | Where-Object { $r = $_.Current.BoundingRectangle; $r.Width -gt 4 -and [Math]::Abs(($r.Y + $r.Height / 2) - ($fr.Y + $fr.Height / 2)) -lt 22 -and -not $_.Current.IsOffscreen } | ForEach-Object { $r = $_.Current.BoundingRectangle; [pscustomobject]@{ name = $_.Current.Name; x = [int]$r.X; w = [int]$r.Width } } | Sort-Object x) }
      $clipped = @($row | Where-Object { $_.x + $_.w -gt $wr.R -or $_.x -lt $wr.L })
      $key = "${variant}_$w"
      $current.data[$key] = [ordered]@{ window_w = $wr.R - $wr.L; field_x = $(if ($fr) { [int]$fr.X } else { $null }); field_w = $(if ($fr) { [int]$fr.Width } else { $null }); buttons_in_row = $row.Count; button_names = (($row | ForEach-Object { $_.name }) -join ', '); clipped = $clipped.Count }
      Note ("toolbar {0}: field {1}px wide, {2} buttons in the row" -f $key, $current.data[$key].field_w, $row.Count)
      Shot "t-$variant-$w"
      Shot "t-$variant-$w-toolbar" $wr.L $wr.T ($wr.R - $wr.L) 110
      if ($variant -eq 'many') {
        $min = $(if ($w -eq 1280) { 300 } else { 200 })
        Check "address field usable at $w wide with 22 items (>= $min px)" ($fr -and $fr.Width -ge $min) "field $([int]$fr.Width) px, $($row.Count) buttons in the row, $($clipped.Count) outside the window"
      }
    }
    Stop-Bowser | Out-Null
  }
}

# ================================================================ flow 5: 40 tabs and the wheel
function Flow-Strip {
  Begin-Flow 'strip'
  $dir = New-DataDir 'strip' @('automatic_tab_grouping=false')
  $words = 'Apple','Banana','Cherry','Damson','Elder','Fig','Grape','Hazel','Iris','Juniper','Kiwi','Lemon','Mango','Nectar','Olive','Papaya','Quince','Rowan','Sage','Thyme','Ugli','Vanilla','Walnut','Xigua','Yam','Zest','Acorn','Basil','Clove','Dill','Endive','Fennel','Ginger','Hops','Ivy','Jasmine','Kale','Lime','Mint','Nutmeg'
  $colours = 'd93025','1a73e8','188038','f9ab00','8430ce','e8710a','007b83','c5221f','3367d6','0b8043'
  $urls = @(); for ($i = 0; $i -lt 40; $i++) { $urls += "$A/site/t$($i + 1)?t=$([uri]::EscapeDataString($words[$i] + ' ' + ($i + 1)))&c=$($colours[$i % 10])" }
  Start-Bowser $dir @($urls[0]) 'strip'
  Start-Sleep -Seconds 4
  Forward $urls[1..13]; Start-Sleep -Seconds 4
  Forward $urls[14..26]; Start-Sleep -Seconds 4
  Forward $urls[27..39]; Start-Sleep -Seconds 6
  Front | Out-Null
  $all = WaitFor { $t = Tabs; if ($t.Count -ge 40) { $t } } 40000
  $t0 = Tabs
  Check '40 tabs open' ($t0.Count -ge 40) "$($t0.Count) tab items in the strip"
  $wr = WinRect
  $strip = FindAll (Root) $CT::Tab { param($n) $n -eq 'Tabs' } | Select-Object -First 1
  $sr = $(if ($strip) { $strip.Current.BoundingRectangle } else { $null })
  $shown = @($t0 | Where-Object { -not $_.off -and $_.x -ge $wr.L -and ($_.x + $_.w) -le $wr.R })
  $current.data.window = "$($wr.L),$($wr.T) $($wr.R - $wr.L)x$($wr.B - $wr.T)"
  $current.data.strip = $(if ($sr) { "x=$([int]$sr.X) w=$([int]$sr.Width) right=$([int]($sr.X + $sr.Width))" } else { 'not found' })
  $current.data.tabs_before = @($t0 | ForEach-Object { "$($_.name)@$($_.x)+$($_.w)$(if ($_.off) { ' off' })" })
  $minW = $(if ($shown.Count) { ($shown | Measure-Object w -Minimum).Minimum } else { 0 })
  Check 'shown tabs keep at least 52px (icon + letters)' ($shown.Count -gt 0 -and $minW -ge 50) "$($shown.Count) tabs fully in view, narrowest $minW px"
  Check 'some tabs are scrolled out of view (strip scrolls instead of a drop-down)' ($shown.Count -lt $t0.Count) "$($shown.Count) of $($t0.Count) in view"
  # Where the strip ends: with no picker it runs to the caption buttons.
  $caption = FindAll (Root) $CT::Button { param($n) $n -in @('Minimize', 'Minimise', 'Maximize', 'Maximise', 'Close', 'Restore', 'New tab') } | ForEach-Object { "$($_.Current.Name)@$([int]$_.Current.BoundingRectangle.X)" }
  $current.data.strip_neighbours = $caption -join ', '
  Shot 's01-40-tabs'
  Shot 's02-40-tabs-strip-zoom' $wr.L $wr.T ($wr.R - $wr.L) 60 2
  Shot 's02b-40-tabs-strip-right-end' ($wr.R - 420) $wr.T 420 60 3
  # The wheel: over the middle of the strip, a plain vertical wheel.
  $y = $(if ($t0.Count) { $t0[0].y + [int]($t0[0].h / 2) } else { $wr.T + 20 })
  $x = $wr.L + [int](($wr.R - $wr.L) / 2)
  $firstBefore = $t0[0].x; $lastBefore = $t0[-1].x
  Wheel $x $y 8; Start-Sleep -Milliseconds 900
  $t1 = Tabs
  Shot 's03-after-wheel-up'
  Shot 's03b-after-wheel-up-strip-zoom' $wr.L $wr.T ($wr.R - $wr.L) 60 2
  Wheel $x $y -5; Start-Sleep -Milliseconds 900
  $t2 = Tabs
  Shot 's04-after-wheel-down'
  $current.data.first_tab_x = "before $firstBefore, after wheel up $($t1[0].x), after wheel down $($t2[0].x)"
  $current.data.tabs_after_up = @($t1 | Where-Object { -not $_.off } | ForEach-Object { "$($_.name)@$($_.x)" })
  $moved1 = $t1[0].x - $firstBefore; $moved2 = $t2[0].x - $t1[0].x
  Check 'wheel up (plain vertical wheel) scrolls the strip sideways' ([Math]::Abs($moved1) -gt 40) "first tab moved $moved1 px"
  Check 'wheel down scrolls it back the other way' ([Math]::Abs($moved2) -gt 40 -and [Math]::Sign($moved2) -ne [Math]::Sign($moved1)) "first tab moved $moved2 px"
  Stop-Bowser | Out-Null
}

# ================================================================ flow 3: service worker load
function Get-Reports([string]$runLabel) {
  if (-not (Test-Path $reports)) { return @() }
  return @(Get-Content $reports | ForEach-Object { try { $_ | ConvertFrom-Json } catch { } } | Where-Object { $_.label -eq $runLabel })
}
function Wait-Runs([string]$runLabel, [int]$count, [int]$timeoutMs = 120000) {
  $r = WaitFor { $got = @(Get-Reports $runLabel | Where-Object { $_.phase -eq 'run' }); if ($got.Count -ge $count) { $got } } $timeoutMs 500
  return @(Get-Reports $runLabel | Where-Object { $_.phase -eq 'run' })
}
function Summary($runs) {
  if (-not $runs -or $runs.Count -eq 0) { return $null }
  $f = @($runs | ForEach-Object { [double]$_.fetch_ms } | Sort-Object)
  $n = @($runs | ForEach-Object { [double]$_.nav_to_done_ms } | Sort-Object)
  return [ordered]@{ runs = $runs.Count; fetch_ms = ($f -join ', '); fetch_median = $f[[int][Math]::Floor(($f.Count - 1) / 2)]; nav_to_done_ms = ($n -join ', '); nav_to_done_median = $n[[int][Math]::Floor(($n.Count - 1) / 2)]; ok = (($runs | ForEach-Object { $_.ok }) -join ',') }
}
function Flow-ServiceWorker {
  Begin-Flow 'sw'
  $dir = New-DataDir 'sw' @('automatic_tab_grouping=false')
  $solo = "sw-$Label-solo"; $crowd = "sw-$Label-crowded"
  Start-Bowser $dir @("$A/sw/page.html?label=$solo&runs=$Runs&n=$Fetches") 'sw'
  $r1 = Wait-Runs $solo $Runs
  Shot 'w01-sw-alone'
  $current.data.alone = Summary $r1
  $installed = @(Get-Reports $solo | Where-Object { $_.phase -eq 'installed' })
  Check 'worker installed and controls the page' ($installed.Count -ge 1 -and $installed[0].controlled) ($installed | ConvertTo-Json -Compress)
  Check "alone: $Runs controlled loads of $Fetches worker fetches" ($r1.Count -ge $Runs) ($current.data.alone | ConvertTo-Json -Compress)
  # Fifteen more live tabs (each forwarded address opens as a live foreground tab).
  $fill = @(); for ($i = 1; $i -le 15; $i++) { $fill += "$A/site/filler${i}?t=Filler%20${i}&c=5f6368" }
  Forward $fill[0..7]; Start-Sleep -Seconds 3
  Forward $fill[8..14]; Start-Sleep -Seconds 5
  $count = @(Tabs).Count
  Check '16 tabs open' ($count -ge 16) "$count tabs"
  $sw = TabNamed 'SW '
  if ($sw) { $c = Center $sw.el; Click $c[0] $c[1]; Start-Sleep -Milliseconds 800 }
  Front | Out-Null
  Go "$A/sw/page.html?label=$crowd&runs=$Runs&n=$Fetches"
  $r2 = Wait-Runs $crowd $Runs
  Shot 'w02-sw-with-15-tabs'
  $current.data.with_15_tabs = Summary $r2
  Check "with 15 other tabs: $Runs controlled loads completed" ($r2.Count -ge $Runs) ($current.data.with_15_tabs | ConvertTo-Json -Compress)
  if ($r1.Count -and $r2.Count) {
    $ratio = [Math]::Round($current.data.with_15_tabs.fetch_median / [Math]::Max(1, $current.data.alone.fetch_median), 2)
    $current.data.crowded_over_alone = $ratio
    Check 'with 15 tabs the worker fetches stay within 3x of alone' ($ratio -le 3) "median $($current.data.with_15_tabs.fetch_median) ms vs $($current.data.alone.fetch_median) ms (x$ratio)"
  }

  # Bonus: Gmail itself, signed out, in a new tab.
  Front | Out-Null
  Keys '^t' 1200
  $clock = [Diagnostics.Stopwatch]::StartNew()
  Go 'https://mail.google.com/'
  $sawStop = $null; $doneAt = $null; $title = ''
  while ($clock.ElapsedMilliseconds -lt 60000) {
    $stop = FindAll (Root) $CT::Button { param($n) $n -eq 'Stop' } | Select-Object -First 1
    if ($stop -and -not $sawStop) { $sawStop = $clock.ElapsedMilliseconds }
    if (-not $stop -and $sawStop -and $clock.ElapsedMilliseconds -gt 1500) {
      $field = FindAll (Root) $null { param($n) $n -like 'Email or phone*' -or $n -like 'Sign in*' } | Select-Object -First 1
      $doneAt = $clock.ElapsedMilliseconds; break
    }
    Start-Sleep -Milliseconds 200
  }
  Start-Sleep -Seconds 2
  $active = (Tabs | Where-Object { $_.name -notlike 'Filler*' -and $_.name -notlike 'SW *' } | ForEach-Object { $_.name }) -join ' | '
  $signin = FindAll (Root) $null { param($n) $n -like 'Email or phone*' } | Select-Object -First 1
  $current.data.gmail = [ordered]@{ loading_seen_ms = $sawStop; stopped_loading_ms = $doneAt; tab_titles = $active; email_field = [bool]$signin }
  Shot 'w03-gmail-signed-out'
  Check 'mail.google.com (signed out) finishes loading within 30 s' ($doneAt -and $doneAt -lt 30000) ($current.data.gmail | ConvertTo-Json -Compress)
  Stop-Bowser | Out-Null
}

# ================================================================ run
try {
  foreach ($flow in ($Flows -split ',')) {
    try {
      switch ($flow.Trim()) {
        'groups' { Flow-Groups }
        'toolbar' { Flow-Toolbar }
        'strip' { Flow-Strip }
        'sw' { Flow-ServiceWorker }
        default { Note "unknown flow $flow"; continue }
      }
    } catch {
      Note "flow $flow stopped: $($_.Exception.Message) at $($_.InvocationInfo.ScriptLineNumber)"
      if ($current) { $current.status = 'ERROR'; $current.error = "$($_.Exception.Message) (line $($_.InvocationInfo.ScriptLineNumber))" }
      try { Shot "error-$flow" } catch { }
      Stop-Bowser 'error' | Out-Null
    }
    End-Flow
  }
} finally {
  Stop-Bowser 'final' | Out-Null
  Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
  Save-Results
  Note 'done'
}
