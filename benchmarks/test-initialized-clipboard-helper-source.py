#!/usr/bin/env python3
"""Source-only contracts for the separate opt-in fixture. Never runs Windows APIs.

These checks and the helper's synthetic policy cases are not native acceptance,
clipboard execution, continuous-custody proof, or permission to run the fixture.
"""
from pathlib import Path
import hashlib
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
HELPER = (ROOT / "benchmarks/scripts/probe-app-host-context-menu.ps1").read_text(encoding="utf-8")
CS = HELPER.split("Add-Type @'\n", 1)[1].split("\n'@", 1)[0]
# Frozen original class, including its existing cleanup gap. Do not silently
# expand this new opt-in candidate into an unrelated ordinary-lane change.
ORDINARY_CLASS_SHA256 = "fa9f96064e086c6f741a4da920137e067e57ac189c22bcd7ad7a8896c2322a59"


def block(signature, text=CS):
    """Balanced C# body extraction, ignoring strings, characters and comments."""
    start = text.index(signature)
    masked = re.sub(r'//[^\n]*|/\*.*?\*/|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'',
                    lambda m: "".join("\n" if c == "\n" else " " for c in m.group()), text,
                    flags=re.S)
    opening = masked.index("{", start)
    depth = 1
    for end in range(opening + 1, len(text)):
        depth += (masked[end] == "{") - (masked[end] == "}")
        if depth == 0:
            return text[start:end + 1]
    raise AssertionError(f"Unbalanced body for {signature}")


class InitializedClipboardHelperSourceContracts(unittest.TestCase):
    def test_entire_ordinary_guard_and_zero_refusal_are_frozen(self):
        ordinary = " " + block("public sealed class ClipboardGuard:IDisposable")
        self.assertEqual(hashlib.sha256(ordinary.encode()).hexdigest(), ORDINARY_CLASS_SHA256)
        self.assertNotIn("Initialized", ordinary)
        sequence = block("uint Sequence()", ordinary)
        self.assertIn("if(value==0)", sequence)
        constructor = block("public ClipboardGuard(string generated)", ordinary)
        self.assertLess(constructor.index("last=Sequence();"), constructor.index("if(!EmptyClipboard())"))
        self.assertIn("first!=0||error!=0", constructor)
        diagnostic_interval = HELPER.split("public static object ClipboardEnvironment(", 1)[1].split("/// External to Rust", 1)[0]
        for forbidden in ("EmptyClipboard(", "GetClipboardData(", "SetClipboardData(", "GetClipboardSequenceNumber("):
            self.assertNotIn(forbidden, diagnostic_interval)

    def test_explicit_copy_only_separate_entry_and_no_fallback(self):
        self.assertIn("[switch]$InitializedEmptyClipboardFixture", HELPER)
        mode_check = "if($InitializedEmptyClipboardFixture){if($Mode -cne 'Copy')"
        self.assertIn(mode_check, HELPER)
        self.assertLess(HELPER.index(mode_check), HELPER.index("$initializer=[BowserHostMenu+InitializedEmptyClipboardFixture]::new()"))
        self.assertEqual(HELPER.count("[BowserHostMenu+InitializedEmptyClipboardFixture]::new()"), 1)
        self.assertIn("$clipboard=if($InitializedEmptyClipboardFixture){[BowserHostMenu+InitializedFixtureClipboardGuard]::new($payload)}else{[BowserHostMenu+ClipboardGuard]::new($payload)}", HELPER)
        self.assertLess(HELPER.index("$clipboard=if($InitializedEmptyClipboardFixture)"), HELPER.index("$initializer.CompleteIndependentAdmission()"))
        self.assertLess(HELPER.index("$initializer.CompleteIndependentAdmission()"), HELPER.index("::mouse_event(8,"))
        strict_constructor = block("public InitializedFixtureClipboardGuard(string generated)")
        self.assertIn("!initializedFixtureReady", strict_constructor)
        self.assertIn("CompareExchange(ref initializedGuardClaim,1,0)", strict_constructor)
        self.assertIn("last=Sequence();", strict_constructor)
        self.assertLess(strict_constructor.index("last=Sequence();"), strict_constructor.index("if(!EmptyClipboard())"))

    def test_attestation_is_bounded_and_explicitly_not_sole_authority(self):
        contract = block("public static void RequireInitializedFixtureContract()")
        for token in ("GITHUB_ACTIONS", "RUNNER_ENVIRONMENT", "github-hosted", "RUNNER_OS", "Windows", "GITHUB_REPOSITORY", "grantcharlie1000-tech/Bowser", "GITHUB_JOB", "initialized-empty-copy", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_SHA", "ImageVersion", "^[0-9a-f]{40}$", "{1,80}", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_CONTRACT", "github-hosted-fresh-exclusive-empty-v1", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_JOB", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_CHECKOUT", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_RUN", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_ATTEMPT"):
            self.assertIn(token, contract)
        self.assertEqual(contract.count("UInt64.TryParse("), 2)
        self.assertIn("parsedRun==0", contract)
        self.assertIn("parsedAttempt==0", contract)
        self.assertIn("NOT proof of exclusivity", CS)
        self.assertIn("reviewed dedicated fresh/exclusive disposable-VM job", CS)
        for forbidden in ("SetProcessWindowStation(", "SetThreadDesktop(", "SetUserObjectSecurity(", "AdjustTokenPrivileges(", "ChangeWindowMessageFilter("):
            self.assertNotIn(forbidden, CS)

    def test_initializer_one_exclusive_empty_attempt_and_no_content(self):
        initializer = block("static bool InitializeEmptyOnce(")
        self.assertEqual(initializer.count("ops.Open()"), 1)
        self.assertEqual(initializer.count("ops.Empty()"), 1)
        self.assertEqual(initializer.count("ops.Close()"), 1)
        for token in ("!ops.DedicatedContract", "ops.FirstFormat(out error)", "first==0&&error==0", "source!=IntPtr.Zero&&!own", "ops.ExactOwner(ops.Helper.Window,ops.Helper)", "receipt.after_sequence==0||!exact||!receipt.after_format_empty", "!receipt.close_confirmed"):
            self.assertIn(token, initializer)
        self.assertLess(initializer.index("receipt.empty_attempted=true;receipt.empty_attempts++"), initializer.index("ops.Empty()"))
        for forbidden in ("GetClipboardData(", "SetClipboardData(", "ops.Data(", "ReadBounded(", "while(", "for(", "Thread.Sleep("):
            self.assertNotIn(forbidden, initializer)
        enumeration = block("public uint FirstFormat(out int error)", block("class NativeClipboardPolicyOps:"))
        self.assertIn("SetLastError(0);uint first=EnumClipboardFormats(0);error=Marshal.GetLastWin32Error();", enumeration)
        fixture = block("public sealed class InitializedEmptyClipboardFixture:")
        self.assertIn("CompareExchange(ref initializedFixtureClaim,1,0)", fixture)
        self.assertEqual(fixture.count("InitializeEmptyOnce("), 1)
        self.assertLess(fixture.index("RequireInitializedFixtureContract();"), fixture.index("new ClipboardOwnerWindow()"))

    def test_owner_queries_distinguish_null_success_from_unknown_before_mutation(self):
        self.assertIn('EntryPoint="GetClipboardOwner",ExactSpelling=true,SetLastError=true', CS)
        native = block("class NativeClipboardPolicyOps:")
        query = block("public bool TryOwner(", native)
        self.assertIn("SetLastError(0);owner=InitializedClipboardOwner();error=Marshal.GetLastWin32Error();return error==0;", query)
        owner = block("public IntPtr Owner()", native)
        self.assertIn("!TryOwner(out value,out error)||error!=0", owner)
        self.assertNotIn("GetClipboardOwner()", native)
        initializer = block("static bool InitializeEmptyOnce(")
        self.assertIn("receipt.before_owner_query_attempted=true", initializer)
        self.assertIn("receipt.before_owner_query_succeeded=ops.TryOwner(out source,out ownerError)", initializer)
        self.assertIn("receipt.before_owner_query_error=ownerError", initializer)
        self.assertIn("!receipt.before_owner_query_succeeded||ownerError!=0", initializer)
        self.assertLess(initializer.index("!receipt.before_owner_query_succeeded||ownerError!=0"), initializer.index("ops.Empty()"))
        self.assertIn('source==IntPtr.Zero?"null-error0"', initializer)
        self.assertIn("!receipt.after_owner_query_succeeded||ownerError!=0", initializer)
        strict = block("public sealed class InitializedFixtureClipboardGuard:")
        self.assertIn("policyOps.ExactOwner(policyOps.Owner(),helperEpoch)", strict)
        self.assertNotIn("GetClipboardOwner()", strict)
        cases = block("public static void CheckInitializedClipboardPolicy()")
        self.assertIn('"initializer null owner error0",new SyntheticClipboardPolicyOps{CurrentOwner=IntPtr.Zero,OwnerQueryError=0},true,1', cases)
        self.assertIn('"initializer null owner error",new SyntheticClipboardPolicyOps{CurrentOwner=IntPtr.Zero,OwnerQueryError=5},false,0', cases)
        self.assertIn('"initializer unknown owner query"', cases)
        self.assertIn('"cleanup owner query error"', cases)
        self.assertIn('"Copy owner query error"', cases)

        for label in ("Copy post-getter null owner error", "Copy post-read null owner error", "cleanup post-clear null owner error"):
            self.assertIn(label, cases)
        fake_query = block("public bool TryOwner(", block("class SyntheticClipboardPolicyOps:"))
        self.assertIn("owner=postError?IntPtr.Zero:CurrentOwner", fake_query)
        self.assertIn("error=postError?5:OwnerQueryError", fake_query)
        self.assertIn("OwnerErrorAfterContent&&ContentReadCompleted", fake_query)

    def test_synthetic_no_input_counter_is_explicit_without_suppressing_compiler_warnings(self):
        synthetic = block("class SyntheticClipboardPolicyOps:")
        self.assertIn("InputCalls=0", synthetic)
        self.assertNotIn("InputCalls++", synthetic)
        self.assertNotIn("#pragma warning disable", CS)
        checker = (ROOT / "benchmarks/scripts/test-native-menu-helpers.ps1").read_text()
        self.assertNotIn("-IgnoreWarnings", checker)

    def test_initializer_handoff_is_not_cleanup_or_custody(self):
        fixture = block("public sealed class InitializedEmptyClipboardFixture:")
        dispose = block("public void Dispose()", fixture)
        for forbidden in ("EmptyClipboard(", ".Empty(", "Finish(", "SetClipboardData(", "GetClipboardData("):
            self.assertNotIn(forbidden, dispose)
        self.assertIn("window.Dispose()", dispose)
        self.assertIn("unchanged-guard-independently-admitted-not-atomic-custody", fixture)
        self.assertIn("initializer_cleanup_authority=false,initializer_cleanup_after_handoff=false,atomic_handoff=false,continuous_custody=false", CS)
        self.assertIn("environment-precondition-only-not-native-copy-or-continuous-custody", CS)

    def test_owned_windows_are_live_pumped_and_lexically_destroyed(self):
        window = block("sealed class ClipboardOwnerWindow:")
        self.assertIn("new System.Threading.Thread(Run)", window)
        self.assertIn('CreateWindowEx(0,"STATIC"', window)
        self.assertIn("ClipboardGetMessage(out message", window)
        self.assertIn("TranslateMessage(ref message);ClipboardDispatchMessage(ref message)", window)
        self.assertIn("finally{lock(sync){window=IntPtr.Zero;}", window)
        self.assertIn("owned==IntPtr.Zero||DestroyWindow(owned)", window)
        self.assertIn("ClipboardPostThreadMessage(ownThread,0x0012", window)
        self.assertIn("windowClass=actualClass", window)
        self.assertIn("actualClass.Length>=63", window)
        self.assertIn('String.Equals(actualClass,"Static",StringComparison.OrdinalIgnoreCase)', window)
        self.assertIn("HelperWindow=this,HelperToken=token", window)
        self.assertIn("token==expectedToken", window)
        self.assertIn("window==expected&&thread.IsAlive", window)
        layout = block("static bool ClipboardMessageLayoutValid()")
        self.assertIn("pointer==8?48:32", layout)
        for offset in ("16:8", "24:12", "32:16", "36:20", "44:28"):
            self.assertIn(offset, layout)
        self.assertIn('EntryPoint="GetMessageW",ExactSpelling=true,SetLastError=true', CS)
        self.assertNotIn("EmptyClipboard(", window)
        self.assertNotIn(".Abort(", window)

    def test_cleanup_reentry_exact_owner_and_one_attempt_fences(self):
        cleanup = block("sealed class ClipboardCleanupFence")
        finish = block("public bool Finish(", cleanup)
        self.assertIn("Interlocked.CompareExchange(ref finishState,1,0)", finish)
        self.assertIn("!ops.DedicatedContract", finish)
        self.assertIn("!armed||sequence==0", finish)
        self.assertEqual(finish.count("ops.Open()"), 1)
        self.assertEqual(finish.count("!Match(ops)"), 2)
        self.assertEqual(finish.count("HelperLive(ops)"), 2)
        self.assertLess(finish.index("Receipt.helper_verified_before_open=HelperLive(ops)"), finish.index("ops.Open()"))
        self.assertLess(finish.index("Receipt.helper_verified_at_final_fence=HelperLive(ops)"), finish.index("ops.Empty()"))
        helper_live = block("bool HelperLive(", cleanup)
        self.assertIn("ops.ExactOwner(helper.Window,helper)", helper_live)
        self.assertEqual(finish.count("ops.Empty()"), 1)
        self.assertLess(finish.index("Attempted=true;Receipt.attempts++"), finish.index("ops.Empty()"))
        last_fence = finish.rsplit("if(!Match(ops))", 1)[1].split("ops.Empty()", 1)[0]
        for forbidden in ("Data(", "ReadBounded(", "ClipboardDispatchMessage(", "ClipboardGetMessage(", "Invoke(", "Sleep("):
            self.assertNotIn(forbidden, last_fence)
        for token in ("after!=0&&Receipt.post_owner_is_helper&&Receipt.post_empty_verified", "Receipt.post_owner_is_helper=ops.ExactOwner(source,ops.Helper)", "Receipt.post_empty_verified=first==0&&error==0", "Receipt.close_confirmed=closed", "Receipt.success=restored"):
            self.assertIn(token, finish)
        identity = block("public bool ExactOwner(", block("class NativeClipboardPolicyOps:"))
        for token in ("actual!=expected.Window", "expected.Browser.SameAlive()", "tid!=expected.Thread", "pid!=expected.Pid", "expected.WindowClass", "IsWindow(actual)"):
            self.assertIn(token, identity)
        self.assertGreaterEqual(identity.count("expected.HelperWindow.SameAlive(actual,expected.HelperToken)"), 2)
        self.assertIn("never a foreign HWND lifetime lease", identity)

    def test_genuine_copy_completion_precedes_all_content_and_cleanup_arm(self):
        observe = block("static void ObserveNativeCopyOnce(")
        self.assertLess(observe.index("!MatchingCopyCompletion(completion)"), observe.index("ops.Open()"))
        self.assertLess(observe.index("observed==0||observed==previous"), observe.index("ops.Data()"))
        self.assertEqual(observe.count("ops.Data()"), 1)
        self.assertIn("size<2||size>8192||(size&1)!=0", observe)
        self.assertGreaterEqual(observe.count("CopyEpochMatches(ops,observed,epoch)"), 4)
        self.assertIn("!terminated||actual!=expected", observe)
        self.assertIn("if(!ops.UnlockData(data))", observe)
        self.assertIn("finally{if(!ops.Close())", observe)
        strict = block("public sealed class InitializedFixtureClipboardGuard:")
        guarded = block("public void ObserveNativeCopy(", strict)
        arming = block("static void ObserveCopyAndArmCleanup(")
        self.assertLess(arming.index("ObserveNativeCopyOnce("), arming.index("cleanup.Arm(observed,epoch)"))
        self.assertIn("ObserveCopyAndArmCleanup(", guarded)
        self.assertIn("catch{uncertain=true;cleanup.Disarm();throw;}", guarded)
        self.assertIn("public ReadOnlyBrowser Browser", block("sealed class ClipboardEpoch"))
        self.assertIn("Browser=browser", block("public static ClipboardEpoch NativeCopy("))
        self.assertIn("$completion.ExpectedSelected=[int]$report.selected_command_id", HELPER)
        self.assertLess(HELPER.index("$report.completed=$settled"), HELPER.index("$clipboard.ObserveNativeCopy($BrowserPid,$browserGuard,('copy-'+$SyntheticToken),$completion)"))

    def test_synthetic_managed_cases_exercise_shared_policy_and_operation_counts(self):
        cases = block("public static void CheckInitializedClipboardPolicy()")
        for label in ("ordinary initial zero never initializes", "ordinary nonempty", "ordinary locked", "ordinary enumeration error", "initializer remains zero", "initializer changed owner", "initializer close uncertainty", "foreign same-sequence owner", "same owner changed sequence", "wrong process", "wrong class", "wrong thread", "browser exited", "unknown identity", "late owner mismatch", "cleanup reentry", "late read or close uncertainty disarms cleanup", "missing completion", "wrong completion nonce", "wrong completion request", "wrong selected command", "duplicate setter", "failed completion", "delayed rendering sequence change", "delayed rendering browser exit", "wrong generated text", "unterminated text", "malformed odd-sized text", "Copy close uncertainty"):
            self.assertIn('"' + label + '"', cases)
        for forbidden in ("new NativeClipboardPolicyOps", "new NativeCopyPolicyOps", "new ClipboardOwnerWindow", "GetClipboardData(", "EmptyClipboard(", "SetClipboardData(", "mouse_event("):
            self.assertNotIn(forbidden, cases)
        initializer_case = block("static void CheckInitializerCase(")
        cleanup_case = block("static void CheckCleanupCase(")
        copy_case = block("static void CheckCopyCase(")
        self.assertIn("InitializeEmptyOnce(ops,receipt)", initializer_case)
        self.assertIn("ops.EmptyCalls==emptyAttempts", initializer_case)
        self.assertIn("fence.Finish(ops,out after)", cleanup_case)
        self.assertIn("ops.MutationsAtReentry==0", cleanup_case)
        self.assertIn("ObserveCopyAndArmCleanup(ops,7,completion", copy_case)
        self.assertIn("failed observation disarms cleanup", copy_case)
        self.assertIn("ops.ContentCalls==getters&&ops.Reads==reads&&ops.EmptyCalls==0&&ops.InputCalls==0", copy_case)
        self.assertIn("Synthetic shared-policy tests", CS)
        for label in ("synthetic distinct browser epoch exact live helper", "synthetic browser valid but helper dead", "synthetic browser valid but helper unknown", "synthetic browser valid but final helper lost", "synthetic browser valid but final helper dead", "synthetic browser valid but final helper unknown"):
            self.assertIn(label, cases)
        self.assertIn("Window=new IntPtr(22)", cases)
        self.assertIn("deadHelper.OpenCalls==0", cases)
        self.assertIn("unknownHelper.OpenCalls==0", cases)
        self.assertIn("lateHelper.OpenCalls==1", cases)
        self.assertIn("finalDeadHelper.OpenCalls==1", cases)
        self.assertIn("finalUnknownHelper.OpenCalls==1", cases)
        self.assertIn("HelperDiesAtMatch=2", cases)
        self.assertIn("HelperUnknownAtMatch=2", cases)

    def test_separate_report_exposes_precondition_and_strict_cleanup_evidence(self):
        for token in ("selected=true,initialization_attempted,initialization_succeeded", "empty_attempts", "after_sequence", "after_owner", "after_format_empty", "after_enum_error", "close_confirmed", "native_copy_arm_succeeded", "armed_before_finish", "exact_owner_verified_twice", "Receipt.attempts++", "post_owner_is_helper", "post_empty_verified", "Receipt.success=restored", "$report.initialized_empty_fixture", "$report.initialized_clipboard_cleanup=$clipboard.CleanupEvidence"):
            self.assertIn(token, HELPER)
        self.assertIn("latest-exact-owner-fences-not-hwnd-lifetime-lease", HELPER)
        final = HELPER.split("}finally{if($clipboard){$clipboard.Dispose()}", 1)[1]
        self.assertLess(final.index("$initializer.Dispose()"), final.index("$browserGuard.Dispose()"))


if __name__ == "__main__":
    unittest.main()
