# Production-path measurement validation. Resource counters live in measurements.psm1.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-ProductionReadiness {
    [CmdletBinding()]
    param([object[]]$Rows, [Parameter(Mandatory)][string]$Token)
    $current = @{}; $success = @{}; $ready = @{}; $invalid = @(); $result = $null
    foreach ($row in $Rows) {
        if ($row.event -notlike 'diagnostic_*') { continue }
        $tab = [string]$row.tab_id; $nav = [string]$row.navigation_id
        $key = "$tab/$nav"
        if (-not $tab -or -not $nav -or $nav -eq '0') { $invalid += 'missing_navigation_identity'; continue }
        switch ($row.event) {
            'diagnostic_navigation_started' {
                $current[$tab] = $nav
                # Redirects may emit another start with the same navigation ID.
                if (-not $success.ContainsKey($key)) { $success[$key] = $null }
            }
            'diagnostic_navigation_completed' {
                if (-not $current.ContainsKey($tab) -or $current[$tab] -ne $nav) { $invalid += 'stale_completion'; continue }
                $success[$key] = ([string]$row.success -ceq 'true')
                if (-not $success[$key]) { $invalid += 'navigation_failed' }
            }
            'diagnostic_fixture_ready' {
                if ($row.fixture_token -cne $Token) { continue }
                if (-not $current.ContainsKey($tab) -or $current[$tab] -ne $nav) { $invalid += 'stale_fixture'; continue }
                $ready[$key] = [long]$row.qpc_ticks
            }
        }
        if ($current.ContainsKey($tab) -and $current[$tab] -eq $nav -and $success.ContainsKey($key) -and $success[$key] -eq $true -and $ready.ContainsKey($key)) {
            $result = [pscustomobject]@{ tab_id=$tab; navigation_id=$nav; qpc_ticks=[Math]::Max([long]$row.qpc_ticks,[long]$ready[$key]); fixture_token=$Token }
        }
    }
    # A newer navigation after readiness invalidates the result. Never inherit another document.
    if ($result -and $current[$result.tab_id] -ne $result.navigation_id) { $result = $null; $invalid += 'superseded_readiness' }
    [pscustomobject]@{ Valid=($null -ne $result); Ready=$result; InvalidReasons=@($invalid | Sort-Object -Unique) }
}

function Get-ProductionCpuDelta {
    param([object[]]$Snapshots)
    $last = @{}; $total = 0.0; $newIdentityCount = 0; $regressions = 0
    foreach ($snapshot in $Snapshots) {
        foreach ($key in $snapshot.CpuByPid.Keys) {
            $value = [double]$snapshot.CpuByPid[$key]
            if ($last.ContainsKey($key)) {
                $delta = $value - $last[$key]
                if ($delta -ge 0) { $total += $delta } else { $regressions++ }
            } else {
                # Include all accumulated CPU for newly discovered descendants, not zero.
                $total += $value; $newIdentityCount++
            }
            $last[$key] = $value
        }
    }
    [pscustomobject]@{ CoreSeconds=$total; ObservedIdentityCount=$last.Count; NewIdentityCount=$newIdentityCount; CounterRegressions=$regressions; Accounting='sampled_identity_lifetime_cpu'; Limitation='Processes born and exited between samples, and CPU after last observation before exit, are unobserved. Not ETW-complete.' }
}

function Get-ProductionDistribution {
    param([double[]]$Values)
    $sorted = @($Values | Sort-Object)
    if (-not $sorted.Count) { return [pscustomobject]@{ Count=0; Median=$null; P95=$null; Min=$null; Max=$null } }
    $median = if ($sorted.Count % 2) { $sorted[[int][Math]::Floor($sorted.Count / 2)] } else { ($sorted[$sorted.Count/2-1]+$sorted[$sorted.Count/2])/2 }
    [pscustomobject]@{ Count=$sorted.Count; Median=$median; P95=$sorted[[Math]::Max(0,[int][Math]::Ceiling($sorted.Count*0.95)-1)]; Min=$sorted[0]; Max=$sorted[-1] }
}
Export-ModuleMember -Function Get-ProductionReadiness,Get-ProductionCpuDelta,Get-ProductionDistribution
