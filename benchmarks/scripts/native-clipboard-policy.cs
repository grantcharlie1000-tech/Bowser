// Pure policy shared by the real ClipboardGuard and fake-API regressions. No native calls.
public interface IBowserClipboardMutation {
    uint Sequence();
    bool Empty();
    bool SetGenerated();
    void RecordOwned(uint sequence);
}
public sealed class BowserClipboardMutation : IBowserClipboardMutation {
    readonly System.Func<uint> sequence;
    readonly System.Func<bool> empty, setGenerated;
    readonly System.Action<uint> record;
    public BowserClipboardMutation(System.Func<uint> sequence, System.Func<bool> empty,
        System.Func<bool> setGenerated, System.Action<uint> record) {
        this.sequence=sequence;this.empty=empty;this.setGenerated=setGenerated;this.record=record;
    }
    public uint Sequence(){return sequence();}
    public bool Empty(){return empty();}
    public bool SetGenerated(){return setGenerated();}
    public void RecordOwned(uint value){record(value);}
}
public static class BowserClipboardMutationPolicy {
    static uint Nonzero(uint value) {
        if(value==0)throw new System.InvalidOperationException("Clipboard sequence unavailable; no authority to read or restore.");
        return value;
    }
    // The real guard enters here only after its existing exclusive empty-format admission.
    // In particular, zero is refused BEFORE either native mutator can be reached.
    public static void WriteGenerated(IBowserClipboardMutation api) {
        Nonzero(api.Sequence());
        if(!api.Empty())throw new System.InvalidOperationException("Empty precondition could not be claimed.");
        api.RecordOwned(Nonzero(api.Sequence()));
        if(!api.SetGenerated())throw new System.InvalidOperationException("Synthetic payload write failed.");
        api.RecordOwned(Nonzero(api.Sequence()));
    }
}

// The CI initializer has NO SetClipboardData/content getter interface. Its sole possible
// mutation is one EmptyClipboard of an already-empty, ownerless, exclusively locked baseline.
public interface IBowserCiClipboardFixture {
    long HelperWindow {get;}
    bool OwnedWindow();
    bool Open();
    long Owner();
    uint FirstFormat(out int error);
    uint Sequence();
    bool Empty();
    bool Close();
}
public sealed class BowserCiClipboardResult {
    public bool Prepared, OpenSucceeded, CloseAttempted, CloseConfirmed;
    public uint InitialSequence, FinalSequence;
    public int EmptyAttempts;
    public string Failure;
    public bool Authority {get{return false;}}
    public string Scope {get{return "disposable-ci-empty-baseline-not-input-authority";}}
}
public static class BowserCiClipboardPolicy {
    static void Require(bool value,string stage) {
        if(!value)throw new System.InvalidOperationException(stage);
    }
    static void EmptyFormats(IBowserCiClipboardFixture api) {
        int error;uint first=api.FirstFormat(out error);
        Require(first==0&&error==0,"nonempty-or-unknown-formats-no-mutation");
    }
    public static BowserCiClipboardResult Prepare(IBowserCiClipboardFixture api,
        bool disposableJobOwnershipVerified, System.Action<BowserCiClipboardResult> writeAhead) {
        var result=new BowserCiClipboardResult();
        if(!disposableJobOwnershipVerified){result.Failure="disposable-job-ownership-unproved";return result;}
        try {
            Require(api.HelperWindow!=0&&api.OwnedWindow(),"exact-helper-window-unavailable");
            Require(api.Open(),"clipboard-locked-no-retry");result.OpenSucceeded=true;
            Require(api.OwnedWindow()&&api.Owner()==0,"foreign-or-unknown-owner-no-mutation");
            EmptyFormats(api);
            result.InitialSequence=api.Sequence();
            if(result.InitialSequence==0) {
                // Authority is the independently verified disposable job, NEVER zero/rights.
                Require(api.OwnedWindow()&&api.Owner()==0,"helper-or-owner-changed-no-mutation");
                result.EmptyAttempts=1;
                writeAhead(result); // A durable intent record is required BEFORE the one call.
                Require(api.Empty(),"single-empty-call-failed-no-retry");
                Require(api.Owner()==api.HelperWindow,"empty-owner-not-exact-helper");
            }
            result.FinalSequence=api.Sequence();
            Require(result.FinalSequence!=0,"sequence-still-zero-no-retry");
            EmptyFormats(api);
            Require(api.OwnedWindow(),"helper-lifetime-unavailable");
        } catch(System.InvalidOperationException error) {result.Failure=error.Message;}
          catch {result.Failure="operation-or-write-ahead-unavailable";}
        finally {
            if(result.OpenSucceeded) {
                result.CloseAttempted=true;
                try {result.CloseConfirmed=api.Close();}catch {result.CloseConfirmed=false;}
                if(!result.CloseConfirmed)result.Failure="close-unconfirmed-no-retry";
            }
        }
        if(result.Failure!=null)return result;
        try {
            long expectedOwner=result.EmptyAttempts==1?api.HelperWindow:0;
            Require(api.OwnedWindow()&&api.Owner()==expectedOwner,"owner-changed-after-close");
            Require(api.Sequence()==result.FinalSequence&&result.FinalSequence!=0,"sequence-changed-after-close");
            result.Prepared=true;
        } catch {result.Failure="post-close-proof-unavailable";}
        return result;
    }
}
