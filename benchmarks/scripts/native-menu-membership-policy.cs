// Pure conjunctive policy. Actual replies must arrive on the private inherited test pipe;
// diagnostic JSON/startup lists are never inputs. Native facts are re-read by the adapter.
public interface IBowserMenuProcessIdentity {
    uint Pid {get;} ulong Created {get;} bool LiveSameRuntime();
}
public sealed class BowserMenuMembershipReply {
    public int Phase,Kind;public string Nonce;
    public uint SourcePid,BrowserPid,GpuPid;
    public ulong View,Args,Controller,Doc0,Doc1,Doc2,BrowserCreated,GpuCreated;
    public long Owner,Popup,Child;
    public BowserMenuMembershipReply Copy(){return (BowserMenuMembershipReply)MemberwiseClone();}
}
public static class BowserMenuMembershipWire {
    static ulong Number(string text,bool allowZero=false){
        ulong n;
        if(text==null||text.Length==0||text.Length>20||!System.UInt64.TryParse(text,System.Globalization.NumberStyles.None,System.Globalization.CultureInfo.InvariantCulture,out n)||(!allowZero&&n==0)||n.ToString(System.Globalization.CultureInfo.InvariantCulture)!=text)
            throw new System.InvalidOperationException("Unknown/noncanonical membership integer.");
        return n;
    }
    static uint Pid(string text){return checked((uint)Number(text));}
    static long Window(string text){return checked((long)Number(text));}
    static int Phase(string value){if(value!="0"&&value!="1")throw new System.InvalidOperationException("Unknown membership phase.");return value=="0"?0:1;}
    static void Nonce(string value){if(value==null||!System.Text.RegularExpressions.Regex.IsMatch(value,@"\A[0-9a-f]{32}\z"))throw new System.InvalidOperationException("Malformed membership nonce.");}
    public static string Request(int phase,string nonce,long popup,long child,uint gpu,ulong created){
        Phase(phase.ToString(System.Globalization.CultureInfo.InvariantCulture));Nonce(nonce);
        if(popup<=0||child<=0||popup==child||gpu==0||created==0)throw new System.InvalidOperationException("Missing exact membership request witness.");
        return System.String.Join("|",new string[]{"BOWSER-MEMBERSHIP-V1",phase.ToString(System.Globalization.CultureInfo.InvariantCulture),nonce,popup.ToString(System.Globalization.CultureInfo.InvariantCulture),child.ToString(System.Globalization.CultureInfo.InvariantCulture),gpu.ToString(System.Globalization.CultureInfo.InvariantCulture),created.ToString(System.Globalization.CultureInfo.InvariantCulture)});
    }
    public static BowserMenuMembershipReply Parse(string line){
        if(line==null||line.Length>8192)throw new System.InvalidOperationException("Missing/oversize private membership response.");
        string[] f=line.Split('|');if(f.Length!=18||f[0]!="BOWSER-MEMBERSHIP-REPLY-V1"||f[17]!="4")throw new System.InvalidOperationException("Unknown/ambiguous private membership response.");
        int phase=Phase(f[1]);Nonce(f[2]);
        return new BowserMenuMembershipReply{Phase=phase,Nonce=f[2],SourcePid=Pid(f[3]),View=Number(f[4]),Args=Number(f[5]),Controller=Number(f[6]),Doc0=Number(f[7]),Doc1=Number(f[8]),Doc2=Number(f[9],true),Owner=Window(f[10]),Popup=Window(f[11]),Child=Window(f[12]),BrowserPid=Pid(f[13]),BrowserCreated=Number(f[14]),GpuPid=Pid(f[15]),GpuCreated=Number(f[16]),Kind=4};
    }
}
public sealed class BowserMenuWrapper {
    public readonly long Owner,Popup,Child;public readonly uint BrowserPid,GpuPid,Thread;
    readonly int[] root,node,parent;
    public BowserMenuWrapper(long owner,long popup,long child,uint browserPid,uint gpuPid,uint thread,int[] root,int[] node,int[] parent){
        if(owner<=0||popup<=0||child<=0||popup==child||browserPid==0||gpuPid==0||browserPid==gpuPid||thread==0||!Ids(root)||!Ids(node)||!Ids(parent)||!Same(root,parent)||Same(root,node))
            throw new System.InvalidOperationException("Unknown/ambiguous native or UIA wrapper identity.");
        Owner=owner;Popup=popup;Child=child;BrowserPid=browserPid;GpuPid=gpuPid;Thread=thread;
        this.root=(int[])root.Clone();this.node=(int[])node.Clone();this.parent=(int[])parent.Clone();
    }
    static bool Ids(int[] ids){return ids!=null&&ids.Length>0&&ids.Length<=32;}
    static bool Same(int[] a,int[] b){if(!Ids(a)||!Ids(b)||a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true;}
    public BowserMenuWrapper Copy(){return new BowserMenuWrapper(Owner,Popup,Child,BrowserPid,GpuPid,Thread,root,node,parent);}
    public bool Node(int provider,int type,long native,int[] id,int[] parentId){return provider>0&&(uint)provider==GpuPid&&type==50033&&native==Child&&Same(id,node)&&Same(parentId,parent);}
    public bool Matches(BowserMenuWrapper other){return other!=null&&Owner==other.Owner&&Popup==other.Popup&&Child==other.Child&&BrowserPid==other.BrowserPid&&GpuPid==other.GpuPid&&Thread==other.Thread&&Same(root,other.root)&&Same(node,other.node)&&Same(parent,other.parent);}
}
public sealed class BowserMenuMembershipSession {
    readonly uint source,browser;readonly ulong view,browserCreated,created;readonly uint gpu;
    readonly BowserMenuWrapper wrapper;
    readonly IBowserMenuProcessIdentity retained; // ONE retained identity object across BOTH replies.
    readonly System.Func<BowserMenuWrapper,bool> currentNative;
    readonly System.Func<long> elapsed;
    int consumed;bool pending,failed;string nonce,priorNonce;
    BowserMenuMembershipReply first;
    public BowserMenuMembershipSession(uint source,ulong view,uint browser,ulong browserCreated,BowserMenuWrapper wrapper,IBowserMenuProcessIdentity retained,System.Func<BowserMenuWrapper,bool> currentNative,System.Func<long> elapsed){
        if(source==0||view==0||browser==0||browserCreated==0||wrapper==null||retained==null||currentNative==null||elapsed==null||wrapper.BrowserPid!=browser||retained.Pid!=wrapper.GpuPid||retained.Created==0)
            throw new System.InvalidOperationException("Incomplete retained source/wrapper/process binding.");
        this.source=source;this.view=view;this.browser=browser;this.browserCreated=browserCreated;
        this.wrapper=wrapper.Copy();this.retained=retained;gpu=retained.Pid;created=retained.Created;
        this.currentNative=currentNative;this.elapsed=elapsed;
    }
    void Refuse(){failed=true;throw new System.InvalidOperationException("Fresh current request/native/process proof refused; no retry or input.");}
    bool Current(){
        if(failed)return false;
        try{if(elapsed()<0||elapsed()>=10000||retained.Pid!=gpu||retained.Created!=created||!retained.LiveSameRuntime()||!currentNative(wrapper)){failed=true;return false;}}
        catch{failed=true;return false;}
        return true;
    }
    public void Begin(int phase,string freshNonce){
        if(!Current()||pending||phase!=consumed||phase<0||phase>1||freshNonce==null||!System.Text.RegularExpressions.Regex.IsMatch(freshNonce,@"\A[0-9a-f]{32}\z")||freshNonce==priorNonce)Refuse();
        nonce=freshNonce;pending=true;
    }
    public void Consume(BowserMenuMembershipReply reply){
        if(!Current()||!pending||reply==null||reply.Phase!=consumed||reply.Nonce!=nonce||reply.SourcePid!=source||reply.View!=view||reply.Args==0||reply.Controller==0||reply.Doc0==0||reply.Doc1==0||reply.Owner!=wrapper.Owner||reply.Popup!=wrapper.Popup||reply.Child!=wrapper.Child||reply.BrowserPid!=browser||reply.BrowserCreated!=browserCreated||reply.GpuPid!=gpu||reply.GpuCreated!=created||reply.Kind!=4)Refuse();
        if(first!=null&&(reply.Args!=first.Args||reply.Controller!=first.Controller||reply.Doc0!=first.Doc0||reply.Doc1!=first.Doc1||reply.Doc2!=first.Doc2))Refuse();
        if(!Current())Refuse();
        if(first==null)first=reply.Copy();
        consumed++;pending=false;priorNonce=nonce;nonce=null;
    }
    public bool CanTraverse(int provider,int type,long native,int[] runtime,int[] parent){
        if(!Current()||pending||consumed<1||consumed>2)return false;
        if(!wrapper.Node(provider,type,native,runtime,parent)){failed=true;return false;}
        return Current();
    }
    public bool SameWrapper(BowserMenuWrapper candidate){return wrapper.Matches(candidate);}
    public bool CanCancel(BowserMenuWrapper candidate){
        if(consumed!=2||pending||!Current())return false;
        if(!wrapper.Matches(candidate)){failed=true;return false;}
        return Current();
    }
}
