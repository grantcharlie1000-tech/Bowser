"""Four-row opt-in bar baseline only: preserve failures, no optimization inference."""
import importlib.util
import json
import math
import random
from statistics import median
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
    value = _sidebar.phase_private(adapted)
    if value is None:
        return None
    for snapshot in phase.get('snapshots') or []:
        private = snapshot.get('NativePrivateBytes')
        details = [detail.get('private_bytes') for detail in snapshot.get('ProcessDetails') or []]
        if any(not isinstance(number, (int, float)) or isinstance(number, bool) or not math.isfinite(number) or number < 0 or int(number) != number for number in [private] + details) or sum(details) != private:
            return None
    return value


def describe(rows, frequency, matched=False):
    output = []
    for row in rows:
        trace = row.get('bookmark_bar_trace') or []
        reasons = []
        arms = ('bookmarkbar_baseline_on', 'bookmarkbar_candidate_on') if matched else ('bookmarkbar_on', 'bookmarkbar_off')
        pair_ok = isinstance(row.get('pair'), int) and not isinstance(row.get('pair'), bool) and (1 <= row['pair'] <= 30 if matched else row['pair'] == 1)
        if row.get('arm') not in arms or not pair_ok or row.get('condition') not in ('fresh', 'reused'):
            reasons.append('outside predeclared bar arm/pair/profile plan')
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
        on = row.get('arm', '').endswith('_on')
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
            clocks_valid = all(isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) for value in (start, end, frequency))
            value = (end-start)*1000/frequency if clocks_valid and end >= start and frequency > 0 else None
            proxies[entry.get('stage')] = value if value is not None and math.isfinite(value) and value >= 0 else None
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
    return {'scope': ('Matched same-workload bar validity, not automatic gain acceptance' if matched else 'Fixed four-row descriptive baseline, not optimization inference or a stable tail'),
            'row_count': len(rows), 'valid_count': sum(bool(row.get('valid')) for row in rows),
            'eligible_count': sum(record['eligible'] for record in output), 'rows': output,
            'limits': ['ON/OFF controls use the same immutable EXE; OFF native diagnostic evidence unavailable',
                       'Tabs grow 1 to 2 to 3; phase differences are not exclusive bookmark-bar attribution',
                       'First/warm proxy includes input guards/UIA observations, not presentation/typing',
                       'Pointer left at each last clicked source target, no extra cursor parking; hover work may occur',
                       'Reused state following failed fresh work remains conditioned on failure',
                       'No overhead subtraction, default-window RAM or 20% claim']}


def comparison(rows, frequency, detail_enabled=False):
    spec = importlib.util.spec_from_file_location('production_analysis', Path(__file__).with_name('production-analyze.py'))
    analysis = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(analysis)
    report = describe(rows, frequency, matched=True)
    groups = {}
    for raw, record in zip(rows, report['rows']):
        if detail_enabled or any(event.get('event', '').startswith(('chrome_bar_memory_', 'chrome_host_', 'chrome_snapshot_')) for event in raw.get('events') or []):
            record['eligible'] = False
            record['exclusions'].append('diagnostic-detail observations excluded from primary inference')
    for record in report['rows']:
        if record['condition'] == 'reused':
            fresh = [item for item in report['rows'] if item['pair'] == record['pair'] and item['arm'] == record['arm'] and item['condition'] == 'fresh']
            if len(fresh) != 1 or not fresh[0]['eligible']:
                record['eligible'] = False
                record['exclusions'].append('reused inference conditioned on excluded fresh observation')
    for raw, record in zip(rows, report['rows']):
        metrics = {'phase_' + name + '_private_bytes': record['phase_private_bytes'].get(name) for name in PHASES}
        metrics.update({name + '_reveal_proxy_ms': record['reveal_proxy_ms'].get(name) for name in ('first', 'warm')})
        metrics['profile_disk_growth_bytes'] = None
        metrics.update({'phase_' + name + suffix: None for name in PHASES for suffix in ('_working_set_bytes', '_cpu_core_seconds', '_cpu_observed_seconds', '_whole_phase_seconds')})
        if record['eligible']:
            before, after = raw.get('disk_before'), raw.get('disk_after')
            if all(isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)
                   and 0 <= value <= 2 ** 53 and int(value) == value for value in (before, after)):
                metrics['profile_disk_growth_bytes'] = int(after) - int(before)
            for phase in raw.get('bookmark_bar_trace') or []:
                if phase.get('kind') != 'bar-resource-phase' or phase_private(phase) is None:
                    continue
                prefix = 'phase_' + phase['name']
                snapshots = phase['snapshots']
                working = [snapshot.get('NativeWorkingBytes') for snapshot in snapshots]
                if all(isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and value >= 0 for value in working) and all(snapshot.get('WorkingSource') == 'native' and all(isinstance(detail.get('working_set_bytes'), (int, float)) and not isinstance(detail.get('working_set_bytes'), bool) and math.isfinite(detail['working_set_bytes']) and detail['working_set_bytes'] >= 0 for detail in snapshot['ProcessDetails']) and sum(detail['working_set_bytes'] for detail in snapshot['ProcessDetails']) == snapshot['NativeWorkingBytes'] for snapshot in snapshots):
                    metrics[prefix + '_working_set_bytes'] = median(working)
                cpu = [snapshot.get('CpuByPid') or {} for snapshot in snapshots]
                duration = _sidebar.observed_duration(snapshots)
                stable_cpu = (duration is not None and bool(cpu[0]) and all(set(values) == set(cpu[0]) for values in cpu)
                              and all(set(values) == {str(detail['process_id']) + ':' + str(detail['created_ticks']) for detail in snapshot['ProcessDetails']} for values, snapshot in zip(cpu, snapshots))
                              and all(all(isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and value >= 0 for value in values.values()) for values in cpu)
                              and all(all(cpu[index][pid] >= cpu[index-1][pid] for pid in cpu[0]) for index in range(1, len(cpu))))
                if stable_cpu:
                    metrics[prefix + '_cpu_core_seconds'] = sum(cpu[-1][pid] - cpu[0][pid] for pid in cpu[0])
                    metrics[prefix + '_cpu_observed_seconds'] = duration
                start, end = phase.get('start_qpc'), phase.get('end_qpc')
                if all(isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) for value in (start, end, frequency)) and frequency > 0 and end > start:
                    metrics[prefix + '_whole_phase_seconds'] = (end-start) / frequency
        if not record['eligible']:
            metrics = {name: None for name in metrics}
        record['metrics'] = metrics
        groups.setdefault(record['condition'], []).append(record)
    paired, summaries = [], []
    for condition, group in sorted(groups.items()):
        names = sorted({name for row in group for name in row['metrics']})
        for arm in ('bookmarkbar_baseline_on', 'bookmarkbar_candidate_on'):
            members = [row for row in group if row['arm'] == arm]
            summaries.append({'condition': condition, 'arm': arm, 'metrics': {name: analysis.dist([row['metrics'].get(name) for row in members]) for name in names}})
        for metric in names:
            result = analysis.paired(group, 'bookmarkbar_candidate_on', 'bookmarkbar_baseline_on', metric)
            candidate = {row['pair']: row['metrics'].get(metric) for row in group if row['arm'] == 'bookmarkbar_candidate_on'}
            reference = {row['pair']: row['metrics'].get(metric) for row in group if row['arm'] == 'bookmarkbar_baseline_on'}
            pairs = []
            for pair in sorted(candidate.keys() | reference.keys()):
                a, b = candidate.get(pair), reference.get(pair)
                valid = a is not None and b is not None
                pairs.append({'pair': pair, 'candidate': a, 'reference': b, 'included': valid,
                              'delta': a-b if valid else None, 'ratio': a/b if valid and b > 0 else None})
            if metric in ('first_reveal_proxy_ms', 'warm_reveal_proxy_ms'):
                result['predeclared_practical_margin'] = 'max(25ms,10% of each matched reference); uncertainty crossing margin remains inconclusive'
                result['all_margin_excess_ms'] = [{'pair': pair['pair'], 'excess': pair['delta']-max(25, .1*pair['reference'])} for pair in pairs if pair['included']]
                excess = [item['excess'] for item in result['all_margin_excess_ms']]
                rng = random.Random(20261018)
                boot = sorted(median(rng.choices(excess, k=len(excess))) for _ in range(2000)) if excess else []
                result['margin_excess_median_bootstrap95_ms'] = [boot[49], boot[1949]] if boot else None
                result['margin_assessment'] = ('insufficient valid pairs' if len(excess) < 10 else 'inconclusive against practical margin' if boot[49] <= 0 < boot[1949] else 'margin breach observed' if boot[49] > 0 else 'no detected practical-margin breach in this batch; human review required')
            paired.append(dict(condition=condition, all_pairs=pairs, **result))
    report['eligible_count'] = sum(record['eligible'] for record in report['rows'])
    report.update(scope='Matched detail-OFF opt-in bar comparison, not automatic gain/default-window acceptance',
                  primary_resource_metric='phase_bar-warm-hidden_private_bytes', summary=summaries, paired_comparisons=paired,
                  comparison_limits=['Ten valid matched pairs separately for EACH metric and fresh/reused condition',
                                     'Final hidden3-tab private commit primary; first hidden corroborates, all shown/reveal metrics are regression checks',
                                     'Working set includes shared pages and may fall by paging without lower private commit; report disk and CPU absolute intervals separately',
                                     'Same runtime/actions/UIA/hover/tab conditioning, no extra dwell/pressure or overhead subtraction',
                                     'No stable ten-pair p95/default-window20%/physical-RAM/per-view ownership claim; old input hold separate'])
    report['limits'] = report['comparison_limits']
    return report


if __name__ == '__main__':
    folder = Path(sys.argv[1])
    rows = [json.loads(path.read_text(encoding='utf-8-sig')) for path in sorted(folder.glob('bookmarkbar_*.json'))]
    environment = json.loads((folder / 'environment.json').read_text(encoding='utf-8-sig'))
    report = (comparison(rows, environment['qpc_frequency'], environment.get('workload_settings', {}).get('bookmark_bar_hint_detail', False)) if environment.get('workload_settings', {}).get('bookmark_bar_compare') else describe(rows, environment['qpc_frequency']))
    (folder / 'bookmark-bar-analysis.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: report[key] for key in ('row_count', 'valid_count', 'eligible_count')}))
