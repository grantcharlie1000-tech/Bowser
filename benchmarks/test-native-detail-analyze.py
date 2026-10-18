import importlib.util, unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('native_detail',Path(__file__).with_name('native-detail-analyze.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
def row(event,view='all',pid=0,private=0,count=1,elapsed=1,tab=''):
    return dict(event=event,view=view,pid=str(pid),private_bytes=str(private),count=str(count),elapsed_ms=str(elapsed),tab_id=tab)
class DetailContract(unittest.TestCase):
    def test_shared_pid_deduplicated_across_views(self):
        data=m.summarize([row('chrome_snapshot_requested'),row('renderer_private_inventory',pid=12,private=100),row('chrome_renderer_private','toolbar',12,100),row('chrome_renderer_private','tabs',12,100),row('chrome_snapshot_completed')])
        snapshot=data['renderer_snapshots'][0]
        self.assertTrue(snapshot['eligible']);self.assertEqual(snapshot['deduplicated_fixed_chrome_private_bytes'],100)
        self.assertEqual(snapshot['fixed_view_pids'],{'toolbar':[12],'tabs':[12]})
    def test_missing_counter_or_completion_ineligible(self):
        for rows in [[row('chrome_snapshot_completed')],[row('chrome_snapshot_requested')],[row('renderer_private_inventory_unavailable',pid=12),row('chrome_snapshot_completed')]]:
            self.assertFalse(m.summarize(rows)['renderer_snapshots'][0]['eligible'])
    def test_mapped_pid_missing_inventory_rejects_whole_snapshot(self):
        result=m.summarize([row('chrome_snapshot_requested'),row('renderer_private_inventory',pid=12,private=100),row('chrome_renderer_private','toolbar',13,200),row('chrome_snapshot_completed')])['renderer_snapshots'][0]
        self.assertFalse(result['eligible']);self.assertEqual(result['mapped_pids_missing_inventory'],[13])
        self.assertIsNone(result['deduplicated_fixed_chrome_private_bytes']);self.assertIsNone(result['all_renderer_private_bytes'])
    def test_completion_without_request_rejected(self):
        result=m.summarize([row('renderer_private_inventory',pid=12,private=100),row('chrome_renderer_private','toolbar',12,100),row('chrome_snapshot_completed')])['renderer_snapshots'][0]
        self.assertFalse(result['eligible']);self.assertIsNone(result['deduplicated_fixed_chrome_private_bytes'])
    def test_spans_and_counts_keep_tab_association(self):
        result=m.summarize([row('configure_filter_registration_started',elapsed=10,tab='3'),row('configure_filter_registration_finished',elapsed=25,tab='3',count=42)])
        self.assertEqual(result['configure_spans_ms']['configure_filter_registration']['median'],15)
        self.assertEqual(result['successful_filter_registrations'][0],{'tab_id':'3','successful_registrations':42})
if __name__=='__main__':unittest.main()
