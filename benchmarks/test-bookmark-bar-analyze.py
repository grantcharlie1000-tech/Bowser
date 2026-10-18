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
if __name__=='__main__':unittest.main()
