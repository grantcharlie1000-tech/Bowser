"""SYNTHETIC ONLY: fabricated inputs for analyzer tests, never Bowser evidence.

Running this file prints JSON to stdout. It opens no browser and takes no samples.
"""
from copy import deepcopy
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import acceptance


def make(ratio=.4, pairs=10, complete=False, purpose="acceptance"):
    keys = list(acceptance.inventory()) if complete else ["new_tab/first_use/ordinary"]
    builds = {}
    for arm, marker in (("baseline", "a"), ("candidate", "b")):
        builds[arm] = {"source_commit": acceptance.CATALOG["baseline_commit"] if arm == "baseline" else "b" * 40,
            "source_tree_sha256": marker * 64, "executable_sha256": marker * 64,
            "assets_sha256": marker * 64, "lockfile_sha256": "c" * 64,
            "features": ["html-chrome-default"], "flags": "synthetic matched release flags",
            "mode": "release_html_chrome", "runtime": "synthetic-runtime-1",
            "instrumentation": "synthetic instrumentation", "toolchain": "synthetic-toolchain",
            "manifest_reference": "SYNTHETIC/build-" + arm}
    plan = {"catalog_version": acceptance.CATALOG["version"], "declared_at": "2026-11-15T00:00:00Z",
            "purpose": purpose, "decision_rule": "median_ratio", "conditions": [],
            "protocol_reference": "SYNTHETIC/not-a-real-sampling-plan"}
    data = {"schema_version": 1, "synthetic": True, "plan": plan, "builds": builds,
            "plan_commitment": {"sha256": "0" * 64, "reference": "SYNTHETIC/plan",
                                "reviewer": "synthetic reviewer", "recorded_at": "2026-11-14T23:59:00Z"},
            "attempt_ledger": {"reference": "SYNTHETIC/ledger", "sha256": "0" * 64,
                               "reviewer": "synthetic reviewer", "complete": True, "attempt_count": 0},
            "attempts": [], "reviews": {"parity": {}, "resources": {}, "diagnostics": {}}}
    clock = 1000
    for index, workload in enumerate(keys):
        area, variant, _ = workload.split("/")
        matching = {"runtime": "synthetic-runtime-1", "machine": "synthetic-machine",
            "os": "synthetic-os", "fixture_sha256": "d" * 64, "profile_seed_sha256": "e" * 64,
            "profile_state": variant if area == "startup" else "warm_browser",
            "os_cache": "unknown", "browser_cache": "synthetic-browser-cache",
            "extensions": "synthetic-defaults", "protections": "synthetic-normal",
            "power": "synthetic-ac", "dpi": "synthetic-96", "viewport": "synthetic-1280x720",
            "window_state": "synthetic-visible-foreground", "load_policy": "synthetic-policy",
            "seed": "synthetic-seed", "ordinary_release": True, "isolation": "isolated_app_data"}
        condition = {"id": f"condition-{index}", "workload": workload, "pairs": pairs,
                     "first_arm": "baseline", "matching": matching}
        plan["conditions"].append(condition)
        for pair in range(1, pairs + 1):
            order = acceptance.ARMS if pair % 2 else tuple(reversed(acceptance.ARMS))
            for arm in order:
                seq = len(data["attempts"]) + 1
                duration = 100 if arm == "baseline" else 100 * ratio
                identity = {"executable_sha256": builds[arm]["executable_sha256"],
                    "process_creation_identity": f"synthetic-process-{seq}", "window": f"synthetic-window-{seq}",
                    "controller": f"synthetic-controller-{seq}", "document": f"synthetic-document-{seq}",
                    "task_nonce": f"synthetic-task-{seq}"}
                evidence = []
                for offset, kind in enumerate(("task_start", "native_presentation", "native_interaction", "work")):
                    evidence.append({"nonce": f"synthetic-proof-{seq}-{offset}", "kind": kind,
                        "reference": f"SYNTHETIC/missing-native-proof-{seq}-{offset}", "sha256": "f" * 64,
                        "identity": deepcopy(identity), "clock_id": "synthetic-monotonic-clock",
                        "observed_ms": clock if kind == "task_start" else clock + duration})
                data["attempts"].append({"sequence": seq, "condition": condition["id"], "pair": pair,
                    "arm": arm, "acquired_at": (datetime(2026, 11, 15, tzinfo=timezone.utc) + timedelta(seconds=seq)).isoformat(),
                    "action_nonce": f"synthetic-action-{seq}", "identity": identity, "matching": deepcopy(matching),
                    "load": {"policy_pass": True, "competing_owned_work": False, "cpu_load": 0,
                             "available_ram_bytes": 1, "reference": "SYNTHETIC/load-no-real-host-read"},
                    "status": "success", "reason": "",
                    "timing": {"unit": "ms", "clock_id": "synthetic-monotonic-clock", "start": clock,
                               "end": clock + duration, "start_event": acceptance.inventory()[workload]["trigger"],
                               "end_event": "presented_and_usable"},
                    "evidence": evidence, "parent_review": {"state": "pass", "reviewer": "synthetic reviewer",
                        "reference": "SYNTHETIC/no-real-review", "checks": sorted(acceptance.CHECKS)}})
                clock += 1000
    seal(data)
    return data


def seal(data):
    """Recompute fixture hashes after deliberate test edits. Never collects evidence."""
    data["plan_commitment"]["sha256"] = acceptance.canonical_hash(data["plan"])
    data["attempt_ledger"]["attempt_count"] = len(data["attempts"])
    # The real ledger is externally retained; this synthetic hash only labels fixture content.
    data["attempt_ledger"]["sha256"] = acceptance.canonical_hash(data["attempts"])


def review(data, state="pass"):
    """SYNTHETIC review, explicitly bound to the fabricated bundle."""
    return {"state": state, "reason": "SYNTHETIC no real resource or parity evidence",
        "reviewer": "synthetic reviewer", "reference": "SYNTHETIC/review",
        "evidence": [{"reference": "SYNTHETIC/missing-real-evidence", "sha256": "f" * 64,
                      "reviewer": "synthetic reviewer", "recorded_at": "2026-11-15T01:00:00Z"}],
        "baseline_source": data["builds"]["baseline"]["source_commit"],
        "candidate_source": data["builds"]["candidate"]["source_commit"],
        "baseline_source_tree_sha256": data["builds"]["baseline"]["source_tree_sha256"],
        "candidate_source_tree_sha256": data["builds"]["candidate"]["source_tree_sha256"],
        "baseline_executable_sha256": data["builds"]["baseline"]["executable_sha256"],
        "candidate_executable_sha256": data["builds"]["candidate"]["executable_sha256"],
        "plan_sha256": data["plan_commitment"]["sha256"], "ledger_sha256": data["attempt_ledger"]["sha256"],
        "coverage": [c["id"] for c in data["plan"]["conditions"]]}


if __name__ == "__main__":
    print(json.dumps(make(), indent=2))
