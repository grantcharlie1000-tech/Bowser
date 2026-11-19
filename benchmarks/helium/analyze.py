"""Reduce complete matched Windows blocks after the capture and telemetry sessions end."""

import argparse
from bisect import bisect_left, bisect_right
import csv
import json
from math import comb, isfinite
from pathlib import Path
from statistics import median

from collect import ARMS, FRAMES, ORDERS, digest


# Each margin is max(absolute, relative to reference). Higher is better only for displayed FPS.
MARGINS = {
    "first_useful_ms": (20, 0.05, False),
    "fcp_ms": (10, 0.05, False),
    "private_commit_bytes": (5 * 1024**2, 0.05, False),
    "summed_working_set_bytes": (5 * 1024**2, 0.05, False),
    "cpu_core_seconds": (0.05, 0.05, False),
    "whole_pc_watts": (0.25, 0.05, False),
    "whole_pc_joules": (4, 0.05, False),
    "presented_fps": (1, 0.02, True),
    "missed_frame_fraction": (0.01, 0, False),
}
DIAGNOSTIC = {
    "owned_title_proxy_ms": (20, 0.05, False),
    "raf_callbacks_per_second": (1, 0.02, True),
}
SCENARIOS = [("light", "idle"), ("demanding", "idle"), ("demanding", "animation")]


def median_interval(values):
    """Exact, distribution-free two-sided 95% sign interval for the paired median."""
    ordered = sorted(values)
    n = len(ordered)
    rank = max((i for i in range(1, n // 2 + 1)
                if 2 * sum(comb(n, j) for j in range(i)) * 20 <= 2**n), default=0)
    return [ordered[rank - 1], ordered[-rank]] if rank else None


def compare(pairs, margin, balanced=True):
    absolute, relative, higher = margin
    result = dict(pairs=len(pairs), balanced=balanced, margin_absolute=absolute, margin_relative=relative)
    if not pairs:
        return {**result, "status": "native validation pending"}
    candidate, reference = zip(*pairs)
    direction = 1 if higher else -1
    differences = [direction * (c - r) for c, r in pairs]
    scaled = [delta / max(absolute, abs(r) * relative) for delta, r in zip(differences, reference)]
    interval = median_interval(scaled)
    status = "inconclusive"
    if len(pairs) >= 10 and balanced and interval:
        if interval[0] > 1:
            status = "verified win"
        elif interval[1] < -1:
            status = "loss"
        elif interval[0] >= -1 and interval[1] <= 1:
            status = "tie"
    return {**result, "status": status,
            "candidate": [median(candidate), min(candidate), max(candidate)],
            "reference": [median(reference), min(reference), max(reference)],
            "paired_improvement": [median(differences), min(differences), max(differences)],
            "paired_improvement_ci95": median_interval(differences),
            "paired_improvement_in_margin_units_ci95": interval}


def power_window(samples, start, end):
    """Trapezoid-integrate a bracketed synchronized meter stream, with no extrapolation."""
    start_ns, end_ns = start["utc_ns"], end["utc_ns"]
    wall, monotonic = end_ns - start_ns, end["qpc_ns"] - start["qpc_ns"]
    if wall <= 0 or monotonic <= 0 or abs(wall - monotonic) > 20_000_000:
        return {}
    start_index = bisect_right(samples, (start_ns, float("inf"))) - 1
    end_index = bisect_left(samples, (end_ns,))
    if start_index < 0 or end_index == len(samples):
        return {}
    joules = 0
    for (left, watts_left), (right, watts_right) in zip(samples[start_index:end_index], samples[start_index+1:end_index+1]):
        if not 0 < right - left <= 1_000_000_000:
            return {}
        a, b = max(left, start_ns), min(right, end_ns)
        slope = (watts_right - watts_left) / (right - left)
        joules += ((2 * watts_left + (a + b - 2 * left) * slope) / 2) * (b - a) / 1e9
    return {"whole_pc_joules": joules, "whole_pc_watts": joules / (wall / 1e9)}


def values(row, presentation, samples, refresh_hz):
    launch = row["launch"]["qpc_ns"]
    cpu = sum(row["cpu_after"][key] - row["cpu_before"][key] for key in ("user_100ns", "kernel_100ns")) / 1e7
    result = {
        "owned_title_proxy_ms": (row["title_observed"]["observed"]["qpc_ns"] - launch) / 1e6,
        "fcp_ms": row["beacons"]["ready"]["fcp_ms"],
        "cpu_core_seconds": cpu,
        "private_commit_bytes": max(sum(proc["private"] for proc in row[key]) for key in ("memory_before", "memory_after")),
        "summed_working_set_bytes": max(sum(proc["working_set"] for proc in row[key]) for key in ("memory_before", "memory_after")),
    }
    if samples:
        result.update(power_window(samples, row["work_start"], row["work_end"]))
    if row["token"] in presentation:
        observed = presentation[row["token"]]
        useful = int(observed["first_useful_qpc_ns"])
        if launch <= useful <= row["work_start"]["qpc_ns"]:
            result["first_useful_ms"] = (useful - launch) / 1e6
        if row["workload"] == "animation" and observed["presented_frames"]:
            count = int(observed["presented_frames"])
            duration = (row["work_end"]["qpc_ns"] - row["work_start"]["qpc_ns"]) / 1e9
            if 0 <= count <= FRAMES and duration > 0 and count / duration <= refresh_hz * 1.01:
                result["presented_fps"] = count / duration
                result["missed_frame_fraction"] = (FRAMES - count) / FRAMES
    if row["workload"] == "animation":
        times = row["beacons"]["done"]["timestamps"]
        result["raf_callbacks_per_second"] = (len(times) - 1) * 1000 / (times[-1] - times[0])
    return result


def reduce_campaign(folder, meter, meter_uncertainty, frames):
    campaign = json.loads((folder / "campaign.json").read_text(encoding="utf-8"))
    if campaign["pilot"] or campaign["config"]["diagnostic"]:
        raise ValueError("pilot and diagnostic archives cannot produce final comparisons")
    if not (folder / "complete.txt").is_file():
        raise ValueError("capture did not complete with matching postflight executable hashes")
    attempts = {}
    tokens = set()
    invalid = []
    for path in sorted(folder.glob("*.json")):
        if path.name in ("build-identities.json", "campaign.json"):
            continue
        row = json.loads(path.read_text(encoding="utf-8"))
        key = tuple(row[field] for field in ("fixture", "workload", "warmth", "block", "arm", "prime"))
        if key in attempts or row["token"] in tokens:
            raise ValueError(f"duplicate attempt: {path}")
        expected = ORDERS[(row["block"] - 1) % len(ORDERS)]
        if tuple(row["arm_order"]) != expected or row["arm"] != expected[row["position"] - 1]:
            raise ValueError(f"unbalanced arm order in {path}")
        attempts[key] = row
        tokens.add(row["token"])
        if not row["valid"] or not row["drained"]:
            invalid.append(path.name)

    samples = []
    inputs = {}
    if meter:
        inputs["meter"] = dict(file=str(meter), sha256=digest(meter), uncertainty_watts=meter_uncertainty)
        with meter.open(encoding="utf-8-sig", newline="") as file:
            samples = [(int(r["utc_ns"]), float(r["watts"])) for r in csv.DictReader(file)]
        if (len(samples) < 2 or any(not isfinite(w) or w < 0 for _, w in samples)
                or any(a[0] >= b[0] for a, b in zip(samples, samples[1:]))):
            raise ValueError("unsafe or non-monotonic meter samples")
    presentation = {}
    if frames:
        inputs["presentation"] = dict(file=str(frames), sha256=digest(frames), traces={})
        with frames.open(encoding="utf-8-sig", newline="") as file:
            for row in csv.DictReader(file):
                token = row["token"]
                if token not in tokens or token in presentation:
                    raise ValueError(f"unknown or duplicate presentation token: {token}")
                if int(row["refresh_hz"]) != campaign["config"]["display"]["refresh_hz"]:
                    raise ValueError(f"refresh mismatch for presentation token: {token}")
                trace = (frames.parent / row["trace"]).resolve(strict=True)
                inputs["presentation"]["traces"][str(trace)] = digest(trace)
                presentation[token] = row

    margins = {**MARGINS, **DIAGNOSTIC}
    if meter:
        # Allow for worst-case absolute error of both arms, and up to 90 s for a validated task.
        margins["whole_pc_watts"] = (max(0.25, 2 * meter_uncertainty), 0.05, False)
        margins["whole_pc_joules"] = (max(4, 2 * meter_uncertainty * 90), 0.05, False)
    report = []
    for fixture, workload in SCENARIOS:
        for warmth in ("fresh", "warm"):
            blocks = []
            for number in range(1, 13):
                prefix = (fixture, workload, warmth, number)
                measured = [attempts.get((*prefix, arm, False)) for arm in ARMS]
                required = measured + ([attempts.get((*prefix, arm, True)) for arm in ARMS] if warmth == "warm" else [])
                if all(row and row["valid"] and row["drained"] for row in required):
                    if campaign["config"]["ac"] == 0:
                        charge = [row[phase]["charge"] for row in measured for phase in ("power_before", "power_after")]
                        if max(charge) > 100 or max(charge) - min(charge) > 5:
                            continue
                    refresh_hz = campaign["config"]["display"]["refresh_hz"]
                    blocks.append((number, {row["arm"]: values(row, presentation, samples, refresh_hz) for row in measured}))
            for metric, margin in margins.items():
                if workload != "animation" and metric in ("presented_fps", "missed_frame_fraction", "raf_callbacks_per_second"):
                    continue
                paired = [(number, block) for number, block in blocks if all(metric in row for row in block.values())]
                positions = [ORDERS[(number - 1) % len(ORDERS)] for number, _ in paired]
                counts = [[sum(order[position] == arm for order in positions) for position in range(3)] for arm in ARMS]
                balanced = all(max(by_position) - min(by_position) <= 1 for by_position in counts)
                for reference in ("main", "helium"):
                    stats = compare([(block["candidate"][metric], block[reference][metric]) for _, block in paired],
                                    margin, balanced)
                    report.append(dict(fixture=fixture, workload=workload, warmth=warmth, reference=reference,
                                       metric=metric, diagnostic=metric in DIAGNOSTIC, **stats))
    all_won = all(row["status"] == "verified win" for row in report if not row["diagnostic"] and row["reference"] == "helium")
    no_main_regressions = all(row["balanced"] and row["pairs"] >= 10
                              and row["paired_improvement_in_margin_units_ci95"][0] >= -1
                              for row in report if not row["diagnostic"] and row["reference"] == "main")
    return dict(all_native_metrics_over_helium_won=all_won, native_non_regression_vs_main=no_main_regressions,
                invalid_attempts=invalid, external_inputs=inputs, metrics=report)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("campaign", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--meter", type=Path, help="synchronized calibrated whole-PC CSV")
    parser.add_argument("--meter-uncertainty-watts", type=float)
    parser.add_argument("--presentation", type=Path, help="externally verified presentation CSV")
    args = parser.parse_args()
    if args.meter and (args.meter_uncertainty_watts is None
                       or not isfinite(args.meter_uncertainty_watts) or args.meter_uncertainty_watts < 0):
        parser.error("specify the meter's absolute uncertainty in watts")
    data = reduce_campaign(args.campaign, args.meter, args.meter_uncertainty_watts, args.presentation)
    with args.out.open("x", encoding="utf-8") as file:
        json.dump(data, file, indent=2, allow_nan=False)
        file.write("\n")


if __name__ == "__main__":
    main()
