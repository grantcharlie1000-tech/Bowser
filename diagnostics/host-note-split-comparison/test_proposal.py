"""Static review guards only, not Windows compilation or native-runtime acceptance."""
from pathlib import Path
import fnmatch
import hashlib
import re
import unittest
import yaml

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
WORKFLOW = ROOT / '.github/workflows/host-note-split-comparison.yml'
EVIDENCE = ROOT.parent / 'evidence'
if not EVIDENCE.is_dir():
    EVIDENCE = ROOT

class ProposalChecks(unittest.TestCase):
    def setUp(self):
        self.workflow = yaml.load(WORKFLOW.read_text(), Loader=yaml.BaseLoader)
        self.observer = (HERE / 'Observer.cs').read_text()
        self.compare = (HERE / 'Compare.cs').read_text()

    def test_one_isolated_free_standard_job(self):
        self.assertEqual(self.workflow['on'], {'push': {
            'branches': ['diagnostics/host-note-split-comparison-20261025'],
            'paths': ['.github/workflows/host-note-split-comparison.yml']}})
        self.assertEqual(self.workflow['permissions'], {'contents': 'read'})
        self.assertEqual(list(self.workflow['jobs']), ['compare'])
        job = self.workflow['jobs']['compare']
        self.assertEqual(job['runs-on'], 'windows-2025-vs2026')
        self.assertEqual(job['timeout-minutes'], '25')
        self.assertEqual(job['defaults']['run']['shell'], 'cmd')
        self.assertIn('github.event.repository.private == false', job['if'])
        self.assertIn('github.run_attempt == 1', job['if'])
        text = WORKFLOW.read_text()
        self.assertNotRegex(text, r'uses:\s*actions/(cache|upload-artifact|download-artifact)')
        self.assertNotIn('secrets.', text)
        self.assertNotRegex(text, r'(?i:[\"\s]-executionpolicy)\s|(?i:set-executionpolicy)|PSExecutionPolicyPreference:')
        for step in job['steps']:
            self.assertNotIn('uses', step)
            self.assertNotIn('continue-on-error', step)
        self.assertIn('Check untouched default native helper policy', job['steps'][0]['name'])
        self.assertNotIn('rustup toolchain install', text)
        self.assertIn('rustup run stable rustc --version', text)

    def test_exact_unchanged_inputs(self):
        clone = self.workflow['jobs']['compare']['steps'][1]['run']
        self.assertIn('baseline fetch --depth=1 --no-tags https://github.com/grantcharlie1000-tech/Bowser.git 74123993475e3db4a26686a1727dfc94ed103255', clone)
        self.assertIn('candidate fetch --depth=1 --no-tags https://github.com/grantcharlie1000-tech/Bowser.git 70b2db35ebbf96ea41eb7a9cb22b5e96146eaab2', clone)
        self.assertNotIn('safe.directory', clone)
        self.assertIn('Test+" --exact --ignored --nocapture --test-threads=1"', self.compare)
        self.assertIn('git.exe', self.compare)
        self.assertIn('hash-object --no-filters', self.compare)
        self.assertIn('status --porcelain --untracked-files=no', self.compare)
        self.assertNotIn('WaitForExit()', self.compare)
        self.assertIn('streams.Wait(10000)', self.compare)
        for path, expected in re.findall(r'\{"([^"\n]+)", "([0-9a-f]{40})"\}', self.compare):
            file = EVIDENCE / path
            self.assertTrue(file.is_file(), path)
            data = file.read_bytes()
            # apply_patch can add one staging newline; runtime Compare accepts only exact
            # canonical Git blob bytes and does not have this preparation-only allowance.
            candidates = [data, data[:-1]] if data.endswith(b'\n\n') and EVIDENCE != ROOT else [data]
            hashes = [hashlib.sha1(b'blob '+str(len(d)).encode()+b'\0'+d).hexdigest() for d in candidates]
            self.assertIn(expected, hashes, path)

    def test_observer_read_only_and_process_bound(self):
        apis = set(re.findall(r'static extern \S+ (\w+)\(', self.observer))
        self.assertEqual(apis, {'OpenProcess','GetProcessId','GetProcessTimes','QueryFullProcessImageName',
            'WaitForSingleObject','CloseHandle','EnumWindows','GetWindowThreadProcessId','IsWindow',
            'IsWindowVisible','GetClassName','GetForegroundWindow','GetWindow','GetAncestor'})
        self.assertIn('QueryAndSynchronize = 0x00101000', self.observer)
        self.assertIn('created.Bits == expectedCreated', self.observer)
        self.assertIn('outside-test-process-details-omitted', self.observer)
        self.assertIn('foreground_in_test_process', self.observer)
        self.assertIn('MaxBytes = 524288, MaxSnapshots = 1024', self.observer)
        self.assertIn('watch.ElapsedMilliseconds < 300000', self.observer)
        self.assertNotRegex(self.observer, r'(?i)GetWindowText|Clipboard\(|SendInput|SendMessage|PostMessage|SetForeground|TerminateProcess|OpenThread|WriteProcess')
        self.assertNotRegex(self.compare, r'\.Kill\(|TerminateProcess|SetForegroundWindow|Set-ExecutionPolicy|(?i:[\"\s]-ExecutionPolicy)\s')

    def test_existing_workflows_do_not_trigger_on_diagnostic_branch(self):
        directory = EVIDENCE / '.github/workflows'
        paths = [p for p in directory.glob('*.yml') if p.name != WORKFLOW.name]
        self.assertEqual(len(paths), 11)
        for path in paths:
            workflow = yaml.load(path.read_text(), Loader=yaml.BaseLoader)
            triggers = workflow.get('on', {})
            self.assertNotIn('workflow_run', triggers, path.name)
            self.assertNotIn('create', triggers, path.name)
            if 'push' in triggers:
                push = triggers['push']
                self.assertIsInstance(push, dict, path.name)
                self.assertIn('branches', push, path.name)
                self.assertFalse(any(fnmatch.fnmatch('diagnostics/host-note-split-comparison-20261025', pattern)
                                     for pattern in push['branches']), path.name)

if __name__ == '__main__':
    unittest.main(verbosity=2)
