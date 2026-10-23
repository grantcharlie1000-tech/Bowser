[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Pure fake-API tests. No native clipboard/window adapter is loaded or invoked here.
$source = Join-Path $PSScriptRoot 'native-clipboard-policy.cs'
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw 'Clipboard mutation policy source is missing.' }
Add-Type -TypeDefinition ([IO.File]::ReadAllText($source) + @'
public static class BowserClipboardPolicyTests {
    static void Require(bool value,string message) { if(!value) throw new System.Exception(message); }
    sealed class Mutation : IBowserClipboardMutation {
        public uint Value; public int EmptyCalls,SetCalls,OwnedCalls;
        public uint Sequence() {return Value;}
        public bool Empty() {EmptyCalls++;Value++;return true;}
        public bool SetGenerated() {SetCalls++;Value++;return true;}
        public void RecordOwned(uint value) {OwnedCalls++;}
    }
    sealed class Fixture : IBowserCiClipboardFixture {
        public bool OwnWindow=true,OpenOk=true,CloseOk=true,EmptyOk=true,Advance=true,OwnerError;
        public uint CurrentSequence,Format; public int FormatError;
        public long CurrentOwner; public int OpenCalls,EmptyCalls,CloseCalls;
        public long HelperWindow {get{return 7;}}
        public bool OwnedWindow() {return OwnWindow;}
        public bool Open() {OpenCalls++;return OpenOk;}
        public long Owner() {if(OwnerError)throw new System.InvalidOperationException("owner unavailable");return CurrentOwner;}
        public uint FirstFormat(out int error) {error=FormatError;return Format;}
        public uint Sequence() {return CurrentSequence;}
        public bool Empty() {EmptyCalls++;if(EmptyOk){CurrentOwner=HelperWindow;if(Advance)CurrentSequence++;}return EmptyOk;}
        public bool Close() {CloseCalls++;return CloseOk;}
    }
    static BowserCiClipboardResult Prepare(Fixture api,bool verified) {
        return BowserCiClipboardPolicy.Prepare(api,verified,result=>{});
    }
    public static int Run() {
        int passed=0;
        var zero=new Mutation(); bool rejected=false;
        try {BowserClipboardMutationPolicy.WriteGenerated(zero);} catch(System.InvalidOperationException){rejected=true;}
        Require(rejected&&zero.EmptyCalls==0&&zero.SetCalls==0&&zero.OwnedCalls==0,"Guard zero must execute NO Empty/Set/ownership writes.");passed++;
        var positive=new Mutation{Value=10};BowserClipboardMutationPolicy.WriteGenerated(positive);
        Require(positive.EmptyCalls==1&&positive.SetCalls==1&&positive.OwnedCalls==2,"Actual guard mutation path order lost.");passed++;
        var unowned=new Fixture();var result=Prepare(unowned,false);
        Require(!result.Prepared&&unowned.OpenCalls==0&&unowned.EmptyCalls==0,"Unverified job must not open/mutate clipboard.");passed++;
        var locked=new Fixture{OpenOk=false};result=Prepare(locked,true);
        Require(!result.Prepared&&locked.EmptyCalls==0&&locked.CloseCalls==0,"Locked clipboard must not be cleared or retried.");passed++;
        var foreign=new Fixture{CurrentOwner=99};result=Prepare(foreign,true);
        Require(!result.Prepared&&foreign.EmptyCalls==0&&foreign.CloseCalls==1,"Foreign owner must remain untouched.");passed++;
        var unknownOwner=new Fixture{OwnerError=true};result=Prepare(unknownOwner,true);
        Require(!result.Prepared&&unknownOwner.EmptyCalls==0&&unknownOwner.CloseCalls==1,"Unknown owner must remain untouched.");passed++;
        var nonempty=new Fixture{Format=13};result=Prepare(nonempty,true);
        Require(!result.Prepared&&nonempty.EmptyCalls==0&&nonempty.CloseCalls==1,"Nonempty clipboard must remain untouched.");passed++;
        var unknown=new Fixture{FormatError=5};result=Prepare(unknown,true);
        Require(!result.Prepared&&unknown.EmptyCalls==0&&unknown.CloseCalls==1,"Unknown empty state must remain untouched.");passed++;
        var fresh=new Fixture();result=Prepare(fresh,true);
        Require(result.Prepared&&fresh.EmptyCalls==1&&fresh.CloseCalls==1&&result.FinalSequence==1,"Verified empty fixture must use one clear and checked nonzero sequence.");passed++;
        var usable=new Fixture{CurrentSequence=17};result=Prepare(usable,true);
        Require(result.Prepared&&usable.EmptyCalls==0&&usable.CloseCalls==1,"Already usable empty baseline must not be cleared.");passed++;
        var stillZero=new Fixture{Advance=false};result=Prepare(stillZero,true);
        Require(!result.Prepared&&stillZero.EmptyCalls==1&&stillZero.CloseCalls==1,"Zero after preparation must refuse without another clear.");passed++;
        var closeFailed=new Fixture{CloseOk=false};result=Prepare(closeFailed,true);
        Require(!result.Prepared&&closeFailed.EmptyCalls==1&&closeFailed.CloseCalls==1,"Failed close must refuse without retry.");passed++;
        var journalFailed=new Fixture();result=BowserCiClipboardPolicy.Prepare(journalFailed,true,r=>{throw new System.Exception("fake journal failure");});
        Require(!result.Prepared&&journalFailed.EmptyCalls==0&&journalFailed.CloseCalls==1,"Missing write-ahead record must prevent EmptyClipboard.");passed++;
        return passed;
    }
}
'@)
$count = [BowserClipboardPolicyTests]::Run()
$guard = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'probe-app-host-context-menu.ps1'))
if (-not $guard.Contains('BowserClipboardMutationPolicy.WriteGenerated(')) { throw 'Real ClipboardGuard does not use the tested mutation entry point.' }
Write-Output "Clipboard policy regressions: $count passed; guard zero -> Empty=0, Set=0; fake APIs only."
