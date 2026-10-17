# Bound compilation separately from runtime. Preserve the full aggregate test gates afterwards.
[CmdletBinding()]
param([string]$Out = (Join-Path $env:RUNNER_TEMP 'bowser-theme-tests'))
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $Out | Out-Null
function Invoke-BoundedCargo([string]$Stage, [string[]]$CargoArgs, [int]$Seconds) {
    $stdout = Join-Path $Out "$Stage.stdout.log"
    $stderr = Join-Path $Out "$Stage.stderr.log"
    $started = (Get-Date).ToUniversalTime().ToString('o')
    Write-Host "Starting $Stage at $started : cargo $($CargoArgs -join ' ')"
    $process = Start-Process -FilePath 'cargo' -ArgumentList $CargoArgs -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    try {
        if (-not $process.WaitForExit($Seconds * 1000)) {
            # Read-only evidence first: include the test child, command line, and visible native
            # window title/handle so a modal-dialog hang is distinguishable from compilation.
            try {
                $relations = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId |
                Select-Object ProcessId, ParentProcessId)
            $owned = [System.Collections.Generic.HashSet[int]]::new()
            [void]$owned.Add($process.Id)
            do {
                $added = $false
                foreach ($row in $relations) {
                    if ($owned.Contains([int]$row.ParentProcessId) -and $owned.Add([int]$row.ProcessId)) { $added = $true }
                }
            } while ($added)
                $details = foreach ($childId in $owned) {
                    Get-CimInstance Win32_Process -Filter "ProcessId = $childId" -ErrorAction Continue |
                        Select-Object ProcessId, ParentProcessId, Name, CommandLine
                }
                @($details) | ConvertTo-Json -Depth 3 | Set-Content (Join-Path $Out "$Stage.processes.json")
                Get-Process -Id @($owned) -ErrorAction Continue |
                    Select-Object Id, ProcessName, CPU, MainWindowTitle, MainWindowHandle |
                    ConvertTo-Json -Depth 3 | Set-Content (Join-Path $Out "$Stage.windows.json")
            } finally {
                & taskkill /PID $process.Id /T /F | Out-Host
            }
            throw "$Stage exceeded its ${Seconds}s runtime budget. Captured process/window evidence."
        }
        $process.WaitForExit()
        $process.Refresh()
        if ($process.ExitCode -ne 0) { throw "$Stage failed with exit code $($process.ExitCode)." }
    } finally {
        Write-Host "Finished $Stage at $((Get-Date).ToUniversalTime().ToString('o'))"
        if (Test-Path $stdout) { Get-Content $stdout | Write-Host }
        if (Test-Path $stderr) { Get-Content $stderr | Write-Host }
        $process.Dispose()
    }
}
Invoke-BoundedCargo 'compile-native' @('+stable', 'test', '--locked', '--no-run') 480
Invoke-BoundedCargo 'theme-flow' @('+stable', 'test', '--locked', 'app::tests::theme_flow', '--', '--nocapture') 120
Invoke-BoundedCargo 'profile-isolation' @('+stable', 'test', '--locked', 'app_profiles', '--', '--nocapture') 120
Invoke-BoundedCargo 'error-dialog' @('+stable', 'test', '--locked', 'error_dialog_tests', '--', '--nocapture') 120
