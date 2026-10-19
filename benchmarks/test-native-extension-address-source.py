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
            self.assertIn('[Windows.Automation.ControlType]::Document){return $false}', body)
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
        self.assertLess(submit.index('Read-OwnedAddress $null'), submit.index("SendWait('^l')"))
        self.assertEqual(submit.count('Read-OwnedAddress $null'), 1)
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

    def test_measurement_and_failure_accounting_untouched(self):
        # Protocol-3 correction is deliberately confined above this preexisting fence.
        # A future metric/gating change requires an explicit review of this snapshot.
        tail = SCRIPT[SCRIPT.index('function Assert-FrozenExecutable'):]
        self.assertEqual(hashlib.sha256(tail.encode()).hexdigest(),
                         '793c9b88b234b114735f92ddfca06f0d14e2515911fd5b5d38d3520dd4d0f1c7')
        self.assertIn('protocol_version=3;', SCRIPT)
        self.assertIn('source-bound-slow-real-per-key-plus-UIA-readbacks', SCRIPT)


if __name__ == '__main__':
    unittest.main()
