# Actual release executable, actual persistent disposable profiles. NO benchmark flags: those
# intentionally disable extension startup. This is a small paired observation, not a full audit.
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$ProfilesRoot,[string]$Out="$env:RUNNER_TEMP/bowser-extension-metrics",[ValidateRange(1,5)][int]$Trials=3)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'));$exe=Join-Path $root 'target/release/bowser.exe'
$ProfilesRoot=[IO.Path]::GetFullPath($ProfilesRoot)
$allowed=[IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\')+'\'
if(!$ProfilesRoot.StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase) -or !([IO.Path]::GetFileName($ProfilesRoot).StartsWith('bowser-extension-measurement-'))){throw 'Not the approved fresh runner measurement root.'}
foreach($name in @('zero','several')){ $item=Get-Item -LiteralPath (Join-Path $ProfilesRoot $name);if(!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Profile must be a plain disposable directory.'} }
if(!(Test-Path $exe)){throw 'The exact HTML-default release executable must be built first.'}
New-Item -ItemType Directory -Force $Out | Out-Null
$run=Join-Path $ProfilesRoot 'fixture';New-Item -ItemType Directory $run | Out-Null
$portFile=Join-Path $run 'port.txt';$events=Join-Path $run 'ready.jsonl'
Add-Type -AssemblyName System.Windows.Forms, UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class BowserExtensionMeasure {
 [StructLayout(LayoutKind.Sequential)]public struct COUNTERS {public uint cb,PageFaultCount;public UIntPtr PeakWorkingSetSize,WorkingSetSize,QuotaPeakPagedPoolUsage,QuotaPagedPoolUsage,QuotaPeakNonPagedPoolUsage,QuotaNonPagedPoolUsage,PagefileUsage,PeakPagefileUsage,PrivateUsage;}
 [DllImport("kernel32.dll",SetLastError=true)]public static extern bool K32GetProcessMemoryInfo(IntPtr h,ref COUNTERS counters,uint size);
 [DllImport("user32.dll")]public static extern bool IsWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")]public static extern bool IsWindowEnabled(IntPtr h);
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int command);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")]public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")]public static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern int GetClassName(IntPtr h,StringBuilder s,int n);
}
// Read-only prefixes from Microsoft's UIAutomationClient.h, in native vtable order.
// LegacyIAccessible Value is property 30093, NOT ValuePattern's property 30045.
[ComImport, Guid("30cbe57d-d9d0-452a-ab13-7ac5ac4825ee"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface BowserResourceAutomation {
 [PreserveSig] int CompareElements(IntPtr a,IntPtr b,out int same);
 [PreserveSig] int CompareRuntimeIds(IntPtr a,IntPtr b,out int same);
 [PreserveSig] int GetRootElement(out IntPtr value);
 [PreserveSig] int ElementFromHandle(IntPtr hwnd,out BowserResourceElement value);
 [PreserveSig] int ElementFromPoint(long point,out IntPtr value);
 [PreserveSig] int GetFocusedElement(out IntPtr value);
 [PreserveSig] int GetRootElementBuildCache(IntPtr cache,out IntPtr value);
 [PreserveSig] int ElementFromHandleBuildCache(IntPtr hwnd,IntPtr cache,out IntPtr value);
 [PreserveSig] int ElementFromPointBuildCache(long point,IntPtr cache,out IntPtr value);
 [PreserveSig] int GetFocusedElementBuildCache(IntPtr cache,out IntPtr value);
 [PreserveSig] int CreateTreeWalker(IntPtr condition,out IntPtr value);
 [PreserveSig] int GetControlViewWalker(out IntPtr value);
 [PreserveSig] int GetContentViewWalker(out IntPtr value);
 [PreserveSig] int GetRawViewWalker(out IntPtr value);
 [PreserveSig] int GetRawViewCondition(out IntPtr value);
 [PreserveSig] int GetControlViewCondition(out IntPtr value);
 [PreserveSig] int GetContentViewCondition(out IntPtr value);
 [PreserveSig] int CreateCacheRequest(out IntPtr value);
 [PreserveSig] int CreateTrueCondition(out IntPtr value);
 [PreserveSig] int CreateFalseCondition(out IntPtr value);
 [PreserveSig] int CreatePropertyCondition(int property,[MarshalAs(UnmanagedType.Struct)] object value,out IntPtr condition);
}
[ComImport, Guid("d22108aa-8ac5-49a5-837b-37bbb3d7591e"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface BowserResourceElement {
 [PreserveSig] int SetFocus();
 [PreserveSig] int GetRuntimeId([MarshalAs(UnmanagedType.SafeArray,SafeArraySubType=VarEnum.VT_I4)] out int[] id);
 [PreserveSig] int FindFirst(int scope,IntPtr condition,out IntPtr value);
 [PreserveSig] int FindAll(int scope,IntPtr condition,out BowserResourceElementArray value);
 [PreserveSig] int FindFirstBuildCache(int scope,IntPtr condition,IntPtr cache,out IntPtr value);
 [PreserveSig] int FindAllBuildCache(int scope,IntPtr condition,IntPtr cache,out IntPtr value);
 [PreserveSig] int BuildUpdatedCache(IntPtr cache,out IntPtr value);
 [PreserveSig] int GetCurrentPropertyValue(int property,[MarshalAs(UnmanagedType.Struct)] out object value);
 [PreserveSig] int GetCurrentPropertyValueEx(int property,[MarshalAs(UnmanagedType.Bool)] bool ignoreDefault,[MarshalAs(UnmanagedType.Struct)] out object value);
}
[ComImport, Guid("14314595-b4bc-4055-95f2-58f2e42c9855"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface BowserResourceElementArray {
 [PreserveSig] int GetLength(out int length);
 [PreserveSig] int GetElement(int index,out BowserResourceElement element);
}
public static class BowserResourceDocument {
 static void Check(int hr) { Marshal.ThrowExceptionForHR(hr); }
 static void Release(object value) { if(value!=null&&Marshal.IsComObject(value))Marshal.ReleaseComObject(value); }
 static bool Same(int[] a,int[] b) { if(a==null||b==null||a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true; }
 public static string LegacyValue(IntPtr hwnd,uint expectedPid,uint expectedThread,int[] rootRuntimeId,int[] runtimeId) {
  if(rootRuntimeId==null||rootRuntimeId.Length==0||rootRuntimeId.Length>128||runtimeId==null||runtimeId.Length==0||runtimeId.Length>128)throw new InvalidOperationException("Invalid native document identity.");
  uint pid;uint thread=BowserExtensionMeasure.GetWindowThreadProcessId(hwnd,out pid);
  var cls=new StringBuilder(128);BowserExtensionMeasure.GetClassName(hwnd,cls,128);
  if(thread==0||thread!=expectedThread||pid!=expectedPid||cls.ToString()!="BowserNativeWindow"||!BowserExtensionMeasure.IsWindow(hwnd)||!BowserExtensionMeasure.IsWindowVisible(hwnd)||!BowserExtensionMeasure.IsWindowEnabled(hwnd)||BowserExtensionMeasure.GetForegroundWindow()!=hwnd)throw new InvalidOperationException("Native document owner unavailable.");
  object instance=null;BowserResourceElement root=null;BowserResourceElementArray documents=null;IntPtr condition=IntPtr.Zero;
  try {
   instance=Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("ff48dba4-60ef-4201-aa87-54103eef594e"),true));
   var automation=(BowserResourceAutomation)instance;
   Check(automation.ElementFromHandle(hwnd,out root));int[] rootBefore;Check(root.GetRuntimeId(out rootBefore));
   if(!Same(rootBefore,rootRuntimeId))throw new InvalidOperationException("Native/managed owned root identity mismatch.");
   Check(automation.CreatePropertyCondition(30003,50030,out condition));
   Check(root.FindAll(4,condition,out documents));int count;Check(documents.GetLength(out count));
   if(count<0||count>64)throw new InvalidOperationException("Native document inventory exceeded limit.");
   int matches=0;string result=null;
   for(int i=0;i<count;i++) {
    BowserResourceElement candidate=null;
    try { Check(documents.GetElement(i,out candidate));int[] before;Check(candidate.GetRuntimeId(out before));if(!Same(before,runtimeId))continue;
     matches++;object value=null;
     try { Check(candidate.GetCurrentPropertyValueEx(30093,true,out value));int[] after;Check(candidate.GetRuntimeId(out after));
      if(!Same(before,after))throw new InvalidOperationException("Native document identity changed during property query.");
      result=value as string;if(result!=null&&result.Length>4096)throw new InvalidOperationException("Native document URL exceeded limit.");
     } finally { Release(value); }
    } finally { Release(candidate); }
   }
   int[] rootAfter;Check(root.GetRuntimeId(out rootAfter));
   if(!Same(rootBefore,rootAfter)||!Same(rootAfter,rootRuntimeId))throw new InvalidOperationException("Native owned root identity changed.");
   uint freshPid;uint freshThread=BowserExtensionMeasure.GetWindowThreadProcessId(hwnd,out freshPid);cls.Clear();BowserExtensionMeasure.GetClassName(hwnd,cls,128);
   if(freshPid!=expectedPid||freshThread!=expectedThread||freshThread!=thread||cls.ToString()!="BowserNativeWindow"||!BowserExtensionMeasure.IsWindow(hwnd)||!BowserExtensionMeasure.IsWindowVisible(hwnd)||!BowserExtensionMeasure.IsWindowEnabled(hwnd)||BowserExtensionMeasure.GetForegroundWindow()!=hwnd)throw new InvalidOperationException("Native document owner changed.");
   if(matches!=1)throw new InvalidOperationException("Native document runtime identity was unavailable or ambiguous.");
   return result;
  } finally { if(condition!=IntPtr.Zero)Marshal.Release(condition);Release(documents);Release(root);Release(instance); }
 }
}
'@
@'
const http=require('node:http'),fs=require('node:fs');const[port,events]=process.argv.slice(2);
const server=http.createServer((req,res)=>{const url=new URL(req.url,'http://127.0.0.1');res.setHeader('Cache-Control','no-store');
 if(url.pathname==='/ready'){fs.appendFileSync(events,JSON.stringify({token:url.searchParams.get('token'),time:Date.now()})+'\n');res.writeHead(204);res.end();return;}
 res.setHeader('Content-Type','text/html');res.setHeader('Content-Security-Policy',"default-src 'none'; script-src 'unsafe-inline'; connect-src 'self'; style-src 'unsafe-inline'");
 res.end(`<!doctype html><title>Disposable native extension measurement</title><style>body{font:20px system-ui;background:#edf4ff;padding:40px}</style><h1>Controlled local page</h1><p>No credentials, accounts, or external page content.</p><script>addEventListener('load',()=>fetch('/ready?token='+encodeURIComponent(new URL(location.href).searchParams.get('token')),{cache:'no-store'}));</script>`);
});server.listen(0,'127.0.0.1',()=>fs.writeFileSync(port,String(server.address().port)));
'@ | Set-Content (Join-Path $run 'server.cjs') -Encoding utf8
$app=$null;$server=$null;$saved=@{};$rows=@();$currentRow=$null;$hadFailure=$false
$report=[ordered]@{protocol_version=3;series_complete=$false;cleanup_errors=@();reference_runtime_version=$null;scope='Release executable with --dev: startup/resources before UIA conditioning; separate mandatory slow-per-key navigation correctness lane';navigation_workload='Source-bound toolbar document and stable current UIA identities, at least30ms per literal key plus UIA reads, exact focused value before one Enter; NOT browser-only latency or comparable to previous burst-input rows';primary_valid_pair_count=0;navigation_valid_pair_count=0;source_commit=(& git -C $root rev-parse HEAD).Trim();executable_sha256=(Get-FileHash $exe -Algorithm SHA256).Hash.ToLowerInvariant();status='failed';samples=@();notes=@('Bowser executable hashes are read before each launch, warming its executable cache; this is a hash-conditioned workload, not pristine cold-disk startup. Synchronous UIA provider calls are not hard-bounded by the surrounding polling deadlines.','No benchmark flags; --dev only suppresses production updater. Extension diagnostic tracing is enabled in both arms so native Add completions, not provisional ownership rows, can be observed; these are diagnostic observations, not clean production latency.','Fresh prepared profiles only in trial1; later startups inherit prior navigation/autocomplete and extension-storage conditioning. UIA input conditioning follows resource capture within each row, not across reused rows.','Startup-to-ready includes process launch and local page load; navigation-to-ready includes keyboard interaction.','Memory is sum of owned process-tree private committed bytes and working sets, not PSS; shared pages may be counted twice.','Four packages were signature-verified and explicitly consented in the preceding runtime probe.');error=$null}
function Ready([string]$Token){if(!(Test-Path $events)){return $false};return [bool](@(Get-Content $events | ConvertFrom-Json | Where-Object {$_.token -eq $Token}).Count)}
function Await([scriptblock]$Condition,[string]$Description){$deadline=[DateTime]::UtcNow.AddSeconds(90);do{if($app){$app.Refresh();if($app.HasExited){throw "App exited before $Description"};if($app.MainWindowHandle -ne [IntPtr]::Zero){$class=[Text.StringBuilder]::new(128);[void][BowserExtensionMeasure]::GetClassName($app.MainWindowHandle,$class,128);if($class.ToString() -eq '#32770'){$dialog=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle);$text=@($dialog.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object{$_.Current.Name});throw ('Owned native startup dialog: '+($text -join ' | '))}}};if(& $Condition){return};Start-Sleep -Milliseconds 50}while([DateTime]::UtcNow -lt $deadline);throw "Timed out: $Description"}
function Native-Count([string]$Profile){$file=Join-Path $Profile 'extension_ids.txt';if(!(Test-Path $file)){return 0};return @(Get-Content $file | Where-Object {$_ -match '^ext-[a-f0-9]{16}\t[a-p]{32}$'}).Count}
function Native-Completed([string]$Log,[int]$Count){
 if($Count -eq 0){return $true};if(!(Test-Path -LiteralPath $Log)){return $false}
 $text=Get-Content -LiteralPath $Log -Raw
 $ids=@('ddkjiahejlhfcafbddmgiahcphecmpfh','nngceckbapebfimnlniiiahkandclblb','clngdbkpkpeebahjckkjfobafhncgmne','iaiomicjabeggjcfkbimgmglanimpnae')
 foreach($id in $ids){
  if($text -notmatch ('extension call=(?:add|enable) bowser=(?:ext-[a-f0-9]{16}|none) result=ok hresult=0x00000000 id='+$id+' ')){return $false}
  if($text -match ('extension call=disable .* id='+$id+' ')){throw 'A measured extension was disabled during startup; four-enabled-extension measurement is invalid.'}
 }
 return $true
}
function Activate-OwnedAddressWindow {
 $app.Refresh();$window=$app.MainWindowHandle;$owner=[uint32]0
 [void][BowserExtensionMeasure]::GetWindowThreadProcessId($window,[ref]$owner)
 $class=[Text.StringBuilder]::new(128);[void][BowserExtensionMeasure]::GetClassName($window,$class,128)
 if($owner -ne $app.Id -or $class.ToString() -cne 'BowserNativeWindow'){throw 'Not the exact owned native browser window; no input sent.'}
 [void][BowserExtensionMeasure]::ShowWindow($window,9)
 try{[Windows.Automation.AutomationElement]::FromHandle($window).SetFocus()}catch{}
 [void][BowserExtensionMeasure]::SetForegroundWindow($window);Start-Sleep -Milliseconds 250
 [void](Assert-AddressWindow $window)
}

# Failure-only observations. These classifications never select a control or authorize input.
# Record caps bound output, not synchronous provider execution or collection allocation.
function Observe-UnavailableAddress($RootElement,[IntPtr]$Window) {
 $result=@{status='incomplete';record_cap=16;collections=@();identity_after=$false;error=$null}
 try {
  foreach($spec in @(
   @{kind='exact-address-name';condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Address and search bar')},
   @{kind='edit-type';condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Edit)},
   @{kind='document-type';condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Document)})) {
   $nodes=$RootElement.FindAll([Windows.Automation.TreeScope]::Descendants,$spec.condition)
   $collection=@{kind=$spec.kind;count=$nodes.Count;truncated=($nodes.Count -gt 16);records=@()}
   $result.collections+=,$collection
   for($i=0;$i -lt [Math]::Min(16,$nodes.Count);$i++) {
    $record=@{index=$i;status='unavailable';control_type=$null;provider_reported_pid=$null;name_kind='unavailable';offscreen=$null;focusable=$null}
    $collection.records+=,$record
    try {
     $node=$nodes[$i];$state=$node.Current
     $record.control_type=$state.ControlType.Id;$record.provider_reported_pid=$state.ProcessId
     $record.name_kind=if($state.Name -ceq 'Address and search bar'){'exact-address'}elseif($state.Name -ceq 'Address and search bar, private tab'){'private-address'}else{'other'}
     $record.offscreen=$state.IsOffscreen;$record.focusable=$state.IsKeyboardFocusable;$record.status='observed-not-authority'
    }catch { $record.status='getter-unavailable' }
   }
  }
  $owner=[uint32]0;[void][BowserExtensionMeasure]::GetWindowThreadProcessId($Window,[ref]$owner)
  $class=[Text.StringBuilder]::new(128);[void][BowserExtensionMeasure]::GetClassName($Window,$class,128)
  $result.identity_after=$owner -eq $app.Id -and $class.ToString() -ceq 'BowserNativeWindow' -and [BowserExtensionMeasure]::GetForegroundWindow() -eq $Window -and $app.MainWindowHandle -eq $Window
  $result.status=if($result.identity_after){'observed-not-control-or-document-authority'}else{'owner-unavailable-or-changed'}
 }catch { $result.error='diagnostic-unavailable' }
 return $result
}
# No title, provider PID, address-name-only or whole-window fallback establishes authority.
# The exact source is the embedded toolbar host in src/chrome_host/assets.rs. Runtime IDs
# are opaque comparison values, revalidated within this single lane, not persisted identifiers.
function Address-IsToolbarSource([string]$Source) { return $Source -ceq 'bowser-ui://app/index.html?view=toolbar' }
function Assert-AddressGeometry($Bounds) {
 if(!$Bounds){throw 'Source-bound address geometry unavailable or nonfinite; no input sent.'}
 foreach($coordinate in @($Bounds.X,$Bounds.Y,$Bounds.Width,$Bounds.Height)){
  if([double]::IsNaN($coordinate) -or [double]::IsInfinity($coordinate)){throw 'Source-bound address geometry unavailable or nonfinite; no input sent.'}
 }
 if($Bounds.Width -le 0 -or $Bounds.Height -le 0){throw 'Source-bound address geometry has nonpositive extents; no input sent.'}
}
function Address-RuntimeKey($Element) {
 if(!$Element){throw 'Current UIA element unavailable; no input sent.'}
 $id=@($Element.GetRuntimeId())
 if(!$id.Count -or $id.Count -gt 128 -or @($id|Where-Object {$_ -isnot [int]}).Count){throw 'Current UIA runtime identity unavailable or over bound; no input sent.'}
 return $id -join ','
}
function Assert-AddressWindow([IntPtr]$Window,[uint32]$Thread=0) {
 $app.Refresh();$owner=[uint32]0
 $actualThread=[BowserExtensionMeasure]::GetWindowThreadProcessId($Window,[ref]$owner)
 $class=[Text.StringBuilder]::new(128);[void][BowserExtensionMeasure]::GetClassName($Window,$class,128)
 if($app.HasExited -or $app.MainWindowHandle -ne $Window -or !$actualThread -or ($Thread -and $actualThread -ne $Thread) -or $owner -ne $app.Id -or $class.ToString() -cne 'BowserNativeWindow' -or ![BowserExtensionMeasure]::IsWindow($Window) -or ![BowserExtensionMeasure]::IsWindowVisible($Window) -or ![BowserExtensionMeasure]::IsWindowEnabled($Window) -or [BowserExtensionMeasure]::GetForegroundWindow() -ne $Window){throw 'Exact owned window/thread/foreground unavailable or changed; no input sent.'}
 return $actualThread
}
function Address-InDocument($Control,$Document) {
 $key=Address-RuntimeKey $Document;$node=$Control
 for($depth=0;$depth -lt 64;$depth++){
  $node=[Windows.Automation.TreeWalker]::RawViewWalker.GetParent($node)
  if(!$node){return $false}
  if((Address-RuntimeKey $node) -ceq $key){return $true}
  if($node.Current.ControlType -eq [Windows.Automation.ControlType]::Document){return $false}
 }
 return $false
}
function Address-OutermostDocument($Document,$RootElement) {
 $key=Address-RuntimeKey $RootElement;$node=$Document
 for($depth=0;$depth -lt 64;$depth++){
  $node=[Windows.Automation.TreeWalker]::RawViewWalker.GetParent($node)
  if(!$node){return $false}
  if((Address-RuntimeKey $node) -ceq $key){return $true}
  if($node.Current.ControlType -eq [Windows.Automation.ControlType]::Document){return $false}
 }
 return $false
}
function Address-DocumentSource($Document,[IntPtr]$Window,[uint32]$Thread,$RootElement) {
 [void](Assert-AddressWindow $Window $Thread)
 $runtimeId=$Document.GetRuntimeId();$key=Address-RuntimeKey $Document
 $rootRuntimeId=$RootElement.GetRuntimeId();$rootKey=Address-RuntimeKey $RootElement
 $values=@();$pattern=$null
 if($Document.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)){$values+=([Windows.Automation.ValuePattern]$pattern).Current.Value}
 # Chromium exposes document tree-data URL as LegacyIAccessible.Value (30093),
 # not Name/title or ValuePattern's property 30045. The native lookup must match
 # the same exact managed runtime ID, with owned-window/thread fences on both sides.
 $legacy=[BowserResourceDocument]::LegacyValue($Window,[uint32]$app.Id,$Thread,$rootRuntimeId,$runtimeId)
 if($legacy){$values+=$legacy}
 if((($rootRuntimeId -join ',') -cne $rootKey) -or (Address-RuntimeKey $RootElement) -cne $rootKey){throw 'Current root identity changed during native source query; no input sent.'}
 if((($runtimeId -join ',') -cne $key) -or (Address-RuntimeKey $Document) -cne $key){throw 'Current document identity changed during source query; no input sent.'}
 [void](Assert-AddressWindow $Window $Thread)
 $values=@($values|Where-Object {$_}|Select-Object -Unique)
 # A default/unsupported/non-string native result never authorizes a ValuePattern fallback.
 if($legacy -isnot [string] -or !$legacy -or $values.Count -ne 1 -or $values[0].Length -gt 4096){return $null}
 return [string]$values[0]
}
function Read-OwnedAddress($Expected) {
 $app.Refresh();$window=$app.MainWindowHandle
 $thread=Assert-AddressWindow $window $(if($Expected){$Expected.thread}else{0})
 if($Expected -and $window -ne $Expected.window){throw 'Original address host changed; no input sent.'}
 $rootElement=[Windows.Automation.AutomationElement]::FromHandle($window);$rootKey=Address-RuntimeKey $rootElement
 if($Expected -and $rootKey -cne $Expected.root_key){throw 'Original address root runtime identity changed; no input sent.'}
 try {
  $documents=$rootElement.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Document))
  if($documents.Count -gt 64){throw 'Address document inventory exceeded the bounded probe; no input sent.'}
  $matches=@()
  foreach($candidate in $documents){
   if(!(Address-OutermostDocument $candidate $rootElement)){continue}
   $source=Address-DocumentSource $candidate $window $thread $rootElement
   if(Address-IsToolbarSource $source){$matches+=@{document=$candidate;source=$source}}
  }
  if($matches.Count -ne 1){throw ("Exact source-bound toolbar document missing or ambiguous; observed_toolbar_document_count="+$matches.Count+"; no input sent.")}
  $document=$matches[0].document;$documentKey=Address-RuntimeKey $document
  if($Expected -and $documentKey -cne $Expected.document_key){throw 'Original toolbar document runtime identity changed; no input sent.'}
  # Search once, only under the attributed document. Filtering the exact-name set by
  # current type and nearest Document ancestry is not a global or first-match fallback.
  $named=$document.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Address and search bar'))
  if($named.Count -gt 10000){throw 'Attributed address inventory exceeded the bounded probe; no input sent.'}
  $edits=@($named|Where-Object {$_.Current.Name -ceq 'Address and search bar' -and $_.Current.ControlType -eq [Windows.Automation.ControlType]::Edit -and (Address-InDocument $_ $document)})
  if($edits.Count -ne 1){throw ("Source-bound address edit missing or ambiguous; observed_exact_edit_count="+$edits.Count+"; no input sent.")}
  $edit=$edits[0];$editKey=Address-RuntimeKey $edit
  if($Expected -and $editKey -cne $Expected.edit_key){throw 'Original address edit runtime identity changed; no input sent.'}
  $bounds=$edit.Current.BoundingRectangle
  Assert-AddressGeometry $bounds
  if(!$edit.Current.IsEnabled -or !$edit.Current.IsKeyboardFocusable -or $edit.Current.IsOffscreen){throw 'Source-bound address edit unavailable for visible input; no input sent.'}
  $pattern=[Windows.Automation.ValuePattern]$edit.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)
  if($pattern.Current.IsReadOnly){throw 'Source-bound address edit is read-only; no input sent.'}
  $selection=$null;try{$ranges=([Windows.Automation.TextPattern]$edit.GetCurrentPattern([Windows.Automation.TextPattern]::Pattern)).GetSelection();$selection=@($ranges|ForEach-Object {$_.GetText(-1)})}catch{}
  if((Address-RuntimeKey $rootElement) -cne $rootKey -or (Address-RuntimeKey $document) -cne $documentKey -or (Address-RuntimeKey $edit) -cne $editKey -or !(Address-OutermostDocument $document $rootElement) -or !(Address-InDocument $edit $document) -or !(Address-IsToolbarSource (Address-DocumentSource $document $window $thread $rootElement))){throw 'Current address document/control source or identity changed; no input sent.'}
  if((Address-RuntimeKey $rootElement) -cne $rootKey -or (Address-RuntimeKey $document) -cne $documentKey -or (Address-RuntimeKey $edit) -cne $editKey){throw 'Address runtime identity changed after final source query; no input sent.'}
  $focused=[Windows.Automation.AutomationElement]::FocusedElement
  $same=$edit.Current.HasKeyboardFocus -and $focused -and $editKey -ceq (Address-RuntimeKey $focused)
  [void](Assert-AddressWindow $window $thread)
  $value=$pattern.Current.Value
  $bound=@{window=$window;thread=$thread;root_key=$rootKey;document_key=$documentKey;edit_key=$editKey}
  return @{focused=[bool]$same;value=$value;selection=$selection;window=$window;bound=$bound;value_pattern=$pattern}
 } catch {
  # Failure observations cannot retry, choose a control, erase a failed sample, or authorize input.
  try {$currentRow.navigation_workload['address_unavailable_diagnostic']=Observe-UnavailableAddress $rootElement $window}catch {$currentRow.navigation_workload['address_unavailable_diagnostic']=@{status='diagnostic-unavailable'}}
  throw
 }
}
function Assert-AddressInput($State) {
 # Every literal key and Enter uses a fresh source/identity/focus observation of the
 # original bound field. Never SetFocus/SetValue, reacquire a new identity, or repair text.
 $fresh=Read-OwnedAddress $State.bound
 if(!$fresh.focused){throw 'Exact source-bound address focus changed before input; no repair or retry.'}
 Assert-NeutralModifiers
 [void](Assert-AddressWindow $State.window $State.bound.thread)
 return $fresh
}

function Assert-NeutralModifiers {
 foreach($key in @(16,17,18,91,92)){if([BowserExtensionMeasure]::GetAsyncKeyState($key) -lt 0){throw 'Unexpected held input modifier; no repair or further input.'}}
}
function Submit-OwnedFixture([string]$Text,$Trace) {
 if($Text -cnotmatch '^http://127\.0\.0\.1:[0-9]+/page\?token=[a-f0-9]{32}$'){throw 'Not the exact generated loopback fixture address; no input sent.'}
 $Trace['workload']='source-bound-slow-real-per-key-plus-UIA-readbacks';$Trace['minimum_per_key_delay_ms']=30;$Trace['expected_length']=$Text.Length;$Trace['keys']=@();$Trace['exact_final_value']= $false
 # Initial activation is native-owner-only; source authority must be established before Ctrl+L.
 Activate-OwnedAddressWindow
 $state=Read-OwnedAddress $null;$bound=$state.bound
$Trace['address_authority']='exact-toolbar-source-nearest-document-and-stable-runtime-ids'
 $state=Read-OwnedAddress $bound
 Assert-NeutralModifiers
 [void](Assert-AddressWindow $state.window $bound.thread)
 [Windows.Forms.SendKeys]::SendWait('^l')
 $wait=[Diagnostics.Stopwatch]::StartNew()
 do{$state=Read-OwnedAddress $bound;if($state.focused){break};Start-Sleep -Milliseconds 20}while($wait.Elapsed.TotalSeconds -lt 3)
 if(!$state.focused){throw 'Exact owned address focus not confirmed; no typing sent.'}
 $Trace['initial_value_length']=$state.value.Length;$Trace['initial_selection_available']=$null -ne $state.selection
 $index=0
 foreach($character in $Text.ToCharArray()){
  # Append the attempt BEFORE any guard/dispatch/post-check can fail. Failed attempts
  # remain in the row; returned dispatch is not proof that the target accepted a key.
  $keyRow=@{index=$index;phase='before-source-read';before_length=$null;after_length=$null;focused_after=$null;selection_available=$null;dispatch_attempted=$false;dispatch_returned=$false;postcheck_completed=$false;error=$null}
  $Trace['keys']+=,$keyRow
  try {
   $state=Read-OwnedAddress $bound;Assert-NeutralModifiers
   if(!$state.focused){throw 'Owned address focus lost before literal key; no repair or retry.'}
   $state=Assert-AddressInput $state;$keyRow.before_length=$state.value.Length
   $keyRow.phase='dispatch';$keyRow.dispatch_attempted=$true
   [Windows.Forms.SendKeys]::SendWait([string]$character);$keyRow.dispatch_returned=$true;Start-Sleep -Milliseconds 30
   $keyRow.phase='post-dispatch-read';$after=Read-OwnedAddress $bound;Assert-NeutralModifiers
   $keyRow.after_length=$after.value.Length;$keyRow.focused_after=$after.focused;$keyRow.selection_available=$null -ne $after.selection;$keyRow.postcheck_completed=$true
   if(!$after.focused){throw 'Owned address focus lost after literal key; no further input.'}
   $keyRow.phase='completed';$index++
  } catch {$keyRow.error=$_.Exception.Message;throw}
 }

 $wait.Restart()
 do{$state=Read-OwnedAddress $bound;if($state.focused -and $state.value -ceq $Text){break};Start-Sleep -Milliseconds 20}while($wait.Elapsed.TotalSeconds -lt 3)
 $Trace['exact_final_value']=$state.focused -and $state.value -ceq $Text
 if(!$Trace['exact_final_value']){throw 'Exact focused final address was not observed; Enter not sent, no correction or retry.'}
 $state=Assert-AddressInput $state
 if([BowserExtensionMeasure]::GetForegroundWindow() -ne $state.window){throw 'Exact owned foreground lost before Enter; no input sent.'}
 # Exact text is the last provider read, after fresh source/identity/focus/modifier/foreground guards.
 if($state.value_pattern.Current.Value -cne $Text){throw 'Exact focused final address changed before Enter; no correction or retry.'}
 [Windows.Forms.SendKeys]::SendWait('{ENTER}')
}
function Assert-FrozenExecutable($Row,[string]$Phase,[bool]$Final) {
 $Row.identity_check_phase=$Phase
 if($Row.executable_identity_failed){throw 'A previous executable identity failure is latched; no rehabilitation or later measurement permitted.'}
 try{
  if((Get-FileHash $exe -Algorithm SHA256).Hash.ToLowerInvariant() -cne $report.executable_sha256){throw 'Pinned executable bytes changed'}
 }catch{
  $Row.primary_valid=$false;$Row.navigation_valid=$false;$Row.executable_identity_verified=$false;$Row.executable_identity_failed=$true
  $Row.identity_error='Executable hash mismatch or read error; this row is invalid and cannot be rehabilitated.'
  throw $Row.identity_error
 }
 if($Final){$Row.executable_identity_verified=$true}
}
function Memory-Tree {
 $snapshot=@(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe' OR ProcessId = $($app.Id)")
 $ids=[Collections.Generic.HashSet[int]]::new();[void]$ids.Add($app.Id)
 do{$changed=$false;foreach($p in $snapshot){if($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)){$changed=$true}}}while($changed)
 $entries=@();foreach($id in $ids){
  $p=Get-Process -Id $id -ErrorAction Stop;$c=[BowserExtensionMeasure+COUNTERS]::new();$c.cb=[Runtime.InteropServices.Marshal]::SizeOf($c)
  if(![BowserExtensionMeasure]::K32GetProcessMemoryInfo($p.Handle,[ref]$c,$c.cb)){throw "Native memory counters unavailable for owned PID $id"}
  $identity=$snapshot | Where-Object {[int]$_.ProcessId -eq $id} | Select-Object -First 1
  $version=$null;if($identity.ExecutablePath){$version=[Diagnostics.FileVersionInfo]::GetVersionInfo($identity.ExecutablePath).ProductVersion}
  $entries+=@{pid=$p.Id;parent_pid=$identity.ParentProcessId;name=$p.ProcessName;executable_path=$identity.ExecutablePath;command_line=$identity.CommandLine;product_version=$version;private_bytes=$c.PrivateUsage.ToUInt64();working_set_bytes=$c.WorkingSetSize.ToUInt64();private_source='K32GetProcessMemoryInfo/PROCESS_MEMORY_COUNTERS_EX.PrivateUsage';working_set_source='K32GetProcessMemoryInfo/PROCESS_MEMORY_COUNTERS_EX.WorkingSetSize'}
 }
 return $entries
}
try{
 foreach($name in @('LOCALAPPDATA','BOWSER_DATA_DIR','BOWSER_UI','BOWSER_EXTENSION_TRACE')){$saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process');[Environment]::SetEnvironmentVariable($name,$null,'Process')}
 $env:BOWSER_EXTENSION_TRACE='1'
 $env:LOCALAPPDATA=Join-Path $ProfilesRoot 'isolated-localappdata';New-Item -ItemType Directory -Force $env:LOCALAPPDATA|Out-Null
 $server=Start-Process (Get-Command node).Source -ArgumentList @("`"$(Join-Path $run 'server.cjs')`"","`"$portFile`"","`"$events`"") -PassThru -WindowStyle Hidden
 Await {Test-Path $portFile} 'loopback server';$base='http://127.0.0.1:'+(Get-Content $portFile -Raw).Trim()
 for($trial=1;$trial -le $Trials;$trial++){
  foreach($case in @(@{name='zero';count=0},@{name='several';count=4})){
   $profile=Join-Path $ProfilesRoot $case.name;$token=[guid]::NewGuid().ToString('N');$env:BOWSER_DATA_DIR=$profile
   $currentRow=@{case=$case.name;trial=$trial;profile_conditioning=$(if($trial -eq 1){'fresh-prepared-profile'}else{'prior-navigation-autocomplete-and-extension-state'});expected_native_extension_count=$case.count;primary_valid=$false;navigation_valid=$false;executable_identity_verified=$false;executable_identity_failed=$false;identity_error=$null;identity_check_phase=$null;phase='before-launch-hash';error=$null;navigation_workload=@{}}
   $rows+=$currentRow
   Assert-FrozenExecutable $currentRow 'before-launch' $false
   $stderr=Join-Path $Out "$($case.name)-$trial-stderr.log"
   $clock=[Diagnostics.Stopwatch]::StartNew()
   $currentRow.phase='process-launch';$app=Start-Process $exe -ArgumentList @('--dev','--data-dir',"`"$profile`"",'--open-url',"$base/page?token=$token") -WorkingDirectory $root -PassThru -RedirectStandardError $stderr -RedirectStandardOutput (Join-Path $Out "$($case.name)-$trial-stdout.log")
   $currentRow.phase='first-native-page';Await {Ready $token} 'first native page ready';$ready=$clock.Elapsed.TotalMilliseconds
   $currentRow.phase='native-ownership-count';Await {(Native-Count $profile) -eq $case.count} 'exact durable signing-ID ownership count'
   $currentRow.phase='native-add-completions';Await {Native-Completed $stderr $case.count} 'all exact native Add completions';$extensionsReady=$clock.Elapsed.TotalMilliseconds
   $currentRow.phase='owned-window-wait';Await {$app.Refresh();$app.MainWindowHandle -ne [IntPtr]::Zero} 'owned native window'
   $currentRow.phase='resource-settle';Start-Sleep -Seconds 4
   $currentRow.phase='memory-snapshot';$memory=@(Memory-Tree);$private=($memory | Measure-Object private_bytes -Sum).Sum;$working=($memory | Measure-Object working_set_bytes -Sum).Sum
   # Capture startup/resources BEFORE address/UIA conditioning. The following correctness lane
   # deliberately changes input workload; its wall time is never browser-only navigation latency.
   $currentRow.native_extension_count=Native-Count $profile;$currentRow.process_to_first_page_ready_ms=$ready;$currentRow.process_to_extension_ready_ms=$extensionsReady
   $currentRow.native_extension_count_source='Durable signing-ID map count plus native Add/Enable trace gates; not an independent live IsEnabled inventory'
   if($currentRow.native_extension_count -ne $case.count -or !(Native-Completed $stderr $case.count)){throw 'Exact ownership count/native completion or no-disable evidence changed at memory snapshot; primary row invalid.'}
   $currentRow.private_bytes_sum=$private;$currentRow.working_set_bytes_sum=$working;$currentRow.processes=$memory
   $currentRow.phase='runtime-version-consistency';$runtimeProcesses=@($memory|Where-Object {$_.name -ceq 'msedgewebview2'})
   $currentRow.runtime_versions=@($runtimeProcesses|ForEach-Object {$_.product_version}|Sort-Object -Unique)
   if(!$runtimeProcesses.Count -or @($runtimeProcesses|Where-Object {!$_.product_version}).Count -or $currentRow.runtime_versions.Count -ne 1){throw 'Observed Evergreen runtime version missing or inconsistent; row is not comparable.'}
   if($null -eq $report.reference_runtime_version){$report.reference_runtime_version=$currentRow.runtime_versions[0]}
   if($currentRow.runtime_versions[0] -cne $report.reference_runtime_version){throw 'Observed Evergreen runtime version drift; row is not comparable and later measurements stopped.'}
   $currentRow.primary_valid=$true
   $currentRow.phase='mandatory-per-key-navigation';$second=[guid]::NewGuid().ToString('N');$nav=[Diagnostics.Stopwatch]::StartNew()
   Submit-OwnedFixture "$base/page?token=$second" $currentRow.navigation_workload
   Await {Ready $second} 'second native page ready';$currentRow.conditioned_per_key_navigation_workload_to_http_ready_ms=$nav.Elapsed.TotalMilliseconds;$currentRow.navigation_valid=$true
   $versions=@();$record=Join-Path $profile 'extensions.txt';if(Test-Path $record){$versions=@(Get-Content $record | ForEach-Object {$fields=$_ -split "`t";@{bowser_id=$fields[0];name=$fields[2];version=$fields[3]}})}
   $currentRow.extensions=$versions;$currentRow.phase='clean-close'
   if(!$app.CloseMainWindow()){throw 'Owned native app did not accept clean close.'}
   if(!$app.WaitForExit(15000)){throw 'Owned native app did not close cleanly; no warm-profile restart attempted.'}
   $app=$null;$currentRow.phase='post-close-hash';Assert-FrozenExecutable $currentRow 'after-clean-close' $true
   $currentRow.phase='completed';Start-Sleep -Seconds 2
  }
 }
 $report.status='measured-no-compatibility-or-performance-improvement-claim'
}catch{$hadFailure=$true;$report.error=$_.Exception.Message;if($currentRow){$currentRow.error=$report.error};throw}
finally{
 # An interrupted correctness lane still needs a final identity check before its earlier resource
 # observations can be classified; a mismatch/read error invalidates that attempted row.
 if($currentRow -and !$currentRow.executable_identity_verified -and !$currentRow.executable_identity_failed){
  try{Assert-FrozenExecutable $currentRow 'after-failed-lane' $true}
  catch{$report.status='failed';$report.error="$($report.error) Executable identity could not be verified.";$currentRow.error=$report.error}
 }
 # Cleanup failures cannot erase the attempted-row report. Do not silently call them success.
 if($app){try{$app.Refresh();if(!$app.HasExited){Stop-Process -Id $app.Id -Force;if(!$app.WaitForExit(10000)){throw 'App cleanup did not finish'}}}catch{$report.cleanup_errors+=@('owned-app-cleanup-failed');$report.status='failed'}}
 if($server){try{$server.Refresh();if(!$server.HasExited){Stop-Process -Id $server.Id -Force}}catch{$report.cleanup_errors+=@('owned-fixture-server-cleanup-failed');$report.status='failed'}}
 if($report.cleanup_errors.Count){$report.error="$($report.error) Owned-process cleanup did not complete; see cleanup_errors."}
 $report.samples=$rows
 for($trial=1;$trial -le $Trials;$trial++){
  $pair=@($rows|Where-Object {$_.trial -eq $trial})
  if($pair.Count -eq 2 -and @($pair|Where-Object {$_.primary_valid}).Count -eq 2){$report.primary_valid_pair_count++}
  if($pair.Count -eq 2 -and @($pair|Where-Object {$_.primary_valid -and $_.navigation_valid}).Count -eq 2){$report.navigation_valid_pair_count++}
 }
 $report.series_complete=$report.status -ceq 'measured-no-compatibility-or-performance-improvement-claim' -and $report.primary_valid_pair_count -eq $Trials -and $report.navigation_valid_pair_count -eq $Trials -and !$report.cleanup_errors.Count
 if(!$report.series_complete){$report.status='failed'}
 $report | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $Out 'extension-metrics.json') -Encoding utf8
 foreach($entry in $saved.GetEnumerator()){[Environment]::SetEnvironmentVariable($entry.Key,$entry.Value,'Process')}
 # Profiles include downloaded packages; never upload them. Only this validated disposable root.
 for($i=0;$i -lt 20 -and (Test-Path $ProfilesRoot);$i++){try{Remove-Item -LiteralPath $ProfilesRoot -Recurse -Force}catch{Start-Sleep -Milliseconds 500}}
 if(!$report.series_complete -and !$hadFailure){throw 'Measurement series or owned-process cleanup was incomplete; attempted-row report was preserved.'}
}
