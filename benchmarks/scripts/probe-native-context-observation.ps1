# Observation/cancellation only. Never selects or recreates an extension menu item.
# WebView2 leaves Handled=false in this exact Rust observer: its real backend is Chromium
# Views, not a Bowser-owned TrackPopupMenu HMENU. Reuse the strict native/UIA row proof;
# foreign providers still refuse before Escape, with separate observer evidence/report scope.
[CmdletBinding()]
param([Parameter(Mandatory)][uint32]$OwnerPid,[Parameter(Mandatory)][long]$OwnerHandle,[Parameter(Mandatory)][string]$OwnerExe,[Parameter(Mandatory)][uint32]$BrowserPid,
 [Parameter(Mandatory)][int]$ClientX,[Parameter(Mandatory)][int]$ClientY,[Parameter(Mandatory)][int]$ViewportWidth,[Parameter(Mandatory)][int]$ViewportHeight,
 [Parameter(Mandatory)][string]$Out)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$global:LASTEXITCODE=0
& (Join-Path $PSScriptRoot 'probe-default-context-cancellation.ps1') @PSBoundParameters -ProbeKind ExtensionObservation
exit $LASTEXITCODE
