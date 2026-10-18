"""Synthetic contracts for descriptive analysis, never substitutes for native results."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('production_analyze', Path(__file__).with_name('production-analyze.py'))
analysis = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analysis)

class ProductionAnalysisTests(unittest.TestCase):
    def test_missing_values_are_not_zero(self):
        self.assertEqual(analysis.dist([None, 2, 4])['median'], 3)
        self.assertIsNone(analysis.dist([])['median'])
        self.assertIsNone(analysis.cpu_delta([]))
        self.assertIsNone(analysis.metrics({'condition': 'fresh'}, 1000)['idle_median_private_bytes'])

    def test_phase_cpu_is_absolute_with_actual_endpoint_duration_and_coverage(self):
        rows = [
            {'SampledAtUtc': '2026-10-18T00:00:00+00:00', 'CpuByPid': {'1:100': 1}},
            {'SampledAtUtc': '2026-10-18T00:00:02.75+00:00', 'CpuByPid': {'1:100': 1.1}},
        ]
        result = analysis.metrics({'idle': rows}, 1000)
        self.assertAlmostEqual(result['idle_cpu_core_seconds'], .1)
        self.assertEqual(result['idle_observed_endpoint_seconds'], 2.75)
        rows[1]['CoverageErrors'] = ['wmi_cpu_unavailable:1']
        self.assertIsNone(analysis.metrics({'idle': rows}, 1000)['idle_cpu_core_seconds'])

    def test_matched_ratios_and_deltas_retain_pair_ids(self):
        rows = []
        for pair in range(1, 11):
            for arm, value in [('baseline_on', 100), ('candidate_on', 80)]:
                rows.append({'pair': pair, 'arm': arm, 'metrics': {'latency': value}})
        result = analysis.paired(rows, 'candidate_on', 'baseline_on', 'latency')
        self.assertEqual(result['valid_pair_count'], 10)
        self.assertEqual(result['paired_absolute_delta']['median'], -20)
        self.assertEqual(result['paired_ratio']['median'], .8)
        self.assertEqual(result['ratio_median_bootstrap95'], [.8, .8])
        self.assertTrue(result['minimum_primary_pair_count_met'])
        # This numerical flag is explicitly not an automatic correctness/noise acceptance decision.
        self.assertTrue(result['ratio_numerically_at_or_below_0_80_not_acceptance'])
        rows.pop()
        result = analysis.paired(rows, 'candidate_on', 'baseline_on', 'latency')
        self.assertEqual(result['missing_pairs'], [10])
        self.assertFalse(result['minimum_primary_pair_count_met'])

    def test_reused_legacy_cpu_excluded_but_drained_identity_policy_distinguished(self):
        row = {'condition': 'reused', 'cpu': {'CoreSeconds': 20}}
        self.assertIsNone(analysis.metrics(row, 1000)['sampled_tree_cpu_core_seconds'])
        row['tree_exit_confirmed'] = True
        self.assertEqual(analysis.metrics(row, 1000)['sampled_tree_cpu_core_seconds'], 20)

    def test_native_memory_never_falls_back_to_working_set_as_private(self):
        self.assertIsNone(analysis.native_private({'PrivateSource': 'wmi_pagefileusage_proxy', 'PrivateBytes': 123}))
        self.assertIsNone(analysis.native_private({'PrivateSource': 'native', 'NativePrivateBytes': 123, 'CoverageErrors': ['missing']}))
        self.assertEqual(analysis.native_private({'PrivateSource': 'native', 'NativePrivateBytes': 123}), 123)
        good = {'PrivateSource': 'native', 'NativePrivateBytes': 123}
        gap = {'PrivateSource': 'unavailable', 'NativePrivateBytes': None}
        result = analysis.metrics({'idle': [good, gap], 'snapshots': [good, gap]}, 1000)
        self.assertIsNone(result['idle_median_private_bytes'])
        self.assertIsNone(result['recorded_window_sampled_peak_private_bytes'])

if __name__ == '__main__':
    unittest.main()
