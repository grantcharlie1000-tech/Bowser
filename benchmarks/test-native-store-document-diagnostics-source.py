#!/usr/bin/env python3
"""Source-only boundaries for failure-only Store document identity observations.

The paired PowerShell check compiles exact declarations and runs supplied-data
predicates/serializers. Neither check is native UIA or journey acceptance.
"""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class StoreDocumentDiagnosticSource(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = (ROOT / "benchmarks/scripts/probe-native-store.ps1").read_text()
        cls.diagnostic = cls.source.split("public const int DiagnosticRuntimeLimit=", 1)[1].split(" static void Check(int hr)", 1)[0]
        cls.selector = cls.source.split(" static string ReadLegacyValue(", 1)[1].split("\n}\n'@", 1)[0]

    def test_diagnostic_is_bounded_and_copies_only_existing_scalar_ids(self):
        self.assertIn("128,DiagnosticCandidateLimit=64,DiagnosticJsonLimit=131072", self.diagnostic)
        self.assertIn("value.Length>DiagnosticRuntimeLimit", self.diagnostic)
        self.assertIn("Id=(int[])value.Clone()", self.diagnostic)
        self.assertIn("Candidates.Count>=DiagnosticCandidateLimit", self.diagnostic)
        self.assertIn("json.Length>DiagnosticJsonLimit", self.diagnostic)
        self.assertIn("non-atomic existing-read observations only; not authority", self.diagnostic.replace('\\"', '"'))
        for forbidden in ("GetWindow", "GetClassName", "GetRuntimeId", "FindAll(", "FindFirst(", "GetCurrentProperty", "TryGetCurrentPattern", "SendWait", "SetFocus", "SetForegroundWindow", "GetWindowText", "File.", ".Url", ".Source", ".Name", ".Message"):
            self.assertNotIn(forbidden, self.diagnostic)

    def test_original_queries_and_refusal_predicates_are_unchanged(self):
        for call, count in {
            "BowserStoreSmoke.GetWindowThreadProcessId(": 2,
            "BowserStoreSmoke.GetClassName(": 2,
            "Activator.CreateInstance(": 1,
            "automation.ElementFromHandle(": 1,
            "automation.CreatePropertyCondition(": 1,
            "root.FindAll(": 1,
            "documents.GetLength(": 1,
            "documents.GetElement(": 1,
            "candidate.GetRuntimeId(": 2,
            "candidate.GetCurrentPropertyValueEx(": 1,
        }.items():
            self.assertEqual(self.selector.count(call), count, call)
        for required in (
            'if(runtimeId==null||runtimeId.Length==0||runtimeId.Length>128)throw new InvalidOperationException("Invalid native document identity.");',
            'if(thread==0||pid!=expectedPid||cls.ToString()!="BowserNativeWindow")throw new InvalidOperationException("Native document owner unavailable.");',
            'if(count<0||count>64)throw new InvalidOperationException("Native document inventory exceeded limit.");',
            "if(!Same(before,runtimeId))continue;",
            'if(!Same(before,after))throw new InvalidOperationException("Native document identity changed during property query.");',
            'if(freshPid!=expectedPid||freshThread!=thread||cls.ToString()!="BowserNativeWindow")throw new InvalidOperationException("Native document owner changed.");',
            'if(matches!=1)throw new InvalidOperationException("Native document runtime identity was unavailable or ambiguous.");',
            "candidate.GetCurrentPropertyValueEx(30093,true,out value)",
            "root.FindAll(4,condition,out documents)",
        ):
            self.assertIn(required, self.selector)
        for forbidden in ("Start-Sleep", "Thread.Sleep", "while(", "FindFirst(", "GetWindowText", "GetForegroundWindow", "SetFocus", "SendWait", "Clipboard", "GetParent(", "GetCurrentPropertyValueEx(30005", "GetRuntimeId(out root"):
            self.assertNotIn(forbidden, self.selector)

    def test_stage_hresult_and_partial_coverage_are_preserved(self):
        for required in ("FailureStage", "LastHResultStage", "TerminalHResult", "NativeDocumentCount", "ExactMatchCount", "CurrentCandidateIndex", "CandidateElementsRead", "CandidateRuntimeIdsRead", "MatchedValuesRead", "MatchedRuntimeIdsRechecked", "InventoryComplete", "CaptureIncomplete", "ClassResult", "ClassIsExpected"):
            self.assertIn(required, self.diagnostic)
        for stage, getter in (
            ("RootFromHandle", "automation.ElementFromHandle"),
            ("PropertyCondition", "automation.CreatePropertyCondition"),
            ("FindDocuments", "root.FindAll"),
            ("DocumentCount", "documents.GetLength"),
            ("CandidateElement", "documents.GetElement"),
            ("CandidateRuntimeBefore", "candidate.GetRuntimeId(out before)"),
            ("CandidateValue", "candidate.GetCurrentPropertyValueEx"),
            ("CandidateRuntimeAfter", "candidate.GetRuntimeId(out after)"),
        ):
            self.assertLess(self.selector.index("Stage." + stage), self.selector.index(getter))
        self.assertIn("if(!observation.FailureStage.HasValue)", self.diagnostic)
        self.assertIn("catch(Exception error){Failure(observation,error.HResult);throw;}", self.selector)

    def test_failure_report_does_not_replace_the_original_error_or_change_input(self):
        report = self.source.split("function Record-StoreDocumentFailure(", 1)[1].split("function Document-Source(", 1)[0]
        self.assertLess(report.index("$report.native_document_identity_failure=$failure"), report.index("$json=& $ReadNativeObservation"))
        self.assertIn("$json.Length -gt 131072", report)
        for status in ("native-query-not-attempted", "unavailable-or-over-bound", "capture-failed"):
            self.assertIn(status, report)
        document = self.source.split("function Document-Source(", 1)[1].split("function Source-Matches(", 1)[0]
        self.assertEqual(document.count("::LegacyValue("), 1)
        self.assertEqual(document.count("$Document.GetRuntimeId()"), 2)
        self.assertEqual(document.count("$Document.TryGetCurrentPattern("), 1)
        self.assertIn("if($nativeAttempted)", document)
        self.assertRegex(document, r"Record-StoreDocumentFailure[^\n]+\n  throw")
        for section in (report, document):
            for forbidden in ("SendWait", "SetFocus", "Start-Sleep", "Clipboard", "FindAll(", "FindFirst(", ".Current.Name"):
                self.assertNotIn(forbidden, section)
        self.assertIn("$pattern.Invoke();Start-Sleep -Milliseconds 400", self.source)

    def test_compile_contract_never_invokes_native_selector(self):
        check = (ROOT / "benchmarks/scripts/test-native-store-document-diagnostics.ps1").read_text()
        self.assertIn("Add-Type -TypeDefinition $native[0].Value", check)
        self.assertIn("$observation.ToJson()", check)
        self.assertIn("$same.Invoke(", check)
        self.assertIn("$checked.Invoke(", check)
        self.assertIn("Record-StoreDocumentFailure 'synthetic-stage'", check)
        for forbidden in ("::LegacyValue(", "GetMethod('LegacyValue'", "GetMethod('ReadLegacyValue'", "Start-Process", "::GetWindow", "::ElementFromHandle", "SendWait"):
            self.assertNotIn(forbidden, check)
        self.assertIn("native_provider=NOT_RUN LegacyValue=NOT_CALLED input=NOT_SENT", check)

    def test_windows_compile_and_source_checks_are_wired_to_ci(self):
        workflow = (ROOT / ".github/workflows/store-probe.yml").read_text()
        for path in ("benchmarks/scripts/test-native-store-document-diagnostics.ps1", "benchmarks/test-native-store-document-diagnostics-source.py"):
            self.assertIn("      - '" + path + "'", workflow)
        step = workflow.split("      - name: Compile native helper sources without running probe bodies", 1)[1].split("      - name:", 1)[0]
        self.assertIn("powershell.exe -NoProfile -File ./benchmarks/scripts/test-native-store-document-diagnostics.ps1", step)
        self.assertIn("python ./benchmarks/test-native-store-document-diagnostics-source.py", step)
        self.assertGreaterEqual(step.count("if ($LASTEXITCODE -ne 0)"), 4)


if __name__ == "__main__":
    unittest.main()
