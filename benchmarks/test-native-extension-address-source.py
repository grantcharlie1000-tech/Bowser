"""Static resource-address source contracts, not PowerShell/UIA execution.

The generated-data PowerShell test is separate and runs on Windows CI. These
checks deliberately inspect wiring/fences only and cannot establish provider
behavior, native correctness, latency, or a valid completed measurement series.
"""
from pathlib import Path
import hashlib
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = (ROOT / 'benchmarks/scripts/measure-native-extensions.ps1').read_text()
STORE = (ROOT / 'benchmarks/scripts/probe-native-store.ps1').read_text()


def function(name):
    match = re.search(r'^function ' + re.escape(name) + r'(?:\([^\n]*\))? \{', SCRIPT, re.M)
    assert match, name
    end = re.search(r'^function |^try\{', SCRIPT[match.end():], re.M)
    return SCRIPT[match.start():match.end() + end.start()] if end else SCRIPT[match.start():]


class ResourceAddressSourceContract(unittest.TestCase):
    def test_toolbar_source_is_exact_embedded_source(self):
        source = function('Address-IsToolbarSource')
        self.assertIn("$Source -ceq 'bowser-ui://app/index.html?view=toolbar'", source)
        self.assertIn('pub const START_URL: &str = "bowser-ui://app/index.html?view=toolbar";',
                      (ROOT / 'src/chrome_host/assets.rs').read_text())
        self.assertNotIn('-match', source)
        self.assertNotIn('StartsWith', source)

    def test_document_authority_precedes_control_query(self):
        read = function('Read-OwnedAddress')
        self.assertLess(read.index('Address-OutermostDocument $candidate'), read.index('Address-DocumentSource $candidate'))
        self.assertLess(read.index('Address-IsToolbarSource $source'), read.index('$named=$document.FindAll'))
        self.assertIn('if($matches.Count -ne 1){throw', read)
        self.assertIn('if($edits.Count -ne 1){throw', read)
        self.assertIn('(Address-InDocument $_ $document)', read)
        self.assertIn("$_.Current.Name -ceq 'Address and search bar'", read)
        self.assertIn('$_.Current.ControlType -eq [Windows.Automation.ControlType]::Edit', read)
        self.assertNotIn('$rootElement.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)', read)
        self.assertNotIn('FindFirst(', read)
        self.assertNotIn('Select-Object -First', read)
        self.assertNotIn('AndCondition', read)

    def test_no_source_error_recovery_or_rebinding(self):
        read = function('Read-OwnedAddress')
        self.assertNotRegex(read, r'(?i)\b(?:do|while)\s*\{|Start-Sleep|SetFocus|SetValue')
        self.assertIn('Observe-UnavailableAddress $rootElement $window', read)
        self.assertRegex(read, r"diagnostic-unavailable'\}\}\s*throw\s*\}")
        for identity in ('root_key', 'document_key', 'edit_key'):
            self.assertIn('$Expected.' + identity, read)
        self.assertIn('$Expected.window', read)
        self.assertIn('$Expected.thread', read)

    def test_ancestry_caps_and_document_boundaries(self):
        for name in ('Address-InDocument', 'Address-OutermostDocument'):
            body = function(name)
            self.assertIn('$depth -lt 64', body)
            self.assertIn('RawViewWalker.GetParent', body)
            self.assertRegex(body, r'\[Windows\.Automation\.ControlType\]::Document\)\{[^\n]*return \$false\}')
        read = function('Read-OwnedAddress')
        self.assertIn('$documents.Count -gt 64', read)
        self.assertIn('$named.Count -gt 10000', read)
        self.assertIn('$id.Count -gt 128', function('Address-RuntimeKey'))

    def test_native_source_lookup_keeps_current_identity_fences(self):
        native = SCRIPT[SCRIPT.index('public static class BowserResourceDocument'):SCRIPT.index("\n'@")]
        self.assertIn('GetCurrentPropertyValueEx(30093,true,out value)', native)
        self.assertIn('if(!Same(before,runtimeId))continue;', native)
        self.assertIn('if(!Same(before,after))throw', native)
        self.assertIn('if(!Same(rootBefore,rootRuntimeId))throw', native)
        self.assertIn('if(!Same(rootBefore,rootAfter)||!Same(rootAfter,rootRuntimeId))throw', native)
        self.assertIn('if($legacy -isnot [string] -or !$legacy', function('Address-DocumentSource'))
        self.assertIn('finally { Release(value); }', native)
        self.assertIn('if(matches!=1)throw', native)
        self.assertIn('count>64', native)
        self.assertIn('result.Length>4096', native)
        self.assertIn('thread!=expectedThread', native)
        self.assertIn('freshThread!=expectedThread', native)
        self.assertEqual(native.count('BowserExtensionMeasure.GetForegroundWindow()!=hwnd'), 2)
        for interface in ('Automation', 'Element', 'ElementArray'):
            pattern = r'interface BowserResource' + interface + r' \{(.*?)\n\}'
            source = re.search(pattern, SCRIPT, re.S).group(1)
            original = re.search(pattern.replace('BowserResource', 'BowserNative'), STORE, re.S).group(1)
            self.assertEqual(source.replace('BowserResource', 'BowserNative'), original)

    def test_current_input_guards_and_one_enter_remain(self):
        submit = function('Submit-OwnedFixture')
        self.assertLess(submit.index('Wait-OwnedAddressBeforeBinding $Trace'), submit.index('Bind-OwnedAddressAfterReadiness $ready $Trace'))
        self.assertLess(submit.index('Bind-OwnedAddressAfterReadiness $ready $Trace'), submit.index("SendWait('^l')"))
        self.assertEqual(function('Bind-OwnedAddressAfterReadiness').count('Read-OwnedAddress $null'), 1)
        self.assertIn('$bound=$state.bound', submit)
        self.assertEqual(submit.count('Assert-AddressInput $state'), 2)
        self.assertIn('Start-Sleep -Milliseconds 30', submit)
        self.assertIn('if($state.value_pattern.Current.Value -cne $Text){throw', submit)
        self.assertLess(submit.index("$Trace['keys']+=,$keyRow"), submit.index("$keyRow.dispatch_attempted=$true"))
        self.assertIn('$keyRow.dispatch_returned=$true', submit)
        self.assertIn('catch {$keyRow.error=$_.Exception.Message;throw}', submit)
        after_value = submit[submit.index('if($state.value_pattern.Current.Value -cne $Text)'):]
        self.assertNotIn('Read-OwnedAddress', after_value)
        self.assertNotIn('GetForegroundWindow', after_value)
        self.assertEqual(submit.count("SendWait('{ENTER}')"), 1)
        self.assertNotIn('SetValue(', submit)
        self.assertIn('$fresh=Read-OwnedAddress $State.bound', function('Assert-AddressInput'))
        self.assertIn("if(!$fresh.focused){throw", function('Assert-AddressInput'))
        self.assertIn('Assert-NeutralModifiers', function('Assert-AddressInput'))
        self.assertIn('Assert-AddressWindow $State.window $State.bound.thread', function('Assert-AddressInput'))
        window = function('Assert-AddressWindow')
        for guard in ('MainWindowHandle -ne $Window', '$actualThread -ne $Thread', '$owner -ne $app.Id',
                      "'BowserNativeWindow'", '::IsWindow($Window)', '::IsWindowVisible($Window)',
                      '::IsWindowEnabled($Window)', '::GetForegroundWindow() -ne $Window'):
            self.assertIn(guard, window)

    def test_visible_geometry_rejects_nonfinite_components(self):
        geometry = function('Assert-AddressGeometry')
        self.assertIn('@($Bounds.X,$Bounds.Y,$Bounds.Width,$Bounds.Height)', geometry)
        self.assertIn('[double]::IsNaN($coordinate)', geometry)
        self.assertIn('[double]::IsInfinity($coordinate)', geometry)
        self.assertIn('$Bounds.Width -le 0 -or $Bounds.Height -le 0', geometry)
        read = function('Read-OwnedAddress')
        self.assertLess(read.index('Assert-AddressGeometry $bounds'), read.index('$pattern='))
        generated = (ROOT / 'benchmarks/scripts/test-native-extension-address-guards.ps1').read_text()
        self.assertIn("@('X','Y','Width','Height')", generated)
        self.assertIn('@([double]::NaN,[double]::PositiveInfinity,[double]::NegativeInfinity)', generated)
        self.assertIn('@(0.0,-1.0)', generated)

    def test_failure_source_observations_do_not_discover_or_authorize(self):
        for name in ('Address-ObservedSourceKind', 'Address-ObservedSourcePair', 'Record-AddressObservedRuntime'):
            body = function(name)
            for forbidden in ('FindAll', 'FindFirst', 'GetRuntimeId', 'GetParent', '.Current', 'LegacyValue', 'SendWait', 'SetFocus', 'Start-Sleep'):
                self.assertNotIn(forbidden, body)
        runtime = function('Record-AddressObservedRuntime')
        self.assertIn('$array.Count -gt 128', runtime)
        self.assertIn("catch {}", runtime)
        read = function('Read-OwnedAddress')
        self.assertIn('record_cap=64', read)
        self.assertLess(read.index('if($documents.Count -gt 64){throw'), read.index('foreach($candidate in $documents)'))
        self.assertEqual(read.count("['document_source_diagnostic']"), 1)
        self.assertGreater(read.index("['document_source_diagnostic']"), read.index('return @{focused='))
        self.assertIn("if(Address-IsToolbarSource $source){$matches+=", read)
        source = function('Address-DocumentSource')
        self.assertEqual(source.count('$Document.GetRuntimeId()'), 1)
        self.assertEqual(source.count('$RootElement.GetRuntimeId()'), 1)
        self.assertEqual(source.count('.TryGetCurrentPattern('), 1)
        self.assertEqual(source.count('::LegacyValue('), 1)
        self.assertEqual(source.count('.Current.Value'), 1)
        self.assertIn("$Observation.source_result=$pair.source_result", source)
        self.assertNotIn('$Observation.source_result=$legacy', source)
        self.assertNotIn('$Observation.source_result=$managed', source)
        ancestry = function('Address-OutermostDocument')
        self.assertEqual(ancestry.count('RawViewWalker.GetParent('), 1)
        self.assertEqual(ancestry.count('Address-RuntimeKey '), 2)
        for status in ('eligible', 'excluded-nearer-document', 'unowned-parent-chain', 'depth-limit'):
            self.assertIn(status, ancestry)

    def test_before_binding_schedule_deadline_and_receipts(self):
        wait = function('Wait-OwnedAddressBeforeBinding')
        self.assertIn('schedule_ms=@(0,1000,2000,3000)', wait)
        self.assertIn('$index -lt 4', wait)
        self.assertIn('absolute_budget_ms=10000', wait)
        self.assertIn('$wait=$receipt.schedule_ms[$index]-$clock.Elapsed.TotalMilliseconds', wait)
        self.assertIn("$receipt.observations+=,$observation", wait)
        self.assertLess(wait.index('$receipt.observations+=,$observation'), wait.index('$documents=Observe-AddressBeforeBinding'))
        self.assertIn('$accepted=$clock.Elapsed.TotalMilliseconds;Assert-AddressReadinessTime $accepted', wait)
        self.assertNotIn('.Restart(', wait)
        self.assertNotIn('.Reset(', wait)
        self.assertEqual(wait.count('Observe-AddressBeforeBinding '), 1)
        self.assertIn('$Elapsed -ge 10000', function('Assert-AddressReadinessTime'))

    def test_before_binding_owner_and_source_integrity(self):
        observer = function('Observe-AddressBeforeBinding')
        self.assertEqual(observer.count('Assert-AddressReadinessOwner $Owner $Clock'), 2)
        self.assertIn('Assert-AddressReadinessTime $Clock.Elapsed.TotalMilliseconds', observer)
        self.assertIn('$read.root_key -cne $Owner.root_key', observer)
        self.assertIn('$read.managed -cne $read.native', observer)
        self.assertIn('$ids.Add($read.document_key)', observer)
        self.assertNotIn('catch', observer)
        owner = function('Assert-AddressReadinessOwner')
        self.assertEqual(owner.count('Assert-AddressWindow $Owner.window $Owner.thread'), 2)
        self.assertIn('$rootKey -cne $Owner.root_key', owner)
        self.assertIn('$app.Id -ne $Owner.pid', owner)
        policy = function('Advance-AddressReadiness')
        for required in ('PendingComplete', 'Ready', '$seen.ContainsKey($entry.key)', '$Previous.seen.Keys',
                         'prior document identity disappeared', 'prior document source changed',
                         '$exact.Count -gt 1', '$Previous.seen.ContainsKey($exact[0])', '$Index -eq 3'):
            self.assertIn(required, policy)
        self.assertNotIn('Read-OwnedAddress', policy)
        self.assertNotIn('SendWait', policy)
        self.assertNotIn('$Receipt.records+=,$read', observer)
        self.assertNotIn('$Receipt.records+=,$private', observer)
        strict_reader = SCRIPT[SCRIPT.index('function Read-OwnedAddress'):SCRIPT.index('function Assert-AddressInput')]
        self.assertEqual(hashlib.sha256(strict_reader.encode()).hexdigest(),
                         '81e04e04cfd690134f7197507327c5a84456267fcd262d4e2fc0f48a1e01f554')

    def test_ready_strict_binding_is_once_and_never_retried(self):
        bind = function('Bind-OwnedAddressAfterReadiness')
        self.assertEqual(bind.count('Read-OwnedAddress $null'), 1)
        self.assertNotRegex(bind, r'(?i)\b(?:do|while|for|foreach)\s*[({]')
        self.assertIn('$state.bound.root_key -cne $Ready.owner.root_key', bind)
        self.assertIn('$state.bound.document_key -cne $Ready.document_key', bind)
        self.assertIn('$state.bound.window -ne $Ready.owner.window', bind)
        self.assertIn('$state.bound.thread -ne $Ready.owner.thread', bind)
        self.assertIn('$accepted=$Ready.clock.Elapsed.TotalMilliseconds;Assert-AddressReadinessTime $accepted', bind)
        self.assertIn("catch {$receipt.strict_binding_status='failed';$receipt.status='failed';throw}", bind)
        self.assertIn('NOT comparable to protocol3 or earlier conditioning', SCRIPT)

    def test_measurement_and_failure_accounting_untouched(self):
        # Protocol-4 correction is deliberately confined above this preexisting fence.
        # A future metric/gating change requires an explicit review of this snapshot.
        tail = SCRIPT[SCRIPT.index('function Assert-FrozenExecutable'):]
        self.assertEqual(hashlib.sha256(tail.encode()).hexdigest(),
                         '793c9b88b234b114735f92ddfca06f0d14e2515911fd5b5d38d3520dd4d0f1c7')
        self.assertIn('protocol_version=4;', SCRIPT)
        self.assertIn('source-bound-slow-real-per-key-plus-UIA-readbacks', SCRIPT)


if __name__ == '__main__':
    unittest.main()
