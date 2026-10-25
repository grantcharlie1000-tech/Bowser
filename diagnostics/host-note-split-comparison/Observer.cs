// Passive, separate-process observer. No input, activation, clipboard, text, screenshot,
// accessibility, injection, hooks, process writes, or window messages exist in this program.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;

static class Observer {
    const uint QueryAndSynchronize = 0x00101000;
    const int MaxBytes = 524288, MaxSnapshots = 1024;
    delegate bool EnumProc(IntPtr window, IntPtr parameter);
    [StructLayout(LayoutKind.Sequential)] struct FileTime {
        public uint low, high;
        public ulong Bits { get { return ((ulong)high << 32) | low; } }
    }
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern uint GetProcessId(IntPtr process);
    [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr process, out FileTime created, out FileTime exited, out FileTime kernel, out FileTime user);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] static extern bool QueryFullProcessImageName(IntPtr process, uint flags, StringBuilder text, ref uint length);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindow(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr window, StringBuilder text, int capacity);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr window, uint command);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr window, uint flags);

    static uint expectedPid;
    static ulong expectedCreated;
    static string expectedImage;
    static IntPtr retained;
    static bool SameAlive() {
        FileTime created, exited, kernel, user;
        var image = new StringBuilder(32768); uint length = 32768;
        return retained != IntPtr.Zero && GetProcessId(retained) == expectedPid &&
            WaitForSingleObject(retained, 0) == 258 &&
            GetProcessTimes(retained, out created, out exited, out kernel, out user) && created.Bits == expectedCreated &&
            QueryFullProcessImageName(retained, 0, image, ref length) &&
            String.Equals(Path.GetFullPath(image.ToString()), expectedImage, StringComparison.OrdinalIgnoreCase);
    }
    static bool Owned(IntPtr window, out uint thread) {
        uint actual; thread = GetWindowThreadProcessId(window, out actual);
        return window != IntPtr.Zero && thread != 0 && actual == expectedPid && IsWindow(window);
    }
    static object Window(IntPtr window) {
        uint thread;
        if (!Owned(window, out thread)) return new { status="not-test-owned-details-omitted" };
        var name = new StringBuilder(128);
        int count = GetClassName(window, name, name.Capacity);
        bool visible = IsWindowVisible(window); uint after;
        if (!Owned(window, out after) || after != thread || !SameAlive())
            return new { status="changed-during-read-details-discarded" };
        return new { status="captured-nonatomic-not-authority", hwnd=window.ToInt64(), thread_id=thread,
            raw_class=count > 0 && count < 127 ? name.ToString() : null, class_complete=count > 0 && count < 127,
            visible=visible };
    }
    static object Link(IntPtr window) {
        uint thread;
        if (window == IntPtr.Zero) return new { status="none" };
        if (!Owned(window, out thread)) return new { status="outside-test-process-details-omitted" };
        return Window(window);
    }
    static object Describe(IntPtr window) {
        var owners = new List<object>(); var seen = new HashSet<long>();
        IntPtr current = window; bool capped = false;
        for (int depth=0; depth<8; depth++) {
            uint thread;
            if (!Owned(current, out thread)) break;
            IntPtr owner = GetWindow(current, 4);
            if (owner == IntPtr.Zero) break;
            owners.Add(Link(owner));
            if (!Owned(owner, out thread) || !seen.Add(owner.ToInt64())) break;
            current = owner; capped = depth == 7;
        }
        return new { window=Window(window), owner_chain=owners, owner_chain_capped=capped,
            root=Link(GetAncestor(window, 2)), root_owner=Link(GetAncestor(window, 3)) };
    }
    static object Snapshot() {
        var windows = new List<IntPtr>(); int visited = 0; bool capped = false;
        bool complete = EnumWindows(delegate(IntPtr window, IntPtr parameter) {
            if (++visited > 512) { capped = true; return false; }
            uint thread;
            if (Owned(window, out thread)) {
                if (windows.Count >= 16) { capped = true; return false; }
                windows.Add(window);
            }
            return true;
        }, IntPtr.Zero);
        windows.Sort(delegate(IntPtr a, IntPtr b) { return a.ToInt64().CompareTo(b.ToInt64()); });
        var descriptions = new List<object>();
        foreach (IntPtr window in windows) descriptions.Add(Describe(window));
        IntPtr foreground = GetForegroundWindow(); uint foregroundThread;
        bool testForeground = Owned(foreground, out foregroundThread);
        // Never record another process's PID, HWND, class, window text, or contents.
        return new { inventory_complete=complete && !capped, test_windows=descriptions,
            foreground_in_test_process=testForeground, foreground=testForeground ? Describe(foreground) : null,
            non_atomic=true, authority=false };
    }
    static int Main(string[] args) {
        try {
            if (args.Length != 4) throw new ArgumentException("Expected exact PID, image, creation FILETIME and output root.");
            expectedPid = UInt32.Parse(args[0], CultureInfo.InvariantCulture);
            expectedImage = Path.GetFullPath(args[1]);
            expectedCreated = UInt64.Parse(args[2], CultureInfo.InvariantCulture);
            string output = Path.GetFullPath(args[3]);
            retained = OpenProcess(QueryAndSynchronize, false, expectedPid);
            if (!SameAlive()) throw new InvalidOperationException("Exact retained process identity unavailable; no observation accepted.");
            var json = new JavaScriptSerializer(); var watch = Stopwatch.StartNew();
            using (var stream = new FileStream(Path.Combine(output, "observer.jsonl"), FileMode.CreateNew, FileAccess.Write, FileShare.Read))
            using (var writer = new StreamWriter(stream, new UTF8Encoding(false))) {
                writer.AutoFlush = true;
                File.WriteAllText(Path.Combine(output, "observer.ready"), "exact-process-read-only-observer-ready", new UTF8Encoding(false));
                string previous = null; int snapshots=0, bytes=0; bool logCapped=false;
                while (SameAlive() && watch.ElapsedMilliseconds < 300000) {
                    string state = json.Serialize(Snapshot());
                    if (!SameAlive()) break;
                    if (state != previous && !logCapped) {
                        string line = "{\"schema\":\"bowser-passive-process-windows-v1\",\"elapsed_ms\":" + watch.ElapsedMilliseconds +
                            ",\"test_pid\":" + expectedPid + ",\"observation\":" + state + "}";
                        int size = Encoding.UTF8.GetByteCount(line) + 2;
                        if (snapshots >= MaxSnapshots || bytes + size > MaxBytes) logCapped=true;
                        else { writer.WriteLine(line); snapshots++; bytes += size; }
                        previous=state;
                    }
                    Thread.Sleep(25);
                }
                bool exited = WaitForSingleObject(retained, 0) == 0;
                File.WriteAllText(Path.Combine(output, "observer-summary.json"), json.Serialize(new {
                    schema="bowser-passive-process-windows-summary-v1", exact_pid=expectedPid, creation_filetime=expectedCreated.ToString(),
                    process_exited=exited, observer_wall_cap_reached=!exited && watch.ElapsedMilliseconds >= 300000,
                    written_snapshots=snapshots, written_bytes=bytes, log_capped=logCapped,
                    clipboard_access=false, input=false, focus_rescue=false, policy_changes=false, non_atomic=true, authority=false
                }), new UTF8Encoding(false));
                return exited ? 0 : 72;
            }
        } catch (Exception error) { Console.Error.WriteLine("Observer blocker: " + error.GetType().Name + ": " + error.Message); return 71; }
        finally { if (retained != IntPtr.Zero) CloseHandle(retained); }
    }
}
