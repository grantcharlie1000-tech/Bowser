"""Failure-inclusive sidebar pilot description. ON/OFF is logging overhead, NOT optimization."""
import json
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
                        and (row['arm'] != 'sidebar_on' or (row.get('sidebar_observation') or {}).get('complete') is True))
        record = {'id': row['id'], 'pair': row['pair'], 'arm': row['arm'],
                  'condition': row['condition'], 'valid': row.get('valid'),
                  'error': row.get('error'), 'prior_fresh_outcome': row.get('prior_fresh_outcome'),
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


if __name__ == '__main__':
    folder = Path(sys.argv[1])
    rows = []
    for path in folder.glob('*.json'):
        data = json.loads(path.read_text(encoding='utf-8-sig'))
        if isinstance(data, dict) and 'arm' in data and 'sidebar_trace' in data:
            rows.append(data)
    result = describe(rows)
    (folder / 'sidebar-analysis.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'row_count': result['row_count'], 'valid_count': result['valid_count']}))
