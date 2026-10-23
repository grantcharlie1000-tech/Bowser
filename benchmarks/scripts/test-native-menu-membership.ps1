[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$path=Join-Path $PSScriptRoot 'native-menu-membership-policy.cs'
if(-not (Test-Path -LiteralPath $path -PathType Leaf)){throw 'Native menu membership policy is missing.'}
# Pure adversarial protocol/identity tests; no native adapter, process or clipboard calls.
Add-Type -TypeDefinition ('using System;'+[IO.File]::ReadAllText($path)+@'
public static class BowserMembershipTests {
 sealed class Identity:IBowserMenuProcessIdentity {
  public uint CurrentPid=7;public ulong CurrentCreated=70;public bool Live=true;
  public uint Pid{get{return CurrentPid;}}public ulong Created{get{return CurrentCreated;}}
  public bool LiveSameRuntime(){return Live;}
 }
 sealed class Harness {
  public Identity Process=new Identity();public bool Native=true;public long Elapsed;
  public BowserMenuWrapper Stamp=new BowserMenuWrapper(10,20,21,3,7,9,new int[]{42,20},new int[]{42,21},new int[]{42,20});
  public BowserMenuMembershipSession Session;
  public Harness(){Session=new BowserMenuMembershipSession(1,100,3,30,Stamp,Process,s=>Native,()=>Elapsed);}
 }
 static void Require(bool condition,string message){if(!condition)throw new Exception(message);}
 static BowserMenuMembershipReply Reply(int phase=0,string nonce="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"){
  return new BowserMenuMembershipReply{Phase=phase,Nonce=nonce,SourcePid=1,View=100,Args=200,Controller=300,Doc0=5,Doc1=1,Doc2=1,Owner=10,Popup=20,Child=21,BrowserPid=3,GpuPid=7,BrowserCreated=30,GpuCreated=70,Kind=4};
 }
 static void First(Harness h){h.Session.Begin(0,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");h.Session.Consume(Reply());}
 static bool Refuses(Action action){try{action();return false;}catch(InvalidOperationException){return true;}}
 public static int Run(){int passed=0;
  string wire="BOWSER-MEMBERSHIP-REPLY-V1|0|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|1|100|200|300|5|1|1|10|20|21|3|30|7|70|4";
  var parsed=BowserMenuMembershipWire.Parse(wire);Require(parsed.Args==200&&parsed.GpuCreated==70&&parsed.Phase==0,"Exact bounded private reply must parse.");passed++;
  foreach(string bad in new string[]{null,new string('x',8193),wire+"|extra",wire.Replace("|100|","|0100|"),wire.Replace("|70|","|0|"),wire+"\r",wire.Replace("|0|","|2|"),wire.Replace("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),wire.Replace("|4","|6"),wire.Replace("|3|30|","|4294967296|30|")}){
   bool refused=false;try{BowserMenuMembershipWire.Parse(bad);}catch(InvalidOperationException){refused=true;}catch(OverflowException){refused=true;}Require(refused,"Malformed/extra/oversize/unknown private reply must refuse.");passed++;
  }
  Require(BowserMenuMembershipWire.Request(0,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",20,21,7,70)=="BOWSER-MEMBERSHIP-V1|0|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|20|21|7|70","Exact private request grammar required.");passed++;
  var initialRevision=new Harness();initialRevision.Session.Begin(0,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");var rev0=BowserMenuMembershipWire.Parse(wire.Replace("|5|1|1|","|5|1|0|"));initialRevision.Session.Consume(rev0);
  initialRevision.Session.Begin(1,"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");rev0.Phase=1;rev0.Nonce="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";initialRevision.Session.Consume(rev0);
  Require(initialRevision.Session.CanCancel(initialRevision.Stamp),"DocumentGate source revision starts at zero; exact stable revision is distinct from unknown/zero PID.");passed++;
  var good=new Harness();First(good);
  Require(good.Session.CanTraverse(7,50033,21,new int[]{42,21},new int[]{42,20}),"Exact structural wrapper must require first fresh reply.");
  Require(!good.Session.CanCancel(good.Stamp),"One response cannot authorize cancellation.");
  good.Session.Begin(1,"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");good.Session.Consume(Reply(1,"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
  Require(good.Session.CanCancel(good.Stamp),"Both fresh replies plus retained identity/native proof required.");passed++;
  foreach(Action<BowserMenuMembershipReply> corrupt in new Action<BowserMenuMembershipReply>[] {r=>r.SourcePid=2,r=>r.View=101,r=>r.Popup=99,r=>r.Child=99,r=>r.GpuPid=0,r=>r.GpuCreated=71,r=>r.Kind=0,r=>r.BrowserCreated=31}){
   var h=new Harness();h.Session.Begin(0,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");var r=Reply();corrupt(r);
   Require(Refuses(()=>h.Session.Consume(r))&&!h.Session.CanCancel(h.Stamp),"Foreign/unknown identity must poison protocol without input.");passed++;
  }
  foreach(Action<BowserMenuMembershipReply> corrupt in new Action<BowserMenuMembershipReply>[] {r=>r.Args=201,r=>r.Controller=301,r=>r.Doc2=2,r=>r.Nonce="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",r=>r.Phase=0}){
   var h=new Harness();First(h);h.Session.Begin(1,"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");var r=Reply(1,"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");corrupt(r);
   Require(Refuses(()=>h.Session.Consume(r))&&!h.Session.CanCancel(h.Stamp),"Changed request/document or replayed/reordered response must refuse.");passed++;
  }
  var duplicate=new Harness();First(duplicate);Require(Refuses(()=>duplicate.Session.Consume(Reply())),"Response may be consumed only once.");passed++;
  var reused=new Harness();First(reused);Require(Refuses(()=>reused.Session.Begin(1,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")),"Phase nonce cannot be reused.");passed++;
  var missing=new Harness();missing.Session.Begin(0,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");Require(Refuses(()=>missing.Session.Consume(null)),"Missing response must refuse without retry.");passed++;
  var expired=new Harness();expired.Elapsed=10000;Require(Refuses(()=>First(expired)),"Existing ten-second budget cannot be extended.");passed++;
  foreach(Action<Harness> change in new Action<Harness>[] {h=>h.Process.Live=false,h=>h.Process.CurrentCreated=71,h=>h.Process.CurrentPid=8,h=>h.Native=false}){
   var h=new Harness();First(h);change(h);Require(!h.Session.CanTraverse(7,50033,21,new int[]{42,21},new int[]{42,20}),"Exited/replaced process or changed native ancestry must refuse.");passed++;
  }
  foreach(int type in new int[]{50009,50011}){var h=new Harness();First(h);Require(!h.Session.CanTraverse(7,type,21,new int[]{42,21},new int[]{42,20}),"GPU actionable Menu/MenuItem may never be admitted.");passed++;}
  foreach(int provider in new int[]{0,8}){var h=new Harness();First(h);Require(!h.Session.CanTraverse(provider,50033,21,new int[]{42,21},new int[]{42,20}),"Zero/foreign provider must refuse.");passed++;}
  var foreignParent=new Harness();First(foreignParent);Require(!foreignParent.Session.CanTraverse(7,50033,21,new int[]{42,21},new int[]{42,99}),"Foreign UIA ancestry must refuse.");passed++;
  var replaced=new Harness();First(replaced);Require(!replaced.Session.CanTraverse(7,50033,22,new int[]{42,22},new int[]{42,20}),"Replaced native/UIA node must refuse.");passed++;
  var malformed=new Harness();First(malformed);Require(!malformed.Session.CanTraverse(7,50033,21,new int[33],new int[]{42,20}),"Malformed/oversized runtime ancestry must refuse.");passed++;
  return passed;
 }
}
'@)
$count=[BowserMembershipTests]::Run()
Write-Output "Native menu membership regressions: $count passed; fake protocol/process/native facts only."
