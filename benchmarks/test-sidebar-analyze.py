import copy
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


if __name__ == '__main__':
    unittest.main()
