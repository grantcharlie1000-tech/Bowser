# Native, ordinary-profile baseline. Requires interactive Windows and HTML-default binaries.
# No --benchmark-* arguments, no remote-debugging, no user profile, no disabled security features.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BaselineExe,
    [Parameter(Mandatory)][string]$DiagnosticExe,
    [Parameter(Mandatory)][string]$ReleaseExe,
    [Parameter(Mandatory)][string]$Out,
    [ValidateRange(1,30)][int]$Pairs=5,
    [ValidateSet('light','demanding','redirect','filter-parity')][string[]]$Fixtures=@('light','demanding','redirect'),
    [ValidateRange(1,20)][int]$MaxInvalidSamples=2,
    [string]$DiagnosticSource='8a60dca2ac7de6d369c9b65584fd621a7013f491',
    [switch]$IncludeTabFlow,
    [switch]$IncludeSurfaceSmoke,
    [ValidateSet('sendkeys','sendkeys-per-key','uia-value')][string]$SurfaceInputMethod='sendkeys',
    [switch]$DiagnosticAutoUpdateOff,
    [string]$ExpectedInputManifest,
    [switch]$PositiveOnly,
    [switch]$DiagnosticDetailPilot,
    [switch]$DiagnosticInteractionPilot,
    [switch]$DiagnosticNativeFocus,
    [switch]$DiagnosticControllerFocus,
    [switch]$DiagnosticFocusTopology,
    [switch]$SidebarLifecyclePilot,
    [switch]$SidebarCapabilityPreflight,
    [switch]$SidebarRouteSmoke,
    [ValidateSet('settled','rapid')][string]$SidebarScenario='settled',
    [switch]$StartupOnly,
    [string]$CandidateExe,
    [string]$CandidateSource,
    [ValidateSet('timing','resource')][string]$MeasurementMode='resource',
    [ValidateRange(10,20)][int]$ReadinessPollMilliseconds=15,
    [ValidateRange(1,30)][int]$IdleSeconds=3,
    [ValidateRange(100,2000)][int]$SampleMilliseconds=250,
    [string]$BaselineSource='80eabd7ae6d421e61c81985c34023bfc824a2e58'
)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/measurements.psm1" -Force
Import-Module "$PSScriptRoot/production-measurements.psm1" -Force
if (-not $IsWindows) { throw 'Native Windows required' }
if($DiagnosticAutoUpdateOff -and -not $ExpectedInputManifest){throw 'Updater-off diagnostic requires the original frozen-input manifest'}
if($SurfaceInputMethod -ne 'sendkeys' -and -not $IncludeSurfaceSmoke){throw 'UIA value control requires the independent surface lane'}
if($StartupOnly -and $IncludeTabFlow){throw 'StartupOnly cannot be combined with IncludeTabFlow'}
if($DiagnosticInteractionPilot -and (-not $CandidateExe -or -not $IncludeSurfaceSmoke -or -not $StartupOnly)){throw 'DiagnosticInteractionPilot requires CandidateExe, IncludeSurfaceSmoke and StartupOnly'}
if($DiagnosticNativeFocus -and (-not $DiagnosticInteractionPilot -or -not $DiagnosticAutoUpdateOff -or $SurfaceInputMethod -ne 'uia-value')){throw 'Native focus observation requires isolated interaction pilot with ordinary updater OFF and unchanged UIA value input'}
if($DiagnosticControllerFocus -and -not $DiagnosticNativeFocus){throw 'Controller focus coverage requires the passive OS focus diagnostic lane'}
if($DiagnosticFocusTopology -and -not $DiagnosticControllerFocus){throw 'Topology diagnostic requires native/controller focus coverage'}
if($SidebarLifecyclePilot -and (-not $CandidateExe -or -not $StartupOnly -or -not $DiagnosticAutoUpdateOff -or $IncludeSurfaceSmoke -or $IncludeTabFlow -or $DiagnosticInteractionPilot -or $DiagnosticDetailPilot)){throw 'Sidebar pilot requires candidate, startup-only, ordinary updater OFF; no mixed surface/input/detail workload'}
if($SidebarRouteSmoke -and (-not $SidebarLifecyclePilot -or $SidebarCapabilityPreflight -or $Pairs -ne 1 -or -not $PositiveOnly -or $Fixtures.Count -ne 1 -or $Fixtures[0] -cne 'light' -or $MeasurementMode -cne 'timing' -or $SidebarScenario -cne 'settled')){throw 'Sidebar route smoke requires one positive light timing-only sidebar pair, fresh/reused, no mixed preflight/rapid/resource lane'}
if($SidebarCapabilityPreflight -and (-not $SidebarLifecyclePilot -or $Pairs -ne 1 -or -not $PositiveOnly -or $Fixtures.Count -ne 1 -or $Fixtures[0] -cne 'light' -or $MeasurementMode -cne 'timing')){throw 'Passive sidebar preflight requires one positive light timing-only sidebar pair; exactly one fresh profile, no action/resource lane'}
if($SidebarCapabilityPreflight){Import-Module "$PSScriptRoot/sidebar-preflight.psm1" -Force}
if($SidebarLifecyclePilot){Import-Module "$PSScriptRoot/sidebar-lifecycle.psm1" -Force}
if($DiagnosticNativeFocus){Import-Module "$PSScriptRoot/production-focus.psm1" -Force}
$Out=[IO.Path]::GetFullPath($Out)
New-Item -ItemType Directory -Force $Out | Out-Null
$contaminationMarker=Join-Path ([IO.Directory]::GetParent($Out).FullName) 'native-isolation-blocked.json'
if(Test-Path $contaminationMarker){throw 'Prior native lane has unconfirmed process isolation; no new native process launched'}
$run=Join-Path $Out 'isolated'
New-Item -ItemType Directory -Force $run | Out-Null
$arms=[ordered]@{release=(Resolve-BowserExecutable $ReleaseExe);baseline=(Resolve-BowserExecutable $BaselineExe);diagnostic_off=(Resolve-BowserExecutable $DiagnosticExe);diagnostic_on=(Resolve-BowserExecutable $DiagnosticExe)}
$inputPaths=@($BaselineExe,$DiagnosticExe,$ReleaseExe)
if ($CandidateExe) {
    if ($CandidateSource -notmatch '^[0-9a-f]{40}$') { throw 'Candidate source must be its exact40-character commit SHA' }
    $arms=[ordered]@{baseline_on=(Resolve-BowserExecutable $DiagnosticExe);candidate_on=(Resolve-BowserExecutable $CandidateExe)}
    if($DiagnosticDetailPilot) { $arms['candidate_detail_on']=(Resolve-BowserExecutable $CandidateExe) }
    if($DiagnosticInteractionPilot){$arms['baseline_interaction_on']=(Resolve-BowserExecutable $DiagnosticExe);$arms['candidate_interaction_on']=(Resolve-BowserExecutable $CandidateExe)}
    if($SidebarLifecyclePilot){$arms=[ordered]@{sidebar_off=(Resolve-BowserExecutable $CandidateExe);sidebar_on=(Resolve-BowserExecutable $CandidateExe)}}
    if($SidebarRouteSmoke){$arms=[ordered]@{sidebar_route_on=(Resolve-BowserExecutable $CandidateExe)}}
    if($SidebarCapabilityPreflight){$arms=[ordered]@{sidebar_preflight_on=(Resolve-BowserExecutable $CandidateExe)}}
    $inputPaths+=@($CandidateExe)
}
$hashes=Get-MeasurementInputHashes $inputPaths
if($ExpectedInputManifest) {
    $manifest=Get-Content -LiteralPath $ExpectedInputManifest -Raw | ConvertFrom-Json
    $roles=[ordered]@{baseline=$BaselineExe;diagnostic=$DiagnosticExe;release=$ReleaseExe}
    if($CandidateExe){$roles['candidate']=$CandidateExe}
    $hashes=[ordered]@{}
    foreach($role in $roles.Keys) {
        $property=$manifest.PSObject.Properties[$role]
        if(-not $property -or $property.Value -cnotmatch '^[0-9a-f]{64}$'){throw "Original manifest missing valid hash: $role"}
        $hashes[(Resolve-Path -LiteralPath $roles[$role]).Path]=$property.Value
    }
    try {Assert-MeasurementInputHashesUnchanged $hashes (Get-MeasurementInputHashes $inputPaths) | Out-Null}
    catch {[ordered]@{reason='Pinned manifest mismatch before lane start; no native process launched';error=$_.Exception.Message} | ConvertTo-Json | Set-Content $contaminationMarker -Encoding utf8;throw}
}
$expected='341486070d2e74cf9a9e5d5fbeffa90c7dd2ec4fd36bc284a8a3878a7a9bcec1'
if ((Get-FileHash $ReleaseExe -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expected) { throw 'Released executable does not match verified v0.0.19 pin' }
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class BowserProductionNative {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent,EnumProc callback,IntPtr param);
 public delegate bool EnumProc(IntPtr window,IntPtr param);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback,IntPtr param);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder text,int count);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder text,int count);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint message,IntPtr w,IntPtr l);
 public static IntPtr FindHost(uint pid) {
  IntPtr found=IntPtr.Zero;
  EnumWindows((window,param)=>{uint owner;GetWindowThreadProcessId(window,out owner);var cls=new StringBuilder(128);GetClassName(window,cls,cls.Capacity);if(owner==pid && cls.ToString()=="BowserNativeWindow"){found=window;return false;}return true;},IntPtr.Zero);
  return found;
 }
 public static string Title(IntPtr h) {var text=new StringBuilder(2048);GetWindowText(h,text,text.Capacity);return text.ToString();}
 public static IntPtr FindSurface(uint pid,string name,int minWidth,int minHeight) {
  IntPtr found=IntPtr.Zero;
  EnumProc visit=(h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);var cls=new StringBuilder(128);GetClassName(h,cls,cls.Capacity);RECT r;
   if(owner==pid && IsWindowVisible(h) && cls.ToString()==name && GetWindowRect(h,out r) && r.Right-r.Left>=minWidth && r.Bottom-r.Top>=minHeight){found=h;return false;} return true;};
  EnumWindows(visit,IntPtr.Zero);
  if(found==IntPtr.Zero)EnumChildWindows(FindHost(pid),visit,IntPtr.Zero);
  return found;
 }
 public static bool Close(uint pid) {var h=FindHost(pid);return h!=IntPtr.Zero && PostMessage(h,0x0010,IntPtr.Zero,IntPtr.Zero);}

 [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int command);
}
'@
$saved=@{}
foreach($name in @('LOCALAPPDATA','BOWSER_DATA_DIR','BOWSER_UI','BOWSER_UPDATE_URL','BOWSER_UPDATE_PUBKEY')) { $saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process'); [Environment]::SetEnvironmentVariable($name,$null,'Process') }
$env:LOCALAPPDATA=Join-Path $run 'local-app-data'
New-Item -ItemType Directory -Force $env:LOCALAPPDATA | Out-Null
$portFile=Join-Path $run 'port.txt'; $requestFile=Join-Path $run 'requests.jsonl'
$script:profileTimestamp=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$invalidSamples=0
$server=$null; $app=$null; $rows=[Collections.Generic.List[object]]::new()
function Qpc { [Diagnostics.Stopwatch]::GetTimestamp() }
function Observe-NativeFocus([string]$boundary) {
    if(-not $DiagnosticNativeFocus){return}
    $outerStart=Qpc
    $root=[BowserProductionNative]::FindHost([uint32]$app.Id)
    if($DiagnosticFocusTopology -and $boundary -eq 'ctrl-l-before-foreground'){$script:originalFocus=$null}
    $topologyArgs=@{}
    if($DiagnosticFocusTopology){$topologyArgs=@{IncludeTopology=$true;OriginalFocus=$script:originalFocus}}
    $record=Get-ProductionNativeFocusSnapshot -RootPid ([uint32]$app.Id) -RootWindow $root -Boundary $boundary -Attempt $script:addressAttempt @topologyArgs
    if($DiagnosticFocusTopology -and $boundary -eq 'ctrl-l-after-keys' -and $record.native.query_ok){$script:originalFocus=$record.native.focus}
    $outerEnd=Qpc
    $record['outer_start_qpc']=$outerStart;$record['outer_end_qpc']=$outerEnd
    $record['lookup_and_snapshot_ms']=($outerEnd-$outerStart)*1000.0/[Diagnostics.Stopwatch]::Frequency
    $record['duration_scope']='Outer interval includes owned-root lookup and snapshot call; inner observation_ms retains API/wrapper interval; record append/caller dispatch not included; no overhead subtraction'
    $script:nativeFocusTrace.Add($record)
}
function Owned-Keys([string]$keys,[string]$focusBoundary='') {
    # Process.MainWindowHandle can be a cached/transient popup handle. Select the exact native host.
    $deadline=[DateTime]::UtcNow.AddSeconds(3); $window=[IntPtr]::Zero; $owner=[uint32]0
    do {
        $app.Refresh(); if ($app.HasExited) { throw "Owned process exited before input: $keys" }
        $window=[BowserProductionNative]::FindHost([uint32]$app.Id)
        if ($window -ne [IntPtr]::Zero -and [BowserProductionNative]::IsWindowVisible($window)) { break }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    [void][BowserProductionNative]::GetWindowThreadProcessId($window,[ref]$owner)
    if ($window -eq [IntPtr]::Zero -or $owner -ne $app.Id -or -not [BowserProductionNative]::IsWindowVisible($window)) { throw 'Owned visible BowserNativeWindow unavailable; no global input sent' }
    if($focusBoundary){Observe-NativeFocus "$focusBoundary-before-foreground"}
    [void][BowserProductionNative]::SetForegroundWindow($window)
    Start-Sleep -Milliseconds 80
    if ([BowserProductionNative]::GetForegroundWindow() -ne $window) { throw 'Foreground unavailable; no global input sent' }
    if($focusBoundary){Observe-NativeFocus "$focusBoundary-before-keys"}
    [Windows.Forms.SendKeys]::SendWait($keys)
    if($focusBoundary){Observe-NativeFocus "$focusBoundary-after-keys"}
}
function Capture-OwnedSurface([string]$name,[IntPtr]$window=[IntPtr]::Zero) {
    if($window -eq [IntPtr]::Zero){$window=[BowserProductionNative]::FindHost([uint32]$app.Id)}
    $owner=[uint32]0; [void][BowserProductionNative]::GetWindowThreadProcessId($window,[ref]$owner)
    if($owner -ne $app.Id -or -not [BowserProductionNative]::IsWindowVisible($window)){throw 'Screenshot window is not owned and visible'}
    $rect=[BowserProductionNative+RECT]::new()
    if(-not [BowserProductionNative]::GetWindowRect($window,[ref]$rect)){throw 'Screenshot rectangle unavailable'}
    $bitmap=[Drawing.Bitmap]::new($rect.Right-$rect.Left,$rect.Bottom-$rect.Top)
    $graphics=[Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size)
        $path=Join-Path $Out "$id-$name.png"
        $bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png)
        return [ordered]@{file=[IO.Path]::GetFileName($path);sha256=(Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant();method='owned-window desktop capture; synthetic isolated profile'}
    } finally {$graphics.Dispose();$bitmap.Dispose()}
}
function Read-OwnedAddress {
    $window=[BowserProductionNative]::FindHost([uint32]$app.Id)
    $root=[Windows.Automation.AutomationElement]::FromHandle($window)
    $condition=[Windows.Automation.AndCondition]::new(
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Address and search bar'),
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Edit))
    $edit=$root.FindFirst([Windows.Automation.TreeScope]::Descendants,$condition)
    $focused=[Windows.Automation.AutomationElement]::FocusedElement
    $same=$edit -and $focused -and (($edit.GetRuntimeId() -join ',') -ceq ($focused.GetRuntimeId() -join ','))
    $value=if($edit){([Windows.Automation.ValuePattern]$edit.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)).Current.Value}else{$null}
    $selection=$null
    if($edit){try{$ranges=([Windows.Automation.TextPattern]$edit.GetCurrentPattern([Windows.Automation.TextPattern]::Pattern)).GetSelection();$selection=@($ranges|ForEach-Object {$_.GetText(-1)})}catch{}}
    return [ordered]@{selection=$selection;element=$edit;focused=[bool]$same;value=$value;focus_name=if($focused){$focused.Current.Name}else{''};provider_process_id=if($focused){$focused.Current.ProcessId}else{0}}
}
function Assert-FrozenInputs($sample,[string]$phase) {
    try {
        Assert-MeasurementInputHashesUnchanged $hashes (Get-MeasurementInputHashes $inputPaths) | Out-Null
        $sample.input_identity_checks+=@([ordered]@{phase=$phase;passed=$true;qpc=(Qpc)})
    } catch {
        $sample.valid=$false;$sample.identity_error=$_.Exception.Message
        $sample.input_identity_checks+=@([ordered]@{phase=$phase;passed=$false;qpc=(Qpc);error=$sample.identity_error})
        if(-not $sample.error){$sample.error=$sample.identity_error}
        [ordered]@{id=$sample.id;reason='Frozen executable identity changed; no subsequent native lane permitted';phase=$phase;error=$sample.identity_error} | ConvertTo-Json | Set-Content $contaminationMarker -Encoding utf8
        throw
    }
}
function Confirm-NoStagedUpdate([string]$profile) {
    if(-not $DiagnosticAutoUpdateOff){return}
    $setting=Join-Path $profile 'auto-update.txt'
    if((Get-Content -LiteralPath $setting -Raw).Trim() -cne 'auto_update=false'){throw 'Updater-off diagnostic switch missing or modified'}
    $updates=Join-Path $profile 'updates'
    $files=if(Test-Path -LiteralPath $updates){@(Get-ChildItem -LiteralPath $updates -Recurse -File -ErrorAction Stop)}else{@()}
    $staged=@($files | Where-Object {$_.Name -in @('pending.txt','trial.txt') -or $_.Extension -in @('.exe','.download')})
    if($staged.Count){throw 'Pending/trial/staged update present; disposable control profile rejected, not cleaned or reused'}
}
function Read-InputModifiers {
    $result=[ordered]@{}
    foreach($key in @(@('shift',16),@('control',17),@('alt',18),@('left_windows',91),@('right_windows',92))) {
        $result[$key[0]]=([BowserProductionNative]::GetAsyncKeyState([int]$key[1]) -lt 0)
    }
    return $result
}
function Submit-OwnedAddress([string]$text) {
    if($DiagnosticNativeFocus){$script:addressAttempt++}
    Owned-Keys '^l' 'ctrl-l'
    $wait=[Diagnostics.Stopwatch]::StartNew();$state=$null
    do {$state=Read-OwnedAddress;if($state.focused){break};Start-Sleep -Milliseconds 20}while($wait.Elapsed.TotalSeconds -lt 3)
    if(-not $state.focused){throw "Omnibox focus not confirmed before typing; observed=$($state.focus_name)"}
    $trace=[ordered]@{surface='semantic-address-input';method=$SurfaceInputMethod;expected_value=$text;before_value=$state.value;before_selection=$state.selection;before_modifiers=(Read-InputModifiers);value_transitions=@();focus_name=$state.focus_name;provider_process_id=$state.provider_process_id;owned_address_focused=$state.focused;value_matches_expected=$false;qpc=(Qpc)}
    $script:surfaceTrace.Add($trace)
    if($SurfaceInputMethod -eq 'uia-value') {
        # Separate navigation/first-use CONTROL. It bypasses keystrokes, but may dispatch DOM
        # input/change and autocomplete; it
        # can NEVER establish keyboard correctness or keyboard latency.
        $pattern=[Windows.Automation.ValuePattern]$state.element.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
        if($pattern.Current.IsReadOnly){throw 'Owned address ValuePattern is read-only'}
        Observe-NativeFocus 'set-value-before'
        $pattern.SetValue($text)
        Observe-NativeFocus 'set-value-after'
    } elseif($SurfaceInputMethod -eq 'sendkeys-per-key') {
        # Predeclared real keystroke control: at least30ms plus UIA work per character, no
        # modifier reset, text repair or retry. It still exercises autocomplete.
        $keys=[Collections.Generic.List[object]]::new();$trace['per_key']=$keys
        foreach($character in $text.ToCharArray()) {
            $state=Read-OwnedAddress;$modifiers=Read-InputModifiers
            $key=[ordered]@{qpc=(Qpc);character=[string]$character;before_value=$state.value;before_selection=$state.selection;focused=$state.focused;modifiers=$modifiers}
            $keys.Add($key)
            if(-not $state.focused -or @($modifiers.Values|Where-Object {$_}).Count){throw 'Per-key diagnostic requires owned focus and neutral modifiers; no repair attempted'}
            if([BowserProductionNative]::GetForegroundWindow() -ne [BowserProductionNative]::FindHost([uint32]$app.Id)){throw 'Per-key foreground lost; no further input sent'}
            [Windows.Forms.SendKeys]::SendWait([string]$character)
            Start-Sleep -Milliseconds 30
            $after=Read-OwnedAddress;$key['after_value']=$after.value;$key['after_selection']=$after.selection;$key['after_modifiers']=Read-InputModifiers
        }
        $trace.per_key=@($keys.ToArray())
    } else { Owned-Keys $text }
    $trace['after_injection_modifiers']=Read-InputModifiers
    $transitions=[Collections.Generic.List[object]]::new();$previous=$null
    $wait=[Diagnostics.Stopwatch]::StartNew()
    do {
        $state=Read-OwnedAddress
        $identity="$($state.focused):$($state.value)"
        if($identity -cne $previous){$transitions.Add([ordered]@{qpc=(Qpc);value=$state.value;selection=$state.selection;focused=$state.focused;focus_name=$state.focus_name;modifiers=(Read-InputModifiers)});$previous=$identity}
        if($state.focused -and $state.value -ceq $text){break};Start-Sleep -Milliseconds 20
    }while($wait.Elapsed.TotalSeconds -lt 3)
    $trace.value_transitions=@($transitions.ToArray());$trace.focus_name=$state.focus_name;$trace.provider_process_id=$state.provider_process_id;$trace.owned_address_focused=$state.focused;$trace.value_matches_expected=($state.value -ceq $text);$trace.qpc=Qpc
    Observe-NativeFocus 'uia-assertion-after'
    if(-not $state.focused -or $state.value -cne $text){throw 'Omnibox exact value/focus not confirmed; Enter was not sent'}
    $window=[BowserProductionNative]::FindHost([uint32]$app.Id)
    if([BowserProductionNative]::GetForegroundWindow() -ne $window){throw 'Owned foreground lost before semantically verified Enter; Enter was not sent'}
    Observe-NativeFocus 'enter-before'
    $start=Qpc;[Windows.Forms.SendKeys]::SendWait('{ENTER}')
    Observe-NativeFocus 'enter-after'
    return $start
}
function Submit-OwnedFixture([string]$fixtureToken,[string]$purpose) {
    $start=Submit-OwnedAddress "$base/light?token=$fixtureToken"
    $wait=[Diagnostics.Stopwatch]::StartNew();$ack=$false;$ownedTitle=$false
    do {
        $ack=Has-Ready $fixtureToken
        $window=[BowserProductionNative]::FindHost([uint32]$app.Id)
        $ownedTitle=([BowserProductionNative]::Title($window)).Contains("BOWSER_PERF_READY_$fixtureToken")
        if($ack -and $ownedTitle){break};Start-Sleep -Milliseconds 20
    }while($wait.Elapsed.TotalSeconds -lt 20)
    $script:surfaceTrace.Add([ordered]@{surface='semantic-fixture-navigation';purpose=$purpose;token=$fixtureToken;start_qpc=$start;end_qpc=(Qpc);ready=$ack;owned_title_matches=$ownedTitle;native_navigation_success=$null;uia_conditioned_enter_to_ack_title_proxy_ms=((Qpc)-$start)*1000.0/[Diagnostics.Stopwatch]::Frequency})
    if(-not $ack -or -not $ownedTitle){throw 'Semantically submitted fixture lacks unique acknowledgement and owned title'}
}
function Confirm-NativeCoreSurfaces {
    # Validity only: enabling accessibility can change renderer resource use.
    Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
    $window=[BowserProductionNative]::FindHost([uint32]$app.Id)
    $root=[Windows.Automation.AutomationElement]::FromHandle($window)
    $matches=[Collections.Generic.List[object]]::new()
    foreach($check in @(@{name='Tabs';type=[Windows.Automation.ControlType]::Tab},@{name='Left rail';type=[Windows.Automation.ControlType]::ToolBar})) {
        $byName=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$check.name)
        $byType=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,$check.type)
        $condition=[Windows.Automation.AndCondition]::new($byName,$byType)
        $wait=[Diagnostics.Stopwatch]::StartNew();$found=$null
        do {
            $node=$root.FindFirst([Windows.Automation.TreeScope]::Descendants,$condition)
            if($node){
                $current=$node.Current;$rect=$current.BoundingRectangle
                if(-not $current.IsOffscreen -and $rect.Width -gt 10 -and $rect.Height -gt 10){$found=$current;break}
            }
            Start-Sleep -Milliseconds 50
        } while($wait.Elapsed.TotalSeconds -lt 10)
        if(-not $found){throw "Native HTML UIA confirmation unavailable: $($check.name); validate provider/fixture on frozen baseline before attributing a regression"}
        $rect=$found.BoundingRectangle
        $matches.Add([ordered]@{name=$found.Name;control_type=$found.ControlType.ProgrammaticName;offscreen=$found.IsOffscreen;bounds=@{left=$rect.Left;top=$rect.Top;width=$rect.Width;height=$rect.Height};provider_process_id=$found.ProcessId})
    }
    return [ordered]@{surface='core-tabs-rail';method='UIA rooted at exact owned native window; descendant provider PID may differ';matches=@($matches.ToArray());screenshot=(Capture-OwnedSurface 'core-html')}
}
function Native-SurfaceSmoke {
    $results=$script:surfaceTrace
    $results.Add((Confirm-NativeCoreSurfaces))
    Submit-OwnedFixture ([guid]::NewGuid().ToString('N')) 'before-popup-settings'
    foreach($use in @('first','warm')) {
        $start=Qpc; Owned-Keys '^+m'
        $wait=[Diagnostics.Stopwatch]::StartNew();$surface=[IntPtr]::Zero
        do {
            $surface=[BowserProductionNative]::FindSurface([uint32]$app.Id,'BowserHtmlPopup',100,40)
            if($surface -ne [IntPtr]::Zero){break};Start-Sleep -Milliseconds 20
        } while($wait.Elapsed.TotalSeconds -lt 10)
        if($surface -eq [IntPtr]::Zero){throw "Native HTML popup $use open failed or fell back"}
        $elapsed=((Qpc)-$start)*1000.0/[Diagnostics.Stopwatch]::Frequency
        Start-Sleep -Milliseconds 200
        $results.Add([ordered]@{surface='popup';use=$use;visible_host_proxy_ms=$elapsed;includes_foreground_verification=$true;screenshot=(Capture-OwnedSurface "popup-$use" $surface)})
        Owned-Keys '{ESC}'
        $wait=[Diagnostics.Stopwatch]::StartNew()
        while([BowserProductionNative]::FindSurface([uint32]$app.Id,'BowserHtmlPopup',100,40) -ne [IntPtr]::Zero -and $wait.Elapsed.TotalSeconds -lt 3){Start-Sleep -Milliseconds 20}
        if([BowserProductionNative]::FindSurface([uint32]$app.Id,'BowserHtmlPopup',100,40) -ne [IntPtr]::Zero){throw 'Native HTML popup did not dismiss'}
        # Preserve the product's intentional300ms same-kind reopen guard, outside the open timer.
        Start-Sleep -Milliseconds 350
    }
    foreach($use in @('first','warm')) {
        $start=Submit-OwnedAddress 'bowser://settings'
        $wait=[Diagnostics.Stopwatch]::StartNew();$surface=[IntPtr]::Zero
        do {
            # This container is only expanded/shown after its HTML bridge has sent ready.
            $surface=[BowserProductionNative]::FindSurface([uint32]$app.Id,'BowserSettingsPage',200,100)
            if($surface -ne [IntPtr]::Zero){break};Start-Sleep -Milliseconds 20
        } while($wait.Elapsed.TotalSeconds -lt 15)
        if($surface -eq [IntPtr]::Zero){throw "Native HTML internal Settings $use open failed or fell back"}
        $elapsed=((Qpc)-$start)*1000.0/[Diagnostics.Stopwatch]::Frequency
        Start-Sleep -Milliseconds 200
        $results.Add([ordered]@{surface='settings';use=$use;visible_host_proxy_ms=$elapsed;includes_foreground_verification=$false;screenshot=(Capture-OwnedSurface "settings-$use")})
        $backToken=[guid]::NewGuid().ToString('N')
        Submit-OwnedFixture $backToken "settings-$use-return"
        $wait=[Diagnostics.Stopwatch]::StartNew()
        while([BowserProductionNative]::FindSurface([uint32]$app.Id,'BowserSettingsPage',200,100) -ne [IntPtr]::Zero -and $wait.Elapsed.TotalSeconds -lt 3){Start-Sleep -Milliseconds 20}
        if([BowserProductionNative]::FindSurface([uint32]$app.Id,'BowserSettingsPage',200,100) -ne [IntPtr]::Zero){throw 'Inactive Settings remained visibly expanded'}
    }
    return @($results.ToArray())
}
function Confirm-OwnedTreeDrain($known,[string]$profile,$sample) {
    $drain=[Diagnostics.Stopwatch]::StartNew()
    do {
        $inventory=@(Get-ProcessInventory)
        if(-not $inventory.Count){throw 'Global process inventory empty/unavailable; process exit unproven'}
        # A negative traversal sentinel cannot seed Windows PID0/System ancestry.
        $remaining=@(Get-ProcessGroupMembers -RootPid -1 -Profile $profile -Inventory $inventory)
        foreach($process in $remaining){$known["$($process.Pid):$($process.CreationTicks)"]=$true}
        $remaining+=@($inventory | Where-Object {$known.ContainsKey("$($_.Pid):$($_.CreationTicks)")})
        if(-not $remaining.Count){break}
        Start-Sleep -Milliseconds 100
    } while($drain.Elapsed.TotalSeconds -lt 20)
    $sample.tree_exit_wait_ms=$drain.Elapsed.TotalMilliseconds
    $sample.tree_exit_confirmed=($remaining.Count -eq 0 -and $sample.ownership_inventory_captured)
    $sample.drain_remaining=@($remaining | ForEach-Object {[ordered]@{pid=$_.Pid;parent=$_.Parent;name=$_.Name;created_ticks=$_.CreationTicks;command_line_contains_profile=([bool]$_.CommandLine -and $_.CommandLine.IndexOf($profile,[StringComparison]::OrdinalIgnoreCase) -ge 0)}})
}
function Disk-Bytes([string]$path) { $files=@(Get-ChildItem -LiteralPath $path -File -Recurse -Force -ErrorAction SilentlyContinue); if (-not $files.Count) { return 0 }; return ($files | Measure-Object Length -Sum).Sum }
function Has-Ready([string]$token) {
    if (-not (Test-Path $requestFile)) { return $false }
    return [bool](@(Get-Content -LiteralPath $requestFile | ConvertFrom-Json | Where-Object {$_.path -eq '/ready' -and $_.token -ceq $token}).Count)
}
function Seed-Profile([string]$path,[int]$bookmarks) {
    New-Item -ItemType Directory -Force $path | Out-Null
    if($DiagnosticAutoUpdateOff){'auto_update=false' | Set-Content (Join-Path $path 'auto-update.txt') -Encoding utf8NoBOM}
    # Fixed, documented synthetic settings. All security, privacy, downloads, blocker defaults remain on.
    "startup=start_page`n" | Set-Content (Join-Path $path 'settings.txt') -Encoding utf8NoBOM
    'answered' | Set-Content (Join-Path $path 'import-offer.txt') -Encoding utf8NoBOM
    $history=[Text.StringBuilder]::new()
    $now=$script:profileTimestamp
    for($i=0;$i -lt 10000;$i++) { [void]$history.AppendLine("$($now-$i)`thttps://history.example.test/item/$i`tSynthetic history item $i") }
    [IO.File]::WriteAllText((Join-Path $path 'history.txt'),$history.ToString())
    $marks=[Text.StringBuilder]::new("bowser-bookmarks-v2`n")
    for($i=1;$i -le $bookmarks;$i++) { [void]$marks.AppendLine("$i`t-`tB`tSynthetic bookmark $i`thttps://bookmark.example.test/$i`tbookmark.example.test`t1700000000") }
    [IO.File]::WriteAllText((Join-Path $path 'bookmarks-v2.txt'),$marks.ToString())
}
try {
    $server=Start-Process node -ArgumentList @("`"$PSScriptRoot/scripts/production-fixture-server.mjs`"","`"$portFile`"","`"$requestFile`"") -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $Out 'server-stderr.txt')
    $deadline=[DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path $portFile)) { if ([DateTime]::UtcNow -gt $deadline) {throw 'Fixture startup timeout'}; Start-Sleep -Milliseconds 50 }
    $base="http://127.0.0.1:$([int](Get-Content $portFile -Raw))"
    $metadata=[ordered]@{
        schema_version=2; measurement_mode=$MeasurementMode; readiness_poll_milliseconds=$ReadinessPollMilliseconds; candidate_source=$CandidateSource; process_state_policy='wait for owned/profile tree exit; retain files for reused profile'; baseline_source=$BaselineSource; diagnostic_source=$DiagnosticSource; harness_source=(& git rev-parse HEAD).Trim(); selected_fixtures=$Fixtures; started_at_utc=[DateTime]::UtcNow.ToString('o'); pairs=$Pairs
        inputs=$hashes; qpc_frequency=[Diagnostics.Stopwatch]::Frequency; sample_milliseconds=$SampleMilliseconds
        runner_image=$env:ImageOS; runner_image_version=$env:ImageVersion; runner_name=$env:RUNNER_NAME
        os=(Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,BuildNumber,TotalVisibleMemorySize)
        cpu=(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors)
        gpu=(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,CurrentHorizontalResolution,CurrentVerticalResolution)
        power=(& powercfg /getactivescheme | Out-String).Trim(); resolution=[Windows.Forms.Screen]::PrimaryScreen.Bounds.ToString()
        webview2_runtime=@(Get-ChildItem 'C:/Program Files (x86)/Microsoft/EdgeWebView/Application' -Directory -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
        noise=@('Shared hosted runner, not a laptop/battery claim','Fresh profile is not cold OS cache',$(if($DiagnosticAutoUpdateOff){'Updater OFF via ordinary disposable-profile setting; interaction control, not primary workload'}else{'Updater remains production-enabled, isolated LOCALAPPDATA'}),'Samples are endpoint process-tree observations, not ETW complete','Working-set sums include shared pages','Fixture double-requestAnimationFrame is acknowledged page readiness, not proven display presentation','Title-based tab switch is only a proxy','Sampling overhead separately recorded; measured values are not overhead-subtracted','DOM telemetry QPC is native receipt time, not DOM event execution time; cross-process causality is not inferred from receipt order alone; flag OFF/ON controls stay separate','GetAsyncKeyState snapshots do not prove queued modifier messages have been processed; instrumented input lanes are diagnosis, not typing-latency acceptance')
        workload_settings=@{history_records=10000; bookmark_records=818; startup='start_page overridden by ordinary --open-url'; first_run_offer='already answered synthetic profile'; security='unchanged defaults'; ui='HTML-default build, no BOWSER_UI override'; tab_flow=[bool]$IncludeTabFlow; surface_smoke=[bool]$IncludeSurfaceSmoke;surface_input_method=$SurfaceInputMethod;auto_update=if($DiagnosticAutoUpdateOff){'OFF ordinary disposable-profile control; not the primary workload'}else{'ON production default'};surface_input_scope=if($SurfaceInputMethod -eq 'uia-value'){'ValuePattern navigation/first-use control; bypasses keystrokes; may dispatch DOM input/change and autocomplete; not keyboard correctness'}else{'instrumented SendKeys keyboard diagnostic with UIA value/selection reads; no primary typing latency, retyping or retry'}; diagnostic_detail_pilot=[bool]$DiagnosticDetailPilot;diagnostic_interaction_pilot=[bool]$DiagnosticInteractionPilot;native_focus_observation=[bool]$DiagnosticNativeFocus;controller_focus_observation=[bool]$DiagnosticControllerFocus;focus_topology_observation=[bool]$DiagnosticFocusTopology;sidebar_lifecycle_pilot=[bool]$SidebarLifecyclePilot;sidebar_capability_preflight=[bool]$SidebarCapabilityPreflight;sidebar_route_smoke=[bool]$SidebarRouteSmoke;sidebar_scenario=$SidebarScenario;native_focus_hypothesis=if($DiagnosticNativeFocus){'H1 OS keyboard focus differs from UIA between CtrlL/SetValue/Enter; HWND cannot establish a specific controller; H2 setText overwrite untested; passive observations perturb timing, no performance claim'}else{$null}; startup_only=[bool]$StartupOnly; resource_endpoint=if($SidebarRouteSmoke){'Unavailable: owned activation + four source-bound Toggle actions only, no resource sampling or performance claim'}elseif($SidebarCapabilityPreflight){'Unavailable: passive capabilities/foreground discovery only, no resource sampling or performance claim'}elseif($SidebarLifecyclePilot){'Sidebar UIA-conditioned lifecycle resource phases; not comparable to prior UIA-free startup-only matrix'}elseif($StartupOnly){'settled initial one-tab navigation; no omnibox submission or UIA; not comparable to old post-Enter state'}else{'settled post-Enter one-tab navigation'}; resource_tab_count=if($IncludeTabFlow){5}else{1}}
    }
    $metadata | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $Out 'environment.json') -Encoding utf8
    # Five-pair pilot by default. Alternate whole arm order each pair, same session/runtime/fixture seed.
    foreach($pair in 1..$Pairs) {
        $order=@($arms.Keys); if ($pair % 2 -eq 0) { [array]::Reverse($order) }
        foreach($arm in $order) {
            if ($arm -eq 'release' -and $pair -gt 1) { continue }
            $cases=if($PositiveOnly){@($Fixtures)}else{@(@($Fixtures)+@('redirect','network-failure','error','hung') | Select-Object -Unique)}
            foreach($fixture in $cases) {
                if ($fixture -notin $Fixtures -and ($arm -notin @('diagnostic_on','baseline_on','candidate_on','candidate_detail_on') -or $pair -ne 1)) { continue }
                if ($fixture -in @('network-failure','error','hung') -and ($arm -notin @('diagnostic_on','baseline_on','candidate_on','candidate_detail_on') -or $pair -ne 1)) { continue }
                $profile=Join-Path $run "$arm-$pair-$fixture"
                if($DiagnosticAutoUpdateOff -and (Test-Path -LiteralPath $profile)){throw 'Updater-off controls require newly created disposable roots, not existing profiles'}
                Seed-Profile $profile 818
                $freshOutcome=$null
                $conditions=if($SidebarCapabilityPreflight){@('fresh')}else{@('fresh','reused')}
                foreach($condition in $conditions) {
                    if ($fixture -notin $Fixtures -and $condition -eq 'reused') { continue }
                    $token=([guid]::NewGuid().ToString('N')); $url="$base/$($fixture)?token=$token"
                    if($SidebarLifecyclePilot){
                        # Ordinary profile configuration, same app IDs and fresh per-launch fixture tokens.
                        $sidebar=New-ProductionSidebarFixture -BaseUrl $base -Token $token
                        [IO.File]::WriteAllText((Join-Path $profile 'sidebar.txt'),$sidebar)
                    }
                    $id="$arm-$pair-$fixture-$condition"; $eventFile=Join-Path $Out "$id-events.csv"
                    $args=@('--data-dir',"`"$profile`"",'--open-url',"`"$url`"")
                    if($arm.EndsWith('_on')) { $args+=@('--diagnostic-log',"`"$eventFile`"") }
                    if($arm -eq 'candidate_detail_on') { $args+=@('--diagnostic-detail') }
                    if($arm -like '*_interaction_on'){$args+=@('--diagnostic-interaction')}
                    $snapshots=[Collections.Generic.List[object]]::new(); $overheads=[Collections.Generic.List[double]]::new()
                    $sample=[ordered]@{id=$id;pair=$pair;arm=$arm;fixture=$fixture;condition=$condition;prior_fresh_outcome=if($condition -eq 'reused'){$freshOutcome}else{$null};token=$token;valid=$false;expected_failure=($fixture -in @('network-failure','error','hung'));error=$null;launch_qpc=$null;ready_qpc=$null;title_ready_qpc=$null;input_ready_qpc=$null;shutdown_ms=$null;disk_before=(Disk-Bytes $profile);measurement_mode=$MeasurementMode;tree_exit_wait_ms=$null;tree_exit_confirmed=$false;ownership_inventory_captured=$false;drain_remaining=@();cleanup_error=$null;cleanup_forced_root=$false;identity_error=$null;input_identity_checks=@();observer_arming_wait_ms=$null;observation_error=$null;native_focus_trace=$null;native_focus_observation_errors=@();controller_focus_coverage=$null;updater_control=if($DiagnosticAutoUpdateOff){'ordinary auto-update.txt=false; no staged/trial state; separate interaction workload'}else{'production default on'};surface_smoke=$null;filter_parity=$null;events=$null;input=$null;input_exclusion_reason=if($StartupOnly){'Startup-only lane: no omnibox interaction; input latency unavailable, not passed or zero'}else{$null};tab_switch_proxy_ms=$null;idle=$null;minimized=$null}
                    $app=$null;$known=@{};$script:surfaceTrace=$null;$script:nativeFocusTrace=$null;$script:addressAttempt=0;$sidebarTrace=$null
                    if($SidebarLifecyclePilot){$sidebarTrace=[Collections.Generic.List[object]]::new();$sample['sidebar_trace']=@();$sample['sidebar_scenario']=$SidebarScenario;$sample['sidebar_observation']=$null}
                    try {
                        Assert-FrozenInputs $sample 'before-launch'
                        Confirm-NoStagedUpdate $profile
                        if($SidebarCapabilityPreflight){$sidebarTrace.Add((Get-ProductionSidebarPreflightSnapshot -Boundary 'prelaunch'))}
                        $sample.launch_qpc=Qpc
                        $app=Start-Process -FilePath $arms[$arm] -ArgumentList $args -PassThru
                        $limit=[Diagnostics.Stopwatch]::StartNew(); $timeout=if($sample.expected_failure){4}else{30}
                        while($limit.Elapsed.TotalSeconds -lt $timeout) {
                            $app.Refresh(); if($app.HasExited){throw "Early exit $($app.ExitCode)"}
                            if($null -eq $sample.input_ready_qpc -and [BowserProductionNative]::FindHost([uint32]$app.Id) -ne [IntPtr]::Zero){$sample.input_ready_qpc=Qpc}
                            $window=[BowserProductionNative]::FindHost([uint32]$app.Id)
                            if(([BowserProductionNative]::Title($window)).Contains("BOWSER_PERF_READY_$token")){$sample.title_ready_qpc=Qpc}
                            if(Has-Ready $token){
                                if ($null -eq $sample.ready_qpc) { $sample.ready_qpc=Qpc }
                                # Native title propagation can lag the page's loopback acknowledgement.
                                if ($null -ne $sample.title_ready_qpc) { break }
                            }
                            if ($MeasurementMode -eq 'resource') {
                                $t=Qpc; $snapshot=Get-ProcessGroupSnapshot -RootPid $app.Id -Profile $profile
                                $overheads.Add(((Qpc)-$t)*1000.0/[Diagnostics.Stopwatch]::Frequency); $snapshots.Add($snapshot)
                                Start-Sleep -Milliseconds $SampleMilliseconds
                            } else { Start-Sleep -Milliseconds $ReadinessPollMilliseconds }
                        }
                        if($sample.expected_failure) {
                            if($sample.ready_qpc){throw 'Failed fixture counted as ready'}
                            $sample.valid=$true
                        } else {
                            if(-not $sample.ready_qpc -or -not $sample.title_ready_qpc){throw 'Fixture did not acknowledge readiness in the owned window'}
                            if(-not $StartupOnly) {
                            # Normal UI address submit, not a benchmark navigation shortcut.
                            $inputToken=[guid]::NewGuid().ToString('N'); $inputUrl="$base/light?token=$inputToken"
                            Owned-Keys '^l'; Owned-Keys $inputUrl
                            $inputStart=Qpc; Owned-Keys '{ENTER}'
                            $wait=[Diagnostics.Stopwatch]::StartNew()
                            while(-not (Has-Ready $inputToken) -and $wait.Elapsed.TotalSeconds -lt 20){Start-Sleep -Milliseconds 50}
                            $sample.input=[ordered]@{token=$inputToken;start_qpc=$inputStart;end_qpc=(Qpc);ready=(Has-Ready $inputToken);method='owned-window SendKeys; endpoint includes foreground verification overhead'}
                            if(-not $sample.input.ready){throw 'UI-enter navigation failed'}
                            # Optional, separate scaling workload. The first focused baseline stays at one tab.
                            if ($IncludeTabFlow) {
                            for($i=1;$i -le 4;$i++){Owned-Keys '^t';Owned-Keys '^l';Owned-Keys "$base/light?token=$($inputToken)_$i";Owned-Keys '{ENTER}';$w=[Diagnostics.Stopwatch]::StartNew();while(-not (Has-Ready "$($inputToken)_$i") -and $w.Elapsed.TotalSeconds -lt 15){Start-Sleep -Milliseconds 50};if(-not(Has-Ready "$($inputToken)_$i")){throw 'Tab fixture failed'}}
                            $switch=Qpc;Owned-Keys '^{TAB}';$w=[Diagnostics.Stopwatch]::StartNew();do{$title=[BowserProductionNative]::Title([BowserProductionNative]::FindHost([uint32]$app.Id));if($title.Contains("BOWSER_PERF_READY_$($inputToken)_3")){break};Start-Sleep -Milliseconds 10}while($w.Elapsed.TotalSeconds -lt 5)
                            if(-not $title.Contains("BOWSER_PERF_READY_$($inputToken)_3")){throw 'Expected tab switch title proxy timed out'}
                            $sample.tab_switch_proxy_ms=((Qpc)-$switch)*1000.0/[Diagnostics.Stopwatch]::Frequency
                            }
                            } # StartupOnly deliberately excludes legacy input/tab probes.
                            if($fixture -eq 'filter-parity') {
                                $receipts=@(Get-Content $requestFile | ForEach-Object {ConvertFrom-Json $_} | Where-Object {$_.path -eq '/parity-receipt' -and $_.token -ceq $token})
                                if($receipts.Count -ne 1 -or -not $receipts[0].result.passed -or $receipts[0].result.checks.Count -ne 23 -or @($receipts[0].result.checks | Where-Object {-not $_.pass}).Count){throw 'Native filter dispatch parity receipt failed'}
                                $sample.filter_parity=$receipts[0].result
                            }
                            # Equal-time foreground and minimized idle windows, resource tree sampled throughout.
                            if ($MeasurementMode -eq 'resource') {
                            # Settled one-tab endpoint is identical for both arms; no startup sampling in timing arm.
                            Start-Sleep -Seconds 1
                            foreach($state in @('idle','minimized')) {
                                if($state -eq 'minimized'){[void][BowserProductionNative]::ShowWindow([BowserProductionNative]::FindHost([uint32]$app.Id),6)}
                                $idleRows=[Collections.Generic.List[object]]::new();$w=[Diagnostics.Stopwatch]::StartNew()
                                while($w.Elapsed.TotalSeconds -lt $IdleSeconds){$idleRows.Add((Get-ProcessGroupSnapshot -RootPid $app.Id -Profile $profile));Start-Sleep -Milliseconds $SampleMilliseconds}
                                $sample[$state]=@($idleRows.ToArray());foreach($r in $idleRows){$snapshots.Add($r)}
                            }
                            }
                            if(-not $SidebarLifecyclePilot){if(-not $SidebarCapabilityPreflight){[void][BowserProductionNative]::ShowWindow([BowserProductionNative]::FindHost([uint32]$app.Id),9)}}
                            $sample.valid=$true
                        }
                        if($IncludeSurfaceSmoke -and -not $sample.expected_failure){
                            if($arm -like '*_interaction_on') {
                                $armed=[Diagnostics.Stopwatch]::StartNew();$observed=$false
                                do {
                                    try{$observed=(Test-Path $eventFile) -and [bool](Select-String -LiteralPath $eventFile -Pattern '^interaction_dom_armed,' -Quiet)}catch{$observed=$false}
                                    if($observed){break};Start-Sleep -Milliseconds 20
                                }while($armed.Elapsed.TotalSeconds -lt 5)
                                $sample.observer_arming_wait_ms=$armed.Elapsed.TotalMilliseconds
                                if(-not $observed){throw 'Interaction observer not armed; no diagnostic input sent'}
                            }
                            $script:surfaceTrace=[Collections.Generic.List[object]]::new();if($DiagnosticNativeFocus){$script:nativeFocusTrace=[Collections.Generic.List[object]]::new()};$sample.surface_smoke=@(Native-SurfaceSmoke)
                        }
                        if($SidebarLifecyclePilot -and -not $sample.expected_failure){
                            $sample['sidebar_core_html']=Confirm-NativeCoreSurfaces
                            if($SidebarCapabilityPreflight){$sample['sidebar_preflight']=Get-ProductionSidebarCapabilities -RootPid ([uint32]$app.Id) -Trace $sidebarTrace;if(-not $sample.sidebar_preflight.complete){throw 'Passive sidebar capability discovery incomplete'}}
                            else {Invoke-ProductionSidebarLifecycle -RootPid ([uint32]$app.Id) -InitialToken $token -RouteSmoke:$SidebarRouteSmoke -Profile $profile -RequestFile $requestFile -AToken "$($token)_a" -BToken "$($token)_b" -Scenario $SidebarScenario -Trace $sidebarTrace}
                        }
                        # Inventory only after all cheap timing endpoints, to establish exact identities for drain.
                        $ownedBeforeClose=@(Get-ProcessGroupMembers -RootPid $app.Id -Profile $profile -Inventory @(Get-ProcessInventory))
                        $sample.ownership_inventory_captured=(@($ownedBeforeClose|Where-Object {$_.Pid -eq $app.Id -and $null -ne $_.CreationTicks}).Count -eq 1)
                        $known=@{}; foreach($process in $ownedBeforeClose){$known["$($process.Pid):$($process.CreationTicks)"]=$true}
                        $close=Qpc
                        if(-not [BowserProductionNative]::Close([uint32]$app.Id) -or -not $app.WaitForExit(15000)){throw 'Graceful close not confirmed'}
                        $sample.shutdown_ms=((Qpc)-$close)*1000.0/[Diagnostics.Stopwatch]::Frequency
                        Confirm-OwnedTreeDrain $known $profile $sample
                        if(-not $sample.tree_exit_confirmed){throw 'Owned/profile helpers survived drain; reused launch forbidden'}
                        if($app.ExitCode -ne 0){throw "Exit $($app.ExitCode)"}
                        if($arm.EndsWith('_on')) {
                            $events=@(Import-Csv $eventFile);$sample.events=$events
                            if(-not $sample.expected_failure) {
                                foreach($required in @('html_toolbar_ready','html_chrome_shown')) {
                                    if(-not @($events | Where-Object event -eq $required).Count){throw "HTML activation missing: $required"}
                                }
                            }
                            $readiness=Get-ProductionReadiness $events $token
                            # Later UI navigations supersede startup, so validate the initial identity segment only.
                            $initial=@();foreach($e in $events){if($e.event -eq 'diagnostic_navigation_started' -and @($initial|Where-Object event -eq diagnostic_navigation_completed).Count){break};$initial+=$e}
                            $readiness=Get-ProductionReadiness $initial $token
                            if($sample.expected_failure -and $readiness.Valid){throw 'Diagnostic failed fixture incorrectly usable'}
                            if(-not $sample.expected_failure -and -not $readiness.Valid){throw "Identity/success readiness invalid: $($readiness.InvalidReasons -join ',')"}
                            $sample.diagnostic_readiness=$readiness
                            foreach($probe in @(Get-ProductionSemanticFixtureProbes $sample.surface_smoke)) {
                                $segment=Get-ProductionTokenSegmentReadiness $events $probe.token
                                $probe.native_navigation_success=$segment.Valid
                                if(-not $segment.Valid){throw "Semantic fixture native navigation failed: $($segment.InvalidReasons -join ',')"}
                            }
                        }
                    } catch {
                        $sample.valid=$false;$sample.error=$_.Exception.Message;$sample['error_script_stack']=$_.ScriptStackTrace;$sample['error_position']=$_.InvocationInfo.PositionMessage
                        if($app -and -not $app.HasExited){try{$sample.failure_screenshot=Capture-OwnedSurface 'failure'}catch{}}
                    }
                    finally {
                        if(-not $sample.tree_exit_confirmed) {
                            try {
                                if($app -and -not $app.HasExited) {
                                    $ownedAtFailure=@(Get-ProcessGroupMembers -RootPid $app.Id -Profile $profile -Inventory @(Get-ProcessInventory))
                                    if(@($ownedAtFailure|Where-Object {$_.Pid -eq $app.Id -and $null -ne $_.CreationTicks}).Count -eq 1){$sample.ownership_inventory_captured=$true}
                                    foreach($process in $ownedAtFailure){$known["$($process.Pid):$($process.CreationTicks)"]=$true}
                                    [void][BowserProductionNative]::Close([uint32]$app.Id)
                                    if(-not $app.WaitForExit(5000)){
                                        Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
                                        $sample.cleanup_forced_root=$true;$sample.valid=$false
                                        if(-not $sample.error){$sample.error='Forced close required'}
                                    }
                                }
                                Confirm-OwnedTreeDrain $known $profile $sample
                                if(-not $sample.tree_exit_confirmed){$sample.cleanup_error=if(-not $sample.ownership_inventory_captured){'Root exited without owned identity inventory; helper absence unproven'}else{'Owned/profile helpers survived independent finally drain'};$sample.valid=$false}
                            } catch {$sample.cleanup_error=$_.Exception.Message;$sample.valid=$false}
                        }
                        try {Assert-FrozenInputs $sample 'after-cleanup'}catch{}
                        if($SidebarLifecyclePilot){
                            $partialClick=@($sidebarTrace|Where-Object {$_.kind -ceq 'sidebar-owned-activation-click' -and $null -ne $_.click -and $_.click.guard_ok -and $_.click.sent_count -ne $_.click.planned_count})
                            if($partialClick.Count){$sample.valid=$false;$sample['sidebar_input_contamination']=$true;[ordered]@{id=$sample.id;reason='Partial/failed ordinary SendInput batch; no later native lane may start';clicks=$partialClick} | ConvertTo-Json -Depth 15 | Set-Content $contaminationMarker -Encoding utf8}
                        }
                        # Isolation is persisted BEFORE any later diagnostic import/serialization can fail.
                        if(-not $sample.tree_exit_confirmed) {
                        [ordered]@{id=$sample.id;original_error=$sample.error;identity_error=$sample.identity_error;cleanup_error=$sample.cleanup_error;ownership_inventory_captured=$sample.ownership_inventory_captured;remaining=$sample.drain_remaining} | ConvertTo-Json -Depth 15 | Set-Content $contaminationMarker -Encoding utf8
                        }
                        if($script:surfaceTrace){$sample.surface_smoke=@($script:surfaceTrace.ToArray())}
                        if($SidebarLifecyclePilot){$sample.sidebar_trace=@($sidebarTrace.ToArray())}
                        if($script:nativeFocusTrace){
                            $sample.native_focus_trace=@($script:nativeFocusTrace.ToArray())
                            $sample.native_focus_observation_errors=@($sample.native_focus_trace|Where-Object {$_.api_exception -or $null -eq $_.native -or -not $_.native.query_ok})
                            if($sample.native_focus_observation_errors.Count){$sample.valid=$false;$sample.observation_error='Native OS focus snapshot unavailable; focus attribution incomplete';if(-not $sample.error){$sample.error=$sample.observation_error}}
                        }
                        if($arm.EndsWith('_on') -and (Test-Path $eventFile)){$sample.events=@(Import-Csv $eventFile)}
                        if($SidebarCapabilityPreflight){$sample.sidebar_observation=[ordered]@{complete=$null;exclusion_reason='Passive capabilities preflight; no app action or lifecycle/count measurement'}}
                        elseif($SidebarLifecyclePilot){
                            $expectedSidebarApps=@($sample.sidebar_trace|Where-Object {$_.kind -ceq 'visible-app' -and $_.ack_count -gt 0 -and $_.visible_document_count -gt 0}|ForEach-Object {$_.app_id}|Sort-Object -Unique)
                            $sample.sidebar_observation=if($arm.EndsWith('_on')){Get-ProductionSidebarLedger $sample.events -ExpectedApps $expectedSidebarApps}else{[ordered]@{complete=$null;exclusion_reason='Diagnostic logging OFF; counts unavailable, not zero or passed'}}
                            if($arm.EndsWith('_on') -and -not $sample.sidebar_observation.complete){$sample.valid=$false;$sample.observation_error='Sidebar lifecycle observation incomplete';if(-not $sample.error){$sample.error=$sample.observation_error}}
                        }
                        if($DiagnosticControllerFocus -and $arm -like '*_interaction_on') {
                            $expectedViews=@('toolbar','tabs','rail','content')
                            foreach($view in @('popup','settings')){if(@($sample.surface_smoke|Where-Object {$null -ne $_ -and $_.surface -ceq $view}).Count){$expectedViews+=@($view)}}
                            $sample.controller_focus_coverage=Get-ProductionControllerFocusCoverage $sample.events -ExpectedViews $expectedViews
                            if(-not $sample.controller_focus_coverage.complete){$sample.valid=$false;$sample.observation_error='Controller focus coverage incomplete; no absence attribution permitted';if(-not $sample.error){$sample.error=$sample.observation_error}}
                        }
                        if($arm -like '*_interaction_on') {
                            $drops=@($sample.events|Where-Object {$_.event -in @('interaction_bridge_rate_drop','interaction_bridge_parse_drop','interaction_diagnostic_rate_drop','interaction_observer_enable_failed') -or ($_.event -eq 'interaction_app_borrow_drops' -and [long]$_.count -ne 0)})
                            $coverage=@($sample.events|Where-Object event -eq interaction_app_borrow_drops)
                            if($drops.Count -or $coverage.Count -ne 1){$sample.valid=$false;$sample.observation_error='Incomplete interaction observation: drop or missing coverage; absence cannot prove non-delivery';if(-not $sample.error){$sample.error=$sample.observation_error}}
                        }
                        $sample.disk_after=Disk-Bytes $profile
                        $sample.snapshots=@($snapshots.ToArray());$sample.sampling_overhead_ms=Get-ProductionDistribution @($overheads.ToArray())
                        $sample.cpu=if($snapshots.Count){Get-ProductionCpuDelta @($snapshots.ToArray())}else{$null}
                        Write-Host ("Sample {0}: valid={1}; error={2}" -f $id,$sample.valid,$sample.error)
                        $sample | ConvertTo-Json -Depth 30 | Set-Content (Join-Path $Out "$id.json") -Encoding utf8
                        $rows.Add([pscustomobject]$sample);$app=$null
                        if($condition -eq 'fresh'){$freshOutcome=[ordered]@{valid=$sample.valid;error=$sample.error;state_conditioning=if($sample.valid){'completed prior fresh sequence'}else{'partial failed prior fresh sequence; reused first-use timings not comparable with successful conditioning'}}}
                        if (-not $sample.valid) { $invalidSamples++ }
                    }
                    if($SidebarLifecyclePilot -and (Test-Path $contaminationMarker)){throw 'Native isolation/input contamination marker retained; no subsequent row allowed'}
                    if($sample.identity_error){throw 'Frozen input mutation retained; no restoration or subsequent launch allowed'}
                    if (-not $sample.tree_exit_confirmed) {
                        throw "Stopped after unconfirmed cleanup: $($sample.error); subsequent native lanes blocked, raw evidence retained"
                    }
                    if ($invalidSamples -ge $MaxInvalidSamples) { throw "Stopped after $invalidSamples invalid samples; raw failures retained for diagnosis" }
                }
            }
        }
    }
    $summary=foreach($group in @($rows|Group-Object arm,fixture,condition)) {
        $first=$group.Group[0];$valid=@($group.Group|Where-Object {$_.valid -and -not $_.expected_failure})
        [ordered]@{arm=$first.arm;fixture=$first.fixture;condition=$first.condition;valid_count=@($group.Group|Where-Object valid).Count;invalid_count=@($group.Group|Where-Object {-not $_.valid}).Count;expected_failure_count=@($group.Group|Where-Object expected_failure).Count;launch_to_fixture_ack_ms=if($SidebarCapabilityPreflight -or $SidebarRouteSmoke){$null}else{Get-ProductionDistribution @($valid|ForEach-Object {($_.ready_qpc-$_.launch_qpc)*1000.0/[Diagnostics.Stopwatch]::Frequency})};latency_exclusion_reason=if($SidebarCapabilityPreflight -or $SidebarRouteSmoke){'Bounded route/capability discovery, not a timing comparison'}else{$null}; errors=@($group.Group|Where-Object error|ForEach-Object error)}
    }
    $summary | ConvertTo-Json -Depth 15 | Set-Content (Join-Path $Out 'summary.json') -Encoding utf8
    Assert-MeasurementInputHashesUnchanged $hashes (Get-MeasurementInputHashes $inputPaths) | Out-Null
    if(@($rows|Where-Object {-not $_.valid}).Count){throw 'Invalid samples retained; baseline is not yet accepted'}
} finally {
    if($server -and -not $server.HasExited){Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue}
    foreach($name in $saved.Keys){[Environment]::SetEnvironmentVariable($name,$saved[$name],'Process')}
}
