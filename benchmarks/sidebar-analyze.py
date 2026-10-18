"""Failure-inclusive sidebar baseline and matched identical-workload comparison."""
import json
import math
import importlib.util
from datetime import datetime
from pathlib import Path
from statistics import median
import sys


def phase_private(phase):
    if not phase.get('eligible'):
        return None
    snapshots = phase.get('snapshots') or []
    if len(snapshots) != 3:
        return None
    for snapshot in snapshots:
        details = snapshot.get('ProcessDetails') or []
        if (snapshot.get('CoverageErrors') or snapshot.get('RacedExitCount', 0)
                or snapshot.get('PrivateSource') != 'native'
                or snapshot.get('NativePrivateBytes') is None or not details
                or any(p.get('native_counter_source') != 'native'
                       or p.get('private_bytes') is None for p in details)):
            return None
    before = phase.get('webapps_inventory_before') or {}
    after = phase.get('webapps_inventory_after') or {}
    if not before.get('available') or not after.get('available'):
        return None
    roots = before.get('identities') or []
    identities = {(p.get('pid'), p.get('created_ticks')) for p in roots}
    if identities != {(p.get('pid'), p.get('created_ticks')) for p in after.get('identities', [])}:
        return None
    if phase.get('name') != 'initial-one-tab' and not roots:
        return None
    if any(pid is None or ticks is None for pid, ticks in identities):
        return None
    for snapshot in snapshots:
        actual = {(p.get('process_id'), p.get('created_ticks')) for p in snapshot['ProcessDetails']}
        if not identities.issubset(actual):
            return None
    return median(s['NativePrivateBytes'] for s in snapshots)


def describe(rows):
    output = []
    indexed = {}
    for row in rows:
        phases = {p['name']: phase_private(p) for p in (row.get('sidebar_trace') or [])
                  if p.get('kind') == 'resource-phase'}
        accepted = bool(row.get('valid') and row.get('tree_exit_confirmed')
                        and len(row.get('input_identity_checks') or []) == 2
                        and all(x.get('passed') for x in row['input_identity_checks'])
                        and (not row['arm'].endswith('_on') or (row.get('sidebar_observation') or {}).get('complete') is True))
        record = {'id': row['id'], 'pair': row['pair'], 'arm': row['arm'],
                  'condition': row['condition'], 'valid': row.get('valid'),
                  'error': row.get('error'), 'measurement_eligible': accepted, 'prior_fresh_outcome': row.get('prior_fresh_outcome'),
                  'phase_private_bytes': {k: v if accepted else None for k, v in phases.items()},
                  'ledger': row.get('sidebar_observation'),
                  'limitation': 'UIA-conditioned whole-tree native private commit; no per-view RAM/PSS claim'}
        output.append(record)
        indexed[row['pair'], row['condition'], row['arm']] = record
    paired = {}
    for row in output:
        if row['arm'] != 'sidebar_on':
            continue
        off = indexed.get((row['pair'], row['condition'], 'sidebar_off'))
        if not off:
            continue
        for phase, on_value in row['phase_private_bytes'].items():
            off_value = off['phase_private_bytes'].get(phase)
            if on_value is None or off_value is None:
                continue
            paired.setdefault((row['condition'], phase), []).append(on_value - off_value)
    return {'scope': 'Fixed failure-inclusive diagnostic-only baseline; paired ON minus OFF is logging overhead, not an optimization benefit or acceptance',
            'rows': output, 'row_count': len(rows), 'valid_count': sum(bool(r.get('valid')) for r in rows),
            'paired_logging_private_delta_bytes': [
                {'condition': k[0], 'phase': k[1], 'n': len(v), 'median': median(v),
                 'min': min(v), 'max': max(v), 'all_deltas': v}
                for k, v in sorted(paired.items())],
            'limits': ['Pilot n is not a stable tail or gain acceptance matrix',
                       'OFF counts are unavailable, never zero/passed',
                       'A successful Close result is not proof of freed private memory',
                       'Reused state following failed fresh work remains conditioned on that failure',
                       'No comparison with old UIA-free startup memory or minus-minimizer reference']}


def candidate_lifecycle_correct(row):
    ledger = row.get('sidebar_observation') or {}
    requests = ledger.get('requests') or []
    return (ledger.get('complete') is True and ledger.get('environment_requests') == 1
            and ledger.get('controller_requests') == 2 and len(requests) == 2
            and {request.get('app') for request in requests} == {1, 2}
            and all(request.get('configured') == 1 and request.get('installed') == 1
                    and request.get('replaced') == 0 and request.get('close_requested') == 1
                    and request.get('close_succeeded') == 1 and request.get('close_failed') == 0
                    and request.get('native_navigation_success', 0) >= 1 for request in requests))


def observed_duration(snapshots):
    try:
        first, last = snapshots[0]['SampledAtUtc'], snapshots[-1]['SampledAtUtc']
        seconds = (datetime.fromisoformat(last.replace('Z', '+00:00'))
                   - datetime.fromisoformat(first.replace('Z', '+00:00'))).total_seconds()
        return seconds if math.isfinite(seconds) and seconds > 0 else None
    except (KeyError, TypeError, ValueError, AttributeError, OverflowError):
        return None


def comparison(rows, frequency):
    """Matched identical-workload inference only; all rows and exclusion reasons are retained."""
    spec = importlib.util.spec_from_file_location('production_analysis', Path(__file__).with_name('production-analyze.py'))
    analysis = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analysis)
    described = describe(rows)['rows']
    groups = {}
    keys = {}
    indexed_rows = {}
    for index, row in enumerate(rows):
        key = (row['pair'], row['condition'], row['arm'], row.get('sidebar_scenario'))
        keys[key] = keys.get(key, 0) + 1
        indexed_rows.setdefault(key, []).append((row, described[index]))
    for raw, record in zip(rows, described):
        key = (raw['pair'], raw['condition'], raw['arm'], raw.get('sidebar_scenario'))
        exclusions = []
        if not record['measurement_eligible']:
            exclusions.append('row lacks valid identity/drain/native lifecycle coverage')
        if keys[key] != 1:
            exclusions.append('duplicate matched pair/arm/condition/scenario')
        if raw.get('condition') == 'reused':
            fresh_rows = indexed_rows.get((raw['pair'], 'fresh', raw['arm'], raw.get('sidebar_scenario')), [])
            fresh_correct = (len(fresh_rows) == 1 and fresh_rows[0][1]['measurement_eligible']
                             and (raw['arm'] != 'sidebar_candidate_on' or candidate_lifecycle_correct(fresh_rows[0][0])))
            if not fresh_correct or (raw.get('prior_fresh_outcome') or {}).get('valid') is not True:
                exclusions.append('reused state lacks unique semantically correct identity/drain/ledger-valid fresh conditioning')
        if raw['arm'] not in ('sidebar_baseline_on', 'sidebar_candidate_on'):
            exclusions.append('not a matched sidebar comparison arm')
        if raw['arm'] == 'sidebar_candidate_on' and not candidate_lifecycle_correct(raw):
            exclusions.append('candidate did not retain correct logical A+B controllers/navigation/paired successful Closes')
        metrics = {'phase_' + name + '_private_bytes': value for name, value in record['phase_private_bytes'].items()}
        if not exclusions:
            before, after = raw.get('disk_before'), raw.get('disk_after')
            if isinstance(before, int) and isinstance(after, int):
                metrics['profile_disk_growth_bytes'] = after - before
            for phase in raw.get('sidebar_trace') or []:
                if phase.get('kind') != 'resource-phase' or phase_private(phase) is None:
                    continue
                snapshots = phase['snapshots']
                cpu = [snap.get('CpuByPid') or {} for snap in snapshots]
                duration = observed_duration(snapshots)
                stable_cpu = (duration is not None and bool(cpu[0]) and all(set(snap) == set(cpu[0]) for snap in cpu)
                              and all(set(values) == {str(p.get('process_id')) + ':' + str(p.get('created_ticks'))
                                                       for p in snapshot['ProcessDetails']}
                                      for values, snapshot in zip(cpu, snapshots))
                              and all(all(isinstance(value, (float, int)) and math.isfinite(value) and value >= 0 for value in snap.values()) for snap in cpu)
                              and all(all(cpu[i][pid] >= cpu[i - 1][pid] for pid in cpu[0]) for i in range(1, len(cpu))))
                if stable_cpu:
                    metrics['phase_' + phase['name'] + '_cpu_core_seconds'] = sum(cpu[-1][pid] - cpu[0][pid] for pid in cpu[0])
                    metrics['phase_' + phase['name'] + '_cpu_observed_seconds'] = duration
            toggles, proxies = {}, {1: [], 2: []}
            for event in raw.get('sidebar_trace') or []:
                if event.get('kind') == 'rail-toggle' and event.get('invoked') and not (event.get('dispatch_guard') or {}).get('refused', True):
                    toggles[event.get('name')] = event['dispatch_guard']['qpc']
                elif event.get('kind') == 'visible-app' and event.get('ack_count', 0) > 0 and event.get('visible_document_count', 0) == 1:
                    start = toggles.pop(event.get('name'), None)
                    if start is not None and frequency > 0 and event['qpc'] >= start:
                        proxies[event['app_id']].append((event['qpc'] - start) * 1000 / frequency)
            for app, name in ((1, 'A'), (2, 'B')):
                if proxies[app]:
                    metrics['first_' + name + '_toggle_to_visible_ready_proxy_ms'] = proxies[app][0]
                if len(proxies[app]) > 1:
                    metrics['warm_' + name + '_toggle_to_visible_ready_proxy_ms'] = median(proxies[app][1:])
        if exclusions or not raw.get('valid') or not raw.get('tree_exit_confirmed'):
            metrics = {name: None for name in metrics}
        record['metrics'] = metrics
        record['comparison_exclusions'] = exclusions
        group = (raw.get('sidebar_scenario') or 'unknown', raw['condition'])
        groups.setdefault(group, []).append(record)
    paired, summaries = [], []
    for (scenario, condition), group in sorted(groups.items()):
        names = sorted({name for row in group for name in row['metrics']})
        for arm in ('sidebar_baseline_on', 'sidebar_candidate_on'):
            members = [row for row in group if row['arm'] == arm]
            summaries.append({'scenario': scenario, 'condition': condition, 'arm': arm,
                              'metrics': {name: analysis.dist([row['metrics'].get(name) for row in members]) for name in names}})
        for metric in names:
            result = analysis.paired(group, 'sidebar_candidate_on', 'sidebar_baseline_on', metric)
            candidate_values = {row['pair']: row['metrics'].get(metric) for row in group if row['arm'] == 'sidebar_candidate_on'}
            reference_values = {row['pair']: row['metrics'].get(metric) for row in group if row['arm'] == 'sidebar_baseline_on'}
            all_pairs = []
            for pair in sorted(candidate_values.keys() | reference_values.keys()):
                candidate, reference = candidate_values.get(pair), reference_values.get(pair)
                available = candidate is not None and reference is not None
                all_pairs.append({'pair': pair, 'candidate': candidate, 'reference': reference,
                                  'included': available, 'delta': candidate - reference if available else None,
                                  'ratio': candidate / reference if available and reference > 0 else None})
            paired.append(dict(scenario=scenario, condition=condition, all_pairs=all_pairs, **result))
    return {'scope': 'Fixed matched whole-tree sidebar comparison; not automatic correctness or gain acceptance',
            'primary_resource_metric': 'phase_closed-cycle-3_private_bytes', 'rows': described,
            'row_count': len(rows), 'valid_count': sum(bool(row.get('valid')) for row in rows),
            'summary': summaries, 'paired_comparisons': paired,
            'limits': ['No comparison with earlier UIA-free memory or the minus-minimizer reference',
                       'Both arms retain identical diagnostics; no overhead subtraction',
                       'Visible-ready proxies include UIA/dispatch overhead and are not paint or keyboard latency',
                       'CPU is absolute core-seconds over actual observed endpoints; changing identities unavailable',
                       'Ten valid pairs are required separately for every accepted metric and fresh/reused condition',
                       'Ten-pair p95 is exploratory, not a stable tail estimate',
                       'Rapid B-ready may follow an earlier B request and is separate from settled first B-open',
                       'PrivateUsage is process private commit, not per-view RAM/PSS; sampled snapshots are not lifetime peaks']}


if __name__ == '__main__':
    folder = Path(sys.argv[1])
    rows = []
    for path in folder.glob('*.json'):
        data = json.loads(path.read_text(encoding='utf-8-sig'))
        if isinstance(data, dict) and 'arm' in data and 'sidebar_trace' in data:
            rows.append(data)
    environment = json.loads((folder / 'environment.json').read_text(encoding='utf-8-sig'))
    result = (comparison(rows, environment['qpc_frequency']) if environment.get('workload_settings', {}).get('sidebar_lifecycle_compare') else describe(rows))
    (folder / 'sidebar-analysis.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'row_count': result['row_count'], 'valid_count': result['valid_count']}))
