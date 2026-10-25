// One pair only. Runs exact unmodified libtests; this program never supplies native input.
using System;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;

static class Compare {
    const string Base = "74123993475e3db4a26686a1727dfc94ed103255";
    const string Candidate = "70b2db35ebbf96ea41eb7a9cb22b5e96146eaab2";
    const string Test = "engine::app_host_menu_probe::live::note_split::live_probe_real_app_host_note_split_context_menu";
    const string Features = "html-chrome-default,experimental-engine-context-menu";
    const int CaptureCap = 196608;
    static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    static readonly List<string> Summary = new List<string>();
    static string workspace, output;
    static readonly Dictionary<string,string> SafetyBlobs = new Dictionary<string,string> {
        {"Cargo.toml", "dcbf3e5b515c1f2cc5f3dfa42ba68ca0368d82a7"},
        {"Cargo.lock", "6df0eca95d40ddc5180336ad108a005dd32f3b23"},
        {"build.rs", "de9c193130e666376f7a2c371578f24086c75df3"},
        {"src/chrome_host/fingerprint.rs", "a3157eaabc027fb80921e980fe4b835408a73270"},
        {"src/engine/app_host_menu_probe/live.rs", "2ddf305f4a009ed7c1e82025f4faa36651d26128"},
        {"src/engine/app_host_menu_probe/live/note_split.rs", "59e7a7621d66d34ffdff82dc9a4fd26c32b7fbea"},
        {"src/engine/native_probe_pump.rs", "9a7810158e134150712882ee1dd4598d46269f6b"},
        {"src/engine/native_menu_membership.rs", "d57d0da40dc87e9f0cc73f3b6728018a4586d258"},
        {"benchmarks/scripts/probe-app-host-context-menu.ps1", "416d81358a21ea6db23c0b37c09ab9dc67b6be37"},
        {"benchmarks/scripts/native-clipboard-policy.cs", "271bcf573cc0bd2ff89418c75fc2182e36b99c19"},
        {"benchmarks/scripts/native-menu-membership-policy.cs", "552a6575fb1f6cc58697dc2e0ad30df05ff57c52"},
        {"benchmarks/scripts/native-runner-state.psm1", "193790bf105f67697b6835dc512c08e10401de29"}
    };
    sealed class Result {
        public int Exit;
        public string Text;
        public bool Truncated, PolicyRefusal;
    }
    sealed class Capture {
        readonly object gate = new object();
        readonly StringBuilder text = new StringBuilder();
        readonly CountdownEvent streams = new CountdownEvent(2);
        public bool Truncated, PolicyRefusal;
        public void End() { streams.Signal(); }
        public bool Drain() { return streams.Wait(10000); }
        public void Add(string line) {
            if (line == null) return;
            lock (gate) {
                // Inspect every drained line, including after bounded log capture fills.
                if (line.IndexOf("PSSecurityException", StringComparison.OrdinalIgnoreCase) >= 0 ||
                    line.IndexOf("running scripts is disabled", StringComparison.OrdinalIgnoreCase) >= 0 ||
                    line.IndexOf("not digitally signed", StringComparison.OrdinalIgnoreCase) >= 0)
                    PolicyRefusal=true;
                int room = CaptureCap - text.Length;
                if (room <= 0) { Truncated=true; return; }
                if (line.Length + 1 > room) { text.Append(line.Substring(0, Math.Min(line.Length, room))); Truncated=true; }
                else text.AppendLine(line);
            }
        }
        public Result Result(int exit) { lock (gate) return new Result { Exit=exit, Text=text.ToString(), Truncated=Truncated, PolicyRefusal=PolicyRefusal }; }
    }
    static ProcessStartInfo StartInfo(string file, string args, string cwd) {
        return new ProcessStartInfo(file, args) { WorkingDirectory=cwd, UseShellExecute=false,
            CreateNoWindow=true, RedirectStandardOutput=true, RedirectStandardError=true };
    }
    static Process Start(ProcessStartInfo info, Capture capture, Action<string> stdout) {
        var process = new Process { StartInfo=info };
        process.OutputDataReceived += delegate(object sender, DataReceivedEventArgs data) {
            if (data.Data == null) { capture.End(); return; }
            capture.Add(data.Data); if (stdout != null) stdout(data.Data);
        };
        process.ErrorDataReceived += delegate(object sender, DataReceivedEventArgs data) {
            if (data.Data == null) capture.End(); else capture.Add(data.Data);
        };
        if (!process.Start()) throw new InvalidOperationException("Process launch refused: " + Path.GetFileName(info.FileName));
        process.BeginOutputReadLine(); process.BeginErrorReadLine(); return process;
    }
    static Result Run(string file, string args, string cwd, int milliseconds, Action<string> stdout) {
        var capture = new Capture();
        using (Process process = Start(StartInfo(file,args,cwd), capture, stdout)) {
            if (!process.WaitForExit(milliseconds)) throw new InvalidOperationException("Supervisory timeout; pair stopped, no process kill or rescue attempted: " + Path.GetFileName(file));
            if (!capture.Drain()) throw new InvalidOperationException("Bounded output drain unavailable; no retry or process rescue: " + Path.GetFileName(file));
            return capture.Result(process.ExitCode);
        }
    }
    static string Quote(string value) { if (value.IndexOf('"') >= 0) throw new ArgumentException("Unexpected quote in generated argument."); return "\"" + value + "\""; }
    static void Log(string value) {
        using (var reader = new StringReader(value)) { string line; while ((line=reader.ReadLine()) != null) Console.WriteLine("DIAG " + line); }
    }
    static string Git(string cwd, string args) {
        Result result=Run("git.exe",args,cwd,30000,null);
        if (result.Exit != 0 || result.Truncated) throw new InvalidOperationException("Git identity/cleanliness check unavailable: " + args);
        return result.Text.Trim();
    }
    static void VerifySource(string cwd, string commit) {
        if (Git(cwd,"rev-parse HEAD") != commit) throw new InvalidOperationException("Source commit mismatch.");
        if (Git(cwd,"status --porcelain --untracked-files=no").Length != 0) throw new InvalidOperationException("Tracked source/test/config changed; pair stopped.");
        foreach (var item in SafetyBlobs) {
            if (Git(cwd,"rev-parse " + Quote("HEAD:" + item.Key)) != item.Value ||
                Git(cwd,"hash-object --no-filters " + Quote(item.Key)) != item.Value)
                throw new InvalidOperationException("Safety source bytes differ from reviewed blob: " + item.Key);
        }
    }
    static Dictionary<string,object> Object(string text) { return Json.Deserialize<Dictionary<string,object>>(text); }
    static string Policy(string phase) {
        if (!String.IsNullOrEmpty(Environment.GetEnvironmentVariable("PSExecutionPolicyPreference")))
            throw new InvalidOperationException("Inherited process execution-policy setting present; no override/clearing attempted.");
        string command = "$ErrorActionPreference='Stop'; [ordered]@{effective=[string](Get-ExecutionPolicy);scopes=@(Get-ExecutionPolicy -List | ForEach-Object { [ordered]@{scope=[string]$_.Scope;policy=[string]$_.ExecutionPolicy} })} | ConvertTo-Json -Depth 4 -Compress";
        Result result=Run("powershell.exe", "-NoProfile -NonInteractive -Command " + Quote(command), workspace, 30000, null);
        if (result.Exit != 0 || result.Truncated) throw new InvalidOperationException("Read-only default policy query unavailable.");
        var policy=Object(result.Text);
        string effective=Convert.ToString(policy["effective"], CultureInfo.InvariantCulture);
        Log("policy " + phase + " " + result.Text.Trim());
        File.WriteAllText(Path.Combine(output,"policy-"+phase+".json"),result.Text,new UTF8Encoding(false));
        if (effective == "Restricted" || effective == "AllSigned")
            throw new InvalidOperationException("Default native helper policy " + effective + " blocks unsigned unchanged helper; no test attempted and no override.");
        return Json.Serialize(policy);
    }
    static string Build(string label, string cwd) {
        string executable=null, parseFailure=null;
        Result result=Run("cargo.exe", "+stable test --locked --lib --features " + Features + " --no-run --message-format=json", cwd, 480000,
            delegate(string line) {
                try {
                    var item=Object(line); object reason, target, profile, filenames;
                    if (!item.TryGetValue("reason",out reason) || Convert.ToString(reason) != "compiler-artifact" ||
                        !item.TryGetValue("target",out target) || !item.TryGetValue("profile",out profile) || !item.TryGetValue("filenames",out filenames)) return;
                    var targetObject=(Dictionary<string,object>)target; var profileObject=(Dictionary<string,object>)profile;
                    if (Convert.ToString(targetObject["name"]) != "bowser" || !Convert.ToBoolean(profileObject["test"])) return;
                    foreach (object name in (IEnumerable)filenames) {
                        string path=Convert.ToString(name);
                        if (path.EndsWith(".exe",StringComparison.OrdinalIgnoreCase)) {
                            if (executable != null && executable != path) parseFailure="Multiple test executable artifacts.";
                            executable=Path.GetFullPath(path);
                        }
                    }
                } catch (Exception error) { parseFailure="Cargo JSON unavailable: " + error.GetType().Name; }
            });
        File.WriteAllText(Path.Combine(output,label+"-build.log"),result.Text,new UTF8Encoding(false));
        Log(label + " build_exit=" + result.Exit + " captured_log_truncated=" + result.Truncated);
        // Cargo JSON is intentionally not copied into the job log: bounded compiler stderr
        // and the exact discovered artifact identity suffice; a failed build emits its cap.
        if (result.Exit != 0 || parseFailure != null || executable == null || !File.Exists(executable)) {
            Log(result.Text); throw new InvalidOperationException(label + " build blocked: " + (parseFailure ?? "exact artifact unavailable or build failed"));
        }
        if (!executable.StartsWith(Path.GetFullPath(Path.Combine(cwd,"target"))+Path.DirectorySeparatorChar,StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException("Artifact not inside exact checkout target directory.");
        return executable;
    }
    static string Hash(string path) {
        using (var hash=SHA256.Create()) using (var file=File.OpenRead(path)) return BitConverter.ToString(hash.ComputeHash(file)).Replace("-","").ToLowerInvariant();
    }
    static void EmitEvidence(string label, string cwd, string laneOutput) {
        string root=Path.Combine(cwd,"store-real-app-host-note-split-context-menu");
        string[] allowed={"scope","status","mode","separator_projection_contract","grayed_projection_contract","right_input_called","selection_called","clipboard","error","failure_cleanup","activation_before_exact_owner","activation_returned_success","activation_after_exact_owner","activation_polls","activation_elapsed_ms","activation_observed_exact_owner"};
        foreach (string mode in new[]{"note","split"}) {
            string path=Path.Combine(root,mode,"app-host-context-menu.json");
            if (!File.Exists(path)) { Log(label+" "+mode+" helper_report=absent"); continue; }
            if (new FileInfo(path).Length > 1048576) throw new InvalidOperationException("Helper evidence exceeds read bound.");
            var source=Object(File.ReadAllText(path)); var selected=new Dictionary<string,object>();
            foreach (string key in allowed) if (source.ContainsKey(key)) selected.Add(key,source[key]);
            string line=Json.Serialize(selected); Log(label+" "+mode+" helper="+line);
            Summary.Add("- "+label+" "+mode+": `"+line.Replace("`", "'")+"`");
        }
        foreach (string name in new[]{"source-readiness-phase.json","app-host-note-split-summary.json","native-handler-teardown.json"}) {
            string path=Path.Combine(root,name);
            if (File.Exists(path) && new FileInfo(path).Length <= 16384) Log(label+" "+name+" "+File.ReadAllText(path));
        }
        // Only the new strictly process-bound observer is emitted, not the unchanged helper's
        // general runner_state snapshot, clipboard metadata, request trees, or image contents.
        foreach (string name in new[]{"observer-summary.json","observer.jsonl"}) {
            string path=Path.Combine(laneOutput,name);
            if (File.Exists(path) && new FileInfo(path).Length <= 524288) Log(label+" "+name+"\n"+File.ReadAllText(path));
        }
    }
    static int Lane(string label, string cwd, string exe) {
        string laneOutput=Path.Combine(output,label); Directory.CreateDirectory(laneOutput);
        if (Directory.Exists(Path.Combine(cwd,"store-real-app-host-note-split-context-menu")))
            throw new InvalidOperationException("Refusing reused fixture evidence directory.");
        var capture=new Capture(); var info=StartInfo(exe,Test+" --exact --ignored --nocapture --test-threads=1",cwd);
        info.EnvironmentVariables["BOWSER_APP_HOST_NOTE_SPLIT_PROBE_CONSENT"]="disposable-app-host-menu-note-split-no-clipboard";
        Log(label+" source="+(label=="baseline"?Base:Candidate)+" executable_sha256="+Hash(exe)+" features="+Features);
        using (Process test=Start(info,capture,null)) {
            // No target suspension, thread manipulation, new input, or activation preparation.
            ulong created=unchecked((ulong)test.StartTime.ToUniversalTime().ToFileTimeUtc());
            var observerCapture=new Capture();
            string observerArgs=test.Id.ToString(CultureInfo.InvariantCulture)+" "+Quote(exe)+" "+created.ToString(CultureInfo.InvariantCulture)+" "+Quote(laneOutput);
            using (Process observer=Start(StartInfo(Path.Combine(output,"Observer.exe"),observerArgs,workspace),observerCapture,null)) {
                bool exited=test.WaitForExit(300000);
                if (!exited) throw new InvalidOperationException(label+" supervisory cap reached; pair stopped without kill, input, or rescue. Existing fixture deadlines are unchanged.");
                if (!capture.Drain()) throw new InvalidOperationException(label+" output pipes did not close within bounded drain; pair stopped.");
                Result result=capture.Result(test.ExitCode);
                bool observerExited=observer.WaitForExit(10000);
                if (observerExited && !observerCapture.Drain()) throw new InvalidOperationException(label+" observer output drain unavailable; pair stopped.");
                File.WriteAllText(Path.Combine(laneOutput,"test.log"),result.Text,new UTF8Encoding(false));
                Log(label+" test_exit="+result.Exit+" captured_log_truncated="+result.Truncated+"\n"+result.Text);
                EmitEvidence(label,cwd,laneOutput);
                if (!observerExited || observer.ExitCode != 0 || !File.Exists(Path.Combine(laneOutput,"observer.ready"))) {
                    if (observerExited) Log(observerCapture.Result(observer.ExitCode).Text);
                    throw new InvalidOperationException(label+" passive observer unavailable/incomplete; no comparison acceptance and no next lane.");
                }
                Summary.Add("- "+label+" libtest exit: **"+result.Exit+"**; exact executable SHA-256 `"+Hash(exe)+"`.");
                if (result.PolicyRefusal) throw new InvalidOperationException(label+" unchanged helper reported execution-policy refusal; candidate not attempted, no bypass.");
                return result.Exit;
            }
        }
    }
    static int Main() {
        int exit=80; string initialPolicy=null; bool started=false;
        try {
            workspace=Path.GetFullPath(Environment.GetEnvironmentVariable("GITHUB_WORKSPACE") ?? "");
            output=Path.Combine(Environment.GetEnvironmentVariable("RUNNER_TEMP") ?? "", "bowser-note-split-comparison");
            if (Environment.GetEnvironmentVariable("GITHUB_ACTIONS") != "true" ||
                Environment.GetEnvironmentVariable("BOWSER_DIAGNOSTIC_RUNNER_ENVIRONMENT") != "github-hosted" ||
                Environment.GetEnvironmentVariable("GITHUB_REPOSITORY") != "grantcharlie1000-tech/Bowser" ||
                Environment.GetEnvironmentVariable("GITHUB_RUN_ATTEMPT") != "1")
                throw new InvalidOperationException("Expected first attempt on the isolated standard public GitHub runner only.");
            var webhook=Object(File.ReadAllText(Environment.GetEnvironmentVariable("GITHUB_EVENT_PATH")));
            var repository=(Dictionary<string,object>)webhook["repository"];
            if (Convert.ToBoolean(repository["private"])) throw new InvalidOperationException("Public-repository free-runner condition absent.");
            foreach (string name in new[]{"RC","RUSTC_WRAPPER","RUSTC_WORKSPACE_WRAPPER","RUSTFLAGS","CARGO_ENCODED_RUSTFLAGS"})
                if (!String.IsNullOrEmpty(Environment.GetEnvironmentVariable(name)))
                    throw new InvalidOperationException("Inherited build override present: "+name+"; stopped without clearing or changing it.");
            Summary.Add("## One bounded host-only Note/Split baseline/candidate pair");
            Summary.Add("- Baseline `"+Base+"`; candidate tested merge `"+Candidate+"`.");
            Summary.Add("- Standard `windows-2025-vs2026`; same fresh VM/toolchain; no cache/artifact upload; one attempt, no retry.");
            Summary.Add("- Test source, native helper, safety predicates/deadlines and input guards are unchanged. The observer grants no authority.");
            string baseline=Path.Combine(workspace,"baseline"), candidate=Path.Combine(workspace,"candidate");
            VerifySource(baseline,Base); VerifySource(candidate,Candidate);
            initialPolicy=Policy("before"); started=true;
            string baselineExe=Build("baseline",baseline), candidateExe=Build("candidate",candidate);
            if (Policy("after-build") != initialPolicy) throw new InvalidOperationException("Observed execution-policy state changed during preparation; stop, no automatic restoration.");
            VerifySource(baseline,Base); VerifySource(candidate,Candidate);
            int baseExit=Lane("baseline",baseline,baselineExe);
            VerifySource(baseline,Base);
            if (Policy("after-baseline") != initialPolicy) throw new InvalidOperationException("Observed execution-policy state changed after baseline; candidate not run, no automatic restoration.");
            int candidateExit=Lane("candidate",candidate,candidateExe);
            VerifySource(candidate,Candidate);
            if (Policy("after-candidate") != initialPolicy) throw new InvalidOperationException("Observed execution-policy state changed after candidate; no automatic restoration.");
            Summary.Add("- Pair completed. Baseline exit "+baseExit+", candidate exit "+candidateExit+". One pair does not establish causality or Store compatibility.");
            exit=baseExit==0 && candidateExit==0 ? 0 : 1;
        } catch (Exception error) {
            string message="BLOCKED: "+error.GetType().Name+": "+error.Message;
            Log(message); Summary.Add("- **"+message.Replace("`","'")+"**");
        } finally {
            // A final read-only policy observation is attempted even after a failed lane. No
            // restoration or security-setting write is ever attempted by this harness.
            if (started && initialPolicy != null) {
                try { bool same=Policy("final")==initialPolicy; Summary.Add("- Final observed PowerShell policy scopes "+(same?"unchanged.":"CHANGED; inspect policy logs. No restoration attempted.")); if (!same) exit=80; }
                catch (Exception error) { Summary.Add("- Final policy state unverified: "+error.Message); exit=80; }
            }
            Summary.Add("- This is not a system-wide security audit. No new observer clipboard/input/activation API, policy override, persistent security write, release, merge, or deployment exists in the diagnostic.");
            string summary=String.Join(Environment.NewLine,Summary)+Environment.NewLine;
            Log(summary);
            string path=Environment.GetEnvironmentVariable("GITHUB_STEP_SUMMARY");
            if (!String.IsNullOrEmpty(path)) File.AppendAllText(path,summary,new UTF8Encoding(false));
        }
        return exit;
    }
}
