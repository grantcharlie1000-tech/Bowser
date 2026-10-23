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
        self.assertIn('RequireKnownFormatsClaim(initializedFixtureContract,"Copy",initializedFixtureReady,ref initializedGuardClaim)', strict_constructor)
        gate = block("static void RequireKnownFormatsClaim(")
        self.assertIn('!contract||mode!="Copy"||!ready||System.Threading.Interlocked.CompareExchange(ref claim,1,0)!=0', gate)
        installer = block("static void InstallKnownFormatsOnce(")
        self.assertLess(installer.index("receipt.admitted_sequence=ops.Sequence()"), installer.index("ops.Empty()"))
        self.assertIn("OrdinaryAdmissionAllowsMutation(receipt.opened,first,enumError,receipt.admitted_sequence)", installer)

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
        self.assertIn("ops.ExactOwner(ops.Owner(),helper)", block("static void RequireFixtureOwnedOnce("))
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

    def test_actual_shared_require_owned_retains_short_circuit_fences_and_disarms(self):
        require = block("public void RequireOwned()", self.strict())
        self.assertIn("RequireFixtureOwnedOnce(policyOps,last,helperEpoch,cleanup,out closeConfirmed)", require)
        shared = block("static void RequireFixtureOwnedOnce(")
        self.assertIn("observed==0||observed!=expected||!ops.ExactOwner(ops.Owner(),helper)", shared)
        self.assertIn('throw new InvalidOperationException("Clipboard sequence/owner changed before native input; no input.")', shared)
        self.assertEqual(shared.count("ops.Open()"), 1)
        self.assertEqual(shared.count("ops.Close()"), 1)
        self.assertIn("finally{closeConfirmed=ops.Close();if(!closeConfirmed)", shared)
        self.assertIn("catch{cleanup.Disarm();throw;}", shared)
        self.assertIn("bool closeConfirmed=false;", require)
        self.assertIn("catch{uncertain=true;cleanup.Disarm();if(closeConfirmed){try{CaptureRequireOwnedMismatchLater();}catch{}}throw;}", require)
        self.assertEqual(require.count("CaptureRequireOwnedMismatchLater();"), 1)
        for forbidden in ("last=", "expected=", "OwnedSequences.", "cleanup.Arm", "SetFormat", "FirstFormat", "Data("):
            self.assertNotIn(forbidden, shared)

    def test_ordinary_copy_and_cleanup_are_frozen_while_opt_in_installer_is_shared(self):
        strict = self.strict()
        hashes = {
            "uint Sequence()": "8411bd80fc84398fd74e35981efe06492c017c1c271cbd7c9c9933fd45f71669",
            "void CloseChecked()": "18a7996e8591277ea8b9819e861f2a28e67174c67e6ce2ead9c44429c63f1f78",
            "public void ObserveNativeCopy(": "cf3a87f9d42ae9213a21e2ea2ee63f1bbcf2fcda3e48ed6aafa8e1f12e7d9515",
            "public bool Finish()": "9d66bf57cb0353509ef29baf3a189825208d2e206db9b1f1f6c6a2e5e96bb9d8",
            "public void Dispose()": "a760af8c5c1a0d87d871c1c03ef7f17c59ff6041be7b9fd3c412d70b4c5ed162",
        }
        for signature, digest in hashes.items():
            with self.subTest(signature=signature):
                actual = block(signature, strict)
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

    def test_snapshots_are_one_shot_around_shared_successful_constructor_close(self):
        strict = block("public sealed class InitializedFixtureClipboardGuard:")
        constructor = block("public InitializedFixtureClipboardGuard(string generated)", strict)
        installer = block("static void InstallKnownFormatsOnce(")
        self.assertIn("delegate(bool after){CaptureConstructorClose(after);}", constructor)
        self.assertLess(constructor.index("InstallKnownFormatsOnce("), constructor.index("cleanup.Arm(last,helperEpoch)"))
        self.assertIn('delegate(uint epoch){changed=true;last=epoch;OwnedSequences.Add(last);Status="synthetic-payload-owned";}', constructor)
        final = installer.index("receipt.final_pre_close_sequence=final;recordEpoch(final);")
        before = installer.index("try{closeSnapshot(false);}catch{}")
        close = installer.index("opened=false;receipt.close_attempted=true;receipt.close_confirmed=ops.Close();")
        after = installer.index("try{closeSnapshot(true);}catch{}")
        self.assertLess(final, before)
        self.assertLess(before, close)
        self.assertLess(close, after)
        capture = block("void CaptureConstructorClose(", strict)
        self.assertIn("CompareExchange(ref beforeCloseCaptureClaim,1,0)", capture)
        self.assertIn("CompareExchange(ref afterCloseCaptureClaim,1,0)", capture)
        self.assertIn("constructorCloseEvidence==null", capture)
        self.assertIn("new ConstructorCloseReceipt{expected_sequence=last}", capture)
        self.assertIn("constructorCloseEvidence.close_confirmed=true", capture)
        self.assertEqual(CS.count("CaptureConstructorClose(after)"), 1)
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

class KnownFormatsSourceContracts(unittest.TestCase):
    """Portable contracts for shared managed policy, not managed/native execution."""

    def test_pure_pre_initializer_validator_reuses_the_actual_bounded_payload_builder(self):
        validator = block("public static void ValidateInitializedCopyPayload(")
        self.assertEqual(validator, "public static void ValidateInitializedCopyPayload(string generated){BuildKnownFormatPayloads(generated);}")
        build = block("static KnownFormatPayload[] BuildKnownFormatPayloads(")
        cases = block("static void CheckInitializedCopyPayloadValidation()")
        self.assertIn('"sentinel-1-2\\n"', cases)
        self.assertIn('"sentinel-"+new string(\'1\',118)+"-2"', cases)
        self.assertIn("try{ValidateInitializedCopyPayload(invalid);}catch(InvalidOperationException){refused=true;}", cases)
        for forbidden in ("ClipboardPolicyOps", "KnownFormatsOps", "InitializeEmptyOnce", "new ClipboardOwnerWindow", "InitializedEmptyClipboardFixture", "OpenClipboard(", "EmptyClipboard(", "SetClipboardData(", "GetClipboardData(", "EnumClipboardFormats(", "GlobalAlloc(", "mouse_event("):
            self.assertNotIn(forbidden, validator + build + cases)
        entry = block("public static void CheckInitializedClipboardPolicy()")
        self.assertLess(entry.index("CheckInitializedCopyPayloadValidation();"), entry.index("CheckKnownFormatsPolicy();"))

    def test_opt_in_validates_before_initializer_and_ordinary_powershell_is_byte_frozen(self):
        # Source-order evidence, not native mutation coverage or executed PowerShell.
        ps = HELPER.split("\n'@", 1)[1]
        call = "[BowserHostMenu]::ValidateInitializedCopyPayload($payload);"
        constructor = "$initializer=[BowserHostMenu+InitializedEmptyClipboardFixture]::new();"
        self.assertEqual(ps.count(call), 1)
        self.assertIn("if($InitializedEmptyClipboardFixture){" + call + constructor, ps)
        self.assertLess(ps.index(call), ps.index(constructor))
        # Removing only the gated pure call recreates every byte of the prior PowerShell,
        # including the ordinary/Paste lane and both existing physical-input sites.
        self.assertEqual(hashlib.sha256(ps.replace(call, "").encode()).hexdigest(),
                         "3f82754dcfd37f36e4ab34964db9391d16bff13b1c43553eaa37af2409d0e901")

    def test_known_family_is_fixed_eager_ascii_copy_only_and_bounded(self):
        self.assertIn("KnownUnicodeText=13,KnownLocale=16,KnownAnsiText=1,KnownOemText=7,KnownMovableZeroInit=0x0042", CS)
        self.assertIn("const int KnownTokenMaxBytes=128", CS)
        build = block("static KnownFormatPayload[] BuildKnownFormatPayloads(")
        self.assertIn('generated.Length>KnownTokenMaxBytes', build)
        self.assertIn('@"\\Asentinel-[0-9]+-[0-9]+\\z"', build)
        self.assertNotIn("$", build)
        self.assertIn("unicode=new byte[(generated.Length+1)*2]", build)
        self.assertIn("ansi=new byte[generated.Length+1],oem=new byte[generated.Length+1]", build)
        self.assertIn("unicode[2*i]=(byte)c;ansi[i]=(byte)c;oem[i]=(byte)c", build)
        self.assertIn("Bytes=new byte[]{9,4,0,0}", build)
        self.assertNotIn("Encoding.", build)
        self.assertNotIn("RegisterClipboardFormat", CS)

    def test_actual_installer_validates_exact_payloads_before_allocating_or_opening(self):
        validate = block("static void ValidateKnownFormatPayloads(")
        self.assertIn("payloads.Length!=4", validate)
        self.assertIn("payloads[i].Format!=exact[i].Format", validate)
        self.assertIn("payloads[i].Bytes.Length!=exact[i].Bytes.Length", validate)
        self.assertIn("payloads[i].Bytes[j]!=exact[i].Bytes[j]", validate)
        install = block("static void InstallKnownFormatsOnce(")
        self.assertLess(install.index("ValidateKnownFormatPayloads(generated,payloads)"), install.index("ops.Allocate("))
        self.assertLess(install.index("ops.Allocate("), install.index("ops.Open()"))
        self.assertLess(install.index("ops.UnlockBuffer("), install.index("ops.Open()"))
        self.assertIn("ops.Allocate(KnownMovableZeroInit,payloads[i].Bytes.Length)", install)
        self.assertIn("buffer.Memory==buffers[j].Memory", install)
        self.assertIn("if(remainsLocked||error!=0)", install)
        native = block("sealed class NativeKnownFormatsOps:")
        unlock = block("public bool UnlockBuffer(", native)
        self.assertIn("SetLastError(0);bool remainsLocked=CheckedGlobalUnlock(memory);error=Marshal.GetLastWin32Error();return remainsLocked;", unlock)
        self.assertNotIn("||", unlock)

    def test_each_mutation_has_lexical_helper_fences_and_final_epoch_is_after_all_sets(self):
        install = block("static void InstallKnownFormatsOnce(")
        self.assertEqual(install.count("ops.Open()"), 1)
        self.assertEqual(install.count("ops.Empty()"), 1)
        self.assertEqual(install.count("ops.SetFormat("), 1)
        self.assertEqual(install.count("ops.FirstFormat("), 1)
        self.assertIn("receipt.stage=5;if(!KnownHelperLive(ops))", install)
        self.assertIn("RequireKnownHelperOwner(ops);\n   for(int i=0;i<4;i++){\n    RequireKnownHelperOwner(ops);", install)
        self.assertIn("RequireKnownHelperOwner(ops);uint final=ops.Sequence();if(final==0)", install)
        final = install.index("uint final=ops.Sequence()")
        after = install.index("RequireKnownHelperOwner(ops);", final)
        self.assertLess(final, after)
        self.assertLess(after, install.index("receipt.final_pre_close_sequence=final;recordEpoch(final)"))
        fence = block("static void RequireKnownHelperOwner(")
        self.assertIn("!KnownHelperLive(ops)||!ops.ExactOwner(ops.Owner(),ops.Helper)", fence)
        self.assertLess(install.index("ops.SetFormat("), install.index("uint final=ops.Sequence()"))
        for forbidden in ("GetClipboardData", "ReadBounded", "EnumClipboardFormats", "cleanup.", "Thread.Sleep", "RegisterClipboardFormat"):
            self.assertNotIn(forbidden, install)

    def test_transfer_precedes_any_receipt_bookkeeping_and_uncertain_handles_are_never_freed(self):
        install = block("static void InstallKnownFormatsOnce(")
        transfer = install.index("if(result!=IntPtr.Zero)buffer.Transferred=true;")
        receipt = install.index("receipt.transferred[i]=true;")
        self.assertLess(transfer, receipt)
        self.assertIn("catch{buffer.OwnershipUncertain=true;receipt.ownership_uncertain[i]=true;throw;}", install)
        self.assertIn("buffer.Transferred||buffer.OwnershipUncertain||buffer.FreeAttempted", install)
        self.assertIn("buffer.FreeAttempted=true;receipt.free_attempted[i]=true;", install)
        self.assertIn("if(ops.FreeBuffer(buffer.Memory)!=IntPtr.Zero)", install)
        self.assertIn("catch{receipt.free_failed[i]=true;freeFailure=true;}", install)
        self.assertLess(install.index("ops.FreeBuffer("), install.index("if(freeFailure)throw"))
        setter_tail = install[transfer:receipt]
        for forbidden in (".Add(", "new ", "ops."):
            self.assertNotIn(forbidden, setter_tail.split("else throw", 1)[0])

    def test_exactly_one_close_attempt_per_successful_open_and_only_pre_close_arms(self):
        install = block("static void InstallKnownFormatsOnce(")
        self.assertEqual(install.count("ops.Close()"), 2)  # exclusive success path or failure finally, never both
        close = "opened=false;receipt.close_attempted=true;receipt.close_confirmed=ops.Close();"
        self.assertEqual(install.count(close), 2)
        self.assertIn("if(opened){" + close, install)
        self.assertIn("if(!receipt.close_confirmed)throw", install)
        self.assertEqual(install.count("recordEpoch(final)"), 1)
        self.assertLess(install.index("recordEpoch(final)"), install.index("closeSnapshot(false)"))
        self.assertLess(install.index("ops.Close()"), install.index("closeSnapshot(true)"))
        constructor = block("public InitializedFixtureClipboardGuard(string generated)")
        self.assertEqual(constructor.count("cleanup.Arm(last,helperEpoch)"), 1)
        self.assertIn("uncertain=true;cleanup.Disarm();Finish();", constructor)
        self.assertNotIn("CloseClipboard", constructor)
        self.assertNotIn("GlobalFree", constructor)

    def test_actual_live_and_managed_matrix_call_the_same_installer_and_input_flow(self):
        constructor = block("public InitializedFixtureClipboardGuard(string generated)")
        managed = block("static bool TrySyntheticKnownInstall(")
        self.assertIn("InstallKnownFormatsOnce(knownOps,generated,payloads,fixtureEvidence", constructor)
        self.assertIn("InstallKnownFormatsOnce(ops,token,payloads,receipt", managed)
        require = block("public void RequireOwned()", block("public sealed class InitializedFixtureClipboardGuard:"))
        synthetic = block("static bool TrySyntheticInputSite(")
        self.assertIn("RequireFixtureOwnedOnce(policyOps,last,helperEpoch,cleanup,out closeConfirmed)", require)
        self.assertIn("RequireFixtureOwnedOnce(ops,last,ops.Helper,cleanup,out closeConfirmed);inputCalls++", synthetic)
        self.assertEqual(HELPER.count("if($clipboard){$clipboard.RequireOwned()}"), 2)
        self.assertIn("CheckKnownFormatsPolicy();", block("public static void CheckInitializedClipboardPolicy()"))
        self.assertIn("foreach(bool second in new bool[]{false,true})", block("static void CheckKnownFormatsPolicy()"))
        fake = block("sealed class SyntheticKnownFormatsOps:")
        self.assertIn("bool ClipboardPolicyOps.Open(){return Open();}", fake)
        self.assertIn("bool ClipboardPolicyOps.ExactOwner(IntPtr actual,ClipboardEpoch expected){return ExactOwner(actual,expected);}", fake)

    def test_matrix_covers_preparation_partial_transfer_and_each_live_input_negative(self):
        cases = block("static void CheckKnownFormatsPolicy()")
        for text in ("contract", "mode", "ready", "reused", "locale-length", "locale-value", "unicode-terminator", "unicode-encoding", "ansi-terminator", "oem-terminator", "format-order", "missing-format", "allocate", "lock", "fill", "unlock-error", "unlock-remains", "free-fails", "free-throws", "setter-throws", "helper-pid", "helper-thread", "helper-class", "helper-token"):
            self.assertIn('"' + text + '"', cases)
        self.assertIn('"sentinel-1-2\\n"', cases)
        self.assertIn('new string(\'1\',129)', cases)
        self.assertIn("for(int position=1;position<=4;position++)", cases)
        self.assertIn("FailIdentityAt=2*position+2", cases)
        self.assertIn("FailurePosition=position", cases)
        self.assertIn("new KnownFormatsReceipt{transferred=null}", cases)
        for text in ("known shared HGLOBAL refused", "known missing dedicated contract", "known clipboard locked", "known nonempty admission", "known enumeration error", "known zero independent admission", "known helper lost before Empty", "known helper lost after Empty", "known failed Empty", "known wrong owner after Empty", "known null owner after Empty", "known unknown owner after Empty", "known final zero", "known final helper lost", "known final owner query error", "known final foreign owner", "known final null owner", "known helper lost after final sequence", "known owner query error after final sequence", "known foreign owner after final sequence", "known null owner after final sequence", "known close unconfirmed", "known close throws", "known allocation padding remains zero", "known same epoch foreign owner", "known same epoch null owner", "known same epoch unknown owner", "known owner query uncertainty", "known owner query error", "known RequireOwned lock", "known RequireOwned close", "known RequireOwned close throws"):
            self.assertIn('"' + text + '"', cases)
        self.assertIn("uint[]{0,4,6,101}", cases)
        self.assertIn('"known pre3 post6 same helper actual RequireOwned refuses no adoption"', cases)
        self.assertIn("last==3&&inputs==0&&ops.ContentCalls==0", cases)
        self.assertIn("!cleanup.Finish(ops,out ignored)&&ops.OpenCalls==opens&&ops.EmptyCalls==1", cases)
        self.assertIn("ops.FreeCounts[i]<=1&&(!ops.Transferred[i]||ops.FreeCounts[i]==0)", block("static void AssertKnownInstallStopped("))
        self.assertNotIn("#pragma warning disable", CS)

    def test_existing_initializer_native_copy_and_cleanup_control_flow_are_byte_frozen(self):
        frozen = {
            'class NativeClipboardPolicyOps:': '83a1777485f79f94fe3bdfe796b0de639efc6d4d9be59265b33d4fd4fcd60b90',
            'sealed class NativeCopyPolicyOps:': '1918f6eb122b01db052df8045f022c2e795ec3ad0b568e94040c5bba3e573705',
            'static bool MatchingCopyCompletion(': '6d8e930577372eb0397cd5285e845bb2efd75a9e85c57a91f328f0c5cf915eb8',
            'static void ObserveNativeCopyOnce(': '14ee4fc3b00fcd4a007a13f1ff117dfa3ea46e2bb665923324a654d98788ed27',
            'static void ObserveCopyAndArmCleanup(': 'fa0076d2b127b4b10c4c1702d63a0c7cf78d9cbecb585369c3a888dad0a573b2',
            'static bool InitializeEmptyOnce(': 'ec4d2ba1aa086337082d6ee19656603749cf8b64f78dc37df87bb925a96b0ec3',
            'public sealed class InitializedEmptyClipboardFixture:': 'ad1d37d6ef25c23f8a410b8b5c0b8d458df4a673f9d13826335775207a94e55c',
            'sealed class ClipboardCleanupFence {': '91acb32ecc0d908e65ef7f504950a520416d4ca02aa4141562dc7a552b969f5a',
        }
        for signature, digest in frozen.items():
            with self.subTest(signature=signature):
                self.assertEqual(hashlib.sha256(block(signature).encode()).hexdigest(), digest)

    def test_receipt_is_generated_metadata_and_never_production_setter_authority(self):
        receipt = block("public sealed class KnownFormatsReceipt")
        for forbidden in ("IntPtr", "generated", "payload", "Owner", "Window", "Text"):
            self.assertNotIn(forbidden, receipt)
        self.assertIn('scope="copy-fixture-four-eager-known-formats-not-production-copy"', receipt)
        self.assertIn("new uint[]{13,16,1,7}", receipt)
        self.assertIn("proof.SetterCalls==1&&proof.SetterOk", block("static bool MatchingCopyCompletion("))
        self.assertNotIn("Known", block("static bool MatchingCopyCompletion("))
        self.assertIn("$report.initialized_fixture_known_formats=$clipboard.FixtureEvidence", HELPER)
        self.assertIn("::LastFixtureEvidence", HELPER)


if __name__ == "__main__":
    unittest.main()
