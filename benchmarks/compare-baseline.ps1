[CmdletBinding()]
param(
    # Either measure now (runs perf-probe startup, idle, tabs and switch against this build) ...
    [string] $BowserExecutable,
    # ... or compare perf-probe output that already exists in a directory.
    [string] $ResultsDirectory,
    [string] $Baseline,
    [ValidateRange(1,100)] [int] $StartupRuns = 5,
    [int[]] $TabCounts = @(1,5),
    [ValidateRange(2,200)] [int] $Switches = 20,
    [string] $OutputDirectory
)

# Exit code 0: every baseline metric is within tolerance. Exit code 1: a metric regressed or is missing.
# Exit code 2: the comparison could not be made (bad input, probe failure).
Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'
# Defaults are resolved here: $PSScriptRoot inside param() defaults was empty when run with -File.
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Baseline) { $Baseline = Join-Path $scriptDir 'baseline.json' }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $scriptDir 'results\compare' }
Import-Module (Join-Path $scriptDir 'measurements.psm1') -Force

try {
    if ([bool]$BowserExecutable -eq [bool]$ResultsDirectory) { throw 'Pass exactly one of -BowserExecutable (measure now) or -ResultsDirectory (compare existing probe output).' }
    $baselinePath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Baseline)
    if (-not (Test-Path -LiteralPath $baselinePath)) { throw "Baseline not found: $baselinePath" }
    $base = Get-Content -LiteralPath $baselinePath -Raw | ConvertFrom-Json

    if ($BowserExecutable) {
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        $ResultsDirectory = Join-Path $OutputDirectory $stamp
        New-Item -ItemType Directory -Force $ResultsDirectory | Out-Null
        $probe = Join-Path $scriptDir 'perf-probe.ps1'
        $exe = (Resolve-Path -LiteralPath $BowserExecutable).Path
        # Each mode runs in a child PowerShell so one failed mode cannot leave a fixture server or profile behind.
        $modes = @(
            @{ Mode = 'startup'; Extra = "-Runs $StartupRuns" },
            @{ Mode = 'idle'; Extra = '' },
            @{ Mode = 'tabs'; Extra = "-TabCounts $($TabCounts -join ',')" },
            @{ Mode = 'switch'; Extra = "-Switches $Switches" }
        )
        foreach ($m in $modes) {
            $cmd = "& '$probe' -BowserExecutable '$exe' -Label compare -Mode $($m.Mode) $($m.Extra) -OutputDirectory '$ResultsDirectory'"
            & powershell.exe -NoProfile -ExecutionPolicy Bypass -Command $cmd | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "perf-probe mode '$($m.Mode)' failed (exit $LASTEXITCODE)." }
        }
    } else {
        $ResultsDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($ResultsDirectory)
    }

    $current = Get-BowserProbeMetrics -Directory $ResultsDirectory
    $rows = @(Compare-BowserBaseline -Baseline $base -Current $current)
    $rows | Format-Table metric, baseline, current, limit, status -AutoSize | Out-String | Write-Host
    $failed = @($rows | Where-Object { $_.status -ne 'ok' })
    if ($failed.Count -gt 0) {
        Write-Host ("REGRESSION: {0} metric(s) outside tolerance: {1}" -f $failed.Count, (($failed | ForEach-Object { $_.metric }) -join ', '))
        exit 1
    }
    Write-Host "OK: all $($rows.Count) metrics within tolerance of the baseline recorded $($base.recorded) on '$($base.machine.cpu)'."
    exit 0
} catch {
    # Write-Error would be terminating under ErrorActionPreference=Stop and skip the exit code below.
    [Console]::Error.WriteLine("ERROR: $($_.Exception.Message)")
    exit 2
}
