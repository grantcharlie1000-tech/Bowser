#!/usr/bin/env python3
"""Source-only integration boundaries, not native clipboard/VM acceptance."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class InitializedCopyLaneSource(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.live = (ROOT / "src/engine/app_host_menu_probe/live.rs").read_text()
        cls.fixture = (ROOT / "src/engine/app_host_menu_probe/live/initialized_clipboard.rs").read_text()
        cls.workflow = (ROOT / ".github/workflows/store-probe.yml").read_text()
        cls.job = cls.workflow.split("\n  initialized-empty-copy:", 1)[1].split("\n  native-store-journey:", 1)[0]

    def test_ordinary_entry_does_not_opt_in_or_retry(self):
        ordinary = self.live.split("fn native_input(", 1)[1].split("fn native_input_in_fixture(", 1)[0]
        self.assertIn("native_input_in_fixture(app,h,tab,proof,mode,control,token,out,false)", ordinary)
        shared = self.live.split("fn native_input_in_fixture(", 1)[1].split("fn snapshot(", 1)[0]
        self.assertIn('if initialized_empty {assert_eq!(mode,"Copy");initialized_clipboard::contract();}', shared)
        self.assertEqual(shared.count('command.arg("-InitializedEmptyClipboardFixture")'), 1)
        self.assertIn('if initialized_empty {command.arg("-InitializedEmptyClipboardFixture");}', shared)
        self.assertEqual(shared.count("command.spawn()"), 1)
        self.assertIn("assert!(output.status.success()", shared)
        for gate in ("current(h,proof)", "complete_calls,1", "complete_error,None", "setter_result,Some(Ok(()))", "fixture-trusted-count", 'text(&report,"clipboard"),"restored-empty"'):
            self.assertIn(gate, shared)
        self.assertIn("if initialized_empty {initialized_clipboard::assert_initialized_evidence(&report);}", shared)

    def test_separate_exact_ignored_copy_has_no_fallback_or_clipboard_api(self):
        self.assertIn("#[ignore =", self.fixture)
        self.assertIn('vec![INITIALIZED_TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"]', self.fixture)
        sequence = self.fixture.split("fn copy_sequence(", 1)[1].split("#[test]", 1)[0]
        self.assertEqual(sequence.count("native_input_in_fixture("), 1)
        self.assertIn('"Copy", "copy", &server.nonce, &out.join("copy"), true', sequence)
        for gate in ("current(&source, proof)", "current(&negative, negative_proof)", "assert_ne!(source.view.as_raw(), negative.view.as_raw())", "snapshot(&negative.view), negative_before", "storage(root), persistent", "effects(server, 0, 0, 0)"):
            self.assertIn(gate, sequence)
        for forbidden in ("EmptyClipboard", "SetClipboardData", "GetClipboardData", "navigator.clipboard", "SetProcessWindowStation", "AdjustTokenPrivileges", "native_input("):
            self.assertNotIn(forbidden, self.fixture)
        self.assertIn("Duration::from_secs(60)", sequence)
        self.assertIn("Duration::from_secs(30)", sequence)
        self.assertIn("Duration::from_secs(240)", self.fixture)

    def test_provenance_is_exact_bounded_and_not_atomic_custody(self):
        for key in ("GITHUB_ACTIONS", "RUNNER_ENVIRONMENT", "RUNNER_OS", "GITHUB_REPOSITORY", "GITHUB_JOB", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_SHA", "ImageVersion", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_CONTRACT", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_JOB", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_CHECKOUT", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_RUN", "BOWSER_INITIALIZED_EMPTY_CLIPBOARD_ATTEMPT"):
            self.assertIn('"' + key + '"', self.fixture)
        for bound in ("values.len() != KEYS.len()", "checkout.len() != 40", "image.len() > 80", "attempt.len() > 10", "value.len() <= 20", "v > 0"):
            self.assertIn(bound, self.fixture)
        self.assertIn('"metadata_alone_is_not_exclusivity_proof":true', self.fixture)
        self.assertIn('"continuous_handoff_custody_claimed":false', self.fixture)
        self.assertIn('"uninitialized_zero_lane":"separate-unchanged-not-this-result"', self.fixture)
        self.assertIn("fn synthetic_initialized_contract_requires_each_exact_binding()", self.fixture)
        self.assertIn("fn synthetic_initialized_contract_is_bounded_and_not_a_custody_claim()", self.fixture)

    def test_genuine_result_requires_every_explicit_initializer_and_cleanup_field(self):
        proof = self.fixture.split("pub(super) fn assert_initialized_evidence(", 1)[1].split("fn copy_sequence(", 1)[0]
        for key in ("initialized_empty_fixture", "initialization_attempted", "initialization_succeeded", "empty_attempts", "after_sequence", "after_owner", "after_format_empty", "after_enum_error", "close_confirmed", "owner_close_attempted", "owner_destroy_confirmed", "initializer_cleanup_authority", "initializer_cleanup_after_handoff", "atomic_handoff", "continuous_custody", "initialized_clipboard_cleanup", "native_copy_arm_succeeded", "armed_before_finish", "exact_owner_verified_twice", "attempts", "success", "final_sequence", "post_owner_is_helper", "post_empty_verified", "latest_epoch_kind"):
            self.assertIn('"' + key + '"', proof)
        self.assertIn('number(init, "empty_attempts"), 1', proof)
        self.assertIn('number(cleanup, "attempts"), 1', proof)
        self.assertIn('number(init, "after_sequence") > 0', proof)
        self.assertIn('number(cleanup, "final_sequence") > 0', proof)
        self.assertIn('assert_bool(init, key, false)', proof)
        self.assertIn('text(cleanup, "latest_epoch_kind"), "native-copy"', proof)
        for key in ("helper_verified_before_open", "helper_verified_at_final_fence"):
            self.assertIn('"' + key + '"', proof)
        for key in ("before_owner_query_attempted", "before_owner_query_succeeded", "after_owner_query_attempted", "after_owner_query_succeeded", "before_owner_query_error", "after_owner_query_error"):
            self.assertIn('"' + key + '"', proof)
        self.assertIn('for key in ["before_owner_query_error", "after_owner_query_error"] { assert_eq!(number(init, key), 0); }', proof)

    def test_dedicated_new_job_has_no_fallback_condition_or_other_clipboard_workload(self):
        self.assertIn("runs-on: windows-latest", self.job)
        self.assertIn("timeout-minutes: 20", self.job)
        self.assertIn("persist-credentials: false", self.job)
        for forbidden in ("self-hosted", "needs:", "if: failure", "actions/cache", "netlify", "workflow_dispatch", "release-v", "native-safety.outcome", "live_probe_real_app_host_context_menu --"):
            self.assertNotIn(forbidden, self.job)
        for name in ("CONTRACT", "JOB", "CHECKOUT", "RUN", "ATTEMPT"):
            self.assertIn("BOWSER_INITIALIZED_EMPTY_CLIPBOARD_" + name + ":", self.job)
        self.assertIn("'bowser-initialized-empty-copy-localappdata'", self.job)
        self.assertIn("${{ github.sha }}", self.job)
        self.assertIn("if: always()", self.job)
        self.assertIn("initialized-empty-copy-diagnostics-", self.job)

    def test_pure_and_one_exact_native_case_are_wired_using_one_variant(self):
        self.assertIn("html-chrome-default,experimental-engine-context-menu --no-run", self.job)
        self.assertGreaterEqual(self.job.count("$test.Count -ne 1"), 2)
        self.assertIn(".Count -ne 2", self.job)
        self.assertIn(".Count -ne 1", self.job)
        self.assertIn("$live --exact --ignored --nocapture --test-threads=1", self.job)
        self.assertIn("$filter --nocapture --test-threads=1", self.job)
        self.assertIn("powershell.exe -NoProfile -File ./benchmarks/scripts/test-native-menu-helpers.ps1", self.job)
        for path in ("benchmarks/test-initialized-copy-lane-source.py", "benchmarks/test-initialized-clipboard-helper-source.py"):
            self.assertIn("      - '" + path + "'", self.workflow)
            self.assertIn("python ./" + path, self.job)
        checker = (ROOT / "benchmarks/scripts/test-native-menu-helpers.ps1").read_text()
        self.assertIn("GetMethod('CheckInitializedClipboardPolicy'", checker)
        self.assertIn("$initializedPolicy.Invoke($null,@())", checker)


if __name__ == "__main__":
    unittest.main()
