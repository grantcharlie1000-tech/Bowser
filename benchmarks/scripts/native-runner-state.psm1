Set-StrictMode -Version Latest

# Observations only: never open/change a desktop, clipboard, process token or input queue.
# Borrowed USER handles are neither closed nor used as permission evidence.
if (-not ('BowserNativeRunnerState' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class BowserNativeRunnerState {
    [StructLayout(LayoutKind.Sequential)] struct Flags { public int inherit, reserved; public uint flags; }
    [DllImport("kernel32.dll")] static extern uint GetCurrentProcessId();
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool ProcessIdToSessionId(uint pid, out uint session);
    [DllImport("user32.dll")] static extern IntPtr GetProcessWindowStation();
    [DllImport("user32.dll")] static extern IntPtr GetThreadDesktop(uint tid);
    [DllImport("user32.dll", EntryPoint="GetUserObjectInformationW", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern bool ObjectText(IntPtr handle, int kind, StringBuilder value, uint size, out uint needed);
    [DllImport("user32.dll", EntryPoint="GetUserObjectInformationW", SetLastError=true)]
    static extern bool ObjectFlags(IntPtr handle, int kind, out Flags value, uint size, out uint needed);
    [DllImport("user32.dll", EntryPoint="GetUserObjectInformationW", SetLastError=true)]
    static extern bool ObjectInput(IntPtr handle, int kind, out int value, uint size, out uint needed);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr handle, out uint pid);
    [DllImport("user32.dll")] static extern uint GetClipboardSequenceNumber();
    static object Text(IntPtr handle, int kind) {
        var value = new StringBuilder(128); uint needed;
        bool ok = ObjectText(handle, kind, value, 256, out needed);
        int error = ok ? 0 : Marshal.GetLastWin32Error();
        return new { query_ok=ok, error=error, needed_bytes=needed, value=ok ? value.ToString() : null };
    }
    static object Session(uint pid) {
        uint value=0; bool performed=pid!=0;
        bool ok=performed && ProcessIdToSessionId(pid, out value);
        int? error=performed ? (int?)(ok ? 0 : Marshal.GetLastWin32Error()) : null;
        return new { query_performed=performed, query_ok=ok, error=error, value=ok ? (uint?)value : null };
    }
    static object UserObject(IntPtr handle, bool desktop) {
        Flags flags; uint needed; bool ok=ObjectFlags(handle, 1, out flags, 12, out needed);
        int error=ok ? 0 : Marshal.GetLastWin32Error();
        int input=0; uint inputNeeded=0; bool inputOk=desktop && ObjectInput(handle, 6, out input, 4, out inputNeeded);
        int? inputError=desktop ? (int?)(inputOk ? 0 : Marshal.GetLastWin32Error()) : null;
        return new { borrowed_handle=handle.ToInt64(), name=Text(handle,2), type=Text(handle,3),
            flags=new { query_ok=ok, error=error, needed_bytes=needed, value=ok ? (uint?)flags.flags : null },
            receives_input=new { query_performed=desktop, query_ok=inputOk, error=inputError,
                value=inputOk ? (bool?)(input!=0) : null }, authority=false };
    }
    public static object DescribeSequence(uint number) {
        return new { number=number, nonzero_observed=number!=0, authority=false };
    }
    public static object Capture() {
        uint pid=GetCurrentProcessId(), tid=GetCurrentThreadId(), foregroundPid;
        IntPtr foreground=GetForegroundWindow();
        uint foregroundTid=GetWindowThreadProcessId(foreground, out foregroundPid);
        return new { process_id=pid, thread_id=tid, session=Session(pid),
            window_station=UserObject(GetProcessWindowStation(),false),
            desktop=UserObject(GetThreadDesktop(tid),true),
            foreground=new { handle=foreground.ToInt64(), process_id=foregroundPid,
                thread_id=foregroundTid, session=Session(foregroundPid) },
            clipboard_sequence=DescribeSequence(GetClipboardSequenceNumber()), non_atomic=true, authority=false };
    }
}
'@
}

function Get-BowserNativeRunnerState {
    [CmdletBinding()]
    param([ValidateSet('preflight', 'failure', 'after-probes', 'test')][string]$Phase)
    $result = [ordered]@{
        schema_version = 1
        scope = 'read-only-native-runner-state-not-authority'
        phase = $Phase
        status = 'unavailable'
        input_authority = $false
        clipboard_authority = $false
        runtime = [ordered]@{
            powershell_version = $PSVersionTable.PSVersion.ToString()
            edition = $PSVersionTable.PSEdition
            apartment = [Threading.Thread]::CurrentThread.GetApartmentState().ToString()
            language_mode = $ExecutionContext.SessionState.LanguageMode.ToString()
        }
        state = $null
    }
    try { $result.state = [BowserNativeRunnerState]::Capture(); $result.status = 'captured' }
    catch { $result.error_type = $_.Exception.GetType().FullName }
    return $result
}
Export-ModuleMember -Function Get-BowserNativeRunnerState
