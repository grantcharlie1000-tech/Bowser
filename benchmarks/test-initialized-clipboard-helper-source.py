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

    def test_synthetic_data_lock_failure_exercises_the_existing_refusal(self):
        cases = block("public static void CheckInitializedClipboardPolicy()")
        self.assertIn('"native Copy data lock unavailable",new SyntheticCopyPolicyOps{CurrentSequence=8,CurrentOwner=new IntPtr(11),LockFails=true},SyntheticCompletion(),false,1,0', cases)
        copy = block("static void ObserveNativeCopyOnce(")
        self.assertIn('if(pointer==IntPtr.Zero)throw new InvalidOperationException("Known native Copy data unavailable.")', copy)

    def test_every_synthetic_refusal_switch_is_exercised_without_warning_suppression(self):
        cases = block("public static void CheckInitializedClipboardPolicy()")
        for name in ("Contract", "OpenOk", "CloseOk", "EmptyOk", "Exact", "Alive", "Pid", "Thread", "WindowClass", "HelperAlive", "HelperKnown", "HelperPid", "HelperThread", "HelperClass", "OwnerQueryOk"):
            self.assertIn(name + "=false", cases, name)
        for name in ("Unknown", "OwnerErrorAfterEmpty", "ZeroAfterEmpty", "WrongOwnerAfterEmpty", "FailAfterEnum", "Reenter", "MissingData", "ZeroSize", "Oversized", "OddSize", "LockFails", "UnlockFails", "Unterminated", "ChangeOnData", "ChangeOnRead", "ExitOnData", "OwnerErrorAfterData", "OwnerErrorAfterContent"):
            self.assertIn(name + "=true", cases, name)
        for name in ("LateMismatchAtMatch", "HelperFailAtMatch", "HelperDiesAtMatch", "HelperUnknownAtMatch"):
            self.assertIn(name + "=2", cases, name)
        for label in ("helper process wrong", "helper thread wrong", "helper class wrong", "cleanup owner query unknown without error", "cleanup post-clear enumeration error", "Copy owner query unknown without error"):
            self.assertIn(label, cases)

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

class RequireOwnedMismatchSourceContracts(unittest.TestCase):
    """Portable source contracts only, not executed Win32 or managed-policy tests."""

    def strict(self):
        return block("public sealed class InitializedFixtureClipboardGuard:")

    def test_mismatch_receipt_has_only_bounded_scalar_fields_and_fixed_categories(self):
        receipt = block("public sealed class RequireOwnedMismatchReceipt", self.strict())
        fields = {}
        for kind, declarations in re.findall(r"public (string|uint\??|int\??|bool) ([^;]+);", receipt):
            for declaration in declarations.split(","):
                name = declaration.split("=", 1)[0].strip()
                fields[name] = kind
        self.assertEqual(fields, {
            "scope": "string", "observation_timing": "string",
            "expected_sequence": "uint", "observed_sequence": "uint?",
            "observed_sequence_status": "string", "checked_owner": "string",
            "helper_liveness": "string", "owner_query_attempted": "bool",
            "owner_query_succeeded": "bool", "owner_query_error": "int?",
        })
        self.assertIn('scope="initialized-fixture-require-owned-refusal-read-only-not-authority"', receipt)
        self.assertIn('observation_timing="later-after-refusal-and-confirmed-close-non-atomic-not-failing-check"', receipt)
        self.assertIn('observed_sequence_status="unavailable",checked_owner="unknown-query",helper_liveness="unknown"', receipt)
        capture = block("void CaptureRequireOwnedMismatchLater()", self.strict())
        # Every emitted category is a literal. No handles, identities, arbitrary errors or content.
        self.assertEqual(set(re.findall(r'"([^"\\]*)"', receipt + capture)), {
            "initialized-fixture-require-owned-refusal-read-only-not-authority",
            "later-after-refusal-and-confirmed-close-non-atomic-not-failing-check",
            "unavailable", "observed", "unknown-query", "null-error0",
            "exact-helper", "other-or-not-proven", "unknown", "exact-live-helper", "not-proven",
        })
        for forbidden in ("IntPtr", "Exception", "Class", "Title", "Path", "Url", "object", "List<"):
            self.assertNotIn(forbidden, receipt)

    def test_original_predicate_short_circuit_api_order_and_throw_are_retained(self):
        require = block("public void RequireOwned()", self.strict())
        self.assertIn("if(Sequence()!=last||!policyOps.ExactOwner(policyOps.Owner(),helperEpoch))", require)
        self.assertIn('throw new InvalidOperationException("Clipboard sequence/owner changed before native input; no input.")', require)
        # Removing only local failure instrumentation recreates the exact baseline method.
        original = require.replace("   bool closeConfirmed=false;\n", "")
        original = original.replace("CloseChecked();closeConfirmed=true;", "CloseChecked();")
        original = original.replace("if(closeConfirmed){try{CaptureRequireOwnedMismatchLater();}catch{}}", "")
        self.assertEqual(hashlib.sha256(original.encode()).hexdigest(),
                         "2f4fedf2f519c61e90eec4e809d993c0607f964428c02569f623c3cc739c4860")
        self.assertIn("bool closeConfirmed=false;", require)
        self.assertIn("finally{CloseChecked();closeConfirmed=true;}", require)
        # Capture is reachable only after refusal irrevocably disarms cleanup and close returned.
        self.assertIn("catch{uncertain=true;cleanup.Disarm();if(closeConfirmed){try{CaptureRequireOwnedMismatchLater();}catch{}}throw;}", require)
        self.assertEqual(require.count("CaptureRequireOwnedMismatchLater();"), 1)
        self.assertNotIn("return", require)

    def test_ordinary_and_authority_paths_are_frozen_except_read_only_close_snapshots(self):
        strict = self.strict()
        hashes = {
            "uint Sequence()": "8411bd80fc84398fd74e35981efe06492c017c1c271cbd7c9c9933fd45f71669",
            "void CloseChecked()": "18a7996e8591277ea8b9819e861f2a28e67174c67e6ce2ead9c44429c63f1f78",
            "public InitializedFixtureClipboardGuard(string generated)": "ac4739fa66af4aab3d40c2665e16627e58a5ff086e3e4e9e0a25f597557851fc",
            "public void ObserveNativeCopy(": "cf3a87f9d42ae9213a21e2ea2ee63f1bbcf2fcda3e48ed6aafa8e1f12e7d9515",
            "public bool Finish()": "9d66bf57cb0353509ef29baf3a189825208d2e206db9b1f1f6c6a2e5e96bb9d8",
            "public void Dispose()": "a760af8c5c1a0d87d871c1c03ef7f17c59ff6041be7b9fd3c412d70b4c5ed162",
        }
        for signature, digest in hashes.items():
            with self.subTest(signature=signature):
                actual = block(signature, strict)
                if signature == "public InitializedFixtureClipboardGuard(string generated)":
                    actual = actual.replace("    try{CaptureConstructorClose(false);}catch{}\n", "")
                    actual = actual.replace("    opened=false;CloseChecked();\n    try{CaptureConstructorClose(true);}catch{}\n    cleanup.Arm(last,helperEpoch);", "    opened=false;CloseChecked();cleanup.Arm(last,helperEpoch);")
                self.assertEqual(hashlib.sha256(actual.encode()).hexdigest(), digest)
        ordinary = " " + block("public sealed class ClipboardGuard:IDisposable")
        self.assertEqual(hashlib.sha256(ordinary.encode()).hexdigest(), ORDINARY_CLASS_SHA256)
        self.assertNotIn("Mismatch", ordinary)
        # Definition plus one invocation, solely in the failure catch checked above.
        self.assertEqual(CS.count("CaptureRequireOwnedMismatchLater("), 2)
        self.assertEqual(CS.count("new RequireOwnedMismatchReceipt"), 1)

    def test_capture_is_one_later_snapshot_without_open_content_mutation_input_or_adoption(self):
        capture = block("void CaptureRequireOwnedMismatchLater()", self.strict())
        claim = "if(System.Threading.Interlocked.CompareExchange(ref mismatchCaptureClaim,1,0)!=0)return;"
        self.assertIn(claim, capture)
        self.assertLess(capture.index(claim), capture.index("new RequireOwnedMismatchReceipt"))
        self.assertLess(capture.index("expected_sequence=last"), capture.index("GetClipboardSequenceNumber()"))
        self.assertEqual(capture.count("GetClipboardSequenceNumber()"), 1)
        self.assertEqual(capture.count("policyOps.TryOwner("), 1)
        self.assertEqual(capture.count("policyOps.ExactOwner("), 2)
        self.assertEqual(capture.count(".SameAlive("), 0)
        self.assertIn('receipt.observed_sequence=observed;receipt.observed_sequence_status=observed==0?"unavailable":"observed"', capture)
        for forbidden in ("OpenClipboard(", "CloseClipboard(", "EmptyClipboard(", "SetClipboardData(",
                          "GetClipboardData(", "EnumClipboardFormats(", "GlobalLock(", "GlobalSize(",
                          "GlobalAlloc(", "ReadBounded(", ".Open(", ".Close(", ".Empty(", ".Data(",
                          "mouse_event(", "keybd_event(", "SendInput(", "Invoke(", "Sleep(",
                          "while(", "for(", "foreach(", "cleanup.", "OwnedSequences.", "Sequence()"):
            self.assertNotIn(forbidden, capture)
        for name in ("last", "owner", "helperEpoch", "window", "uncertain", "changed", "finished", "Status"):
            self.assertNotRegex(capture, r"\b" + name + r"\s*=(?!=)")
        self.assertNotIn("throw", capture)
        self.assertNotIn("catch(Exception", capture)

    def test_owner_error_is_immediate_and_null_error_or_unknown_remain_distinct(self):
        capture = block("void CaptureRequireOwnedMismatchLater()", self.strict())
        self.assertIn("receipt.owner_query_attempted=true;bool ok=policyOps.TryOwner(out actual,out error);", capture)
        self.assertIn("receipt.owner_query_error=error;receipt.owner_query_succeeded=ok&&error==0;", capture)
        self.assertIn('if(receipt.owner_query_succeeded)receipt.checked_owner=actual==IntPtr.Zero?"null-error0":policyOps.ExactOwner(actual,helperEpoch)?"exact-helper":"other-or-not-proven";', capture)
        # Existing checked wrapper clears last error, reads it immediately, then returns.
        query = block("public bool TryOwner(", block("class NativeClipboardPolicyOps:"))
        self.assertIn("SetLastError(0);owner=InitializedClipboardOwner();error=Marshal.GetLastWin32Error();return error==0;", query)
        self.assertLess(capture.index("receipt.owner_query_error=error"), capture.index("policyOps.ExactOwner("))
        self.assertIn('if(helperEpoch!=null)receipt.helper_liveness=policyOps.ExactOwner(helperEpoch.Window,helperEpoch)?"exact-live-helper":"not-proven";', capture)
        same_alive = block("public bool SameAlive(", block("sealed class ClipboardOwnerWindow:"))
        for forbidden in ("IsWindow(", "Class(", "GetWindowThreadProcessId(", "GetClipboard", "EmptyClipboard("):
            self.assertNotIn(forbidden, same_alive)

    def test_receipt_export_is_opt_in_failure_only_and_does_not_reenter_guard(self):
        export = "try{if($clipboard -and $clipboard.MismatchEvidence){$report.initialized_guard_require_owned_mismatch=$clipboard.MismatchEvidence}}catch{}"
        self.assertEqual(HELPER.count(export), 1)
        failure = HELPER.split("}catch{\n $report.error=$_.Exception.Message", 1)[1]
        opt_in = failure.split(" if($InitializedEmptyClipboardFixture){", 1)[1].split("\n }", 1)[0]
        self.assertIn(export, opt_in)
        self.assertLess(failure.index(export), failure.index("$restored=$clipboard.Finish()"))
        self.assertNotIn("MismatchEvidence", HELPER.split("}catch{\n $report.error=$_.Exception.Message", 1)[0].split("\n'@", 1)[1])
        for forbidden in ("RequireOwned(", "CaptureRequireOwnedMismatchLater(", "Sequence(", "EmptyClipboard(", "GetClipboardData(", "SetClipboardData("):
            self.assertNotIn(forbidden, export)


class ConstructorCloseObservationContracts(unittest.TestCase):
    """Source contracts; managed synthetic checks run separately in Windows CI."""

    def test_snapshots_are_one_shot_around_only_original_successful_constructor_close(self):
        strict = block("public sealed class InitializedFixtureClipboardGuard:")
        constructor = block("public InitializedFixtureClipboardGuard(string generated)", strict)
        close = "try{CaptureConstructorClose(false);}catch{}\n    opened=false;CloseChecked();\n    try{CaptureConstructorClose(true);}catch{}\n    cleanup.Arm(last,helperEpoch);"
        self.assertIn(close, constructor)
        self.assertLess(constructor.index("last=Sequence();OwnedSequences.Add(last);Status=\"synthetic-payload-owned\""), constructor.index(close))
        capture = block("void CaptureConstructorClose(", strict)
        self.assertIn("CompareExchange(ref beforeCloseCaptureClaim,1,0)", capture)
        self.assertIn("CompareExchange(ref afterCloseCaptureClaim,1,0)", capture)
        self.assertIn("constructorCloseEvidence==null", capture)
        self.assertIn("new ConstructorCloseReceipt{expected_sequence=last}", capture)
        self.assertIn("constructorCloseEvidence.close_confirmed=true", capture)
        self.assertEqual(CS.count("CaptureConstructorClose(false)"), 1)
        self.assertEqual(CS.count("CaptureConstructorClose(true)"), 1)
        for name in ("last", "owner", "helperEpoch", "window", "uncertain", "changed", "finished", "Status"):
            self.assertNotRegex(capture, r"\b" + name + r"\s*=(?!=)")
        self.assertNotIn("cleanup.", capture)
        self.assertNotIn("OwnedSequences.", capture)

    def test_capture_has_only_bounded_read_only_operations_and_fixed_scalar_receipt(self):
        capture = block("static ClipboardCloseSnapshot CaptureClipboardCloseSnapshot(")
        self.assertEqual(capture.count("ops.Sequence()"), 1)
        self.assertEqual(capture.count("ops.TryOwner("), 1)
        self.assertEqual(capture.count("ops.ExactOwner("), 2)
        for forbidden in (".Open(", ".Close(", ".Empty(", ".Data(", "ReadBounded(", "FirstFormat(", "GetClipboardData(", "SetClipboardData(", "EnumClipboardFormats(", "GlobalLock(", "mouse_event(", "keybd_event(", "while(", "for(", "foreach(", "throw", "catch(Exception"):
            self.assertNotIn(forbidden, capture)
        receipt = block("public sealed class ClipboardCloseSnapshot")
        outer = block("public sealed class ConstructorCloseReceipt")
        for forbidden in ("IntPtr", "Exception", "Class", "Title", "Path", "Url", "object", "List<"):
            self.assertNotIn(forbidden, receipt + outer)
        self.assertIn("not-cause-or-epoch-authority", outer)
        self.assertIn('observed==0?"unavailable":"observed"', capture)
        self.assertIn("receipt.owner_query_error=error;receipt.owner_query_succeeded=ok&&error==0;", capture)

    def test_actual_synthetic_snapshot_policy_is_wired_without_native_operations(self):
        cases = block("public static void CheckInitializedClipboardPolicy()")
        for label in ("exact helper", "changed close sequence is observation only", "zero close snapshot stays unavailable", "null owner", "owner error", "query unknown", "foreign owner", "helper lost", "helper unknown"):
            self.assertIn(label, cases)
        case = block("static void CheckCloseSnapshotCase(")
        self.assertIn("CaptureClipboardCloseSnapshot(ops)", case)
        for name in ("OpenCalls", "CloseCalls", "EmptyCalls", "EnumCalls", "ContentCalls", "Reads", "InputCalls"):
            self.assertIn("ops." + name + "==0", case)
        self.assertIn("ops.SequenceCalls==1&&ops.OwnerCalls==1", case)
        self.assertIn("ops.CurrentSequence==before&&ops.CurrentOwner==ownerBefore", case)
        checker = (ROOT / "benchmarks/scripts/test-native-menu-helpers.ps1").read_text()
        self.assertIn("$null=$initializedPolicy.Invoke($null,@())", checker)

    def test_export_only_reads_retained_receipt_after_opt_in_failure(self):
        export = "try{if($clipboard -and $clipboard.CloseEvidence){$report.initialized_guard_constructor_close=$clipboard.CloseEvidence}}catch{}"
        self.assertEqual(HELPER.count(export), 1)
        failure = HELPER.split("}catch{\n $report.error=$_.Exception.Message", 1)[1]
        opt_in = failure.split(" if($InitializedEmptyClipboardFixture){", 1)[1].split("\n }", 1)[0]
        self.assertIn(export, opt_in)
        self.assertLess(failure.index(export), failure.index("$restored=$clipboard.Finish()"))
        self.assertNotIn("CloseEvidence", HELPER.split("}catch{\n $report.error=$_.Exception.Message", 1)[0].split("\n'@", 1)[1])
        for forbidden in ("CaptureConstructorClose(", "Sequence(", "EmptyClipboard(", "GetClipboardData(", "SetClipboardData("):
            self.assertNotIn(forbidden, export)


if __name__ == "__main__":
    unittest.main()
