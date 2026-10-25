"""Non-executing installer source guardrails; Windows helper tests are installer-safety.ps1."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "Open-Bowser.ps1").read_text(encoding="utf-8")


class InstallerContract(unittest.TestCase):
    def test_pin_minimum_is_after_selection_and_before_install(self):
        minimum = SOURCE.index("try { Assert-BowserPinMinimum")
        self.assertGreater(minimum, SOURCE.index("$pin = $remote"))
        self.assertLess(minimum, SOURCE.index("$localAppData ="))
        self.assertIn("exit 23", SOURCE[minimum : SOURCE.index("$localAppData =")])
        self.assertIn("[uint32]$MinimumRevision = 0", SOURCE)

    def test_only_fixed_pin_and_fixed_release_origin(self):
        self.assertIn("https://raw.githubusercontent.com/grantcharlie1000-tech/Bowser/main/stable/release.json", SOURCE)
        self.assertIn("[string]$value.repository -cne 'grantcharlie1000-tech/Bowser'", SOURCE)
        self.assertIn("[string]$value.asset -cne 'Bowser.exe'", SOURCE)
        self.assertIn('https://github.com/$($pin.repository)/releases/download/$($pin.tag)/$($pin.asset)', SOURCE)

    def test_download_is_bounded_before_writing(self):
        self.assertNotIn("Invoke-WebRequest", SOURCE)
        self.assertIn("$request.AllowAutoRedirect = $false", SOURCE)
        self.assertIn("$request.Timeout = 15000", SOURCE)
        self.assertIn("$request.ReadWriteTimeout = 30000", SOURCE)
        self.assertIn("$hop -le 5", SOURCE)
        self.assertIn("if ($Hop -eq 5)", SOURCE)
        self.assertIn("Assert-PlainHttpsUri -Uri $next", SOURCE)
        self.assertLess(SOURCE.index("if ($total -gt $Limit)"), SOURCE.index("$Destination.Write($buffer"))
        self.assertIn("$response.ContentLength -gt $Limit", SOURCE)
        self.assertIn("-Limit 16384", SOURCE)
        self.assertIn("-Limit $expectedBytes -Destination $download", SOURCE)
        self.assertIn("[IO.FileMode]::CreateNew", SOURCE)

    def test_integrity_is_checked_before_install(self):
        install = SOURCE.index("Install-StagedFile -Staged $staged")
        self.assertLess(SOURCE.index("$downloadHash -ne $expectedHash"), install)
        self.assertLess(SOURCE.index("$size -ne $expectedBytes"), install)
        self.assertLess(SOURCE.index("Test-BowserSignature -Path $download"), install)
        self.assertLess(SOURCE.index("Test-BowserSignature -Path $staged"), install)
        self.assertIn("$signature.Status -notin @('Valid', 'NotSigned')", SOURCE)
        self.assertIn("Test-BowserSignature -Path $candidateExe", SOURCE)
        self.assertIn("Test-BowserExecutable -Path $candidateExe", SOURCE)
        self.assertIn("[long]$value.bytes -gt 268435456", SOURCE)
        self.assertIn("$Bytes.Length -gt 16384", SOURCE)

    def test_paths_and_existing_files_are_not_blindly_overwritten(self):
        self.assertIn("[IO.FileAttributes]::ReparsePoint", SOURCE)
        for target in ("$localAppData", "$InstallDirectory", "$candidateExe", "$Staged", "$Destination", "$notePath"):
            self.assertIn("Assert-NoReparsePath -Path " + target, SOURCE)
        self.assertIn("Get-ManagedInstallHash -Directory $dir", SOURCE)
        self.assertIn("$previousHash = $managedHash", SOURCE)
        self.assertIn("-PreviousHash $previousHash", SOURCE)
        self.assertIn("(Get-LowerSha256 -Path $Destination) -ne $PreviousHash", SOURCE)
        self.assertNotIn("Copy-Item -LiteralPath $download -Destination $staged -Force", SOURCE)
        self.assertNotRegex(SOURCE, r"(?i)Stop-Process|taskkill")

    def test_destination_rollback_and_shortcut_arguments_are_guarded(self):
        self.assertIn("$version -lt $info.Version", SOURCE)
        self.assertIn("An existing managed installation changed since its record", SOURCE)
        self.assertIn("(Get-Item -LiteralPath $exe).Length -gt 268435456", SOURCE)
        self.assertIn("[uint32]$Pin.revision -lt $info.Revision", SOURCE)
        self.assertLess(SOURCE.index("try { Assert-PinNotOlderThanManagedInstalls"), SOURCE.index("$ready = $false"))
        self.assertIn("$Shortcut.Arguments = ''", SOURCE)
        self.assertIn("IsNullOrEmpty([string]$Shortcut.Arguments)", SOURCE)
        self.assertIn("Assert-NoReparsePath -Path $link.Path", SOURCE)
        self.assertIn("$response.Close()", SOURCE)
        self.assertNotIn("$response.Dispose()", SOURCE)

    def test_install_mutex_serializes_preflight_through_shortcuts_and_exit(self):
        acquire = SOURCE.index("$installMutex = Enter-BowserInstallMutex")
        self.assertIn("'Global\\BowserStableInstall-' + $identity.User.Value", SOURCE)
        self.assertIn("$mutex.WaitOne(0)", SOURCE)
        self.assertIn("catch [Threading.AbandonedMutexException] { $acquired = $true }", SOURCE)
        self.assertIn("try { $Mutex.ReleaseMutex() }", SOURCE)
        self.assertIn("finally { $Mutex.Dispose() }", SOURCE)
        self.assertLess(acquire, SOURCE.index("try { Assert-PinNotOlderThanManagedInstalls"))
        final = SOURCE.rindex("finally {")
        self.assertGreater(final, SOURCE.index("$process = Start-Process"))
        self.assertIn("Exit-BowserInstallMutex -Mutex $installMutex", SOURCE[final:])

    def test_no_launch_returns_before_start_process(self):
        no_launch = SOURCE.index("if ($NoLaunch)")
        self.assertLess(no_launch, SOURCE.index("$process = Start-Process"))
        self.assertIn("exit 0", SOURCE[no_launch : SOURCE.index("$process = Start-Process")])
        self.assertIn("Close all Bowser windows", SOURCE)
        self.assertIn("Install-StableShortcuts", SOURCE[:no_launch])

    def test_standalone_recovery_never_launches_alongside_any_bowser(self):
        check = SOURCE.index("$openBowsers = @(Get-Process -Name 'Bowser*'")
        self.assertLess(SOURCE.index("if ($NoLaunch)"), check)
        self.assertLess(check, SOURCE.index("$process = Start-Process"))
        self.assertIn("Test-BowserLaunchBlocked -Processes $openBowsers", SOURCE)
        self.assertIn("Close ALL Bowser windows", SOURCE)
        self.assertNotIn("Only another stable copy shares this profile", SOURCE)
        helper = SOURCE[SOURCE.index("function Test-BowserLaunchBlocked {") : SOURCE.index("function Set-BowserShortcut {")]
        self.assertNotIn(".Path", helper)
        self.assertNotIn("Get-Process", helper)

    def test_windows_suite_extracts_helpers_instead_of_running_installer(self):
        tests = (ROOT / "tests" / "installer-safety.ps1").read_text(encoding="utf-8")
        self.assertIn("FunctionDefinitionAst", tests)
        self.assertIn("$definition.Extent.Text", tests)
        self.assertNotRegex(tests, r"(?m)^\s*\.\s+[\"']?\$installer")
        self.assertIn("-ItemType Junction", tests)
        self.assertIn("Local\\BowserStableInstall-unit-", tests)
        self.assertIn("An abandoned synthetic mutex is acquired", tests)
        self.assertIn("Install-StagedFile -Staged $stage -Destination $old", tests)


if __name__ == "__main__":
    unittest.main()
