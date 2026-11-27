param(
    [Parameter(Mandatory)][long] $Window,
    [Parameter(Mandatory)][int] $Owner,
    [Parameter(Mandatory)][string] $Choice,
    [Parameter(Mandatory)][ValidateSet('Focus', 'Verify')][string] $Action
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$windowElement = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Window)
if ($windowElement.Current.ProcessId -ne $Owner) { throw 'Benchmark window ownership changed' }

function Find-Radio($Name) {
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::NameProperty, $Name,
        [System.Windows.Automation.PropertyConditionFlags]::IgnoreCase)
    $until = [Diagnostics.Stopwatch]::StartNew()
    do {
        foreach ($item in $windowElement.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)) {
            if ($item.Current.ControlType -eq [System.Windows.Automation.ControlType]::RadioButton) {
                return $item
            }
        }
        Start-Sleep -Milliseconds 100
    } while ($until.Elapsed.TotalSeconds -lt 8)
    $labels = $windowElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.Condition]::TrueCondition) |
        ForEach-Object { $_.Current.Name } | Where-Object { $_ } | Select-Object -First 55
    throw "Startup radio choice unavailable: $Name. Owned window labels: $($labels -join ' | ')"
}

$radio = Find-Radio $Choice
if ($Action -eq 'Focus') {
    $radio.SetFocus()
    $until = [Diagnostics.Stopwatch]::StartNew()
    while (-not $radio.Current.HasKeyboardFocus -and $until.Elapsed.TotalSeconds -lt 3) {
        Start-Sleep -Milliseconds 100
    }
    if (-not $radio.Current.HasKeyboardFocus) { throw "The startup radio did not take focus: $Choice" }
    [pscustomobject]@{ focused = $true; choice = $Choice } | ConvertTo-Json -Compress
} else {
    $pattern = $null
    $selected = $radio.TryGetCurrentPattern(
        [System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern) -and
        $pattern.Current.IsSelected
    if (-not $selected -and $radio.TryGetCurrentPattern(
            [System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
        $selected = $pattern.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On
    }
    if (-not $selected) { throw "The startup radio was not selected: $Choice" }
    [pscustomobject]@{ selected = $true; choice = $Choice } | ConvertTo-Json -Compress
}
