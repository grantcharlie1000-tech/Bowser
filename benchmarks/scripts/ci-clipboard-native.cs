// CI adapter only. Construct solely after actual runner/configuration/server proof succeeds.
// Deliberately contains no content getter, SetClipboardData, activation or permission API.
public sealed class BowserCiClipboardNative : IBowserCiClipboardFixture, System.IDisposable {
    [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern uint GetCurrentProcessId();
    [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [System.Runtime.InteropServices.DllImport("kernel32.dll")] static extern void SetLastError(uint error);
    [System.Runtime.InteropServices.DllImport("user32.dll",EntryPoint="CreateWindowExW",CharSet=System.Runtime.InteropServices.CharSet.Unicode)]
    static extern System.IntPtr CreateWindowEx(uint ex,string name,string title,uint style,int x,int y,int w,int h,System.IntPtr parent,System.IntPtr menu,System.IntPtr instance,System.IntPtr parameter);
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern bool DestroyWindow(System.IntPtr window);
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern bool IsWindow(System.IntPtr window);
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(System.IntPtr window,out uint pid);
    [System.Runtime.InteropServices.DllImport("user32.dll",CharSet=System.Runtime.InteropServices.CharSet.Unicode)] static extern int GetClassNameW(System.IntPtr window,System.Text.StringBuilder value,int count);
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern bool OpenClipboard(System.IntPtr owner);
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern bool CloseClipboard();
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern bool EmptyClipboard();
    [System.Runtime.InteropServices.DllImport("user32.dll",SetLastError=true)] static extern System.IntPtr GetClipboardOwner();
    [System.Runtime.InteropServices.DllImport("user32.dll")] static extern uint GetClipboardSequenceNumber();
    [System.Runtime.InteropServices.DllImport("user32.dll",SetLastError=true)] static extern uint EnumClipboardFormats(uint previous);
    readonly System.Func<bool> runnerAlive;
    readonly uint pid,thread;
    System.IntPtr window;
    public int NativeEmptyCalls,NativeOpenCalls,NativeCloseCalls;
    public bool WindowCleanupConfirmed;
    public BowserCiClipboardNative(System.Func<bool> runnerAlive) {
        this.runnerAlive=runnerAlive;pid=GetCurrentProcessId();thread=GetCurrentThreadId();
        if(!runnerAlive())throw new System.InvalidOperationException("retained-runner-proof-lost");
        window=CreateWindowEx(0,"STATIC","",0,0,0,0,0,new System.IntPtr(-3),System.IntPtr.Zero,System.IntPtr.Zero,System.IntPtr.Zero);
        if(window==System.IntPtr.Zero||!OwnedWindow()){Dispose();throw new System.InvalidOperationException("exact-message-only-helper-unavailable");}
    }
    public long HelperWindow {get{return window.ToInt64();}}
    bool SameWindow() {
        uint actual;var name=new System.Text.StringBuilder(64);
        return window!=System.IntPtr.Zero&&IsWindow(window)&&GetWindowThreadProcessId(window,out actual)==thread&&actual==pid&&
            GetClassNameW(window,name,64)>0&&System.String.Equals(name.ToString(),"Static",System.StringComparison.OrdinalIgnoreCase);
    }
    public bool OwnedWindow(){return runnerAlive()&&SameWindow();}
    public bool Open(){if(!OwnedWindow())return false;NativeOpenCalls++;return OpenClipboard(window);}
    public long Owner(){SetLastError(0);var owner=GetClipboardOwner();if(System.Runtime.InteropServices.Marshal.GetLastWin32Error()!=0)throw new System.InvalidOperationException("clipboard-owner-unavailable");return owner.ToInt64();}
    public uint FirstFormat(out int error){SetLastError(0);uint first=EnumClipboardFormats(0);error=System.Runtime.InteropServices.Marshal.GetLastWin32Error();return first;}
    public uint Sequence(){return GetClipboardSequenceNumber();}
    public bool Empty() {
        int error;
        if(!OwnedWindow()||Owner()!=0||FirstFormat(out error)!=0||error!=0)return false;
        NativeEmptyCalls++; // Exact mutator boundary; at most one call in the policy.
        return EmptyClipboard();
    }
    public bool Close(){NativeCloseCalls++;return CloseClipboard();}
    public void Dispose() {
        if(window==System.IntPtr.Zero)return;
        if(SameWindow()){WindowCleanupConfirmed=DestroyWindow(window)&&!IsWindow(window);}
        window=System.IntPtr.Zero;
    }
}
