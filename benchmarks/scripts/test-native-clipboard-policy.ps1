[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Pure fake-API tests. No native clipboard/window adapter is loaded or invoked here.
$source = Join-Path $PSScriptRoot 'native-clipboard-policy.cs'
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw 'Clipboard mutation policy source is missing.' }
Add-Type -TypeDefinition ('using System;' + [IO.File]::ReadAllText($source) + @'
public static class BowserClipboardPolicyTests {
    static void Require(bool value,string message) { if(!value) throw new System.Exception(message); }
    sealed class Mutation : IBowserClipboardMutation {
        public uint Value; public int EmptyCalls,SetCalls,OwnedCalls,FailSet=-1,ZeroAfterSet=-1;
        public bool ZeroAfterEmpty; public System.Collections.Generic.List<uint> Formats=new System.Collections.Generic.List<uint>();
        public uint Sequence() {return Value;}
        public bool Empty() {EmptyCalls++;Value=ZeroAfterEmpty?0:Value+1;return true;}
        public bool SetGenerated(uint format) {int index=SetCalls++;Formats.Add(format);if(index==FailSet)return false;Value=index==ZeroAfterSet?0:Value+1;return true;}
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
        Require(positive.EmptyCalls==1&&positive.SetCalls==4&&positive.OwnedCalls==5&&String.Join(",",positive.Formats)=="13,16,1,7","Known generated bundle/order/ownership records lost.");passed++;
        for(int index=0;index<4;index++){
            var partial=new Mutation{Value=10,FailSet=index};rejected=false;
            try{BowserClipboardMutationPolicy.WriteGenerated(partial);}catch(System.InvalidOperationException){rejected=true;}
            Require(rejected&&partial.EmptyCalls==1&&partial.SetCalls==index+1&&partial.OwnedCalls==index+1,"Partial bundle failure must stop without retry or unknown ownership record.");passed++;
        }
        var afterEmptyZero=new Mutation{Value=10,ZeroAfterEmpty=true};rejected=false;
        try{BowserClipboardMutationPolicy.WriteGenerated(afterEmptyZero);}catch(System.InvalidOperationException){rejected=true;}
        Require(rejected&&afterEmptyZero.EmptyCalls==1&&afterEmptyZero.SetCalls==0&&afterEmptyZero.OwnedCalls==0,"Zero after known Empty must stop before Set/ownership.");passed++;
        for(int index=0;index<4;index++){
            var afterSetZero=new Mutation{Value=10,ZeroAfterSet=index};rejected=false;
            try{BowserClipboardMutationPolicy.WriteGenerated(afterSetZero);}catch(System.InvalidOperationException){rejected=true;}
            Require(rejected&&afterSetZero.SetCalls==index+1&&afterSetZero.OwnedCalls==index+1,"Zero after Set must stop without an unknown sequence record or later Set.");passed++;
        }
        BowserClipboardMutationPolicy.RequireTrackedAfterClose(15,15,true);passed++;
        foreach(uint observed in new uint[]{0,16}){
            rejected=false;try{BowserClipboardMutationPolicy.RequireTrackedAfterClose(15,observed,true);}catch(System.InvalidOperationException){rejected=true;}
            Require(rejected,"Post-close zero/same-owner change must NEVER be adopted.");passed++;
        }
        rejected=false;try{BowserClipboardMutationPolicy.RequireTrackedAfterClose(15,15,false);}catch(System.InvalidOperationException){rejected=true;}
        Require(rejected,"Post-close unknown/foreign owner must refuse even with same sequence.");passed++;
        var bundle=BowserGeneratedClipboardData.Create("sentinel-12-345");
        Require(System.Text.Encoding.Unicode.GetString(bundle.Payload(13))=="sentinel-12-345\0"&&System.Text.Encoding.ASCII.GetString(bundle.Payload(1))=="sentinel-12-345\0"&&System.Text.Encoding.ASCII.GetString(bundle.Payload(7))=="sentinel-12-345\0"&&BitConverter.ToUInt32(bundle.Payload(16),0)==0x0409,"Generated formats must encode the same ASCII token and fixed valid locale.");passed++;
        foreach(string invalid in new string[]{null,"paste-1-2\n","paste-1-\u0662","other-1-2","paste-"+new string('1',130)+"-2"}){
            rejected=false;try{BowserGeneratedClipboardData.Create(invalid);}catch(System.InvalidOperationException){rejected=true;}
            Require(rejected,"Payload grammar/ASCII/bound must reject before native allocation or mutation.");passed++;
        }
        int set=0,freed=0;var transferred=new BowserClipboardBuffer(13,new IntPtr(7),(format,memory)=>{set++;return memory;},memory=>freed++);
        Require(transferred.Transfer(),"Fake successful Set must transfer ownership.");transferred.Dispose();transferred.Dispose();rejected=false;
        try{transferred.Transfer();}catch(System.InvalidOperationException){rejected=true;}
        Require(rejected&&set==1&&freed==0,"Transferred handle must never be set twice or freed by fixture.");passed++;
        set=0;freed=0;var failed=new BowserClipboardBuffer(16,new IntPtr(8),(format,memory)=>{set++;return IntPtr.Zero;},memory=>freed++);
        Require(!failed.Transfer(),"Fake failed Set must refuse.");failed.Dispose();failed.Dispose();
        Require(set==1&&freed==1,"Only untransferred handle must be freed, exactly once.");passed++;
        freed=0;var uncertain=new BowserClipboardBuffer(1,new IntPtr(9),(format,memory)=>{throw new System.Exception("uncertain native outcome");},memory=>freed++);
        rejected=false;try{uncertain.Transfer();}catch(System.Exception){rejected=true;}uncertain.Dispose();
        Require(rejected&&freed==0,"Uncertain native transfer must refuse and never risk freeing a system-owned handle.");passed++;
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
