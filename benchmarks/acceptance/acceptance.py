"""Validate supplied task evidence; never measures Bowser or verifies pixels itself."""
from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import html
import json
import math
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent
CATALOG = json.loads((ROOT / "catalog.json").read_text(encoding="utf-8"))
SCHEMA = json.loads((ROOT / "schema.json").read_text(encoding="utf-8"))
ARMS = ("baseline", "candidate")
PROOF_KINDS = {"task_start", "native_presentation", "native_interaction", "work"}
CHECKS = {"foreground_executable_window", "current_controller_document_nonce",
          "correct_content", "actual_work_in_timed_region", "usable_interaction",
          "build_source_assets_runtime", "full_task_start_endpoint"}
LIMITATION = ("Checks supplied evidence and parent review attestations only. A reference or "
              "hash does not prove that this analyzer observed pixels, sound, or usable input. "
              "Native presentation proof, ledger completeness, plan predeclaration, resource "
              "coverage and final council approval require independent parent review.")


def canonical_hash(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                    allow_nan=False).encode("utf-8")).hexdigest()


def schema_errors(value, spec=None, path="$"):
    """Small validator for exactly the JSON Schema keywords used by schema.json."""
    import re
    spec = SCHEMA if spec is None else spec
    if "$ref" in spec:
        spec = SCHEMA["$defs"][spec["$ref"].split("/")[-1]]
    if "anyOf" in spec:
        alternatives = [schema_errors(value, option, path) for option in spec["anyOf"]]
        return [] if any(not item for item in alternatives) else min(alternatives, key=len)
    errors = []
    kind = spec.get("type")
    valid_type = {"object": isinstance(value, dict), "array": isinstance(value, list),
                  "string": isinstance(value, str), "boolean": isinstance(value, bool),
                  "integer": type(value) is int,
                  "number": type(value) in (int, float), "null": value is None}
    if kind and not valid_type[kind]:
        return [f"{path}: expected {kind}"]
    if "enum" in spec and value not in spec["enum"]:
        errors.append(f"{path}: not an allowed value")
    if type(value) in (int, float):
        try:
            finite = math.isfinite(value)
        except OverflowError:
            finite = False
        if not finite:
            errors.append(f"{path}: non-finite number")
        elif value < spec.get("minimum", -math.inf):
            errors.append(f"{path}: below minimum")
    if isinstance(value, str):
        if len(value.strip()) < spec.get("minLength", 0):
            errors.append(f"{path}: empty string")
        if "pattern" in spec and not re.fullmatch(spec["pattern"], value):
            errors.append(f"{path}: invalid format")
        if spec is SCHEMA["$defs"]["utc"]:
            try:
                utc(value)
            except ValueError:
                errors.append(f"{path}: invalid UTC timestamp")
    if isinstance(value, list):
        if len(value) < spec.get("minItems", 0):
            errors.append(f"{path}: too few items")
        if spec.get("uniqueItems") and len({json.dumps(v, sort_keys=True) for v in value}) != len(value):
            errors.append(f"{path}: duplicate items")
        for i, item in enumerate(value):
            errors.extend(schema_errors(item, spec.get("items", {}), f"{path}[{i}]"))
    if isinstance(value, dict):
        for name in spec.get("required", []):
            if name not in value:
                errors.append(f"{path}.{name}: missing")
        props = spec.get("properties", {})
        for name, item in value.items():
            child = props.get(name, spec.get("additionalProperties", {}))
            if child is False:
                errors.append(f"{path}.{name}: unknown field")
            elif isinstance(child, dict):
                errors.extend(schema_errors(item, child, f"{path}.{name}"))
    return errors


def utc(value):
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None or parsed.utcoffset() != timezone.utc.utcoffset(parsed):
        raise ValueError("timestamp must have UTC offset")
    return parsed


def inventory():
    return {"/".join((area, variant, site)): {"area": area, "variant": variant, "site": site,
                "trigger": item.get("triggers", {}).get(variant, item["trigger"])}
            for area, item in CATALOG["areas"].items()
            for variant in item["variants"] for site in item.get("sites", ["ordinary"])}


def distribution(values):
    if not values:
        return None
    ordered = sorted(values)
    return {"count": len(values), "median": median(values),
            "p95_nearest_rank": ordered[math.ceil(.95 * len(ordered)) - 1],
            "min": ordered[0], "max": ordered[-1]}


def median(values):
    ordered = sorted(values)
    middle = len(ordered) // 2
    return ordered[middle] if len(ordered) % 2 else ordered[middle - 1] / 2 + ordered[middle] / 2


def median_interval(values):
    """Exact sign/order-statistic CI, coverage >=95%; no normality assumption."""
    if not values:
        return None
    ordered = sorted(values)
    n = len(values)
    k, coverage, tail = 0, 1.0, 0
    for candidate in range(1, n // 2 + 1):
        tail += math.comb(n, candidate - 1)
        achieved = 1 - 2 * tail / 2**n
        if achieved < .95:
            break
        k, coverage = candidate, achieved
    return {"lower": ordered[k - 1] if k else 0,
            "upper": ordered[n - k] if k else None,
            "coverage_at_least": coverage, "unbounded": not bool(k)}


def safe(value):
    if isinstance(value, float) and not math.isfinite(value):
        return f"NONFINITE:{value}"
    if isinstance(value, list):
        return [safe(v) for v in value]
    if isinstance(value, dict):
        return {k: safe(v) for k, v in value.items()}
    return value


def analyze(data):
    errors = schema_errors(data)
    expected = inventory()
    report = {"schema_version": 1, "synthetic": None, "global_status": "invalid",
              "timing_status": "invalid", "global_acceptance": "outstanding_parent_decision",
              "limitation": LIMITATION, "errors": errors, "conditions": [],
              "missing_workloads": sorted(expected), "reviews": {},
              "review_errors": [], "native_proof": {"analyzer_observed": False,
                  "state": "not_established", "gaps": []},
              "attempts": [], "supplied_input": safe(data)}
    # Preserve malformed/legacy input in the report; never infer missing identities.
    if errors:
        legacy = isinstance(data, dict) and "plan" not in data and data.get("schema_version") != 1
        if legacy:
            report["timing_status"] = "insufficient"
            report["global_status"] = "acceptance_outstanding"
            report["legacy_input"] = True
        if isinstance(data, dict) and isinstance(data.get("attempts"), list):
            report["attempts"] = [{"raw": safe(a), "valid": False,
                                   "errors": ["input schema invalid; see errors"]}
                                  for a in data["attempts"]]
        return report
    report["synthetic"] = data["synthetic"]
    plan = data["plan"]
    if plan["catalog_version"] != CATALOG["version"]:
        errors.append("plan: catalog version mismatch")
    if data["builds"]["baseline"]["source_commit"] != CATALOG["baseline_commit"]:
        errors.append("builds: baseline source differs from frozen contract")
    a, b = (data["builds"][arm] for arm in ARMS)
    for field in ("features", "flags", "mode", "runtime", "instrumentation", "toolchain"):
        same = set(a[field]) == set(b[field]) if field == "features" else a[field] == b[field]
        if not same:
            errors.append(f"builds: mismatched {field}")
    if data["plan_commitment"]["sha256"] != canonical_hash(plan):
        errors.append("plan: commitment hash mismatch")
    try:
        frozen = utc(data["plan_commitment"]["recorded_at"])
        if frozen > utc(plan["declared_at"]):
            errors.append("plan: commitment recorded after declared collection boundary")
        boundary = utc(plan["declared_at"])
    except ValueError as exc:
        errors.append(f"plan: {exc}")
        boundary = None
    ledger = data["attempt_ledger"]
    if not ledger["complete"] or ledger["attempt_count"] != len(data["attempts"]):
        errors.append("ledger: incomplete or inconsistent attempt count")
    conditions = {}
    for condition in plan["conditions"]:
        cid = condition["id"]
        if cid in conditions:
            errors.append(f"plan: duplicate condition {cid}")
        conditions[cid] = condition
        workload = condition["workload"]
        if workload not in expected:
            errors.append(f"{cid}: unknown workload {workload}")
        elif workload.startswith("startup/"):
            if condition["matching"]["profile_state"] != expected[workload]["variant"]:
                errors.append(f"{cid}: startup profile does not match workload")
    report["missing_workloads"] = sorted(set(expected) - {c["workload"] for c in conditions.values()})
    profiles = {c["matching"]["profile_state"] for c in conditions.values()}
    report["missing_profile_states"] = sorted(set(CATALOG["profile_states"]) - profiles)
    by_condition = {cid: [] for cid in conditions}
    nonces, sequences = Counter(), []
    for attempt in data["attempts"]:
        nonces.update([attempt["action_nonce"], attempt["identity"]["task_nonce"]])
        nonces.update(e["nonce"] for e in attempt["evidence"])
        sequences.append(attempt["sequence"])
    if sequences != list(range(1, len(sequences) + 1)):
        errors.append("attempts: sequence must be unique, contiguous and in acquisition order")
    for attempt in data["attempts"]:
        issues = []
        cid = attempt["condition"]
        condition = conditions.get(cid)
        def fail(message):
            issues.append(message)
        if condition is None:
            fail("unplanned condition")
        elif attempt["matching"] != condition["matching"]:
            fail("matching conditions differ from frozen plan")
        build = data["builds"][attempt["arm"]]
        if attempt["identity"]["executable_sha256"] != build["executable_sha256"]:
            fail("executable identity differs from frozen arm")
        if attempt["matching"]["runtime"] != build["runtime"]:
            fail("runtime differs from frozen build")
        try:
            if boundary is None or utc(attempt["acquired_at"]) <= boundary:
                fail("attempt not after predeclared sampling boundary")
        except ValueError as exc:
            fail(str(exc))
        own_nonces = [attempt["action_nonce"], attempt["identity"]["task_nonce"]]
        own_nonces += [e["nonce"] for e in attempt["evidence"]]
        if any(nonces[n] != 1 for n in own_nonces):
            fail("duplicate or reused action/task/evidence nonce")
        if attempt["status"] != "success":
            fail(f"retained {attempt['status']}: {attempt['reason']}")
            if not attempt["reason"].strip():
                fail("unsuccessful attempt requires reason")
        if not attempt["load"]["policy_pass"] or attempt["load"]["competing_owned_work"]:
            fail("load policy failed or owned work competed")
        checks = set(attempt["parent_review"]["checks"])
        if not CHECKS <= checks:
            fail("parent review lacks foreground/identity/content/work/usability checks")
        if attempt["parent_review"]["state"] != "pass":
            fail("parent evidence review is outstanding or failed")
        timing = attempt["timing"]
        duration = None
        missing_kinds = sorted(PROOF_KINDS - {e["kind"] for e in attempt["evidence"]})
        if missing_kinds or attempt["parent_review"]["state"] != "pass":
            report["native_proof"]["gaps"].append({"sequence": attempt["sequence"],
                "missing_required_kinds": missing_kinds,
                "parent_review_state": attempt["parent_review"]["state"]})
        if timing is None:
            fail("missing task timing")
        else:
            if timing["end"] < timing["start"]:
                fail("non-monotonic task timing")
            else:
                duration = timing["end"] - timing["start"]
            if condition and condition["workload"] in expected:
                if timing["start_event"] != expected[condition["workload"]]["trigger"]:
                    fail("wrong task start endpoint")
            kinds = {e["kind"] for e in attempt["evidence"]}
            if not PROOF_KINDS <= kinds:
                fail("missing native presentation/interaction, start or work proof; proxies do not qualify")
            endpoints = []
            starts = []
            for evidence in attempt["evidence"]:
                if evidence["identity"] != attempt["identity"]:
                    fail("evidence executable/window/controller/document/task identity mismatch")
                if evidence["clock_id"] != timing["clock_id"]:
                    fail("evidence clock differs from task clock")
                if evidence["kind"] in PROOF_KINDS:
                    observed = evidence["observed_ms"]
                    if not timing["start"] <= observed <= timing["end"]:
                        fail("evidence outside timed region")
                    if evidence["kind"] == "task_start":
                        starts.append(observed)
                    else:
                        endpoints.append(observed)
            if starts != [timing["start"]]:
                fail("task start must have exactly one bound evidence timestamp")
            if not endpoints or max(endpoints) != timing["end"]:
                fail("end must equal last presented/usable/work completion evidence")
        row = {"raw": attempt, "valid": not issues, "errors": issues, "duration_ms": duration}
        report["attempts"].append(row)
        if cid in by_condition:
            by_condition[cid].append(row)
        else:
            errors.append(f"attempt {attempt['sequence']}: unplanned condition {cid}")
    # Global monotonic acquisition clock; timing is never derived from wall timestamps.
    previous_end = None
    previous_time = None
    clock_ids = {a["timing"]["clock_id"] for a in data["attempts"] if a["timing"]}
    if len(clock_ids) > 1:
        errors.append("attempts: one acquisition monotonic clock is required")
    for row in report["attempts"]:
        attempt = row["raw"]
        timing = attempt["timing"]
        try:
            current_time = utc(attempt["acquired_at"])
            if previous_time is not None and current_time < previous_time:
                errors.append("attempts: wall acquisition order moved backwards")
            previous_time = current_time
        except ValueError:
            pass  # Attempt-level error above.
        if timing:
            if previous_end is not None and timing["start"] < previous_end:
                errors.append("attempts: overlapping or backwards acquisition intervals")
            previous_end = timing["end"]
    for cid, condition in conditions.items():
        rows = by_condition[cid]
        planned_pairs = condition["pairs"]
        slots = []
        for pair in range(1, planned_pairs + 1):
            first = condition["first_arm"] if pair % 2 else ARMS[1 - ARMS.index(condition["first_arm"]) ]
            slots.extend((pair, arm) for arm in (first, ARMS[1 - ARMS.index(first)]))
        actual = [(r["raw"]["pair"], r["raw"]["arm"]) for r in rows]
        order_ok = actual == slots
        # A missing last slot is insufficient, whereas duplicates/order manipulation are invalid.
        prefix_ok = actual == slots[:len(actual)]
        pairs, ratios, deltas = [], [], []
        for pair in range(1, planned_pairs + 1):
            members = [r for r in rows if r["raw"]["pair"] == pair]
            group = {arm: [r for r in members if r["raw"]["arm"] == arm] for arm in ARMS}
            if any(len(group[arm]) != 1 or not group[arm][0]["valid"] for arm in ARMS):
                continue
            baseline, candidate = (group[arm][0]["duration_ms"] for arm in ARMS)
            if baseline <= 0:
                group["baseline"][0]["errors"].append("zero baseline cannot define paired ratio")
                group["baseline"][0]["valid"] = False
                continue
            ratio = candidate / baseline
            if not math.isfinite(ratio):
                group["candidate"][0]["errors"].append("non-finite derived paired ratio")
                group["candidate"][0]["valid"] = False
                continue
            ratios.append(ratio)
            deltas.append(candidate - baseline)
            pairs.append({"pair": pair, "baseline_ms": baseline,
                          "candidate_ms": candidate, "ratio": ratio})
        interval = median_interval(ratios)
        point_met = bool(ratios) and median(ratios) <= .5
        uncertainty_met = bool(interval) and interval["upper"] is not None and interval["upper"] <= .5
        invalid_count = sum(not r["valid"] for r in rows)
        if errors or invalid_count or not prefix_ok:
            status = "invalid"
        elif plan["purpose"] == "exploratory" and len(pairs) >= 5 and order_ok:
            status = "exploratory"
        elif not order_ok or len(pairs) < 10:
            status = "insufficient"
        elif not point_met:
            status = "measured_target_missed"
        elif plan["decision_rule"] == "upper_95_bound" and not uncertainty_met:
            status = "insufficient"
        else:
            status = "supported_measured_acceptance"
        counts = Counter(r["raw"]["status"] for r in rows)
        report["conditions"].append({"id": cid, "workload": condition["workload"], "status": status,
            "planned_pairs": planned_pairs, "valid_pairs": len(pairs), "attempt_count": len(rows),
            "invalid_attempts": invalid_count, "status_counts": dict(counts),
            "unsuccessful_rate": sum(n for s, n in counts.items() if s != "success") / len(rows) if rows else None,
            "invalid_rate": invalid_count / len(rows) if rows else None,
            "order_complete": order_ok, "order_valid_prefix": prefix_ok,
            "all_observed_attempt_elapsed_ms_including_invalid": {
                arm: distribution([r["duration_ms"] for r in rows
                     if r["raw"]["arm"] == arm and r["duration_ms"] is not None]) for arm in ARMS},
            "outcomes_by_arm": {arm: dict(Counter(r["raw"]["status"] for r in rows
                                                  if r["raw"]["arm"] == arm)) for arm in ARMS},
            "baseline_ms": distribution([p["baseline_ms"] for p in pairs]),
            "candidate_ms": distribution([p["candidate_ms"] for p in pairs]),
            "paired_ratio": distribution(ratios), "paired_delta_ms": distribution(deltas),
            "median_ratio_95_interval": interval, "point_target_met": point_met,
            "upper_95_bound_at_target": uncertainty_met, "pairs": pairs})
    # Missing conditions are first-class report rows, never omitted by a group-by.
    for workload in report["missing_workloads"]:
        report["conditions"].append({"id": None, "workload": workload,
                                     "status": "insufficient", "valid_pairs": 0})
    for category, names in CATALOG["reviews"].items():
        provided = data.get("reviews", {}).get(category, {})
        unknown = set(provided) - set(names)
        if unknown:
            report["review_errors"].append(f"reviews.{category}: unknown gates {sorted(unknown)}")
        report["reviews"][category] = {name: dict(provided.get(name, {"state": "outstanding",
                            "reason": "missing supplied evidence/review"})) for name in names}
        for name, review in provided.items():
            issues = []
            if review["baseline_source"] != a["source_commit"] or review["candidate_source"] != b["source_commit"]:
                issues.append("stale build review")
            if (review["baseline_source_tree_sha256"] != a["source_tree_sha256"] or
                    review["candidate_source_tree_sha256"] != b["source_tree_sha256"]):
                issues.append("review does not bind exact source trees")
            if review["state"] == "pass" and (not review["evidence"] or set(review["coverage"]) != set(conditions)):
                issues.append("pass lacks evidence/full condition coverage")
            if (review["baseline_executable_sha256"] != a["executable_sha256"] or
                    review["candidate_executable_sha256"] != b["executable_sha256"] or
                    review["plan_sha256"] != data["plan_commitment"]["sha256"] or
                    review["ledger_sha256"] != ledger["sha256"]):
                issues.append("review does not bind exact binaries, plan and ledger")
            for issue in issues:
                report["review_errors"].append(f"reviews.{category}.{name}: {issue}")
            if issues and name in report["reviews"][category]:
                report["reviews"][category][name]["state"] = "invalid"
    statuses = {c["status"] for c in report["conditions"]}
    if errors or "invalid" in statuses:
        timing_status = "invalid"
    elif "measured_target_missed" in statuses:
        timing_status = "measured_target_missed"
    elif report["missing_profile_states"] or "insufficient" in statuses:
        timing_status = "insufficient"
    elif "exploratory" in statuses:
        timing_status = "exploratory"
    else:
        timing_status = "supported_measured_acceptance"
    report["timing_status"] = timing_status
    report["native_proof"]["state"] = "supplied_claims_require_parent_verification"
    all_reviews = not report["review_errors"] and all(r["state"] == "pass" for group in report["reviews"].values() for r in group.values())
    report["global_status"] = ("synthetic_not_performance_proof" if data["synthetic"] else
                               "supplied_evidence_supports_parent_review" if all_reviews and
                               timing_status == "supported_measured_acceptance" else
                               "acceptance_outstanding")
    return safe(report)


def render_html(report):
    esc = lambda value: html.escape(str(value), quote=True)
    def stats(value):
        if not value:
            return "missing"
        return "; ".join(f"{key}={number:.6g}" for key, number in value.items())
    rows = []
    for c in report["conditions"]:
        rows.append("<tr>" + "".join(f"<td>{esc(v)}</td>" for v in (
            c["id"] or "MISSING", c["workload"], c["status"], c["valid_pairs"],
            stats(c.get("baseline_ms")), stats(c.get("candidate_ms")), stats(c.get("paired_ratio")),
            c.get("median_ratio_95_interval"), c.get("status_counts"), c.get("invalid_rate"))) + "</tr>")
    raw = esc(json.dumps(safe(report), indent=2, allow_nan=False))
    reviews = []
    for category, group in report["reviews"].items():
        for name, review in group.items():
            reviews.append("<tr>" + "".join(f"<td>{esc(v)}</td>" for v in
                (category, name, review["state"], review["reason"], review.get("reference", "missing"))) + "</tr>")
    return ("<!doctype html><html lang='en'><meta charset='utf-8'><meta name='viewport' "
            "content='width=device-width,initial-scale=1'><title>Bowser supplied evidence report</title>"
            "<style>body{font:16px system-ui;margin:2rem;color:#17202a}table{border-collapse:collapse}"
            "td,th{border:1px solid #aaa;padding:.4rem;text-align:left}pre{white-space:pre-wrap;"
            "overflow-wrap:anywhere}.scroll{overflow:auto}.label{font-weight:bold;color:#8b1a13}</style>"
            f"<h1>{'SYNTHETIC FIXTURE — NOT PERFORMANCE PROOF' if report['synthetic'] else 'Supplied evidence report'}</h1>"
            f"<p class='label'>{esc(report['global_status'])}; timing: {esc(report['timing_status'])}</p>"
            f"<p>Global acceptance: {esc(report['global_acceptance'])}. {esc(LIMITATION)}</p>"
            f"<p>Native proof status: {esc(report['native_proof']['state'])}; "
            f"attempts with missing proof/review: {len(report['native_proof']['gaps'])}. "
            "The analyzer independently observed no native presentation.</p>"
            "<p>All times in ms. Ratios are candidate / baseline; target ≤0.5. p95 uses nearest rank; "
            "small-sample tails are descriptive. Order-statistic 95% median interval assumes independent "
            "matched pairs. Invalid attempts remain below and prevent condition acceptance.</p>"
            "<div class='scroll'><table><thead><tr><th>Condition</th><th>Workload</th><th>Status</th>"
            "<th>Valid pairs</th><th>Baseline ms</th><th>Candidate ms</th><th>Paired ratios</th>"
            "<th>95% interval</th><th>Attempt outcomes</th><th>Invalid rate</th></tr></thead><tbody>"
            + "".join(rows) + "</tbody></table></div><h2>Separate resource, parity and diagnostic gates</h2>"
            "<div class='scroll'><table><thead><tr><th>Category</th><th>Gate</th><th>State</th>"
            "<th>Reason</th><th>Review reference</th></tr></thead><tbody>" + "".join(reviews) +
            "</tbody></table></div><h2>Full report, reviews and retained evidence</h2>"
            f"<p>Includes input provenance, all attempts, errors, references and outstanding resource/parity gates.</p><pre>{raw}</pre></html>\n")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("--json", type=Path, dest="json_path")
    parser.add_argument("--html", type=Path, dest="html_path")
    args = parser.parse_args(argv)
    paths = [args.input] + [p for p in (args.json_path, args.html_path) if p]
    aliases = len({p.resolve() for p in paths}) != len(paths)
    for index, left in enumerate(paths):
        for right in paths[index + 1:]:
            aliases |= left.exists() and right.exists() and left.samefile(right)
    if aliases:
        parser.error("input and output paths must be distinct")
    try:
        raw = args.input.read_bytes()
        data = json.loads(raw.decode("utf-8-sig"), object_pairs_hook=unique_keys)
        report = analyze(data)
        report["input_sha256"] = hashlib.sha256(raw).hexdigest()
        encoded = json.dumps(report, indent=2, allow_nan=False) + "\n"
        if args.json_path:
            args.json_path.write_text(encoded, encoding="utf-8")
        if args.html_path:
            args.html_path.write_text(render_html(report), encoding="utf-8")
        print(json.dumps({"global_status": report["global_status"],
                          "timing_status": report["timing_status"], "synthetic": report["synthetic"],
                          "errors": len(report["errors"]), "retained_attempts": len(report["attempts"]),
                          "missing_workloads": len(report["missing_workloads"])}))
        return 0 if report["global_status"] == "supplied_evidence_supports_parent_review" else 1
    except (OSError, ValueError, RecursionError) as exc:
        print(f"input/output error: {exc}", file=sys.stderr)
        return 2


def unique_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


if __name__ == "__main__":
    sys.exit(main())
