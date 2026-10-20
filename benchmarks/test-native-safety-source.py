#!/usr/bin/env python3
"""Portable fail-closed source contracts, not Windows/native behavior acceptance."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


def source(path):
    return (ROOT / path).read_text(encoding="utf-8")


class NativeSafetySourceContracts(unittest.TestCase):
    def test_observation_uses_one_strict_chromium_backend(self):
        wrapper = source("benchmarks/scripts/probe-native-context-observation.ps1")
        self.assertIn("'probe-default-context-cancellation.ps1') @PSBoundParameters -ProbeKind ExtensionObservation", wrapper)
        self.assertIn("$global:LASTEXITCODE=0", wrapper)
        self.assertIn("exit $LASTEXITCODE", wrapper)
        self.assertNotIn("Add-Type", wrapper)
        helper = source("benchmarks/scripts/probe-default-context-cancellation.ps1")
        self.assertIn("[ValidateSet('DefaultCancellation','ExtensionObservation')]", helper)
        self.assertIn("backend='unhandled-WebView2-Chromium-Views'", helper)
        self.assertIn("'native-context-observation.json'", helper)
        self.assertIn("'default-context-cancellation.json'", helper)
        for guard in ("::ReadMenu(", "::NativeReady(", "::SameProof(", "::Released("):
            self.assertIn(guard, helper)
        self.assertNotIn("continue-on-error", helper)

    def test_foreign_provider_still_refuses_unconditionally(self):
        helper = source("benchmarks/scripts/probe-default-context-cancellation.ps1")
        branch = helper.split("if(providerPid!=(int)browserPid){", 1)[1].split("\n   }", 1)[0]
        self.assertIn("WriteMismatch(", branch)
        self.assertIn('throw new InvalidOperationException("UIA menu provider PID mismatch.")', branch)
        self.assertNotIn("continue", branch)
        diagnostic = helper.split("static string NativeMismatch(", 1)[1].split("static void WriteMismatch(", 1)[0]
        for forbidden in ("GetWindowText", "AutomationElement.FromHandle", "keybd_event", "mouse_event", "SetForegroundWindow", "SetFocus"):
            # Comments may describe forbidden operations; executable call sites may not exist.
            self.assertNotIn(forbidden + "(", diagnostic)
        self.assertIn("GetWindowThreadProcessId(node,out afterPid)", diagnostic)
        self.assertIn("IsChild(popup,node)", diagnostic)
        mismatch = helper.split("static void WriteMismatch(", 1)[1].split("public static MenuProof ReadMenu(", 1)[0]
        self.assertLess(mismatch.index("string nativeMetadata=NativeMismatch("), mismatch.index("bool after=RootFence("))

    def test_clipboard_zero_refusal_precedes_every_initial_mutation(self):
        helper = source("benchmarks/scripts/probe-app-host-context-menu.ps1")
        sequence = helper.split("uint Sequence(){", 1)[1].split("void CloseChecked()", 1)[0]
        self.assertIn("if(value==0)", sequence)
        self.assertIn('Status="sequence-unavailable-left-untouched";throw new InvalidOperationException(', sequence)
        constructor = helper.split("public ClipboardGuard(string generated){", 1)[1].split("public void RequireOwned()", 1)[0]
        self.assertLess(constructor.index("last=Sequence();"), constructor.index("if(!EmptyClipboard())"))
        self.assertIn("first!=0||error!=0", constructor)
        diagnostics = helper.split("public static object ClipboardEnvironment(", 1)[1].split("/// External to Rust", 1)[0]
        for forbidden in ("GetClipboardData(", "SetClipboardData(", "EmptyClipboard(", "GetClipboardSequenceNumber(", "SetProcessWindowStation(", "SetUserObjectSecurity(", "AdjustTokenPrivileges("):
            self.assertNotIn(forbidden, diagnostics)
        self.assertIn("OpenWindowStation(stationName,false,0x0004)", diagnostics)
        self.assertIn("CloseWindowStation(probe)", diagnostics)
        self.assertIn("::AdmissionCloseConfirmed", helper.split("# Admission has already refused", 1)[1])

    def test_managed_helpers_compile_in_one_source_unit(self):
        checker = source("benchmarks/scripts/test-native-menu-helpers.ps1")
        self.assertEqual(checker.count("Add-Type -TypeDefinition"), 1)
        self.assertIn("Add-Type -TypeDefinition $combined -ReferencedAssemblies $references", checker)
        self.assertIn("$definitions.Add($source.Substring($prefix.Length))", checker)
        self.assertIn("$typeNames=[ordered]@{}", checker)
        self.assertIn("!$_.IsGenericMethod", checker)
        self.assertIn("$_.GetParameters()[0].ParameterType -eq [Type]", checker)
        self.assertIn("$sizeOfType[0].Invoke($null,@($objectFlags)) -ne 12", checker)
        self.assertNotIn("::SizeOf($objectFlags)", checker)

    def test_reload_has_one_activation_and_observation_only_rechecks(self):
        reload = source("src/engine/app_context_menu_probe/reload.rs")
        self.assertEqual(reload.count("SetForegroundWindow(owner)"), 1)
        self.assertEqual(reload.count("(hide_receipt.hide_once)()"), 1)
        self.assertIn('receipt_hidden(),"receipt window reappeared after queued events;', reload)
        self.assertIn('receipt_hidden(),"receipt window reappeared at final helper boundary;', reload)
        self.assertIn('focused:document.hasFocus()&&document.activeElement===t', reload)
        self.assertIn('assert_eq!(script(&h.view,source_script),source', reload)
        self.assertIn('receipt_hidden(),"receipt window reappeared before conditioning witness;', reload)
        self.assertIn('assert!(no_native_menu()', reload)
        helper = source("benchmarks/scripts/probe-app-context-menu-reload.ps1")
        self.assertNotIn("extern bool SetForegroundWindow", helper)
        self.assertNotIn("::SetForegroundWindow", helper)
        self.assertNotIn("::keybd_event", helper)
        page = source("src/engine/extension_page.rs")
        seam = page.split("pub(super) fn probe_hide_receipt", 1)[1].split("/// Closes the window", 1)[0]
        for fence in ("alive.get()", "std::process::id()", "GetCurrentThreadId()", '"BowserExtensionPage"', "IsWindowVisible(hwnd)"):
            self.assertIn(fence, seam)
        self.assertEqual(seam.count("ShowWindow(hwnd, SW_HIDE)"), 1)
        self.assertRegex(page, r'#\[cfg\(all\(test, feature = "experimental-engine-context-menu"\)\)\]\s+pub\(super\) fn probe_hide_receipt')


if __name__ == "__main__":
    unittest.main(verbosity=2)
