# Behaviour checks for the repository's PowerShell scripts, run by tests/scripts.rs (also runnable
# by hand: powershell -NoProfile -File tests/script_checks.ps1 -Repo . -Temp $env:TEMP\x).
# Each script's functions are loaded from its syntax tree, so no script body runs: nothing here
# touches the registry, an installation, a profile, or a shortcut outside -Temp.
param(
    [Parameter(Mandatory = $true)][string]$Repo,
    [Parameter(Mandatory = $true)][string]$Temp
)

$ErrorActionPreference = 'Stop'
$failures = New-Object System.Collections.Generic.List[string]

function Import-ScriptFunctions {
    param([string]$Name, [string[]]$Functions)
    $path = Join-Path $Repo $Name
    $tokens = $null
    $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
    if ($errors.Count -gt 0) { throw "$Name does not parse: $($errors[0].Message)" }
    foreach ($function in $Functions) {
        $definition = $ast.Find({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $function
        }, $true)
        if (-not $definition) { throw "$Name has no function $function" }
        . ([scriptblock]::Create($definition.Extent.Text))
        Set-Item -Path "function:global:$function" -Value (Get-Item "function:$function").ScriptBlock
    }
    return $ast
}

function Check {
    param([string]$Label, [scriptblock]$Test)
    try {
        $result = & $Test
        if ($result -ne $true) { $failures.Add("$Label (got: $result)") }
    }
    catch {
        $failures.Add("$Label (threw: $($_.Exception.Message))")
    }
}

$odd = @('https://example.com/', 'shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App', 'C:\a|b', '', $null)

# P-11 / P-17: Uninstall-Bowser.ps1
try {
    $uninstall = Import-ScriptFunctions -Name 'Uninstall-Bowser.ps1' -Functions @('Test-SameFilePath', 'Get-CommandProgram', 'Remove-BowserBrowserRegistration')
    foreach ($target in $odd) {
        Check "Uninstall: an odd shortcut target '$target' is not the install" { (Test-SameFilePath -Candidate $target -Expected 'C:\Programs\Bowser\Bowser.exe') -eq $false }
    }
    Check 'Uninstall: the installed exe matches regardless of case and dots' { Test-SameFilePath -Candidate 'c:\programs\BOWSER\.\Bowser.exe' -Expected 'C:\Programs\Bowser\Bowser.exe' }
    Check 'Uninstall: the registered open command names its program' { (Get-CommandProgram -Command '"C:\Program Files\Bowser\Bowser.exe" "%1"') -eq 'C:\Program Files\Bowser\Bowser.exe' }
    Check 'Uninstall: an unquoted command names its program' { (Get-CommandProgram -Command 'C:\B\Bowser.exe %1') -eq 'C:\B\Bowser.exe' }
    Check 'Uninstall: an empty command names nothing' { $null -eq (Get-CommandProgram -Command '') }
    $calls = @($uninstall.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] }, $true))
    $unregister = @($calls | Where-Object { $_.GetCommandName() -eq 'Remove-BowserBrowserRegistration' })
    $removeInstall = @($calls | Where-Object { $_.GetCommandName() -eq 'Remove-Item' -and $_.Extent.Text -match '\$InstallDirectory' })
    Check 'Uninstall: the browser registration is removed before the files' {
        $unregister.Count -eq 1 -and $removeInstall.Count -eq 1 -and $unregister[0].Extent.StartOffset -lt $removeInstall[0].Extent.StartOffset
    }
    $unguarded = @($uninstall.FindAll({
        param($n)
        $n -is [System.Management.Automation.Language.InvokeMemberExpressionAst] -and $n.Member.Extent.Text -eq 'GetFullPath' -and
            $n.Arguments.Count -eq 1 -and $n.Arguments[0].Extent.Text -match '^\$(target|_\.ExecutablePath)$'
    }, $true))
    Check 'Uninstall: no shortcut or process path reaches GetFullPath unguarded' { $unguarded.Count -eq 0 }
}
catch { $failures.Add("Uninstall-Bowser.ps1: $($_.Exception.Message)") }

# P-12: Update-Bowser.ps1
try {
    $update = Import-ScriptFunctions -Name 'Update-Bowser.ps1' -Functions @('Test-SameFilePath')
    foreach ($target in $odd) {
        Check "Update: a legacy shortcut target '$target' is not the debug launcher" { (Test-SameFilePath -Candidate $target -Expected 'C:\repo\Launch-Bowser-Debug.cmd') -eq $false }
    }
    $legacy = @($update.FindAll({
        param($n)
        $n -is [System.Management.Automation.Language.InvokeMemberExpressionAst] -and $n.Member.Extent.Text -eq 'GetFullPath' -and
            $n.Arguments.Count -eq 1 -and $n.Arguments[0].Extent.Text -eq '$legacyTarget'
    }, $true))
    Check 'Update: the legacy shortcut target never reaches GetFullPath unguarded' { $legacy.Count -eq 0 }
}
catch { $failures.Add("Update-Bowser.ps1: $($_.Exception.Message)") }

# P-14: Open-Bowser.ps1
try {
    $null = Import-ScriptFunctions -Name 'Open-Bowser.ps1' -Functions @('Test-SameFilePath', 'Assert-NoReparsePath', 'Get-LowerSha256', 'Install-StagedFile')
    $dir = Join-Path $Temp 'open-bowser'
    $null = New-Item -ItemType Directory -Path $dir -Force
    $dest = Join-Path $dir 'Bowser.exe'
    $staged = Join-Path $dir 'Bowser.exe.download'
    [IO.File]::WriteAllText($dest, 'old')
    [IO.File]::WriteAllText($staged, 'new')
    Install-StagedFile -Staged $staged -Destination $dest -PreviousHash (Get-LowerSha256 -Path $dest)
    Check 'Open: the staged exe replaces the installed one and clears rollback backups' {
        ([IO.File]::ReadAllText($dest) -eq 'new') -and -not (Test-Path -LiteralPath $staged) -and
            (@(Get-ChildItem -LiteralPath $dir -Filter 'Bowser.exe.previous*' -Force).Count -eq 0)
    }
    $failed = $false
    try { Install-StagedFile -Staged (Join-Path $dir 'missing.download') -Destination $dest -PreviousHash (Get-LowerSha256 -Path $dest) } catch { $failed = $true }
    Check 'Open: a failed replace keeps the installed exe' { $failed -and ([IO.File]::ReadAllText($dest) -eq 'new') }
    [IO.File]::WriteAllText($staged, 'unrecorded')
    $failed = $false
    try { Install-StagedFile -Staged $staged -Destination $dest } catch { $failed = $true }
    Check 'Open: an unrecorded destination is never replaced' {
        $failed -and ([IO.File]::ReadAllText($dest) -eq 'new') -and ([IO.File]::ReadAllText($staged) -eq 'unrecorded')
    }
    Remove-Item -LiteralPath $dest -Force
    [IO.File]::WriteAllText($staged, 'first')
    Install-StagedFile -Staged $staged -Destination $dest
    Check 'Open: a first install moves the staged exe in' { [IO.File]::ReadAllText($dest) -eq 'first' }
}
catch { $failures.Add("Open-Bowser.ps1: $($_.Exception.Message)") }

# P-15: Dev-Bowser.ps1
try {
    $null = Import-ScriptFunctions -Name 'Dev-Bowser.ps1' -Functions @('Test-WatcherLock')
    $global:lockPath = Join-Path $Temp 'watcher.lock'
    [IO.File]::WriteAllText($global:lockPath, '')
    Check 'Dev: an empty watcher.lock does not stop startup' { Test-WatcherLock; $true }
}
catch { $failures.Add("Dev-Bowser.ps1: $($_.Exception.Message)") }

if ($failures.Count -gt 0) {
    $failures | ForEach-Object { Write-Output "FAIL: $_" }
    exit 1
}
Write-Output 'script checks passed'
exit 0
