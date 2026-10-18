"""Four-row opt-in bar baseline only: preserve failures, no optimization inference."""
import importlib.util
import json
from pathlib import Path
import sys

_spec = importlib.util.spec_from_file_location('sidebar_analysis', Path(__file__).with_name('sidebar-analyze.py'))
_sidebar = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_sidebar)
PHASES = ['bar-first-shown', 'bar-first-hidden', 'bar-reshown', 'bar-warm-hidden']
KINDS = ['sidebar-owned-activation-click', 'bar-owned-point-click', 'bar-visible', 'bar-resource-phase',
         'bar-owned-point-click', 'bar-fixture-ready', 'bar-resource-phase', 'bar-hidden-fixture-update',
         'bar-owned-point-click', 'bar-visible', 'bar-resource-phase', 'bar-owned-point-click',
         'bar-fixture-ready', 'bar-resource-phase', 'bar-observation-end']


def phase_private(phase):
    adapted = dict(phase, webapps_inventory_before=phase.get('owned_inventory_before'),
                   webapps_inventory_after=phase.get('owned_inventory_after'))
    before, after = phase.get('counts_before') or {}, phase.get('counts_after') or {}
    expected = 2 if phase.get('name') in PHASES[:2] else 3
    if any(counts.get('owned_tablist_count') != 1 or counts.get('tab_count') != expected
           or not isinstance(counts.get('visible_document_count'), int) or counts.get('visible_document_count',0) <= 0 for counts in (before, after)):
        return None
    return _sidebar.phase_private(adapted)


def describe(rows, frequency):
    output = []
    for row in rows:
        trace = row.get('bookmark_bar_trace') or []
        reasons = []
        if row.get('arm') not in ('bookmarkbar_on', 'bookmarkbar_off') or row.get('pair') != 1 or row.get('condition') not in ('fresh', 'reused'):
            reasons.append('outside fixed one-pair ON/OFF fresh/reused plan')
        if not row.get('valid') or not row.get('tree_exit_confirmed') or len(row.get('input_identity_checks') or []) != 2 or not all(x.get('passed') for x in row.get('input_identity_checks') or []):
            reasons.append('row validity/identity/drain unavailable')
        if [entry.get('kind') for entry in trace] != KINDS:
            reasons.append('fixed workload missing, reordered or incomplete')
        clicks = [entry for entry in trace if entry.get('kind') == 'bar-owned-point-click']
        if [entry.get('label') for entry in clicks] != ['New tab', 'Fixture bookmark A', 'New tab', 'Fixture bookmark updated B'] or any(not entry.get('point_target_matched') or not entry.get('name_matches') or not (entry.get('click') or {}).get('guard_ok') or (entry.get('click') or {}).get('sent_count') != 3 for entry in clicks):
            reasons.append('source-bound fixed physical click sequence failed')
        visible = [entry for entry in trace if entry.get('kind') == 'bar-visible']
        if len(visible) != 2 or [entry.get('stage') for entry in visible] != ['first', 'warm'] or visible[1].get('old_label_count') != 0 or visible[0].get('container_hwnd', 0) <= 0 or visible[0].get('container_hwnd') != visible[1].get('container_hwnd') or visible[0].get('container_tid') != visible[1].get('container_tid'):
            reasons.append('fresh snapshot or retained native bar container unavailable')
        probes = [entry for entry in trace if entry.get('kind') == 'bar-fixture-ready']
        on = row.get('arm') == 'bookmarkbar_on'
        if [entry.get('token') for entry in probes] != [row['token'] + '_a', row['token'] + '_b'] or any(entry.get('ack_count') != 1 or entry.get('visible_document_count') != 1 or not entry.get('owned_title_matched') or not entry.get('bar_hidden') or not entry.get('bar_hwnd_retained') or (on and entry.get('native_navigation_success') is not True) for entry in probes):
            reasons.append('bookmark action external readiness/hidden retention or ON native segment failed')
        updates = [entry for entry in trace if entry.get('kind') == 'bar-hidden-fixture-update']
        if len(updates) != 1 or updates[0].get('same_bookmark_id') != 1 or not updates[0].get('content_changed') or (updates[0].get('before_length'), updates[0].get('before_mtime_ticks')) == (updates[0].get('after_length'), updates[0].get('after_mtime_ticks')):
            reasons.append('synthetic same-ID hidden update/cache stamp unavailable')
        ends = [entry for entry in trace if entry.get('kind') == 'bar-observation-end']
        if len(ends) != 1 or ends[0].get('complete') is not True or ends[0].get('click_count') != 4 or ends[0].get('resource_phase_count') != 4:
            reasons.append('complete fixed end marker missing')
        phases = [entry for entry in trace if entry.get('kind') == 'bar-resource-phase']
        if [entry.get('name') for entry in phases] != PHASES:
            reasons.append('fixed resource phase sequence incomplete')
        values = {phase['name']: phase_private(phase) for phase in phases}
        if len(values) != 4 or any(value is None for value in values.values()):
            reasons.append('resource identity/counter/tab-state coverage unavailable')
        proxies = {}
        for entry in visible:
            start, end = entry.get('action_start_qpc'), entry.get('qpc')
            proxies[entry.get('stage')] = (end - start) * 1000 / frequency if start is not None and end is not None and end >= start and frequency > 0 else None
        if set(proxies) != {'first', 'warm'} or any(value is None for value in proxies.values()):
            reasons.append('reveal proxy endpoint unavailable')
        output.append({'id': row['id'], 'pair': row['pair'], 'arm': row['arm'], 'condition': row['condition'], 'valid': row.get('valid'),
                       'error': row.get('error'), 'eligible': not reasons, 'exclusions': reasons,
                       'phase_private_bytes': values, 'reveal_proxy_ms': proxies,
                       'prior_fresh_outcome': row.get('prior_fresh_outcome'),
                       'native_navigation_evidence': 'additional post-flush segments' if on else 'unavailable OFF; external gates only',
                       'scope': 'Opt-in UIA-conditioned 2/2/3/3-tab whole-tree phases; HWND retention not controller ownership; no gain/default-window claim'})
    indexed = {}
    for record in output:
        indexed.setdefault((record['pair'], record['arm'], record['condition']), []).append(record)
    for record in output:
        if len(indexed[(record['pair'], record['arm'], record['condition'])]) != 1:
            record['exclusions'].append('duplicate fixed row identity')
            record['eligible'] = False
    for record in output:
        if record['condition'] != 'reused':
            continue
        fresh = indexed.get((record['pair'], record['arm'], 'fresh'), [])
        if len(fresh) != 1 or not fresh[0]['eligible'] or (record.get('prior_fresh_outcome') or {}).get('valid') is not True:
            record['exclusions'].append('reused state lacks unique valid complete fresh conditioning')
            record['eligible'] = False
    return {'scope': 'Fixed four-row descriptive baseline, not optimization inference or a stable tail',
            'row_count': len(rows), 'valid_count': sum(bool(row.get('valid')) for row in rows),
            'eligible_count': sum(record['eligible'] for record in output), 'rows': output,
            'limits': ['ON/OFF controls use the same immutable EXE; OFF native diagnostic evidence unavailable',
                       'Tabs grow 1 to 2 to 3; phase differences are not exclusive bookmark-bar attribution',
                       'First/warm proxy includes input guards/UIA observations, not presentation/typing',
                       'Pointer left at each last clicked source target, no extra cursor parking; hover work may occur',
                       'Reused state following failed fresh work remains conditioned on failure',
                       'No overhead subtraction, default-window RAM or 20% claim']}


if __name__ == '__main__':
    folder = Path(sys.argv[1])
    rows = [json.loads(path.read_text(encoding='utf-8-sig')) for path in sorted(folder.glob('bookmarkbar_*.json'))]
    environment = json.loads((folder / 'environment.json').read_text(encoding='utf-8-sig'))
    report = describe(rows, environment['qpc_frequency'])
    (folder / 'bookmark-bar-analysis.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: report[key] for key in ('row_count', 'valid_count', 'eligible_count')}))
