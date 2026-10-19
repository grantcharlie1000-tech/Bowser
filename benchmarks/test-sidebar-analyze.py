import copy
import json
import subprocess
import sys
import tempfile
import importlib.util
from pathlib import Path
import unittest
spec = importlib.util.spec_from_file_location('sidebar_analyze', Path(__file__).with_name('sidebar-analyze.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def phase():
    snapshot = {'CoverageErrors': [], 'RacedExitCount': 0, 'PrivateSource': 'native', 'NativePrivateBytes': 100,
                'ProcessDetails': [{'process_id': 7, 'created_ticks': 8, 'private_bytes': 100, 'native_counter_source': 'native'}]}
    return {'eligible': True, 'name': 'A-ready', 'snapshots': [copy.deepcopy(snapshot) for _ in range(3)],
            'webapps_root_identities': [{'pid': 7, 'created_ticks': 8}],
            'webapps_inventory_before': {'available': True, 'identities': [{'pid': 7, 'created_ticks': 8}]},
            'webapps_inventory_after': {'available': True, 'identities': [{'pid': 7, 'created_ticks': 8}]}}


class Contracts(unittest.TestCase):
    def test_complete_native_phase(self):
        self.assertEqual(module.phase_private(phase()), 100)

    def test_any_snapshot_gap_rejects_entire_phase(self):
        p = phase(); p['snapshots'][1]['CoverageErrors'] = ['missing']
        self.assertIsNone(module.phase_private(p))

    def test_proxy_or_raced_exit_is_not_native_coverage(self):
        for key, value in [('PrivateSource', 'wmi_pagefileusage_proxy'), ('RacedExitCount', 1)]:
            p = phase(); p['snapshots'][0][key] = value
            self.assertIsNone(module.phase_private(p))

    def test_missing_webapps_identity_rejects(self):
        p = phase(); p['webapps_inventory_before']['identities'][0]['created_ticks'] = 9
        self.assertIsNone(module.phase_private(p))

    def test_early_snapshot_missing_root_rejects_whole_phase(self):
        p = phase(); p['snapshots'][0]['ProcessDetails'] = [{'process_id': 1, 'created_ticks': 2, 'private_bytes': 100, 'native_counter_source': 'native'}]
        self.assertIsNone(module.phase_private(p))

    def test_unavailable_inventory_rejects_phase(self):
        p = phase(); p['webapps_inventory_after']['available'] = False
        self.assertIsNone(module.phase_private(p))

    def test_no_empty_or_incomplete_native_details(self):
        p = phase(); p['snapshots'][0]['ProcessDetails'][0]['private_bytes'] = None
        self.assertIsNone(module.phase_private(p))

    def test_invalid_row_never_contributes_resource_pair(self):
        rows = []
        for arm in ['sidebar_off', 'sidebar_on']:
            rows.append({'id': arm, 'pair': 1, 'arm': arm, 'condition': 'fresh', 'valid': arm == 'sidebar_off',
                         'tree_exit_confirmed': True, 'input_identity_checks': [{'passed': True}, {'passed': True}],
                         'sidebar_trace': [dict(phase(), kind='resource-phase')]})
        result = module.describe(rows)
        self.assertEqual(result['row_count'], 2)
        self.assertEqual(result['valid_count'], 1)
        self.assertFalse(result['paired_logging_private_delta_bytes'])





def row(pair, arm, condition='fresh', value=100):
    identity = {'pid':7, 'created_ticks':8}
    snapshots = []
    for i in range(3):
        snapshots.append({'PrivateSource':'native','NativePrivateBytes':value,'CoverageErrors':[],
            'RacedExitCount':0,'ProcessDetails':[{'process_id':7,'created_ticks':8,'private_bytes':value,'native_counter_source':'native'}],
            'CpuByPid':{'7:8':1+i/10},'SampledAtUtc':f'2026-10-18T00:00:0{i}+00:00'})
    phase = {'kind':'resource-phase','name':'closed-cycle-3','eligible':True,'snapshots':snapshots,
        'webapps_inventory_before':{'available':True,'identities':[identity]},
        'webapps_inventory_after':{'available':True,'identities':[identity]}}
    return {'id':f'{arm}-{pair}-{condition}','pair':pair,'arm':arm,'condition':condition,
        'valid':True,'sidebar_scenario':'settled','prior_fresh_outcome':{'valid':True},
        'tree_exit_confirmed':True,'input_identity_checks':[{'passed':True},{'passed':True}],
        'sidebar_observation':{'complete':True,'environment_requests':1,'controller_requests':2,'requests':[{'app':app,'configured':1,'installed':1,'replaced':0,'close_requested':1,'close_succeeded':1,'close_failed':0,'native_navigation_success':1} for app in [1,2]]},'sidebar_trace':[phase,
            {'kind':'rail-toggle','name':'Fixture B','invoked':True,'dispatch_guard':{'qpc':100,'refused':False}},
            {'kind':'visible-app','name':'Fixture B','app_id':2,'qpc':150,'ack_count':1,'visible_document_count':1}]}

def matrix():
    return [row(pair, arm, condition, value) for pair in range(1,11) for condition in ['fresh','reused']
            for arm,value in [('sidebar_baseline_on',100),('sidebar_candidate_on',80)]]

def primary(result, condition='fresh'):
    return next(x for x in result['paired_comparisons'] if x['condition']==condition
                and x['metric']=='phase_closed-cycle-3_private_bytes')

class ComparisonContracts(unittest.TestCase):
    def test_actual_cli_writes_comparison_and_retains_counts(self):
        with tempfile.TemporaryDirectory() as directory:
            folder=Path(directory)
            (folder/'environment.json').write_text(json.dumps({'qpc_frequency':1000,'workload_settings':{'sidebar_lifecycle_compare':True}}))
            for index, sample in enumerate(matrix()):
                (folder/f'row-{index}.json').write_text(json.dumps(sample))
            completed=subprocess.run([sys.executable,str(Path(__file__).with_name('sidebar-analyze.py')),directory],capture_output=True,text=True)
            self.assertEqual(completed.returncode,0,completed.stderr)
            self.assertEqual(json.loads(completed.stdout),{'row_count':40,'valid_count':40})
            result=json.loads((folder/'sidebar-analysis.json').read_text())
            self.assertEqual(primary(result)['valid_pair_count'],10)

    def test_ten_pairs_per_each_condition_and_descriptive_uncertainty(self):
        result=module.comparison(matrix(),1000)
        for condition in ['fresh','reused']:
            p=primary(result,condition); self.assertEqual(p['valid_pair_count'],10)
            self.assertTrue(p['minimum_primary_pair_count_met']);self.assertEqual(p['paired_ratio']['median'],.8)
            self.assertEqual(p['ratio_median_bootstrap95'],[.8,.8])
    def test_missing_lifecycle_cannot_contribute_otherwise_valid_cpu_or_proxy(self):
        rows=matrix();rows[1]['sidebar_observation']['complete']=False
        r=module.comparison(rows,1000);self.assertEqual(primary(r)['valid_pair_count'],9)
        record=r['rows'][1];self.assertTrue(record['comparison_exclusions'])
        self.assertFalse(any(v is not None for v in record['metrics'].values()))
    def test_coverage_gap_excludes_whole_private_phase(self):
        rows=matrix();rows[1]['sidebar_trace'][0]['snapshots'][0]['CoverageErrors']=['missing']
        self.assertEqual(primary(module.comparison(rows,1000))['valid_pair_count'],9)
    def test_failed_fresh_conditioning_excludes_reused_not_repaired(self):
        rows=matrix();rows[3]['prior_fresh_outcome']['valid']=False
        r=module.comparison(rows,1000);self.assertEqual(primary(r,'reused')['valid_pair_count'],9)
        self.assertTrue(r['rows'][3]['comparison_exclusions'])
    def test_duplicate_pair_arm_excludes_all_duplicates(self):
        rows=matrix();rows.append(row(1,'sidebar_candidate_on'))
        r=module.comparison(rows,1000);self.assertEqual(primary(r)['valid_pair_count'],9)
        self.assertTrue(r['rows'][1]['comparison_exclusions']);self.assertTrue(r['rows'][-1]['comparison_exclusions'])
    def test_cpu_churn_is_unavailable_not_lifetime_cpu(self):
        rows=matrix();rows[1]['sidebar_trace'][0]['snapshots'][1]['CpuByPid']['9:9']=30
        r=module.comparison(rows,1000);record=r['rows'][1]
        self.assertNotIn('phase_closed-cycle-3_cpu_core_seconds',record['metrics'])
        baseline=r['rows'][0]['metrics'];self.assertAlmostEqual(baseline['phase_closed-cycle-3_cpu_core_seconds'],.2)
        self.assertEqual(baseline['phase_closed-cycle-3_cpu_observed_seconds'],2)
    def test_missing_cpu_for_one_owned_identity_is_unavailable(self):
        rows=matrix()
        for snapshot in rows[1]['sidebar_trace'][0]['snapshots']:
            snapshot['ProcessDetails'].append({'process_id':9,'created_ticks':10,'private_bytes':0,'native_counter_source':'native'})
        result=module.comparison(rows,1000)
        self.assertNotIn('phase_closed-cycle-3_cpu_core_seconds',result['rows'][1]['metrics'])
        self.assertEqual(primary(result)['valid_pair_count'],10)
    def test_profile_disk_growth_is_independent_signed_endpoint(self):
        rows=matrix();rows[1]['disk_before']=100;rows[1]['disk_after']=90
        result=module.comparison(rows,1000)
        self.assertEqual(result['rows'][1]['metrics']['profile_disk_growth_bytes'],-10)
    def test_complete_but_discarded_candidate_is_excluded_not_credited_lower_ram(self):
        rows=matrix();ledger=rows[1]['sidebar_observation'];ledger['controller_requests']=3
        ledger['requests'].append(dict(ledger['requests'][0]))
        result=module.comparison(rows,1000)
        self.assertEqual(primary(result)['valid_pair_count'],9)
        self.assertEqual(result['rows'][1]['phase_private_bytes']['closed-cycle-3'],80)
        self.assertTrue(result['rows'][1]['comparison_exclusions'])
        self.assertTrue(all(value is None for value in result['rows'][1]['metrics'].values()))
    def test_each_candidate_navigation_close_and_logical_identity_is_required(self):
        for key,value in [('app',1),('configured',0),('installed',0),('replaced',1),('close_requested',0),('close_succeeded',0),('close_failed',1),('native_navigation_success',0)]:
            rows=matrix();rows[1]['sidebar_observation']['requests'][1][key]=value
            self.assertEqual(primary(module.comparison(rows,1000))['valid_pair_count'],9,key)
    def test_baseline_duplicate_count_is_not_assumed_or_rejected(self):
        rows=matrix();ledger=rows[0]['sidebar_observation'];ledger['controller_requests']=3
        ledger['requests'].append(dict(ledger['requests'][0]))
        self.assertEqual(primary(module.comparison(rows,1000))['valid_pair_count'],10)
    def test_invalid_cpu_intervals_remain_unavailable_without_throw(self):
        for stamp in [None,'bad','2026-10-18T00:00:00+00:00','2026-10-17T00:00:00+00:00']:
            rows=matrix();rows[1]['sidebar_trace'][0]['snapshots'][-1]['SampledAtUtc']=stamp
            result=module.comparison(rows,1000)
            self.assertNotIn('phase_closed-cycle-3_cpu_core_seconds',result['rows'][1]['metrics'])
            self.assertEqual(primary(result)['valid_pair_count'],10)
    def test_every_paired_value_delta_and_ratio_retains_pair_identity(self):
        result=module.comparison(matrix(),1000);pairs=primary(result)['all_pairs']
        self.assertEqual([p['pair'] for p in pairs],list(range(1,11)))
        self.assertEqual(pairs[0],{'pair':1,'candidate':80,'reference':100,'included':True,'delta':-20,'ratio':.8})
    def test_wrong_fresh_candidate_also_excludes_reused_inference(self):
        rows=matrix();rows[1]['sidebar_observation']['requests'][1]['app']=1
        result=module.comparison(rows,1000)
        self.assertEqual(primary(result,'reused')['valid_pair_count'],9)
        self.assertEqual(result['rows'][3]['phase_private_bytes']['closed-cycle-3'],80)
        self.assertTrue(result['rows'][3]['comparison_exclusions'])
    def test_missing_duplicate_or_bad_identity_fresh_cannot_condition_reused(self):
        for kind in ['missing','duplicate','bad-identity','bad-drain','bad-ledger']:
            rows=matrix()
            if kind=='missing':del rows[1]
            elif kind=='duplicate':rows.append(copy.deepcopy(rows[1]))
            elif kind=='bad-identity':rows[1]['input_identity_checks'][0]['passed']=False
            elif kind=='bad-drain':rows[1]['tree_exit_confirmed']=False
            else:rows[1]['sidebar_observation']['complete']=False
            self.assertEqual(primary(module.comparison(rows,1000),'reused')['valid_pair_count'],9,kind)
    def test_powershell_integer_valued_double_byte_sums_are_valid(self):
        rows=matrix();rows[1]['disk_before']=100.0;rows[1]['disk_after']=90.0
        result=module.comparison(rows,1000)
        self.assertEqual(result['rows'][1]['metrics']['profile_disk_growth_bytes'],-10)
        self.assertEqual(primary(result)['valid_pair_count'],10)
    def test_unknown_fractional_or_imprecise_disk_values_are_unavailable(self):
        for value in [float('nan'),float('inf'),-1,1.25,None,True,2**53+2]:
            rows=matrix();rows[1]['disk_before']=value;rows[1]['disk_after']=100.0
            result=module.comparison(rows,1000)
            self.assertNotIn('profile_disk_growth_bytes',result['rows'][1]['metrics'])
    def test_failure_is_retained_and_never_replaced_with_zero(self):
        rows=matrix();rows[1]['valid']=False;rows[1]['error']='fixture failed'
        r=module.comparison(rows,1000);self.assertEqual(primary(r)['valid_pair_count'],9)
        self.assertEqual(r['rows'][1]['error'],'fixture failed');self.assertTrue(all(x is None for x in r['rows'][1]['metrics'].values()))


if __name__ == '__main__':
    unittest.main()
