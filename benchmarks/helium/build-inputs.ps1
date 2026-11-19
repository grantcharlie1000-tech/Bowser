# Run from the candidate checkout. Builds both arms into disjoint new directories.
param(
    [Parameter(Mandatory)][string]$Lab,
    [Parameter(Mandatory)][string]$Helium,
    [Parameter(Mandatory)][string]$HeliumVersion,
    [Parameter(Mandatory)][string]$HeliumChromium,
    [Parameter(Mandatory)][string]$HeliumArchive,
    [Parameter(Mandatory)][string]$WebView2,
    [Parameter(Mandatory)][string]$BaselineRef
)
$ErrorActionPreference = 'Stop'
$Lab = [IO.Path]::GetFullPath($Lab)
if (Test-Path -LiteralPath $Lab) { throw 'Choose a new lab directory.' }
$Helium = (Resolve-Path -LiteralPath $Helium).Path
$HeliumArchive = (Resolve-Path -LiteralPath $HeliumArchive).Path
$WebView2 = (Resolve-Path -LiteralPath $WebView2).Path
$source = (Get-Location).Path
$ref = git rev-parse --verify --end-of-options "${BaselineRef}^{commit}"
if ($LASTEXITCODE) { throw 'Could not resolve the baseline commit' }
$ref = $ref.Trim()
$baseline = (New-Item -ItemType Directory -Path "$Lab\baseline").FullName
git archive --format=tar --output="$Lab\baseline.tar" $ref
if ($LASTEXITCODE) { throw 'Could not archive the baseline' }
tar.exe -xf "$Lab\baseline.tar" -C $baseline
if ($LASTEXITCODE) { throw 'Could not extract the baseline' }
git diff --binary --output="$Lab\candidate.patch" $ref -- src ui Cargo.toml Cargo.lock build.rs bowser.manifest bowser.rc
if ($LASTEXITCODE) { throw 'Could not record the candidate patch' }
git status --porcelain=v1 --branch | Set-Content "$Lab\candidate-status.txt"
$rust = rustc +1.99.1 -vV
if ($LASTEXITCODE) { throw 'Install Rust 1.99.1 with the x64 MSVC target' }
$cargo = cargo +1.99.1 -V
$target = 'x86_64-pc-windows-msvc'
$features = 'html-chrome-default'
$id = [ordered]@{}
foreach ($arm in @('main', 'candidate')) {
    $tree = if ($arm -eq 'main') { $baseline } else { $source }
    $build = Join-Path $Lab "$arm-target"
    Push-Location $tree
    try {
        cargo +1.99.1 build --release --locked --bin bowser --target $target --features $features --target-dir $build
        if ($LASTEXITCODE) { throw "$arm build failed" }
        $exe = Join-Path $build "$target\release\bowser.exe"
        $id[$arm] = [ordered]@{
            exe = $exe
            source = $(if ($arm -eq 'main') { $ref } else { (git rev-parse HEAD).Trim() })
            patch_sha256 = $(if ($arm -eq 'candidate') { (Get-FileHash "$Lab\candidate.patch").Hash } else { $null })
            sha256 = (Get-FileHash $exe).Hash
            cargo_lock_sha256 = (Get-FileHash Cargo.lock).Hash
            ui_manifest_sha256 = (Get-FileHash ui/dist.manifest).Hash
            cargo = $cargo; rust = $rust; target = $target; features = $features; profile = 'release'
        }
    } finally { Pop-Location }
}
$id.helium = @{
    exe = $Helium
    sha256 = (Get-FileHash $Helium).Hash
    version = $HeliumVersion
    chromium = $HeliumChromium
    archive_sha256 = (Get-FileHash $HeliumArchive).Hash
    file_version = (Get-Item $Helium).VersionInfo.FileVersion
}
$id.webview2 = @{
    exe = $WebView2; sha256 = (Get-FileHash $WebView2).Hash
    version = (Get-Item $WebView2).VersionInfo.FileVersion
}
$id | ConvertTo-Json -Depth 5 | Set-Content "$Lab\identities.json" -Encoding utf8
