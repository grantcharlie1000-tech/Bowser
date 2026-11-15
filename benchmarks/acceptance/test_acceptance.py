"""Only synthetic standard-library behavior tests. No Bowser execution or timings."""
from copy import deepcopy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import acceptance
from fixtures.synthetic import make, seal, review


class AcceptanceTests(unittest.TestCase):
    def condition(self, data):
        report = acceptance.analyze(data)
        self.assertEqual(report["errors"], [])
        return report["conditions"][0]

    def test_known_point_ratios_absolute_times_tails_and_interval(self):
        for ratio, status in ((.4, "supported_measured_acceptance"), (.6, "measured_target_missed")):
            with self.subTest(ratio=ratio):
                result = self.condition(make(ratio))
                self.assertEqual(result["status"], status)
                self.assertEqual(result["baseline_ms"]["median"], 100)
                self.assertEqual(result["candidate_ms"]["p95_nearest_rank"], ratio * 100)
                self.assertAlmostEqual(result["paired_ratio"]["median"], ratio)
                self.assertEqual(result["paired_delta_ms"]["median"], ratio * 100 - 100)
                self.assertAlmostEqual(result["median_ratio_95_interval"]["lower"], ratio)
                self.assertAlmostEqual(result["median_ratio_95_interval"]["upper"], ratio)
                self.assertAlmostEqual(result["median_ratio_95_interval"]["coverage_at_least"], .978515625)

    def test_missing_workloads_are_visible_even_when_one_condition_meets_target(self):
        result = acceptance.analyze(make())
        self.assertEqual(result["timing_status"], "insufficient")
        for key in ("startup/fresh_profile/ordinary", "search/remote_suggestions/ordinary",
                    "reply_streaming/chunk_to_visible/chatgpt.com", "download/durable_completion/ordinary"):
            self.assertIn(key, result["missing_workloads"])
            self.assertTrue(any(c["workload"] == key and c["status"] == "insufficient" for c in result["conditions"]))

    def test_complete_inventory_still_synthetic_and_resources_outstanding(self):
        report = acceptance.analyze(make(complete=True))
        self.assertEqual(report["errors"], [])
        self.assertEqual(report["timing_status"], "supported_measured_acceptance")
        self.assertEqual(report["missing_workloads"], [])
        self.assertEqual(report["missing_profile_states"], [])
        self.assertEqual(report["global_status"], "synthetic_not_performance_proof")
        self.assertEqual(report["global_acceptance"], "outstanding_parent_decision")
        self.assertEqual(report["reviews"]["resources"]["ram"]["state"], "outstanding")

    def test_completely_absent_resource_and_parity_reviews_do_not_erase_timing(self):
        data = make(complete=True)
        del data["reviews"]
        report = acceptance.analyze(data)
        self.assertEqual(report["errors"], [])
        self.assertEqual(report["timing_status"], "supported_measured_acceptance")
        for category in ("resources", "parity"):
            self.assertTrue(all(g["state"] == "outstanding" for g in report["reviews"][category].values()))

    def test_five_exploratory_pairs_never_acceptance(self):
        self.assertEqual(self.condition(make(pairs=5, purpose="exploratory"))["status"], "exploratory")
        self.assertEqual(self.condition(make(pairs=5))["status"], "insufficient")
        self.assertEqual(self.condition(make(pairs=4, purpose="exploratory"))["status"], "insufficient")

    def test_missing_attempt_is_retained_as_incomplete_plan(self):
        data = make()
        data["attempts"].pop()
        seal(data)
        result = self.condition(data)
        self.assertEqual(result["status"], "insufficient")
        self.assertEqual(result["valid_pairs"], 9)
        self.assertEqual(result["attempt_count"], 19)

    def test_failure_timeout_and_cancellation_block_despite_ten_good_pairs(self):
        for status in ("failure", "timeout", "cancelled"):
            data = make(pairs=11)
            data["attempts"][0].update(status=status, reason="synthetic retained failure", timing=None, evidence=[])
            result = self.condition(data)
            self.assertEqual(result["valid_pairs"], 10)
            self.assertEqual(result["status"], "invalid")
            self.assertEqual(result["status_counts"][status], 1)
            self.assertAlmostEqual(result["unsuccessful_rate"], 1 / 22)
            self.assertEqual(len(acceptance.analyze(data)["attempts"]), 22)

    def test_no_completion_from_proxy_or_missing_native_capture(self):
        for kinds in (("proxy",), ("task_start", "work", "native_interaction")):
            data = make()
            data["attempts"][0]["evidence"] = [e for e in data["attempts"][0]["evidence"] if e["kind"] in kinds]
            result = acceptance.analyze(data)
            self.assertEqual(result["conditions"][0]["status"], "invalid")
            self.assertIn("native_presentation", result["native_proof"]["gaps"][0]["missing_required_kinds"])

    def test_wrong_executable_runtime_condition_and_document(self):
        cases = (
            lambda a: a["identity"].update(executable_sha256="9" * 64),
            lambda a: a["matching"].update(runtime="other-runtime"),
            lambda a: a["matching"].update(power="different-power"),
            lambda a: a["evidence"][1]["identity"].update(document="stale-document"),
            lambda a: a["evidence"][1]["identity"].update(window="wrong-window"),
            lambda a: a["parent_review"].update(state="outstanding"),
            lambda a: a["load"].update(competing_owned_work=True),
        )
        for change in cases:
            data = make()
            change(data["attempts"][0])
            self.assertEqual(self.condition(data)["status"], "invalid")

    def test_bad_units_negative_nonfinite_and_nonmonotonic(self):
        cases = ({"unit": "s"}, {"start": -1}, {"end": float("nan")},
                 {"end": float("inf")}, {"end": 999}, {"start": True})
        for update in cases:
            data = make()
            data["attempts"][0]["timing"].update(update)
            report = acceptance.analyze(data)
            self.assertEqual(report["timing_status"], "invalid")
            json.dumps(report, allow_nan=False)

    def test_zero_baseline_and_cross_attempt_clock_errors_are_not_acceptance(self):
        data = make()
        first = data["attempts"][0]
        first["timing"]["end"] = first["timing"]["start"]
        for evidence in first["evidence"]:
            evidence["observed_ms"] = first["timing"]["start"]
        self.assertEqual(self.condition(data)["status"], "invalid")
        for field, value in (("clock_id", "other-clock"), ("start", 1000)):
            data = make()
            data["attempts"][1]["timing"][field] = value
            self.assertEqual(acceptance.analyze(data)["timing_status"], "invalid")

    def test_equal_half_boundary_and_missing_workload_cannot_hide_a_miss(self):
        self.assertEqual(self.condition(make(.5))["status"], "supported_measured_acceptance")
        report = acceptance.analyze(make(.6))
        self.assertEqual(report["timing_status"], "measured_target_missed")
        self.assertTrue(report["missing_workloads"])

    def test_json_schema_unknown_fields_are_rejected(self):
        data = make()
        data["attempts"][0]["timing"]["dom_ready_ms"] = 1
        report = acceptance.analyze(data)
        self.assertEqual(report["timing_status"], "invalid")
        self.assertEqual(len(report["attempts"]), 20)

    def test_duplicate_action_task_or_evidence_nonce_invalidates_both_attempts(self):
        for field in ("action", "task", "evidence"):
            data = make()
            a, b = data["attempts"][:2]
            if field == "action":
                b["action_nonce"] = a["action_nonce"]
            elif field == "task":
                b["identity"]["task_nonce"] = a["identity"]["task_nonce"]
            else:
                b["evidence"][0]["nonce"] = a["evidence"][0]["nonce"]
            report = acceptance.analyze(data)
            self.assertFalse(report["attempts"][0]["valid"])
            self.assertFalse(report["attempts"][1]["valid"])

    def test_non_alternating_pairs_duplicate_pair_and_wrong_sequence_rejected(self):
        data = make()
        a, b = data["attempts"][2:4]
        # Keep every attempt valid while violating the declared AB / BA schedule.
        data["attempts"][2:4] = [b, a]
        for seq, row in enumerate(data["attempts"], 1):
            shift = seq * 1000 - row["timing"]["start"]
            row["sequence"] = seq
            row["acquired_at"] = f"2026-11-15T00:00:{seq:02d}Z"
            for key in ("start", "end"):
                row["timing"][key] += shift
            for evidence in row["evidence"]:
                evidence["observed_ms"] += shift
        result = acceptance.analyze(data)
        self.assertTrue(all(a["valid"] for a in result["attempts"]))
        self.assertFalse(result["conditions"][0]["order_valid_prefix"])
        self.assertEqual(result["conditions"][0]["status"], "invalid")
        data = make()
        data["attempts"][2]["pair"] = 1
        self.assertEqual(self.condition(data)["status"], "invalid")
        data = make()
        data["attempts"][1]["sequence"] = 1
        self.assertEqual(acceptance.analyze(data)["timing_status"], "invalid")

    def test_plan_commitment_time_and_ledger_cannot_silently_change(self):
        cases = (
            lambda d: d["plan"]["conditions"][0].update(pairs=11),
            lambda d: d["plan_commitment"].update(recorded_at="2026-11-16T00:00:00Z"),
            lambda d: d["attempts"][0].update(acquired_at="2026-11-14T00:00:00Z"),
            lambda d: d["attempt_ledger"].update(complete=False),
            lambda d: d["attempt_ledger"].update(attempt_count=19),
        )
        for change in cases:
            data = make()
            change(data)
            self.assertEqual(acceptance.analyze(data)["timing_status"], "invalid")

    def test_build_features_flags_and_runtime_must_match(self):
        for field, value in (("features", ["different"]), ("runtime", "different"), ("flags", "different")):
            data = make()
            data["builds"]["candidate"][field] = value
            self.assertEqual(acceptance.analyze(data)["timing_status"], "invalid")

    def test_declared_confidence_rule_is_visible_and_does_not_replace_point_rule(self):
        data = make()
        # Five ratios .4 and five .8: median .6 => point target missed.
        for row in data["attempts"]:
            if row["arm"] == "candidate" and row["pair"] > 5:
                row["timing"]["end"] = row["timing"]["start"] + 80
                for e in row["evidence"]:
                    if e["kind"] != "task_start":
                        e["observed_ms"] = row["timing"]["end"]
        self.assertEqual(self.condition(data)["status"], "measured_target_missed")
        # Six .4 and four .8: point meets target but interval crosses it.
        for row in data["attempts"]:
            if row["arm"] == "candidate" and row["pair"] == 6:
                row["timing"]["end"] = row["timing"]["start"] + 40
                for e in row["evidence"]:
                    if e["kind"] != "task_start":
                        e["observed_ms"] = row["timing"]["end"]
        result = self.condition(data)
        self.assertEqual(result["status"], "supported_measured_acceptance")
        self.assertFalse(result["upper_95_bound_at_target"])
        data["plan"]["decision_rule"] = "upper_95_bound"
        seal(data)
        self.assertEqual(self.condition(data)["status"], "insufficient")

    def test_resource_reviews_separate_from_timing_and_bound_to_final_sources(self):
        data = make()
        supplied_review = review(data)
        supplied_review["evidence"] = []
        data["reviews"]["resources"]["ram"] = supplied_review
        report = acceptance.analyze(data)
        self.assertEqual(report["conditions"][0]["status"], "supported_measured_acceptance")
        self.assertEqual(report["reviews"]["resources"]["ram"]["state"], "invalid")
        self.assertTrue(any("lacks evidence" in e for e in report["review_errors"]))
        supplied_review["state"] = "outstanding"
        self.assertEqual(self.condition(data)["status"], "supported_measured_acceptance")

    def test_global_support_needs_all_gates_and_exact_review_provenance(self):
        data = make(complete=True)
        data["synthetic"] = False  # Still fabricated; exercise the supplied-evidence decision only.
        self.assertEqual(acceptance.analyze(data)["global_status"], "acceptance_outstanding")
        for category, names in acceptance.CATALOG["reviews"].items():
            data["reviews"][category] = {name: review(data) for name in names}
        result = acceptance.analyze(data)
        self.assertEqual(result["global_status"], "supplied_evidence_supports_parent_review")
        self.assertEqual(result["global_acceptance"], "outstanding_parent_decision")
        for field, value in (("candidate_source", "0" * 40), ("candidate_executable_sha256", "0" * 64),
                             ("plan_sha256", "0" * 64), ("coverage", []), ("state", "fail")):
            changed = deepcopy(data)
            changed["reviews"]["resources"]["ram"][field] = value
            result = acceptance.analyze(changed)
            self.assertIn(result["global_status"], ("acceptance_outstanding", "invalid"))
            if field != "coverage":  # Empty coverage is structurally invalid.
                self.assertEqual(result["timing_status"], "supported_measured_acceptance")

    def test_review_cannot_survive_a_dirty_source_tree_change_at_the_same_commit(self):
        for arm in acceptance.ARMS:
            with self.subTest(arm=arm):
                data = make()
                data["reviews"]["resources"]["ram"] = review(data)
                data["builds"][arm]["source_tree_sha256"] = "0" * 64
                report = acceptance.analyze(data)
                self.assertEqual(report["reviews"]["resources"]["ram"]["state"], "invalid")
                self.assertEqual(report["conditions"][0]["status"], "supported_measured_acceptance")
                self.assertTrue(any("source tree" in e for e in report["review_errors"]))

    def test_full_new_tab_search_cannot_start_at_enter_or_ready_callback(self):
        data = make()
        condition = data["plan"]["conditions"][0]
        condition["workload"] = "search/full_new_tab_to_results/ordinary"
        seal(data)
        for attempt in data["attempts"]:
            attempt["timing"]["start_event"] = "physical_enter"
        report = acceptance.analyze(data)
        self.assertEqual(report["conditions"][0]["status"], "invalid")
        for attempt in data["attempts"]:
            attempt["timing"]["start_event"] = "physical_ctrl_t_or_button"
        self.assertEqual(self.condition(data)["status"], "supported_measured_acceptance")

    def test_legacy_logs_remain_insufficient_without_upgrading_proxy_fields(self):
        legacy = {"id": "legacy", "valid": True, "launch_qpc": 100, "ready_qpc": 200}
        report = acceptance.analyze(legacy)
        self.assertEqual(report["timing_status"], "insufficient")
        self.assertTrue(report["legacy_input"])
        self.assertIsNone(report["synthetic"])
        self.assertEqual(report["supplied_input"], legacy)
        self.assertIn("reply_streaming/chunk_to_visible/chatgpt.com", report["missing_workloads"])

    def test_cli_renders_self_contained_truthful_escaped_report(self):
        with tempfile.TemporaryDirectory(prefix="bowser-acceptance-test-") as directory:
            base = Path(directory)
            data = make()
            data["attempts"][0]["evidence"][1]["reference"] = "SYNTHETIC/<script>alert(1)</script>"
            source, output, page = (base / name for name in ("input.json", "report.json", "report.html"))
            source.write_text(json.dumps(data), encoding="utf-8")
            command = [sys.executable, "-B", str(acceptance.ROOT / "acceptance.py"), str(source),
                       "--json", str(output), "--html", str(page)]
            run = subprocess.run(command, capture_output=True, text=True, check=False)
            self.assertEqual(run.returncode, 1, run.stderr)
            report = json.loads(output.read_text(encoding="utf-8"))
            rendered = page.read_text(encoding="utf-8")
            self.assertEqual(report["input_sha256"], __import__("hashlib").sha256(source.read_bytes()).hexdigest())
            self.assertIn("SYNTHETIC FIXTURE", rendered)
            self.assertIn("missing-native-proof", rendered)
            self.assertIn("does not prove", rendered)
            self.assertIn("&lt;script&gt;", rendered)
            self.assertNotIn("<script>", rendered)
            self.assertNotIn("<script src", rendered)
            self.assertEqual(len(report["attempts"]), 20)
            malformed = base / "bad.json"
            malformed.write_text('{"schema_version":1,"schema_version":1}', encoding="utf-8")
            run = subprocess.run(command[:3] + [str(malformed)], capture_output=True, text=True, check=False)
            self.assertEqual(run.returncode, 2)
            self.assertIn("duplicate JSON key", run.stderr)
            before = source.read_bytes()
            run = subprocess.run(command[:4] + ["--json", str(source)], capture_output=True, text=True, check=False)
            self.assertEqual(run.returncode, 2)
            self.assertEqual(source.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
