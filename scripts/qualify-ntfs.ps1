param(
    [Parameter(Mandatory = $true)][string]$EvidenceDirectory,
    [string]$ProductSha = '00d0878a6f2da4f19870005ee76749e1b37f2eb4',
    [switch]$AllowQualificationLockRefresh
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'MP-4L1 requires native Windows' }
if ($ProductSha -notmatch '^[0-9a-f]{40}$') { throw 'Product SHA must contain exactly 40 lowercase hexadecimal characters' }
$repo = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot)).TrimEnd('\')
$evidence = [IO.Path]::GetFullPath($EvidenceDirectory)
if (-not [IO.Path]::IsPathRooted($EvidenceDirectory)) { throw 'Evidence path must be absolute' }
if ($evidence.TrimEnd('\').Equals($repo, [StringComparison]::OrdinalIgnoreCase) -or $evidence.StartsWith($repo + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Keep qualification fixtures outside the repository'
}
if ($evidence -notmatch '^[A-Za-z]:\\') { throw 'Use a local NTFS drive path' }
$ancestor = $evidence
while ($ancestor) {
    if (Test-Path -LiteralPath $ancestor) {
        if ((Get-Item -Force -LiteralPath $ancestor).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Evidence path must not traverse a junction or reparse point'
        }
    }
    $ancestor = Split-Path -Parent $ancestor
}
if (Test-Path -LiteralPath $evidence) { throw 'Evidence directory already exists; select a fresh run directory' }
$drive = [IO.Path]::GetPathRoot($evidence).TrimEnd('\').TrimEnd(':')
if ((Get-Volume -DriveLetter $drive).FileSystem -ne 'NTFS') { throw 'Evidence volume must be NTFS' }
Push-Location $repo
$saved = @{}
try {
    $resolved = (& git rev-parse "$ProductSha^{commit}").Trim()
    if ($LASTEXITCODE -ne 0 -or $resolved -ne $ProductSha) { throw 'Use an exact known product SHA' }
    & git merge-base --is-ancestor $ProductSha HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Product baseline is not an ancestor of this harness' }
    # Tests, examples, scripts and receipts may differ. Product source may not.
    $protected = @('crates', 'src', 'src-tauri', 'extension', '.cargo', 'Cargo.toml', 'package.json', 'package-lock.json', 'rust-toolchain', 'rust-toolchain.toml')
    $changed = @(& git diff --name-only $ProductSha -- @protected)
    $changed += @(& git ls-files --others --exclude-standard -- @protected)
    $changed += @(& git ls-files --others --ignored --exclude-standard -- .cargo rust-toolchain rust-toolchain.toml 'crates/*/build.rs')
    $productChanges = @($changed | Where-Object { $_ -notmatch '^crates/[^/]+/(examples|tests)/' })
    if ($productChanges.Count -ne 0) { throw ('Product changes invalidate baseline: ' + ($productChanges -join ', ')) }
    $lockDelta = @(& git diff $ProductSha -- Cargo.lock)
    if ($lockDelta.Count -ne 0 -and -not $AllowQualificationLockRefresh) {
        throw 'Lockfile differs from baseline; preserve the failure and use explicit qualification-only override'
    }
    New-Item -ItemType Directory -Path $evidence | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $evidence 'fixtures') | Out-Null
    $lockDelta | Set-Content -Encoding UTF8 (Join-Path $evidence 'qualification-lock.diff')
    foreach ($name in @('MP_NTFS_FIXTURES', 'MP_NTFS_RECEIPT', 'MP_NTFS_PRODUCT_SHA', 'CARGO_TARGET_DIR')) {
        $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
    $env:MP_NTFS_FIXTURES = Join-Path $evidence 'fixtures'
    $env:MP_NTFS_RECEIPT = Join-Path $evidence 'native-receipt.json'
    $env:MP_NTFS_PRODUCT_SHA = $ProductSha
    # Avoid writing native binaries into any other checkout's shared target.
    $env:CARGO_TARGET_DIR = Join-Path $evidence 'target'
    $toolchain = (& rustc -Vv) -join "`n"
    $head = (& git rev-parse HEAD).Trim()
    & cargo run --locked -p mind-place-index --example qualify_ntfs 2>&1 | Tee-Object -FilePath (Join-Path $evidence 'native-transcript.log')
    $nativeExit = $LASTEXITCODE
    $native = if (Test-Path -LiteralPath $env:MP_NTFS_RECEIPT) { Get-Content -Raw -LiteralPath $env:MP_NTFS_RECEIPT | ConvertFrom-Json } else { $null }
    $status = 'BLOCKED'
    if ($null -ne $native) {
        $status = $native.status
        if ($native.cases.Count -ne 10) { $status = 'FAIL' }
        if ($status -eq 'PASS' -and $nativeExit -ne 0) { $status = 'FAIL' }
        if ($status -eq 'PASS' -and $lockDelta.Count -ne 0) { $status = 'PARTIAL' }
    }
    $binaryRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repo 'target' }
    $binary = Join-Path $binaryRoot 'debug/examples/qualify_ntfs.exe'
    $metadata = [ordered]@{
        schema = 'mind-place/mp-4l1-run/v1'; task_id = 'MP-4L1'; status = $status
        product_sha = $ProductSha; harness_head = $head; native_exit = $nativeExit
        product_source_unchanged = $true
        dependency_lock = $(if ($lockDelta.Count -eq 0) { 'EXACT' } else { 'QUALIFICATION_ONLY_REFRESH_HOLD' })
        rustc = $toolchain; os = [Environment]::OSVersion.VersionString
        architecture = $env:PROCESSOR_ARCHITECTURE; filesystem = 'NTFS'; drive = $drive
        cargo_target_relative_to_evidence = 'target'
        harness_sha256 = (Get-FileHash -LiteralPath (Join-Path $repo 'crates/index-core/examples/qualify_ntfs.rs')).Hash
        runner_sha256 = (Get-FileHash -LiteralPath $PSCommandPath).Hash
        binary_sha256 = $(if ($null -ne $native -and (Test-Path -LiteralPath $binary)) { (Get-FileHash -LiteralPath $binary).Hash } else { $null })
        native_receipt_sha256 = $(if ($null -ne $native) { (Get-FileHash -LiteralPath $env:MP_NTFS_RECEIPT).Hash } else { $null })
        executed_at_utc = [DateTime]::UtcNow.ToString('o')
        installed_app = 'NOT RUN'; private_corpus = 'NOT ACCESSED'; fixture_cleanup = 'RETAINED; no recursive deletion'
    }
    $metadata | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $evidence 'run-metadata.json')
    Write-Output "MP-4L1 $status; evidence: $evidence"
    if ($status -ne 'PASS') { exit 1 }
}
finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
    Pop-Location
}
