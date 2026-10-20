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

    def test_attached_station_query_is_failure_only_read_only_and_optional(self):
        helper = source("benchmarks/scripts/probe-app-host-context-menu.ps1")
        diagnostic = helper.split("static StationAccessDiagnostic AttachedStationAccess(", 1)[1].split('[DllImport("user32.dll",SetLastError=true)] static extern IntPtr GetProcessWindowStation();', 1)[0]
        for fence in ("ObjectBasicLayoutValid()", "expected==IntPtr.Zero", "before==expected", "after!=IntPtr.Zero&&after==expected", "if(status!=0)", "if(returned!=56)"):
            self.assertIn(fence, diagnostic)
        self.assertEqual(diagnostic.count("QueryObjectBasic("), 1)
        self.assertIn("QueryObjectBasic(expected,0,out data,56,out returned)", diagnostic)
        self.assertLess(diagnostic.index("if(status!=0)"), diagnostic.index("result.granted_access=data.GrantedAccess"))
        self.assertLess(diagnostic.index("if(returned!=56)"), diagnostic.index("result.granted_access=data.GrantedAccess"))
        for forbidden in ("DuplicateHandle(", "CloseHandle(", "CloseWindowStation(", "OpenWindowStation(", "SetProcessWindowStation(", "GetClipboardSequenceNumber(", "OpenClipboard(", "EmptyClipboard(", "GetClipboardData(", "SetClipboardData("):
            self.assertNotIn(forbidden, diagnostic)
        self.assertIn("catch(EntryPointNotFoundException)", diagnostic)
        self.assertIn("catch(DllNotFoundException)", diagnostic)
        self.assertIn("attached_station_access=AttachedStationAccess(station)", helper)
        checker = source("benchmarks/scripts/test-native-menu-helpers.ps1")
        self.assertIn("GetMethod('ObjectBasicLayoutValid',$flags)", checker)
        self.assertNotIn("GetMethod('AttachedStationAccess'", checker)

    def test_rejected_leaf_diagnostics_cannot_authorize_a_provider(self):
        helper = source("benchmarks/scripts/probe-default-context-cancellation.ps1")
        mismatch = helper.split("static void WriteMismatch(", 1)[1].split("public static MenuProof ReadMenu(", 1)[0]
        for property_name in ("IsControlElement", "IsContentElement", "IsEnabled", "IsKeyboardFocusable", "HasKeyboardFocus"):
            self.assertIn("AutomationElement." + property_name + "Property", mismatch)
        self.assertIn("TreeWalker.RawViewWalker.GetFirstChild(node)==null", mismatch)
        self.assertIn("DiagnosticSetLastError(0);IntPtr child=DiagnosticGetWindow(node,5);int childError=Marshal.GetLastWin32Error();", helper)
        self.assertIn("node.GetCurrentPropertyValue(property,true)", helper)
        self.assertIn('"not-supported"', helper.replace('\\"', '"'))
        self.assertNotIn("current.IsControlElement", mismatch)
        self.assertNotIn("current.IsContentElement", mismatch)
        self.assertNotIn("current.IsEnabled", mismatch)
        self.assertNotIn("current.IsKeyboardFocusable", mismatch)
        self.assertNotIn("current.HasKeyboardFocus", mismatch)
        self.assertNotIn("current.Name", mismatch)
        process = source("src/engine/extension_probe_live.rs").split("fn native_context_process_group_diagnostic(", 1)[1].split("/// Observation/cancellation", 1)[0]
        self.assertIn("count>64", process)
        self.assertIn("process-group-browser-mismatch", process)
        self.assertIn("exact-disposable-udf-mismatch", process)
        self.assertIn("COREWEBVIEW2_PROCESS_KIND_GPU", process)
        self.assertIn("create_new(true)", process)
        for forbidden in ("SelectedCommandId", "SetHandled", "SetForegroundWindow", "TerminateProcess", "MoveFocus"):
            self.assertNotIn(forbidden, process)

    def test_production_stale_modal_cancellation_is_exact_and_not_a_test_rescue(self):
        coordinator = source("src/engine/webview2/owned_context_menu.rs")
        continuation = coordinator.split("pub(crate) fn continue_request(", 1)[1].split("pub(crate) fn tick(", 1)[0]
        self.assertLess(continuation.index("if request.phase.get()==Phase::Showing {request.cancel_stale_modal();return;}"), continuation.index("if Instant::now()>=request.deadline.get()"))
        tick = coordinator.split("pub(crate) fn tick(", 1)[1].split("pub(crate) fn drain_destroyed(", 1)[0]
        self.assertIn("if request.phase.get()==Phase::Showing {request.cancel_stale_modal();}", tick)
        self.assertIn("else if Instant::now()>=request.deadline.get()", tick)
        validity = coordinator.split("fn showing_validity(self:", 1)[1].split("fn cancel_stale_modal(", 1)[0]
        for fence in ("try_borrow_mut()", "Rc::ptr_eq(&entry.owner,&self.owner)", "entry.claim==Some(self.token)", "Rc::ptr_eq(pending,self)", "gate.proof()==Some(self.proof)"):
            self.assertIn(fence, validity)
        self.assertNotIn("deadline", validity)
        self.assertNotIn("EndMenu", validity)
        cancel = coordinator.split("fn cancel_stale_modal(", 1)[1].split("fn complete(", 1)[0]
        self.assertEqual(cancel.count("EndMenu()"), 1)
        self.assertIn("exact_modal_popup(&self.owner,root)!=Some((popup,capture))", cancel)
        self.assertLess(cancel.index("modal_cancel_attempted.replace(true)"), cancel.index("EndMenu()"))
        for forbidden in ("self.finish(", "self.complete(", "DestroyMenu(", "SetSelectedCommandId(", "PostMessageW(", "SendMessageW("):
            self.assertNotIn(forbidden, cancel)
        popup = coordinator.split("fn modal_popup_capture(", 1)[1].split("struct Owner {", 1)[0]
        for fence in ("bar.hMenu!=root", "gui.hwndMenuOwner!=owner.hwnd", "gui.flags.contains(GUI_POPUPMENUMODE)", "gui.hwndCapture==owner.hwnd", "gui.hwndCapture==popup", "scan.ambiguous", "scan.complete"):
            self.assertIn(fence, popup)
        self.assertIn("let _modal=ModalRoot(&request.modal_root)", continuation)
        self.assertIn("self.0.set(None)", coordinator)
        helper = source("benchmarks/scripts/probe-app-context-menu-reload.ps1")
        self.assertNotIn("EndMenu", helper)

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
        self.assertIn('receipt_hidden(),"receipt window reappeared after hide preparation;', reload)
        self.assertIn('receipt_hidden(),"receipt window reappeared at final helper boundary;', reload)
        self.assertIn('focused:document.hasFocus()&&document.activeElement===t', reload)
        self.assertIn('assert_eq!(conditioning_script(&h.view,source_script,conditioning_deadline),source', reload)
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
        preparation = reload.split("fn reload_input(", 1)[1].split("fn deny_all(", 1)[0]
        self.assertEqual(preparation.count("SetForegroundWindow(owner)"), 1)
        self.assertEqual(preparation.count("h.controller.MoveFocus("), 1)
        self.assertEqual(preparation.count("t.focus();"), 1)
        self.assertEqual(preparation.count("Instant::now()+Duration::from_secs(3)"), 1)
        self.assertNotIn("pump_until(Duration::from_secs(3)", preparation)
        self.assertNotIn("script(&h.view,", preparation.replace("conditioning_script(&h.view,", ""))
        self.assertIn("for _ in 0..32", reload)
        self.assertIn("PostQuitMessage(message.wParam.0 as i32)", reload)
        self.assertIn("conditioning_source_focus(owner,browser)==Some(source_focus.clone())", reload)
        self.assertRegex(page, r'#\[cfg\(all\(test, feature = "experimental-engine-context-menu"\)\)\]\s+pub\(super\) fn probe_hide_receipt')


if __name__ == "__main__":
    unittest.main(verbosity=2)
