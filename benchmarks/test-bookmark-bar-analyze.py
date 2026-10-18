"""Synthetic postprocessor contracts, never native measurement rows."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
spec=importlib.util.spec_from_file_location('bar_analysis',Path(__file__).with_name('bookmark-bar-analyze.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

def phase(name):
 snapshot={'PrivateSource':'native','NativePrivateBytes':100,'CoverageErrors':[],'RacedExitCount':0,
           'ProcessDetails':[{'process_id':7,'created_ticks':8,'private_bytes':100,'native_counter_source':'native'}]}
 inventory={'available':True,'identities':[{'pid':7,'created_ticks':8}]}
 counts={'owned_tablist_count':1,'tab_count':2 if name in m.PHASES[:2] else 3,'visible_document_count':4}
 return {'kind':'bar-resource-phase','name':name,'eligible':True,'snapshots':[copy.deepcopy(snapshot) for _ in range(3)],
         'owned_inventory_before':copy.deepcopy(inventory),'owned_inventory_after':copy.deepcopy(inventory),
         'counts_before':copy.deepcopy(counts),'counts_after':copy.deepcopy(counts)}
def row(arm,condition):
 token='a'*32
 def click(label):return {'kind':'bar-owned-point-click','label':label,'point_target_matched':True,'name_matches':True,'click':{'guard_ok':True,'sent_count':3}}
 def visible(stage):return {'kind':'bar-visible','stage':stage,'container_hwnd':10,'container_tid':20,'old_label_count':0,'action_start_qpc':100,'qpc':200}
 def ready(suffix):return {'kind':'bar-fixture-ready','token':token+'_'+suffix,'ack_count':1,'visible_document_count':1,'owned_title_matched':True,'bar_hidden':True,'bar_hwnd_retained':True,'native_navigation_success':True if arm=='bookmarkbar_on' else None}
 trace=[{'kind':'sidebar-owned-activation-click'},click('New tab'),visible('first'),phase(m.PHASES[0]),
        click('Fixture bookmark A'),ready('a'),phase(m.PHASES[1]),
        {'kind':'bar-hidden-fixture-update','same_bookmark_id':1,'content_changed':True,'before_length':10,'after_length':11,'before_mtime_ticks':1,'after_mtime_ticks':2},
        click('New tab'),visible('warm'),phase(m.PHASES[2]),click('Fixture bookmark updated B'),ready('b'),phase(m.PHASES[3]),
        {'kind':'bar-observation-end','complete':True,'click_count':4,'resource_phase_count':4}]
 return {'id':arm+'-1-light-'+condition,'pair':1,'arm':arm,'condition':condition,'token':token,'valid':True,'tree_exit_confirmed':True,
         'input_identity_checks':[{'passed':True},{'passed':True}],'prior_fresh_outcome':{'valid':True} if condition=='reused' else None,'bookmark_bar_trace':trace}
def rows():return [row(arm,condition) for arm in ['bookmarkbar_off','bookmarkbar_on'] for condition in ['fresh','reused']]
class Contracts(unittest.TestCase):
 def test_complete_fixed_plan_and_off_native_unavailable(self):
  result=m.describe(rows(),1000);self.assertEqual(result['eligible_count'],4)
  self.assertIn('unavailable OFF',result['rows'][0]['native_navigation_evidence'])
 def test_on_requires_both_native_segments(self):
  samples=rows();samples[2]['bookmark_bar_trace'][5]['native_navigation_success']=None
  result=m.describe(samples,1000);self.assertFalse(result['rows'][2]['eligible']);self.assertFalse(result['rows'][3]['eligible'])
 def test_wrong_tab_conditioning_or_early_counter_gap_unavailable(self):
  for kind in ['tabs','counter','identity']:
   samples=rows();p=samples[0]['bookmark_bar_trace'][3]
   if kind=='tabs':p['counts_before']['tab_count']=1
   elif kind=='counter':p['snapshots'][0]['CoverageErrors']=['missing']
   else:p['owned_inventory_after']['identities'][0]['created_ticks']=9
   result=m.describe(samples,1000);self.assertIsNone(result['rows'][0]['phase_private_bytes'][m.PHASES[0]])
 def test_complete_container_not_controller_attribution(self):
  result=m.describe(rows(),1000);self.assertIn('not controller ownership',result['rows'][0]['scope'])
 def test_old_dom_label_cannot_be_rescued_by_successful_new_url(self):
  samples=rows();samples[0]['bookmark_bar_trace'][9]['old_label_count']=1
  self.assertFalse(m.describe(samples,1000)['rows'][0]['eligible'])
 def test_same_id_atomic_hidden_stamp_and_content_required(self):
  for key,value in [('same_bookmark_id',2),('content_changed',False),('after_length',10)]:
   samples=rows();update=samples[0]['bookmark_bar_trace'][7];update[key]=value
   if key=='after_length':update['after_mtime_ticks']=1
   self.assertFalse(m.describe(samples,1000)['rows'][0]['eligible'])
 def test_unique_ack_and_source_point_required(self):
  for kind in ['ack','point','partial']:
   samples=rows()
   if kind=='ack':samples[0]['bookmark_bar_trace'][5]['ack_count']=2
   elif kind=='point':samples[0]['bookmark_bar_trace'][1]['point_target_matched']=False
   else:samples[0]['bookmark_bar_trace'][1]['click']['sent_count']=2
   self.assertFalse(m.describe(samples,1000)['rows'][0]['eligible'])
 def test_fixed_order_and_end_marker(self):
  for kind in ['reorder','end']:
   samples=rows();trace=samples[0]['bookmark_bar_trace']
   if kind=='reorder':trace[3],trace[6]=trace[6],trace[3]
   else:trace[-1]['complete']=False
   self.assertFalse(m.describe(samples,1000)['rows'][0]['eligible'])
 def test_duplicate_fresh_or_failed_identity_excludes_reused(self):
  for kind in ['duplicate','identity','failed']:
   samples=rows()
   if kind=='duplicate':samples.append(copy.deepcopy(samples[0]))
   elif kind=='identity':samples[0]['input_identity_checks'][0]['passed']=False
   else:samples[0]['valid']=False
   self.assertFalse(m.describe(samples,1000)['rows'][1]['eligible'])
 def test_failure_raw_values_retained_not_zero(self):
  samples=rows();samples[0]['valid']=False;samples[0]['error']='fixture unavailable'
  result=m.describe(samples,1000);self.assertEqual(result['rows'][0]['error'],'fixture unavailable');self.assertEqual(result['rows'][0]['phase_private_bytes'][m.PHASES[0]],100)
 def test_actual_cli_writes_descriptive_report(self):
  with tempfile.TemporaryDirectory() as directory:
   folder=Path(directory);(folder/'environment.json').write_text(json.dumps({'qpc_frequency':1000}))
   for sample in rows():(folder/(sample['id']+'.json')).write_text(json.dumps(sample))
   p=subprocess.run([sys.executable,str(Path(__file__).with_name('bookmark-bar-analyze.py')),directory],capture_output=True,text=True)
   self.assertEqual(p.returncode,0,p.stderr);self.assertEqual(json.loads(p.stdout),{'row_count':4,'valid_count':4,'eligible_count':4})

def matched_rows(pairs=1):
 result=[]
 for pair in range(1,pairs+1):
  for arm in ('bookmarkbar_baseline_on','bookmarkbar_candidate_on'):
   for condition in ('fresh','reused'):
    sample=row('bookmarkbar_on',condition);sample.update(arm=arm,pair=pair,id=f'{arm}-{pair}-{condition}',disk_before=100,disk_after=150)
    for event in sample['bookmark_bar_trace']:
     if event['kind']=='bar-resource-phase':
      event.update(start_qpc=100,end_qpc=3100)
      for i,snapshot in enumerate(event['snapshots']):
       private=80 if arm=='bookmarkbar_candidate_on' else 100
       snapshot.update(NativePrivateBytes=private,NativeWorkingBytes=200,WorkingSource='native',CpuByPid={'7:8':i*.25},SampledAtUtc=f'2026-10-18T00:00:0{i}Z')
       snapshot['ProcessDetails'][0].update(private_bytes=private,working_set_bytes=200)
    result.append(sample)
 return result

class MatchedComparisonContracts(unittest.TestCase):
 def metric(self,report,name,condition='fresh'):
  return next(x for x in report['paired_comparisons'] if x['condition']==condition and x['metric']==name)
 def test_primary_pairs_keep_ids_deltas_ratios_and_separate_conditions(self):
  report=m.comparison(matched_rows(10),1000)
  self.assertEqual(report['row_count'],40);self.assertEqual(report['eligible_count'],40)
  for condition in ('fresh','reused'):
   metric=self.metric(report,'phase_bar-warm-hidden_private_bytes',condition)
   self.assertEqual(metric['valid_pair_count'],10);self.assertEqual(metric['paired_ratio']['median'],.8)
   self.assertEqual([x['pair'] for x in metric['all_pairs']],list(range(1,11)))
   self.assertTrue(all(x['delta']==-20 and x['included'] for x in metric['all_pairs']))
 def test_missing_peer_unavailable_not_zero_or_unpaired_win(self):
  samples=matched_rows();samples=[s for s in samples if not(s['arm']=='bookmarkbar_candidate_on' and s['condition']=='fresh')]
  report=m.comparison(samples,1000);self.assertEqual(self.metric(report,'phase_bar-warm-hidden_private_bytes')['valid_pair_count'],0)
  self.assertEqual(self.metric(report,'phase_bar-warm-hidden_private_bytes','reused')['valid_pair_count'],0)
 def test_duplicate_pair_arm_cannot_overwrite_into_inference(self):
  samples=matched_rows();samples.append(copy.deepcopy(samples[2]));report=m.comparison(samples,1000)
  self.assertEqual(self.metric(report,'phase_bar-warm-hidden_private_bytes')['valid_pair_count'],0)
  self.assertEqual(self.metric(report,'phase_bar-warm-hidden_private_bytes','reused')['valid_pair_count'],0)
 def test_fresh_stale_snapshot_excludes_reused_but_preserves_descriptive_values(self):
  samples=matched_rows();next(e for e in samples[2]['bookmark_bar_trace'] if e['kind']=='bar-visible' and e['stage']=='warm')['old_label_count']=1
  report=m.comparison(samples,1000);self.assertFalse(report['rows'][2]['eligible']);self.assertFalse(report['rows'][3]['eligible'])
  self.assertEqual(report['rows'][2]['phase_private_bytes'][m.PHASES[-1]],80)
  self.assertTrue(all(v is None for v in report['rows'][2]['metrics'].values()))
 def test_counter_gap_in_one_snapshot_excludes_entire_row_and_conditioning(self):
  samples=matched_rows();samples[2]['bookmark_bar_trace'][3]['snapshots'][0]['CoverageErrors']=['gap']
  report=m.comparison(samples,1000);self.assertFalse(report['rows'][2]['eligible']);self.assertFalse(report['rows'][3]['eligible'])
 def test_navigation_and_partial_input_cannot_become_lower_memory_gain(self):
  for mutation in ('navigation','input','identity','drain'):
   samples=matched_rows()
   if mutation=='navigation':samples[2]['bookmark_bar_trace'][5]['native_navigation_success']=False
   elif mutation=='input':samples[2]['bookmark_bar_trace'][1]['click']['sent_count']=2
   elif mutation=='identity':samples[2]['input_identity_checks'][1]['passed']=False
   else:samples[2]['tree_exit_confirmed']=False
   self.assertEqual(self.metric(m.comparison(samples,1000),'phase_bar-warm-hidden_private_bytes')['valid_pair_count'],0)
 def test_cpu_uses_actual_positive_duration_and_stable_native_identity(self):
  report=m.comparison(matched_rows(),1000)
  self.assertEqual(self.metric(report,'phase_bar-warm-hidden_cpu_core_seconds')['paired_absolute_delta']['median'],0)
  self.assertEqual(self.metric(report,'phase_bar-warm-hidden_cpu_observed_seconds')['all_pairs'][0]['reference'],2)
  self.assertEqual(self.metric(report,'phase_bar-warm-hidden_whole_phase_seconds')['all_pairs'][0]['reference'],3)
 def test_malformed_or_nonpositive_cpu_duration_unavailable_not_exception(self):
  for value in ('unknown','2026-10-18T00:00:00Z',None):
   samples=matched_rows();samples[2]['bookmark_bar_trace'][3]['snapshots'][-1]['SampledAtUtc']=value
   report=m.comparison(samples,1000)
   metric=self.metric(report,'phase_bar-first-shown_cpu_core_seconds')
   self.assertEqual(metric['valid_pair_count'],0)
 def test_cpu_identity_change_and_reset_unavailable(self):
  for changed in ({'9:10':1},{'7:8':-.1}):
   samples=matched_rows();samples[2]['bookmark_bar_trace'][3]['snapshots'][-1]['CpuByPid']=changed
   self.assertEqual(self.metric(m.comparison(samples,1000),'phase_bar-first-shown_cpu_core_seconds')['valid_pair_count'],0)
 def test_working_set_and_disk_separate_from_private_primary(self):
  report=m.comparison(matched_rows(),1000)
  self.assertEqual(self.metric(report,'phase_bar-warm-hidden_working_set_bytes')['paired_ratio']['median'],1)
  self.assertEqual(self.metric(report,'profile_disk_growth_bytes')['all_pairs'][0]['reference'],50)
  self.assertEqual(report['primary_resource_metric'],'phase_bar-warm-hidden_private_bytes')
 def test_unknown_working_set_or_fractional_disk_unavailable(self):
  samples=matched_rows();samples[2]['disk_after']=150.5;samples[2]['bookmark_bar_trace'][3]['snapshots'][0]['NativeWorkingBytes']=None
  report=m.comparison(samples,1000)
  self.assertEqual(self.metric(report,'profile_disk_growth_bytes')['valid_pair_count'],0)
  self.assertEqual(self.metric(report,'phase_bar-first-shown_working_set_bytes')['valid_pair_count'],0)
 def test_nonfinite_malformed_or_boolean_reveal_clocks_exclude_row_and_reused(self):
  for field in ('action_start_qpc','qpc'):
   for value in (float('nan'),float('inf'),float('-inf'),True,'unknown',None):
    samples=matched_rows();samples[2]['bookmark_bar_trace'][2][field]=value
    report=m.comparison(samples,1000)
    self.assertFalse(report['rows'][2]['eligible']);self.assertFalse(report['rows'][3]['eligible'])
    self.assertTrue(all(value is None for value in report['rows'][2]['metrics'].values()))
 def test_invalid_frequency_and_overflowed_reveal_proxy_unavailable(self):
  for value in (float('nan'),float('inf'),True,'unknown',None,0,-1):
   report=m.comparison(matched_rows(),value)
   self.assertEqual(report['eligible_count'],0)
  samples=matched_rows();samples[2]['bookmark_bar_trace'][2]['qpc']=1e308
  report=m.comparison(samples,1e-308);self.assertFalse(report['rows'][2]['eligible'])
 def test_detail_on_never_enters_primary_comparison(self):
  samples=matched_rows()
  report=m.comparison(samples,1000,detail_enabled=True)
  self.assertEqual(report['eligible_count'],0)
  self.assertEqual(self.metric(report,'phase_bar-warm-hidden_private_bytes')['valid_pair_count'],0)
  samples[2]['events']=[{'event':'chrome_host_requested'}]
  report=m.comparison(samples,1000)
  self.assertEqual(self.metric(report,'phase_bar-warm-hidden_private_bytes')['valid_pair_count'],0)
  self.assertFalse(report['rows'][3]['eligible'])
 def test_nonfinite_or_inconsistent_native_private_counter_unavailable(self):
  for value in (float('nan'),float('inf'),True,-1,80.5,99):
   samples=matched_rows();samples[2]['bookmark_bar_trace'][3]['snapshots'][0]['NativePrivateBytes']=value
   self.assertFalse(m.comparison(samples,1000)['rows'][2]['eligible'])
 def test_predeclared_reveal_margin_retains_every_pair(self):
  metric=self.metric(m.comparison(matched_rows(),1000),'first_reveal_proxy_ms')
  self.assertEqual(metric['all_margin_excess_ms'],[{'pair':1,'excess':-25}]);self.assertIn('25ms',metric['predeclared_practical_margin'])

if __name__=='__main__':unittest.main()
