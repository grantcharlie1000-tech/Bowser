# Genuine native storefront smoke. No fake host, no remote debugger, no install consent bypass.
# Captures are evidence of rendering only, not installation/functionality acceptance.
[CmdletBinding()]
param([string]$Out = "$env:RUNNER_TEMP/bowser-native-store", [switch]$InstallJourney)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$exe = Join-Path $root 'target/debug/bowser.exe'
if (!(Test-Path $exe)) { throw 'Build the HTML-default debug executable first.' }
New-Item -ItemType Directory -Force $Out | Out-Null
$run = Join-Path ([IO.Path]::GetTempPath()) ('bowser-native-store-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $run | Out-Null
$portFile=Join-Path $run 'port.txt';$readyFile=Join-Path $run 'ready.txt'
@'
const http=require('node:http'),fs=require('node:fs');const[port,ready]=process.argv.slice(2);
const server=http.createServer((req,res)=>{const u=new URL(req.url,'http://127.0.0.1');res.setHeader('Cache-Control','no-store');if(u.pathname==='/ready'){fs.writeFileSync(ready,u.searchParams.get('token')||'');res.writeHead(204);res.end();return}res.setHeader('Content-Type','text/html');res.setHeader('Content-Security-Policy',"default-src 'none';script-src 'unsafe-inline';connect-src 'self'");res.end(`<!doctype html><title>Bowser toolbar</title><div role="document" aria-label="Bowser toolbar"><button>Add to Bowser</button><input aria-label="Address and search bar"><input aria-label="Search Chrome Web Store"><a href="#">Stylus</a></div><h1>Disposable native browsing control</h1><script>addEventListener('load',()=>fetch('/ready?token='+encodeURIComponent(new URL(location.href).searchParams.get('token')),{cache:'no-store'}))</script>`)});server.listen(0,'127.0.0.1',()=>fs.writeFileSync(port,String(server.address().port)));
'@|Set-Content (Join-Path $run 'server.cjs') -Encoding utf8
$initialUrl=$null;$server=$null;$readyToken=$null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class BowserStoreSmoke {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h,StringBuilder value,int count);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int command);
 [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int height,bool repaint);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
}
// Read-only prefixes from Microsoft's UIAutomationClient.h, in native vtable order.
// LegacyIAccessible Value is property 30093, NOT ValuePattern's property 30045.
[ComImport, Guid("30cbe57d-d9d0-452a-ab13-7ac5ac4825ee"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface BowserNativeAutomation {
 [PreserveSig] int CompareElements(IntPtr a,IntPtr b,out int same);
 [PreserveSig] int CompareRuntimeIds(IntPtr a,IntPtr b,out int same);
 [PreserveSig] int GetRootElement(out IntPtr value);
 [PreserveSig] int ElementFromHandle(IntPtr hwnd,out BowserNativeElement value);
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
interface BowserNativeElement {
 [PreserveSig] int SetFocus();
 [PreserveSig] int GetRuntimeId([MarshalAs(UnmanagedType.SafeArray,SafeArraySubType=VarEnum.VT_I4)] out int[] id);
 [PreserveSig] int FindFirst(int scope,IntPtr condition,out IntPtr value);
 [PreserveSig] int FindAll(int scope,IntPtr condition,out BowserNativeElementArray value);
 [PreserveSig] int FindFirstBuildCache(int scope,IntPtr condition,IntPtr cache,out IntPtr value);
 [PreserveSig] int FindAllBuildCache(int scope,IntPtr condition,IntPtr cache,out IntPtr value);
 [PreserveSig] int BuildUpdatedCache(IntPtr cache,out IntPtr value);
 [PreserveSig] int GetCurrentPropertyValue(int property,[MarshalAs(UnmanagedType.Struct)] out object value);
 [PreserveSig] int GetCurrentPropertyValueEx(int property,[MarshalAs(UnmanagedType.Bool)] bool ignoreDefault,[MarshalAs(UnmanagedType.Struct)] out object value);
}
[ComImport, Guid("14314595-b4bc-4055-95f2-58f2e42c9855"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface BowserNativeElementArray {
 [PreserveSig] int GetLength(out int length);
 [PreserveSig] int GetElement(int index,out BowserNativeElement element);
}
public static class BowserNativeDocument {
 static void Check(int hr) { Marshal.ThrowExceptionForHR(hr); }
 static void Release(object value) { if(value!=null&&Marshal.IsComObject(value))Marshal.ReleaseComObject(value); }
 static bool Same(int[] a,int[] b) { if(a==null||b==null||a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true; }
 public static string LegacyValue(IntPtr hwnd,uint expectedPid,int[] runtimeId) {
  if(runtimeId==null||runtimeId.Length==0||runtimeId.Length>128)throw new InvalidOperationException("Invalid native document identity.");
  uint pid;uint thread=BowserStoreSmoke.GetWindowThreadProcessId(hwnd,out pid);
  var cls=new StringBuilder(128);BowserStoreSmoke.GetClassName(hwnd,cls,128);
  if(thread==0||pid!=expectedPid||cls.ToString()!="BowserNativeWindow")throw new InvalidOperationException("Native document owner unavailable.");
  object instance=null;BowserNativeElement root=null;BowserNativeElementArray documents=null;IntPtr condition=IntPtr.Zero;
  try {
   instance=Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("ff48dba4-60ef-4201-aa87-54103eef594e"),true));
   var automation=(BowserNativeAutomation)instance;
   Check(automation.ElementFromHandle(hwnd,out root));Check(automation.CreatePropertyCondition(30003,50030,out condition));
   Check(root.FindAll(4,condition,out documents));int count;Check(documents.GetLength(out count));
   if(count<0||count>64)throw new InvalidOperationException("Native document inventory exceeded limit.");
   int matches=0;string result=null;
   for(int i=0;i<count;i++) {
    BowserNativeElement candidate=null;
    try { Check(documents.GetElement(i,out candidate));int[] before;Check(candidate.GetRuntimeId(out before));if(!Same(before,runtimeId))continue;
     matches++;object value=null;
     try { Check(candidate.GetCurrentPropertyValueEx(30093,true,out value));int[] after;Check(candidate.GetRuntimeId(out after));
      if(!Same(before,after))throw new InvalidOperationException("Native document identity changed during property query.");
      result=value as string;if(result!=null&&result.Length>4096)throw new InvalidOperationException("Native document URL exceeded limit.");
     } finally { Release(value); }
    } finally { Release(candidate); }
   }
   uint freshPid;uint freshThread=BowserStoreSmoke.GetWindowThreadProcessId(hwnd,out freshPid);cls.Clear();BowserStoreSmoke.GetClassName(hwnd,cls,128);
   if(freshPid!=expectedPid||freshThread!=thread||cls.ToString()!="BowserNativeWindow")throw new InvalidOperationException("Native document owner changed.");
   if(matches!=1)throw new InvalidOperationException("Native document runtime identity was unavailable or ambiguous.");
   return result;
  } finally { if(condition!=IntPtr.Zero)Marshal.Release(condition);Release(documents);Release(root);Release(instance); }
 }
}
'@
$app = $null
$saved = @{}
$report = [ordered]@{ scope='Actual original Chrome Web Store browsing plus native Bowser consent/management; not complete extension functionality'; status='failed'; commit=(& git -C $root rev-parse HEAD).Trim(); screenshots=@(); error=$null }
function Assert-Owned {
 $pidOwner=[uint32]0
 [void][BowserStoreSmoke]::GetWindowThreadProcessId($script:window,[ref]$pidOwner)
 $class=[Text.StringBuilder]::new(128);[void][BowserStoreSmoke]::GetClassName($script:window,$class,$class.Capacity)
 if($pidOwner -eq $app.Id -and $class.ToString() -eq '#32770') {
  $dialog=[Windows.Automation.AutomationElement]::FromHandle($script:window)
  $texts=$dialog.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
  $report.startup_error_dialog=@($texts | ForEach-Object {try{$_.Current.Name}catch{''}} | Where-Object {$_})
  throw ('Owned startup dialog, not browser UI: ' + ($report.startup_error_dialog -join ' | '))
 }
 if ($pidOwner -ne $app.Id -or ![BowserStoreSmoke]::IsWindowVisible($script:window) -or $class.ToString() -ne 'BowserNativeWindow') { throw ('Not the disposable native browser window. Class=' + $class.ToString()) }
}
function Keys([string]$Value) {
 Assert-Owned
 foreach($key in @(16,17,18,91,92)){if([BowserStoreSmoke]::GetAsyncKeyState($key) -lt 0){throw 'Unexpected held input modifier before owned chord; no repair.'}}
 [void][BowserStoreSmoke]::ShowWindow($script:window,9)
 for($attempt=0;$attempt -lt 3;$attempt++) {
  try { [Windows.Automation.AutomationElement]::FromHandle($script:window).SetFocus() } catch {}
  [void][BowserStoreSmoke]::SetForegroundWindow($script:window)
  Start-Sleep -Milliseconds 250
  if([BowserStoreSmoke]::GetForegroundWindow() -eq $script:window){break}
 }
 if ([BowserStoreSmoke]::GetForegroundWindow() -ne $script:window) { $report.foreground_window=[BowserStoreSmoke]::GetForegroundWindow().ToInt64();$report.owned_window=$script:window.ToInt64();throw 'Owned foreground unavailable; no keyboard sent.' }
 foreach($key in @(16,17,18,91,92)){if([BowserStoreSmoke]::GetAsyncKeyState($key) -lt 0){throw 'Unexpected held input modifier before owned chord dispatch; no repair.'}}
 [Windows.Forms.SendKeys]::SendWait($Value)
}
function Capture([string]$Name,[IntPtr]$Target=[IntPtr]::Zero,[switch]$Extension) {
 if($Target -eq [IntPtr]::Zero){$Target=$script:window}
 if($Extension){
  $owner=[uint32]0;[void][BowserStoreSmoke]::GetWindowThreadProcessId($Target,[ref]$owner)
  $class=[Text.StringBuilder]::new(128);[void][BowserStoreSmoke]::GetClassName($Target,$class,128)
  if($owner -ne $app.Id -or $class.ToString() -ne 'BowserExtensionPage' -or ![BowserStoreSmoke]::IsWindowVisible($Target)){throw 'Not the exact owned native extension page.'}
 }else{Assert-Owned}
 $rect=[BowserStoreSmoke+RECT]::new()
 if (![BowserStoreSmoke]::GetWindowRect($Target,[ref]$rect)) { throw 'No window bounds.' }
 $bitmap=[Drawing.Bitmap]::new($rect.Right-$rect.Left,$rect.Bottom-$rect.Top)
 $graphics=[Drawing.Graphics]::FromImage($bitmap); $dc=$graphics.GetHdc()
 try { $ok=[BowserStoreSmoke]::PrintWindow($Target,$dc,2) }
 finally { $graphics.ReleaseHdc($dc); $graphics.Dispose() }
 try {
  if (!$ok) { throw 'Native screenshot API failed.' }
  $colors=[Collections.Generic.HashSet[int]]::new()
  for($x=0;$x -lt $bitmap.Width;$x+=19) { for($y=0;$y -lt $bitmap.Height;$y+=19) { [void]$colors.Add($bitmap.GetPixel($x,$y).ToArgb()) } }
  if($colors.Count -lt 4) { throw 'Uniform/blank native GPU capture; rendering is inconclusive.' }
  $path=Join-Path $Out "$Name.png"; $bitmap.Save($path,[Drawing.Imaging.ImageFormat]::Png)
  $report.screenshots+=@{file="$Name.png";sha256=(Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant();sampled_colors=$colors.Count}
 } finally { $bitmap.Dispose() }
}
function Same-Uia($A,$B) { return $A -and $B -and (($A.GetRuntimeId() -join ',') -ceq ($B.GetRuntimeId() -join ',')) }
function Document-Source($Document) {
 # Chromium's native provider exposes tree-data URL for document roles, not an aria/title value.
 $values=@();$pattern=$null
 if($Document.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)){$values+=([Windows.Automation.ValuePattern]$pattern).Current.Value}
 $runtimeId=$Document.GetRuntimeId()
 $legacy=[BowserNativeDocument]::LegacyValue($script:window,[uint32]$app.Id,$runtimeId)
 if(($Document.GetRuntimeId() -join ',') -cne ($runtimeId -join ',')){throw 'Managed document identity changed during native property query.'}
 if($legacy){$values+=$legacy}
 $values=@($values|Where-Object {$_}|Select-Object -Unique)
 if($values.Count -ne 1 -or $values[0].Length -gt 4096){return $null}
 return [string]$values[0]
}
function Source-Matches([string]$Kind,[string]$Source) {
 if($Kind -eq 'toolbar'){return $Source -ceq 'bowser-ui://app/index.html?view=toolbar'}
 if($Kind -eq 'settings'){return $Source -ceq 'bowser-ui://app/index.html?view=settings'}
 $uri=$null
 if($Kind -eq 'loopback'){return [Uri]::TryCreate($Source,[UriKind]::Absolute,[ref]$uri) -and $uri.Scheme -ceq 'http' -and $uri.Host -ceq '127.0.0.1' -and !$uri.UserInfo}
 return $Kind -eq 'cws' -and [Uri]::TryCreate($Source,[UriKind]::Absolute,[ref]$uri) -and $uri.Scheme -ceq 'https' -and $uri.Host -ceq 'chromewebstore.google.com' -and !$uri.UserInfo -and $uri.IsDefaultPort
}
function Outermost-OwnedDocument($Document,$Owned) {
 $node=$Document
 for($depth=0;$depth -lt 64;$depth++){
  $node=[Windows.Automation.TreeWalker]::RawViewWalker.GetParent($node)
  if(!$node){return $false}
  if(Same-Uia $node $Owned){return $true}
  if($node.Current.ControlType -eq [Windows.Automation.ControlType]::Document){return $false}
 }
 return $false
}
function Find-Document([string]$Kind) {
 Assert-Owned;$owned=[Windows.Automation.AutomationElement]::FromHandle($script:window)
 $documents=$owned.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Document))
 if($documents.Count -gt 64){throw 'Native document inventory exceeded the bounded probe.'}
 $matches=@()
 foreach($document in $documents){
  if(!(Outermost-OwnedDocument $document $owned)){continue}
  $source=Document-Source $document
  if($source -and (Source-Matches $Kind $source)){$matches+=@{document=$document;source=$source;kind=$Kind}}
 }
 if($matches.Count -gt 1){throw 'Native document source attribution is ambiguous; no input sent.'}
 if($matches.Count -eq 1){return $matches[0]}
 return $null
}
function In-Document($Control,$Document) {
 $node=$Control
 for($depth=0;$depth -lt 64;$depth++){
  if(Same-Uia $node $Document){return $true}
  $node=[Windows.Automation.TreeWalker]::RawViewWalker.GetParent($node)
  if(!$node){return $false}
  if($node.Current.ControlType -eq [Windows.Automation.ControlType]::Document){return (Same-Uia $node $Document)}
 }
 return $false
}
function Find-BoundControl([string]$Kind,[string]$Name,[switch]$Prefix,[Windows.Automation.ControlType]$ControlType,$Within) {
 $document=Find-Document $Kind;if(!$document){return $null}
 $all=$document.document.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
 if($all.Count -gt 10000){throw 'Attributed document control inventory exceeded the bounded probe.'}
 $scope=$document.document
 if($Within){
  Assert-BoundControl $Within
  if($Within.kind -cne $Kind -or $Within.source -cne $document.source -or !(Same-Uia $Within.document $document.document)){throw 'Confirmation container is not in the exact current attributed document.'}
  $scope=$Within.control
  $all=$scope.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
  if($all.Count -gt 10000){throw 'Attributed confirmation inventory exceeded the bounded probe.'}
 }
 # Preserve the full-inventory refusal above, but ask the provider for exact-name candidates
 # before expensive per-node property/ancestry reads. Name never establishes document authority.
 if(!$Prefix){
  $all=$scope.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$Name))
  if($all.Count -gt 10000){throw 'Attributed filtered control inventory exceeded the bounded probe.'}
 }
 $matches=@()
 foreach($item in $all){
  if($ControlType -and $item.Current.ControlType -ne $ControlType){continue}
  if($Within -and !(In-Document $item $Within.control)){continue}
  if(!$item.Current.IsEnabled -or $item.Current.ControlType -eq [Windows.Automation.ControlType]::Text -or !(In-Document $item $document.document)){continue}
  $actual=$item.Current.Name
  if(($Prefix -and $actual.StartsWith($Name,[StringComparison]::Ordinal)) -or (!$Prefix -and $actual -ceq $Name)){$matches+=@{control=$item;document=$document.document;source=$document.source;kind=$Kind;name=$actual;controlType=$ControlType;within=$Within}}
 }
 if($matches.Count -gt 1){throw 'Attributed native control is ambiguous; no input sent.'}
 if($matches.Count -eq 1){return $matches[0]}
 return $null
}
function Wait-BoundControl([string]$Kind,[string]$Name,[Windows.Automation.ControlType]$ControlType,$Within) {
 $deadline=[DateTime]::UtcNow.AddSeconds(60)
 do{$bound=Find-BoundControl $Kind $Name -ControlType $ControlType -Within $Within;if($bound){return $bound};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow -lt $deadline)
 throw 'Required native document source/control was not exposed; no title/name fallback.'
}
function Assert-BoundControl($Bound,[switch]$Focused) {
 Assert-Owned
 $fresh=Find-BoundControl $Bound.kind $Bound.name -ControlType $Bound.controlType -Within $Bound.within
 if(!$fresh -or !(Same-Uia $fresh.document $Bound.document) -or !(Same-Uia $fresh.control $Bound.control) -or $fresh.source -cne $Bound.source){throw 'Native document/control source changed; no input sent.'}
 if([BowserStoreSmoke]::GetForegroundWindow() -ne $script:window){throw 'Exact native foreground lost; no input sent.'}
 $bounds=$fresh.control.Current.BoundingRectangle
 if($fresh.control.Current.IsOffscreen -or $bounds.Width -le 0 -or $bounds.Height -le 0){throw 'Attributed native control is not visible; no input sent.'}
 if($Focused -and (!$fresh.control.Current.HasKeyboardFocus -or !(Same-Uia $fresh.control ([Windows.Automation.AutomationElement]::FocusedElement)))){throw 'Exact attributed field is not focused; no input sent.'}
}
function Field-Keys($Bound,[string]$Value) {
 Assert-BoundControl $Bound -Focused
 foreach($key in @(16,17,18,91,92)){if([BowserStoreSmoke]::GetAsyncKeyState($key) -lt 0){throw 'Unexpected held input modifier; no input repair.'}}
 [Windows.Forms.SendKeys]::SendWait($Value)
}
function Field-Enter($Bound,[string]$Expected) {
 Assert-BoundControl $Bound -Focused
 foreach($key in @(16,17,18,91,92)){if([BowserStoreSmoke]::GetAsyncKeyState($key) -lt 0){throw 'Unexpected held modifier before exact text submission; no repair.'}}
 $pattern=$null
 if(!$Bound.control.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)){throw 'Exact submission ValuePattern unavailable; no Enter.'}
 if(!$Bound.control.Current.HasKeyboardFocus -or !(Same-Uia $Bound.control ([Windows.Automation.AutomationElement]::FocusedElement))){throw 'Exact submission focus changed; no Enter.'}
 $actual=$pattern.Current.Value
 if($actual -cne $Expected){throw 'Exact submitted text changed after source/input guards; no Enter or repair.'}
 $report.last_enter=[ordered]@{expected=$Expected;actual=$actual;source=$Bound.source;method='single real Enter after fresh source, identity, neutral modifier, focus and exact value checks'}
 [Windows.Forms.SendKeys]::SendWait('{ENTER}')
}
function Field-LiteralText($Bound,[string]$Text) {
 # A new correctness workload, not a SetValue repair or typing-latency benchmark.
 # Only the fixed test URL/search alphabet is allowed; SendKeys control characters are refused.
 if($Text.Length -gt 512 -or $Text -cnotmatch '^[A-Za-z0-9:/._?=&-]+$'){throw 'Fixed native text input contains an unsupported literal character; no input sent.'}
 $trace=[ordered]@{kind=$Bound.kind;method='real per-character SendKeys, minimum 30ms plus source-bound UIA observations';expected=$Text;keys=@();final_exact=$false}
 if(!$report.Contains('literal_input')){$report.literal_input=@()};$report.literal_input+=,$trace
 Field-Keys $Bound '^a';Field-Keys $Bound '{BACKSPACE}';Start-Sleep -Milliseconds 30
 Assert-BoundControl $Bound -Focused
 $pattern=$null
 if(!$Bound.control.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)){throw 'Attributed text field ValuePattern unavailable; no text sent.'}
 $trace.clear_value=$pattern.Current.Value
 if($trace.clear_value -cne ''){throw 'Genuine select-all/backspace did not clear the exact field; no repair.'}
 foreach($character in $Text.ToCharArray()){
  $key=[ordered]@{character=[string]$character;phase='before-source-check';dispatch_returned=$false;before=$null;after=$null;focused=$null;error=$null}
  $trace.keys+=,$key
  try {
   Assert-BoundControl $Bound -Focused
   $key.before=$pattern.Current.Value;$key.phase='guard-and-dispatch'
   Field-Keys $Bound ([string]$character)
   $key.dispatch_returned=$true;$key.phase='post-key-source-check'
   Start-Sleep -Milliseconds 30
   Assert-BoundControl $Bound -Focused
   $key.after=$pattern.Current.Value;$key.focused=$Bound.control.Current.HasKeyboardFocus;$key.phase='observed'
  } catch { $key.error=$_.Exception.Message;throw }
 }
 Assert-BoundControl $Bound -Focused
 $trace.final_value=$pattern.Current.Value;$trace.final_focused=$Bound.control.Current.HasKeyboardFocus
 $trace.final_exact=($trace.final_value -ceq $Text -and $trace.final_focused)
 if(!$trace.final_exact){throw 'Real literal input did not retain exact text/focus; no Enter, repair or retry.'}
}
function Invoke-BoundControl($Bound) {
 Assert-BoundControl $Bound
 $pattern=$null
 if(!$Bound.control.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){throw 'Attributed native control has no InvokePattern; no fallback.'}
 $pattern.Invoke();Start-Sleep -Milliseconds 400
}
function Wait-CwsPath([string]$Path) {
 $deadline=[DateTime]::UtcNow.AddSeconds(20)
 do{$document=Find-Document 'cws';if($document -and ([Uri]$document.source).AbsolutePath -ceq $Path){return $document};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $deadline)
 throw 'Actual native Chrome Web Store document did not transition to the required route.'
}
function Cws-ListingId {
 $document=Find-Document 'cws';if(!$document){return $null}
 $uri=[Uri]$document.source
 if($uri.AbsolutePath -cmatch '^/detail/(?:[^/]+/)?([a-p]{32})/?$'){return $Matches[1]}
 return $null
}
function Find-Control([string]$Name, [switch]$Prefix) {
 $bound=Find-BoundControl 'settings' $Name -Prefix:$Prefix
 if(!$bound){return $null}
 if($bound.control.Current.IsOffscreen){
  $scroll=$null
  if($bound.control.TryGetCurrentPattern([Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}else{$bound.control.SetFocus()}
  Start-Sleep -Milliseconds 300
  $fresh=Find-BoundControl 'settings' $bound.name
  if(!$fresh -or !(Same-Uia $bound.document $fresh.document) -or !(Same-Uia $bound.control $fresh.control) -or $fresh.source -cne $bound.source){throw 'Trusted review source/control changed while revealing it.'}
  $bound=$fresh
 }
 return $bound.control
}
function Wait-Control([string]$Name,[switch]$Prefix) {
 $deadline=[DateTime]::UtcNow.AddSeconds(90)
 do { $found=Find-Control $Name -Prefix:$Prefix; if($found){return $found}; $app.Refresh();if($app.HasExited){throw 'Native app exited during journey.'};Start-Sleep -Milliseconds 300 } while([DateTime]::UtcNow -lt $deadline)
 try {
  $owned=[Windows.Automation.AutomationElement]::FromHandle($script:window)
  @($owned.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object{$_.Current.Name}) | ConvertTo-Json | Set-Content (Join-Path $Out 'last-accessibility-text.json') -Encoding utf8
  Capture '99-native-failure-state'
 } catch {}
 throw "Native accessible control did not appear: $Name"
}
function Invoke-Control($Control) {
 $bound=Find-BoundControl 'settings' $Control.Current.Name
 if(!$bound -or !(Same-Uia $bound.control $Control)){throw 'Trusted review control changed before invocation.'}
 Assert-BoundControl $bound
 $pattern=$null
 if(!$Control.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){throw "No native InvokePattern for $($Control.Current.Name)"}
 $pattern.Invoke();Start-Sleep -Milliseconds 400
}
function Wait-Window {
 $deadline=[DateTime]::UtcNow.AddSeconds(60);$script:window=[IntPtr]::Zero
 do{Start-Sleep -Milliseconds 200;$app.Refresh();if($app.HasExited){throw 'Disposable native app exited.'};$script:window=$app.MainWindowHandle}while($script:window -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
 Assert-Owned;[void][BowserStoreSmoke]::MoveWindow($script:window,20,20,1100,800,$true);Start-Sleep -Seconds 5
}
function Toggle-Control($Control){
 $bound=Find-BoundControl 'settings' $Control.Current.Name
 if(!$bound -or !(Same-Uia $bound.control $Control)){throw 'Trusted management control changed before toggle.'}
 Assert-BoundControl $bound
 $pattern=$null
 if($Control.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){$pattern.Toggle();return}
 if($Control.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke();return}
 throw 'The owned extension switch exposes neither a native toggle nor invoke action.'
}
function Wait-RecordFlag([string]$File,[int]$Flag){
 $deadline=[DateTime]::UtcNow.AddSeconds(15)
 do{if((Get-Content -LiteralPath $File -Raw) -match ('^ext-[a-f0-9]{16}\t'+$Flag+'\t')){return};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow -lt $deadline)
 throw "The native switch did not durably commit flag $Flag."
}
function Wait-NativeReady([string]$Token){
 $deadline=[DateTime]::UtcNow.AddSeconds(60)
 do{if((Test-Path -LiteralPath $readyFile) -and ((Get-Content -LiteralPath $readyFile -Raw) -eq $Token)){return};$app.Refresh();if($app.HasExited){throw 'The owned app exited before its real browsing page loaded.'};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $deadline)
 throw 'The actual native browsing WebView did not acknowledge its unique loopback fixture.'
}
function Records {
 # The exact disposable owning profile record, never a recursive scan of package folders that
 # may legitimately be removed while a cancelled download or uninstall finishes.
 $file=Join-Path $env:BOWSER_DATA_DIR 'extensions.txt'
 if((Test-Path -LiteralPath $file -PathType Leaf) -and ((Get-Content -LiteralPath $file -Raw) -match '\S')){return @(Get-Item -LiteralPath $file)}
 return @()
}
function Navigate-Owned([string]$Url) {
 Assert-Owned;Keys '^l'
 $bound=Wait-BoundControl 'toolbar' 'Address and search bar'
 Field-LiteralText $bound $Url
 Field-Enter $bound $Url;Start-Sleep -Seconds 3
}
function Open-ListingReview([string]$Id) {
 if($Id -cnotmatch '^[a-p]{32}$'){throw 'Invalid fixed test listing ID.'}
 Navigate-Owned "https://chromewebstore.google.com/detail/$Id"
 # This control is Bowser chrome, offered only after the actual top-level listing commits.
 $add=Wait-BoundControl 'toolbar' 'Add to Bowser'
 if((Cws-ListingId) -cne $Id){throw 'Actual native website document did not commit the expected listing ID.'}
 Invoke-BoundControl $add
}
function Capture-CwsControls {
 $document=Find-Document 'cws';if(!$document){return}
 $items=$document.document.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
 if($items.Count -gt 10000){throw 'CWS diagnostic inventory exceeded the unchanged cap.'}
 $rows=@()
 for($i=0;$i -lt [Math]::Min($items.Count,256);$i++){
  $item=$items[$i]
  if(!(In-Document $item $document.document)){continue}
  $name=$item.Current.Name;if($name.Length -gt 256){$name=$name.Substring(0,256)}
  $rows+=@{name=$name;type=$item.Current.ControlType.ProgrammaticName;enabled=$item.Current.IsEnabled;offscreen=$item.Current.IsOffscreen}
 }
 $fresh=Find-Document 'cws'
 if(!$fresh -or !(Same-Uia $fresh.document $document.document) -or $fresh.source -cne $document.source){throw 'CWS diagnostic source changed during enumeration; candidates refused.'}
 $rows|ConvertTo-Json -Depth 4|Set-Content (Join-Path $Out 'cws-accessibility-candidates.json') -Encoding utf8
}
function Probe-OriginalDiscovery {
 $result=[ordered]@{status='failed';category=$false;search=$false;listing=$false;error=$null}
 try {
  Navigate-Owned 'bowser://store';Capture '01-original-chrome-web-store'
  [void](Wait-CwsPath '/category/extensions');$result.category=$true
  # Ordinary Google promotional choice, not a browser security warning or permission bypass.
  $dismiss=Find-BoundControl 'cws' 'No thanks'
  if($dismiss){
   if($dismiss.control.Current.ControlType -ne [Windows.Automation.ControlType]::Button){throw 'Google promotional dismissal was not an actual attributed button.'}
   Invoke-BoundControl $dismiss;$report.google_promotion='native-source-bound-no-thanks-invoked'
   [void](Wait-CwsPath '/category/extensions')
  }
  $bound=Wait-BoundControl 'cws' 'Search Chrome Web Store' -ControlType ([Windows.Automation.ControlType]::ComboBox);$search=$bound.control
  if($search.Current.ControlType -ne [Windows.Automation.ControlType]::ComboBox){throw 'Original CWS search was not the observed exact native ComboBox.'}
  $search.SetFocus();Field-LiteralText $bound 'Stylus'
  $value=$null
  if(!$search.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$value) -or $value.Current.Value -cne 'Stylus' -or !$search.Current.HasKeyboardFocus){throw 'Original CWS search did not retain exact native input/focus; no Enter sent.'}
  Field-Enter $bound 'Stylus'
  [void](Wait-CwsPath '/search/Stylus');Capture '02-original-cws-search'
  $resultBound=Wait-BoundControl 'cws' 'Stylus';$listing=$resultBound.control
  if($listing.Current.ControlType -ne [Windows.Automation.ControlType]::Hyperlink){throw 'Original CWS search result was not an actual hyperlink.'}
  $result.search=$true;Invoke-BoundControl $resultBound
  [void](Wait-BoundControl 'toolbar' 'Add to Bowser')
  if((Cws-ListingId) -cne 'clngdbkpkpeebahjckkjfobafhncgmne'){throw 'Native CWS result navigation did not commit the actual Stylus listing ID.'}
  Capture '03-original-cws-stylus-listing'
  $result.listing=$true;$result.status='passed-native-category-search-listing-owned-add'
 } catch {
  $result.error=$_.Exception.Message
  try{Capture '03-original-cws-discovery-unavailable'}catch{}
  try{Capture-CwsControls}catch{$report.cws_candidate_capture_error=$_.Exception.Message}
 }
 $report.original_cws_discovery=$result
}
function Run-InstallJourney {
 # These are standard Windows accessibility actions against this exact disposable app window.
 # No WebView script, test bridge, native consent bypass or user credential is involved.
 Open-ListingReview 'ddkjiahejlhfcafbddmgiahcphecmpfh'
 Capture '04-native-original-listing-to-review'
 $approve=Wait-Control 'Install uBlock Origin Lite ' -Prefix
 if(@(Records).Count -ne 0){throw 'An extension was recorded before consent.'}
 Capture '05-native-full-permission-review'
 $report.review_button=$approve.Current.Name
 $reviewDocument=Find-Document 'settings';if(!$reviewDocument){throw 'Trusted native review document source unavailable.'};$owned=$reviewDocument.document
 $texts=$owned.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
 @($texts | ForEach-Object { try{$_.Current.Name}catch{''} } | Where-Object {$_}) | ConvertTo-Json | Set-Content (Join-Path $Out 'review-accessibility-text.json') -Encoding utf8
 Invoke-Control (Wait-Control 'Cancel installation')
 if(@(Records).Count -ne 0){throw 'Cancellation recorded an installation.'}
 $report.cancel_before_consent='passed-no-record'
 Capture '06-native-cancelled'
 # A separate actual signed appearance package supplies real static script declarations for
 # native visual review. This review is cancelled; it grants no installation consent.
 Open-ListingReview 'clngdbkpkpeebahjckkjfobafhncgmne'
 $appearanceApprove=Wait-Control 'Install Stylus ' -Prefix
 if(@(Records).Count -ne 0){throw 'Appearance review recorded an extension before consent.'}
 $reviewDocument=Find-Document 'settings';if(!$reviewDocument){throw 'Trusted native review document source unavailable.'};$owned=$reviewDocument.document
 $all=$owned.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
 $declaration=$null
 foreach($item in $all){try{if($item.Current.Name.StartsWith('{"declared":',[StringComparison]::Ordinal) -and $item.Current.Name.Contains('documented_defaults_for_omitted_fields')){$declaration=$item;break}}catch{}}
 if(!$declaration){throw 'Actual appearance-package complete script declaration was not exposed in native review.'}
 $text=$declaration.Current.Name;$parsed=$text|ConvertFrom-Json
 if(!$parsed.declared -or !$parsed.documented_defaults_for_omitted_fields){throw 'Actual native script review omitted declarations/defaults objects.'}
 $declared=$parsed.declared;$defaults=$parsed.documented_defaults_for_omitted_fields
 foreach($key in @('all_frames','match_about_blank','match_origin_as_fallback','world','run_at')){
  $property=$declared.PSObject.Properties|Where-Object {$_.Name -ceq $key}|Select-Object -First 1
  if(!$property){$property=$defaults.PSObject.Properties|Where-Object {$_.Name -ceq $key}|Select-Object -First 1}
  if(!$property){throw "Actual native script review omitted effective $key."}
  $effective=$property.Value
  if($key -in @('all_frames','match_about_blank','match_origin_as_fallback')){if($effective -isnot [bool]){throw "Native $key review is not a Boolean value."}}
  elseif($key -eq 'world'){if($effective -isnot [string] -or $effective -cnotin @('ISOLATED','MAIN')){throw 'Native world review is not a documented string enum.'}}
  elseif($effective -isnot [string] -or $effective -cnotin @('document_start','document_end','document_idle')){throw 'Native run_at review is not a documented string enum.'}
 }
 $matchesProperty=$declared.PSObject.Properties|Where-Object {$_.Name -ceq 'matches'}|Select-Object -First 1
 if(!$matchesProperty -or $matchesProperty.Value -isnot [Array] -or $matchesProperty.Value.Count -eq 0 -or @($matchesProperty.Value|Where-Object {$_ -isnot [string] -or !$_}).Count -ne 0){throw 'Actual native script review has no nonempty declared match array.'}
 $hasFiles=$false
 foreach($key in @('js','css')){
  $property=$declared.PSObject.Properties|Where-Object {$_.Name -ceq $key}|Select-Object -First 1
  if($property){if($property.Value -isnot [Array] -or @($property.Value|Where-Object {$_ -isnot [string] -or !$_}).Count -ne 0){throw 'Actual native js/css declaration is not a string array.'};if($property.Value.Count -gt 0){$hasFiles=$true}}
 }
 if(!$hasFiles){throw 'Actual native script review has no declared script/style files.'}
 $scroll=$null
 if($declaration.TryGetCurrentPattern([Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}
 else {
  for($i=0;$i -lt 12 -and $declaration.Current.IsOffscreen;$i++){
   foreach($item in $all){try{$pattern=$null;if($item.TryGetCurrentPattern([Windows.Automation.ScrollPattern]::Pattern,[ref]$pattern) -and $pattern.Current.VerticallyScrollable){$pattern.Scroll([Windows.Automation.ScrollAmount]::NoAmount,[Windows.Automation.ScrollAmount]::LargeIncrement)}}catch{}}
   Start-Sleep -Milliseconds 200
  }
 }
 Start-Sleep -Milliseconds 400
 $bounds=$declaration.Current.BoundingRectangle
 if($declaration.Current.IsOffscreen -or $bounds.Width -le 0 -or $bounds.Height -le 0){throw 'Complete actual script declaration could not be brought into native view.'}
 Capture '05b-native-stylus-complete-script-review'
 $text|Set-Content (Join-Path $Out 'native-complete-script-review.txt') -Encoding utf8
 $report.complete_static_script_review='actual-signed-Stylus-native-visible-full-frame-world-timing-declaration-with-defaults'
 Invoke-Control (Wait-Control 'Cancel installation')
 if(@(Records).Count -ne 0){throw 'Cancelled appearance review recorded an installation.'}
 Open-ListingReview 'ddkjiahejlhfcafbddmgiahcphecmpfh'
 $approve=Wait-Control 'Install uBlock Origin Lite ' -Prefix
 # Explicit authorized disposable-test consent to the real displayed package/version/permissions.
 $report.explicit_test_consent=$approve.Current.Name
 Invoke-Control $approve
 Invoke-Control (Wait-Control 'Manage installed extensions')
 $deadline=[DateTime]::UtcNow.AddSeconds(30)
 while(@(Records).Count -eq 0 -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 300}
 if(@(Records).Count -ne 1){throw 'Expected exactly one disposable owning-profile extension record.'}
 $recordFile=(Records)[0].FullName
 $report.installed_record=Get-Content $recordFile -Raw
 Capture '07-native-installed-before-restart'
 # Real process restart of the same persistent disposable profile, not a fresh engine simulation.
 if(!$app.CloseMainWindow()){throw 'The owned app did not accept a normal close request.'}
 if(!$app.WaitForExit(15000)){throw 'The owned app did not close cleanly; restart acceptance not attempted.'}
 Start-Sleep -Seconds 2
 $readyToken=[guid]::NewGuid().ToString('N');$initialUrl="$base/restart?token=$readyToken"
 $script:app=Start-Process $exe -ArgumentList @('--dev','--data-dir',"`"$env:BOWSER_DATA_DIR`"",'--open-url',$initialUrl) -WorkingDirectory $root -PassThru
 Wait-Window
 Wait-NativeReady $readyToken
 $report.restart_browsing_ready='unique-loopback-acknowledged'
 Navigate-Owned 'bowser://settings/extensions'
 $popup=Wait-Control 'Open uBlock Origin Lite popup'
 $report.restart_persisted='record-and-running-popup-control-present'
 Capture '08-native-installed-after-restart'
 Invoke-Control $popup
 Start-Sleep -Seconds 3
 # Popup hosting is genuine; this does not by itself prove blocker behavior or targeting.
 $report.popup_invoked='native-owned-extension-popup-control'
 Capture '09-native-popup-invoked'
 $windows=[Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty,$app.Id))
 $popupWindows=@()
 foreach($candidate in $windows){$handle=[IntPtr]$candidate.Current.NativeWindowHandle;$class=[Text.StringBuilder]::new(128);[void][BowserStoreSmoke]::GetClassName($handle,$class,128);if($class.ToString() -eq 'BowserExtensionPage'){$popupWindows+=,$handle}}
 if($popupWindows.Count -ne 1){throw 'The actual native extension popup window was absent or ambiguous.'}
 $popupWindow=$popupWindows[0]
 $popupInitialPid=[uint32]0;$popupThread=[BowserStoreSmoke]::GetWindowThreadProcessId($popupWindow,[ref]$popupInitialPid)
 if($popupInitialPid -ne $app.Id -or $popupThread -eq 0){throw 'Exact popup owner unavailable.'}
 Capture '09b-native-popup-content' -Target $popupWindow -Extension
 $popupElement=[Windows.Automation.AutomationElement]::FromHandle($popupWindow)
 $popupRuntimeId=$popupElement.GetRuntimeId()
 @($popupElement.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object{$_.Current.Name})|ConvertTo-Json|Set-Content (Join-Path $Out 'popup-accessibility-text.json') -Encoding utf8
 # Explicitly finish the popup user task before a separate Settings interaction.
 $popupPid=[uint32]0;[void][BowserStoreSmoke]::GetWindowThreadProcessId($popupWindow,[ref]$popupPid)
 $popupClass=[Text.StringBuilder]::new(128);[void][BowserStoreSmoke]::GetClassName($popupWindow,$popupClass,128)
 if($popupPid -ne $app.Id -or $popupClass.ToString() -ne 'BowserExtensionPage' -or ![BowserStoreSmoke]::IsWindowVisible($popupWindow)){throw 'Exact popup identity changed before ordinary close.'}
 $close=$null
 if(!$popupElement.TryGetCurrentPattern([Windows.Automation.WindowPattern]::Pattern,[ref]$close)){throw 'Owned popup has no ordinary WindowPattern close; no fallback.'}
 $freshPopup=[Windows.Automation.AutomationElement]::FromHandle($popupWindow)
 $freshPid=[uint32]0;$freshThread=[BowserStoreSmoke]::GetWindowThreadProcessId($popupWindow,[ref]$freshPid)
 $popupClass.Clear();[void][BowserStoreSmoke]::GetClassName($popupWindow,$popupClass,128)
 if(![BowserStoreSmoke]::IsWindow($popupWindow) -or $freshPid -ne $app.Id -or $freshThread -ne $popupThread -or $popupClass.ToString() -ne 'BowserExtensionPage' -or !(Same-Uia $freshPopup $popupElement) -or (($freshPopup.GetRuntimeId() -join ',') -cne ($popupRuntimeId -join ','))){throw 'Exact popup native/runtime identity changed after obtaining close pattern; no close.'}
 $close.Close()
 $deadline=[DateTime]::UtcNow.AddSeconds(10)
 while([BowserStoreSmoke]::IsWindow($popupWindow) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 100}
 if([BowserStoreSmoke]::IsWindow($popupWindow)){throw 'Ordinary popup close did not destroy its native window; no Settings input.'}
 $report.popup_closed='native-owned-window-pattern-close-observed'
 Assert-Owned;[void][BowserStoreSmoke]::ShowWindow($script:window,9)
 [void][BowserStoreSmoke]::SetForegroundWindow($script:window)
 # The owned native root need not itself support UIA focus; exact foreground remains mandatory.
 if([BowserStoreSmoke]::GetForegroundWindow() -ne $script:window){throw 'Separate Settings task could not activate the exact owned main window.'}
 # Toggle via the actual accessible switch, then re-enable before removal.
 $toggle=Wait-Control 'uBlock Origin Lite on';Toggle-Control $toggle
 Wait-RecordFlag $recordFile 0
 $report.disabled_record=Get-Content $recordFile -Raw
 $toggle=Wait-Control 'uBlock Origin Lite on';Toggle-Control $toggle
 Wait-RecordFlag $recordFile 1
 Invoke-Control (Wait-Control 'Remove uBlock Origin Lite')
 $confirm=Wait-BoundControl 'settings' 'Confirm removing uBlock Origin Lite'
 $remove=Wait-BoundControl 'settings' 'Remove uBlock Origin Lite' -ControlType ([Windows.Automation.ControlType]::Button) -Within $confirm
 Invoke-BoundControl $remove
 $deadline=[DateTime]::UtcNow.AddSeconds(30)
 while(@(Records).Count -ne 0 -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 300}
 if(@(Records).Count -ne 0){throw 'Native removal did not clear the owning profile record.'}
 $report.removal='passed-no-record'
 Capture '10-native-removed'
}

try {
 foreach($name in @('LOCALAPPDATA','BOWSER_DATA_DIR','BOWSER_UI')) { $saved[$name]=[Environment]::GetEnvironmentVariable($name,'Process'); [Environment]::SetEnvironmentVariable($name,$null,'Process') }
 $env:LOCALAPPDATA=Join-Path $run 'localappdata'; $env:BOWSER_DATA_DIR=Join-Path $run 'appdata'
 New-Item -ItemType Directory -Force $env:LOCALAPPDATA,$env:BOWSER_DATA_DIR | Out-Null
 $server=Start-Process (Get-Command node).Source -ArgumentList @("`"$(Join-Path $run 'server.cjs')`"","`"$portFile`"","`"$readyFile`"") -PassThru -WindowStyle Hidden
 $deadline=[DateTime]::UtcNow.AddSeconds(15);while(!(Test-Path -LiteralPath $portFile) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 50}
 if(!(Test-Path -LiteralPath $portFile)){throw 'The disposable loopback fixture did not start.'}
 $base='http://127.0.0.1:'+(Get-Content -LiteralPath $portFile -Raw).Trim();$readyToken=[guid]::NewGuid().ToString('N');$initialUrl="$base/start?token=$readyToken"
 # --dev skips production updater; unlike benchmark mode it does not disable extension startup.
 $app=Start-Process $exe -ArgumentList @('--dev','--data-dir',"`"$env:BOWSER_DATA_DIR`"",'--open-url',$initialUrl) -WorkingDirectory $root -PassThru
 $deadline=[DateTime]::UtcNow.AddSeconds(60); $script:window=[IntPtr]::Zero
 do { Start-Sleep -Milliseconds 200; $app.Refresh(); if($app.HasExited){throw 'Disposable native app exited.'}; $script:window=$app.MainWindowHandle } while($script:window -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
 Wait-NativeReady $readyToken
 $report.initial_browsing_ready='unique-loopback-acknowledged'
 Assert-Owned; [void][BowserStoreSmoke]::MoveWindow($script:window,20,20,1100,800,$true)
 Start-Sleep -Seconds 5
 Capture '00-native-control-before-keyboard'
 [void](Wait-BoundControl 'toolbar' 'Address and search bar')
 $spoof=Find-Document 'loopback'
 if(!$spoof -or $spoof.source -cne $initialUrl){throw 'Actual controlled spoof document source was not exposed.'}
 foreach($label in @('Add to Bowser','Address and search bar','Search Chrome Web Store')){
  $spoofs=$spoof.document.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$label))
  if($spoofs.Count -ne 1){throw 'Required website spoof-label positive control was not exposed.'}
 }
 if(Find-BoundControl 'toolbar' 'Add to Bowser'){throw 'Install control was offered on the actual non-listing loopback page.'}
 $report.nonlisting_add_absent=$true
 Probe-OriginalDiscovery
 if($InstallJourney){Run-InstallJourney}
 if($report.original_cws_discovery.status -ne 'passed-native-category-search-listing-owned-add'){throw 'Original Chrome Web Store discovery/search/listing is not verified; see retained native screenshots/report.'}
 $app.Refresh(); $report.native_process_working_set_bytes=$app.WorkingSet64
 $report.status=if($InstallJourney){'native-journey-awaiting-visual-review-not-functional-compatibility'}else{'captured-awaiting-visual-review'}
} catch {
 $report.error=$_.Exception.Message
 try{Capture '99-native-failure-state';$owned=[Windows.Automation.AutomationElement]::FromHandle($script:window);@($owned.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object{$_.Current.Name})|ConvertTo-Json|Set-Content (Join-Path $Out 'last-accessibility-text.json') -Encoding utf8}catch{}
 throw
}
finally {
 if($app -and !$app.HasExited) { Stop-Process -Id $app.Id -Force; $app.WaitForExit(10000) | Out-Null }
 if($server -and !$server.HasExited){Stop-Process -Id $server.Id -Force}
 foreach($entry in $saved.GetEnumerator()){[Environment]::SetEnvironmentVariable($entry.Key,$entry.Value,'Process')}
 $report | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $Out 'native-store-report.json') -Encoding utf8
 # Only our randomly named disposable root. No installed browser/user profile is touched.
 for($i=0;$i -lt 10 -and (Test-Path $run);$i++){try{Remove-Item -LiteralPath $run -Recurse -Force}catch{Start-Sleep -Milliseconds 500}}
}
