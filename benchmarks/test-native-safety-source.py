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

    def test_unproved_foreign_provider_refuses_before_any_escape(self):
        helper = source("benchmarks/scripts/probe-default-context-cancellation.ps1")
        branch = helper.split("if(providerPid!=(int)browserPid){", 1)[1].split("\n   }", 1)[0]
        self.assertIn("WriteMismatch(", branch)
        self.assertIn('throw new InvalidOperationException("UIA menu provider PID/owned-leaf proof mismatch.")', branch)
        self.assertIn("gpu==null||providerPid!=(int)gpu.Pid||gpuLeaves!=0", branch)
        self.assertIn("ReadGpuLeaf(", branch)
        self.assertIn("catch{", branch)
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

    def test_gpu_category_counts_a_proved_leaf_and_repeats_the_full_inventory(self):
        helper = source("benchmarks/scripts/probe-default-context-cancellation.ps1")
        gpu = helper.split("static GpuLeafWitness GpuShape(", 1)[1].split("static string ProtocolProof(", 1)[0]
        for required in ("ProcessIdProperty", "NativeWindowHandleProperty", "ControlTypeProperty", "BoundingRectangleProperty", "IsEnabledProperty", "HasKeyboardFocusProperty"):
            self.assertIn(required, gpu)
        self.assertIn("Required<", gpu)
        self.assertIn("OptionalBoolean(node,AutomationElement.IsControlElementProperty)", gpu)
        self.assertIn("ItemContainerPattern.Pattern", gpu)
        self.assertIn("VirtualizedItemPattern.Pattern", gpu)
        self.assertIn("before.Signature!=after.Signature", gpu)
        for forbidden in (".Realize(", ".Expand(", ".Invoke(", "current.Name", "node.Current.Name"):
            self.assertNotIn(forbidden, gpu)
        self.assertIn("if(!seen.Add(runtimeKey))", helper)
        self.assertIn("GPU leaf gained a child.", helper)
        self.assertIn("a.Inventory==b.Inventory&&a.GpuLeaves==b.GpuLeaves", helper)
        self.assertIn("Inventory=inventory.ToString(),GpuLeaves=gpuLeaves", helper)
        self.assertIn("gpuLeaf.Matches(c.ProcessId,observedNative,observedType,box,currentId,queued.Item3,c.IsEnabled,c.HasKeyboardFocus)", helper)
        self.assertIn("RecheckGpuTyped(e,gpuLeaf)", helper)
        self.assertIn("var facts=new NativeLeafFacts", helper)
        self.assertIn("if(!NativeLeafPolicy(facts))", helper)
        self.assertIn("var facts=new UiaLeafFacts", helper)
        self.assertIn("if(!UiaLeafPolicy(facts))", helper)
        checker = source("benchmarks/scripts/test-native-menu-helpers.ps1")
        self.assertIn("CheckOwnedLeafPolicy", checker)
        self.assertIn("CheckInventoryPolicy", checker)
        matrix = helper.split("public static void CheckInventoryPolicy(){", 1)[1].split("public static MenuProof ReadMenu(", 1)[0]
        for field in ("count", "order", "parent", "runtime", "pid", "hwnd", "type", "bounds", "gpu_proof"):
            self.assertIn('{"' + field + '",', matrix)
        self.assertIn("SameProof(baseline,fresh)", matrix)
        receipt = helper.split("public static void CheckMembershipPolicy(){", 1)[1].split("static MenuProof SyntheticMenuProof(", 1)[0]
        for field in ("truncated", "browser_pid", "browser_creation", "inventory_digest", "deadline_tick", "duplicate_field"):
            self.assertIn('{"' + field + '",', receipt)
        workflow = source(".github/workflows/store-probe.yml")
        step = workflow.split("id: gpu_ownership_unit", 1)[1].split("      - name:", 1)[0]
        self.assertIn("$test.Count -ne 1", step)
        self.assertIn("& $test[0].FullName engine::extension_probe_live::native_menu_ownership::synthetic_policy_tests", step)
        self.assertNotIn("cargo +stable test", step)

    def test_one_shot_gpu_membership_binds_source_lifetimes_inventory_and_deadline(self):
        rust = source("src/engine/extension_probe_live/native_menu_ownership.rs")
        for required in ("PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE", "view.cast::<IUnknown>()", "environment.cast::<IUnknown>()", "original-source-document-fence-unavailable", "source.membership(deadline, processes, None, Some(gpu_pid))", "read_request(&request_path)? != bytes", "readiness-deadline-changed", "deadline_tick", "GetTickCount64", "create_new(true)", "MOVE_FILE_FLAGS(0)"):
            self.assertIn(required, rust)
        for forbidden in ("remove_file(self.out.join(RESPONSE))", "SetForegroundWindow(", "SetFocus(", "SetSelectedCommandId("):
            self.assertNotIn(forbidden, rust)
        helper = source("benchmarks/scripts/probe-default-context-cancellation.ps1")
        self.assertIn("ExactMembershipResponse($response,$expected)", helper)
        self.assertIn("GetTickCount64() -ge $AuthorityDeadlineTick", helper)
        self.assertLess(helper.index("$response=[BowserDefaultContext]::ReadMembershipResponse"), helper.index("Neutral;$fresh=Menu-State"))
        self.assertLess(helper.index("Neutral;$fresh=Menu-State"), helper.index("::keybd_event(27,0,0"))
        self.assertIn("$gpuGuard -and [BowserDefaultContext]::GetTickCount64() -ge $AuthorityDeadlineTick", helper)
        fence = source("src/engine/webview2.rs").split("fn probe_context_ownership_fence(", 1)[1].split("/// Opens an extension page", 1)[0]
        self.assertIn("self.core.try_borrow_mut()", fence)
        self.assertIn("gate.try_borrow_mut()", fence)
        self.assertIn("current_gate.proof()==Some(proof)", fence)
        live = source("src/engine/extension_probe_live.rs")
        self.assertEqual(live.count("authority.service_once(ownership_deadline,ownership_tick)"), 2)
        self.assertEqual(live.count('assert!(ownership_error.is_none(),"fresh exact GPU membership refused:'), 2)

    def test_raw_browser_unknown_geometry_is_typed_counted_and_not_a_gpu_or_fixture_exception(self):
        helper = source("benchmarks/scripts/probe-default-context-cancellation.ps1")
        policy = helper.split("static BrowserNodeWitness CreateBrowserWitness(", 1)[1].split("static BrowserNodeWitness CaptureBrowserNode(", 1)[0]
        for guard in ("if(!nativeUnknown||type!=ControlType.Pane.Id", "depth==0", "!patternsAbsent", "Supported browser geometry is nonfinite; no Empty exception.", "Mixed unknown-HWND browser Pane requires supported finite non-empty bounds."):
            self.assertIn(guard, policy)
        capture = helper.split("static BrowserNodeWitness CaptureBrowserNode(", 1)[1].split("static void RecheckBrowserNode(", 1)[0]
        self.assertIn("GetCurrentPropertyValue(AutomationElement.ProcessIdProperty,true)", capture)
        self.assertIn("GetCurrentPropertyValue(AutomationElement.NativeWindowHandleProperty,true)", capture)
        self.assertIn("GetCurrentPropertyValue(AutomationElement.BoundingRectangleProperty,true)", capture)
        self.assertIn("NoPattern(node,ItemContainerPattern.Pattern)", capture)
        self.assertIn("NoPattern(node,VirtualizedItemPattern.Pattern)", capture)
        self.assertIn("CountedAncestor(browserNodes,ancestorId)", helper)
        self.assertIn("IsBrowserRow(browserNode,ControlType.Menu.Id)", helper)
        self.assertIn("IsBrowserRow(browserNode,ControlType.MenuItem.Id)", helper)
        self.assertIn("itemId=browserNode.Id();chosen=FixtureBounds(browserNode)", helper)
        self.assertIn("CountedParent(browserNodes,counted,next==null?null:Runtime(next))", helper)
        self.assertIn("browserNode.MatchesOuter(c.ProcessId,observedType,observedNative,box,currentId,queued.Item3)", helper)
        self.assertIn("RecheckBrowserNode(ancestor,counted,browserPid,browser)", helper)
        self.assertNotIn("if(ac.NativeWindowHandle!=0", helper)
        self.assertIn("(NativeUnknown?native==0:native==Native)&&(BoundsUnknown?CanonicalEmpty(bounds):bounds==Bounds)", helper)
        self.assertIn('NativeToken=nativeUnknown?"not-supported"', helper)
        self.assertIn('BoundsToken=boundsUnknown?"not-supported"', helper)
        self.assertIn("!node.NativeUnknown&&!node.BoundsUnknown", helper)
        self.assertIn("!counted.NativeUnknown&&counted.Native!=0", helper)
        self.assertNotIn("UnknownPane", helper)
        self.assertIn("Synthetic mixed property availability change accepted.", helper)
        self.assertIn("AppendInventoryNode(inventory,examined", helper)
        self.assertIn("CheckBrowserStructurePolicy", source("benchmarks/scripts/test-native-menu-helpers.ps1"))
        diagnostic = helper.split("static void WriteBoundsFailure(", 1)[1].split("static string Real(", 1)[0]
        for required in ("observed_outer_bounds", "later_no_default_bounds", "node_identity_before_later_query", "node_identity_after_later_query", "NodeIdentityMetadata(node,parentId!=null)", "catch{beforeFence=", "catch{afterFence=", "json.Length>8192", "FileMode.CreateNew"):
            self.assertIn(required, diagnostic)
        for forbidden in ("keybd_event(", "mouse_event(", "ReadMenu(", "SetForegroundWindow(", "node.Current.Name"):
            self.assertNotIn(forbidden, diagnostic)

    def test_complete_inventory_evidence_uses_only_captured_strings_after_release_or_cleanup(self):
        helper = source("benchmarks/scripts/probe-default-context-cancellation.ps1")
        receipt = helper.split("public static string InventoryReceiptValue(", 1)[1].split("static string ProtocolProof(", 1)[0]
        for needed in ("capture_status", "unavailable", "65536", "128", "InventoryDigest(proof)", "JsonText(proof.Inventory)", "membership_request_digest", "run_nonce", "source_proof", "root_runtime"):
            self.assertIn(needed, receipt)
        for forbidden in ("GetCurrentPropertyValue", "GetWindow", "ReadMenu(", "FileStream", "keybd_event", "mouse_event"):
            self.assertNotIn(forbidden, receipt)
        self.assertIn("$firstInventoryProof=$menu", helper)
        self.assertIn("$fresh=Menu-State;$freshInventoryProof=$fresh", helper)
        self.assertIn("$report.status='observed-owned-popup-and-real-cancellation';Attach-InventoryEvidence;Save-Report", helper)
        cleanup = helper.split("if($report.failure_cleanup -ceq 'exact-single-test-process-terminated'){", 1)[1]
        self.assertIn("Attach-InventoryEvidence", cleanup)
        attach = helper.split("function Attach-InventoryEvidence {", 1)[1].split("function Observe-Topology(", 1)[0]
        self.assertNotIn("::ReadMenu", attach)
        self.assertIn("evidence-serialization-failed", attach)
        checker = source("benchmarks/scripts/test-native-menu-helpers.ps1")
        self.assertIn("$receipt.inventory -cne $inventory", checker)
        self.assertIn("$receipt.utf8_sha256 -cne $digest", checker)

    def test_host_script_telemetry_observes_existing_calls_without_authority_or_extra_script(self):
        live = source("src/engine/app_host_menu_probe/live.rs")
        script = live.split("fn script(", 1)[1].split("fn current(", 1)[0]
        self.assertEqual(script.count("view.ExecuteScript("), 1)
        self.assertIn("Duration::from_secs(10)", script)
        self.assertIn("pump_until_observed(", script)
        self.assertLess(script.index("trace.start(label)"), script.index("view.ExecuteScript("))
        self.assertLess(script.index("call.execute_returned(submitted.is_ok())"), script.index("submitted.unwrap()"))
        self.assertIn("assert!(completed,\"exact COM script completion timed out\")", script)
        trace = source("src/engine/app_host_menu_probe/live/script_trace.rs")
        for primitive in ("ordinal", "ExecuteEntered", "ExecuteReturned", "callback_seen", "callback_ok", "PumpEntered", "PumpExited", "Cell<", "diagnostic_json()", "line.len()<=2048", "self.reports.get()>=4"):
            self.assertIn(primitive, trace)
        for forbidden in ("ExecuteScript(", "PostMessage", "SetFocus", "SetForegroundWindow", "App::", "RefCell", "source.as_", "result.to_string()"):
            self.assertNotIn(forbidden, trace)
        for lane in (live, source("src/engine/app_host_menu_probe/live/note_split.rs")):
            self.assertIn('report("watchdog-before-close")', lane)
            self.assertIn("TraceScope::enter(script_trace.clone())", lane)
        workflow = source(".github/workflows/store-probe.yml")
        self.assertIn("Exact two synthetic script-trace tests were not discovered.", workflow)

    def test_host_loopback_uses_complete_headers_and_one_bounded_transport_budget(self):
        server = source("src/engine/app_host_menu_probe/live/loopback.rs")
        for needed in ("Duration::from_secs(1)", "HEADER_CAP: usize = 8192", "fn remaining(", "checked_sub(clock.elapsed())", "b\"\\r\\n\\r\\n\"", "Outcome::DuplicateKey", "Outcome::LateCompletion", "fn read_header(", "fn write_response(", "fn serve_connection(", "try_lock()", "DIAGNOSTIC_RECORD_CAP: usize = 32"):
            self.assertIn(needed, server)
        self.assertNotIn("write_all(", server)
        self.assertNotIn("format!(\"{error}", server)
        live = source("src/engine/app_host_menu_probe/live.rs")
        self.assertIn("mod loopback;", live)
        self.assertNotIn("struct Loopback", live)
        self.assertIn("receipts.paste_full_responses,receipts.note_full_responses,receipts.destination_full_responses", live)
        self.assertIn("save_transport_diagnostic(&server,&output)", live)
        fixture = source("benchmarks/fixtures/app-host-context-menu/index.html")
        self.assertEqual(fixture.count("fetch("), 1)
        self.assertIn("if(state.exact_body_match)document.documentElement.setAttribute", fixture)
        self.assertIn("state.response_status=r.status", fixture)
        self.assertIn(".catch(error=>", fixture)
        workflow = source(".github/workflows/store-probe.yml")
        self.assertIn("Exact four synthetic loopback tests were not discovered.", workflow)

    def test_rejected_uia_shape_inventory_is_bounded_non_authority_after_irrevocable_failure(self):
        helper = source("benchmarks/scripts/probe-default-context-cancellation.ps1")
        diagnostic = helper.split("static string rejectedInventoryBuffer;", 1)[1].split("public static MenuProof ReadMenu(", 1)[0]
        for required in ("RejectedNodeCap=128", "RejectedRuntimeCap=32", "RejectedOutputCap=65536", "now+750", "if(readiness<local)", "if(ownership!=0&&ownership<local)", "provider-getter-failed", "getter-failed", "not-supported", "wrong-kind", "not-attempted", "foreign-or-unknown-pid-subtree-excluded", "RejectedParent(c,child,runtime", "GetNextSibling(siblingSource)", "GetCurrentPropertyValue(property,true)", "output-cap-details-discarded", "blocking_provider_call_cannot_be_preempted"):
            self.assertIn(required, diagnostic)
        for forbidden in (".Current.Name", "NameProperty", "AutomationElement.FromHandle", "keybd_event(", "mouse_event(", "WriteMembershipRequest(", "FileStream", "Start-Sleep"):
            self.assertNotIn(forbidden, diagnostic)
        self.assertLess(diagnostic.index('if(!RejectedBrowserPid(row.First.Pid,browserPid))'), diagnostic.index('GetFirstChild(work.Item1)'))
        refusal = helper.split('browserNode=CaptureBrowserNode(', 1)[1].split('string nativeToken=', 1)[0]
        self.assertIn('try{BufferRejectedInventory(', refusal)
        self.assertIn('catch{}', refusal)
        self.assertRegex(refusal, r'catch\{\}\s*WriteBoundsFailure\([^;\n]*\);throw;')
        cleanup = helper.split("if($report.failure_cleanup -ceq 'exact-single-test-process-terminated'){", 1)[1]
        self.assertIn('::RejectedInventoryEvidence()', cleanup)
        self.assertIn("'rejected-popup-shapes.json'", cleanup)
        self.assertIn("CheckRejectedInventoryPolicy", source("benchmarks/scripts/test-native-menu-helpers.ps1"))
        workflow = source(".github/workflows/store-probe.yml")
        self.assertLess(workflow.index("name: Preserve bounded native menu proof evidence"), workflow.index("name: Probe default-off native engine menu in actual HTML Bowser App"))

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

    def test_app_probe_pump_is_bounded_and_never_accepts_late_readiness(self):
        app = source("src/engine/app_context_menu_probe.rs")
        pump = app.split("fn pump_until_observed(", 1)[1].split("#[test]", 1)[0]
        self.assertIn("for batch in 1..=32", pump)
        self.assertNotIn("while PeekMessageW", pump)
        self.assertIn("PostQuitMessage(message.wParam.0 as i32)", pump)
        self.assertLess(pump.index("if expired {return finish(PumpExit::DeadlineAfterReady);}"), pump.index("if pump_exit_in_budget(expired,is_ready)"))
        self.assertIn("PumpExit::DeadlineBeforeDispatch", pump)
        self.assertIn("PumpExit::DeadlineAfterDispatch", pump)
        self.assertIn("pump_message_action(start.elapsed()>=timeout,present,message.message==WM_QUIT)", pump)
        self.assertIn("PumpMessageAction::Deadline=>return finish(PumpExit::DeadlineAfterPeek)", pump)
        self.assertIn("if late_before_dispatch {return finish(PumpExit::DeadlineAfterPeek);}", pump)
        self.assertEqual(pump.count("PostQuitMessage("), 1)
        quit_branch = pump.split("PumpMessageAction::Quit=>{", 1)[1].split("PumpMessageAction::Dispatch|", 1)[0]
        self.assertIn("return finish(PumpExit::Quit)", quit_branch)
        self.assertNotIn("DispatchMessageW", quit_branch)
        self.assertEqual(pump.count("let is_ready=ready();"), 1)
        host = source("src/engine/app_host_menu_probe/live.rs")
        self.assertIn("NativeContextProbeHandles,MenuProbeEvent,pump_until", host)
        self.assertNotIn("fn pump_until(", host)
        self.assertNotIn("PeekMessageW", host)
        note = source("src/engine/app_host_menu_probe/live/note_split.rs")
        self.assertIn("pump_until(Duration::from_secs(60)", note)
        self.assertIn("Duration::from_secs(240)", note)
        self.assertIn("phases.pump_after(source_ready)", note)
        self.assertNotIn("SetForegroundWindow", pump)
        self.assertIn('startup.report(app,"primary-pump-refused-before-teardown")', app)
        self.assertIn('startup_timer.report(&app,"existing-180s-timer-before-close")', app)
        diagnostic = app.split("fn report(&self,app:", 1)[1].split("fn pump_exit_in_budget", 1)[0]
        for forbidden in (".ExecuteScript(", ".Navigate(", "SetForegroundWindow(", "MoveFocus(", "WM_CLOSE", "message.wParam", "message.lParam"):
            self.assertNotIn(forbidden, diagnostic)
        self.assertIn("catch_unwind", diagnostic)
        self.assertIn("handles_with_successful_document_available", diagnostic)
        workflow = source(".github/workflows/store-probe.yml")
        experimental = workflow.split("id: experimental_menu_build", 1)[1].split("      - name:", 1)[0]
        self.assertIn("engine::app_context_menu_probe::synthetic_pump_ -- --list", experimental)
        self.assertIn("engine::app_context_menu_probe::synthetic_pump_ -- --nocapture", experimental)
        self.assertIn(".Count -ne 2", experimental)
        for metric in ("in_ready", "active_ready_ms", "max_ready_ms", "in_peek", "active_peek_ms", "max_peek_ms", "in_dispatch", "active_dispatch_ms", "max_dispatch_ms", "active_tab_matches"):
            self.assertIn(metric, diagnostic)

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
