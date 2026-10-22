param(
    [Parameter(Mandatory = $true)][string]$EvidenceDirectory
)
$ErrorActionPreference = 'Stop'
$ProductSha = '5301ad773d32a5ac88ba5ec9dcc1232c8b8e8f10'
if ($env:OS -ne 'Windows_NT') { throw 'MP-4L2 requires native Windows' }
if ($ProductSha -cnotmatch '^[0-9a-f]{40}$') { throw 'Supply the full exact product SHA' }
$repo = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot)).TrimEnd('\')
if (-not [IO.Path]::IsPathRooted($EvidenceDirectory)) { throw 'Evidence directory must be absolute' }
$evidence = [IO.Path]::GetFullPath($EvidenceDirectory)
if ($evidence -notmatch '^D:\\') { throw 'Qualification output must use local D: NTFS' }
$separator = [IO.Path]::DirectorySeparatorChar
if ($evidence.TrimEnd($separator).Equals($repo, [StringComparison]::OrdinalIgnoreCase) -or
    $evidence.StartsWith($repo + $separator, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Keep qualification evidence and binaries outside the checkout'
}
$ancestor = $evidence
while ($ancestor) {
    if (Test-Path -LiteralPath $ancestor) {
        if ((Get-Item -Force -LiteralPath $ancestor).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Evidence directory may not cross a reparse point'
        }
    }
    $ancestor = Split-Path -Parent $ancestor
}
if (Test-Path -LiteralPath $evidence) { throw 'Use a fresh evidence directory' }
if ((Get-Volume -DriveLetter D).FileSystem -ne 'NTFS') { throw 'D: must be NTFS' }
$saved = @{}
$savedPath = $env:PATH
Push-Location $repo
try {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        $rustBin = Join-Path $env:USERPROFILE '.cargo\bin'
        if (-not (Test-Path -LiteralPath (Join-Path $rustBin 'cargo.exe'))) {
            throw 'Rust cargo toolchain unavailable'
        }
        $env:PATH = "$rustBin;$env:PATH"
    }
    $resolved = (& git rev-parse "$ProductSha^{commit}").Trim()
    if ($LASTEXITCODE -ne 0 -or $resolved -cne $ProductSha) { throw 'Unknown product SHA' }
    & git merge-base --is-ancestor $ProductSha HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Baseline must be ancestor of harness' }
    $protected = @('crates', 'src', 'src-tauri', 'extension', '.cargo', 'Cargo.toml', 'Cargo.lock', 'package.json', 'package-lock.json')
    $changed = @(& git diff --name-only $ProductSha -- @protected)
    $changed += @(& git ls-files --others --exclude-standard -- @protected)
    $changesToProduct = @($changed | Where-Object { $_ -notmatch '^crates/[^/]+/(examples|tests)/' })
    if ($changesToProduct.Count -ne 0) {
        throw ('Product source differs from qualification baseline: ' + ($changesToProduct -join ', '))
    }
    New-Item -ItemType Directory -Path (Join-Path $evidence 'fixtures') -Force | Out-Null
    foreach ($name in @('MP_CORPUS_FIXTURES', 'MP_CORPUS_RECEIPT', 'MP_CORPUS_PRODUCT_SHA', 'CARGO_TARGET_DIR')) {
        $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
    $env:MP_CORPUS_FIXTURES = Join-Path $evidence 'fixtures'
    $env:MP_CORPUS_RECEIPT = Join-Path $evidence 'native-receipt.json'
    $env:MP_CORPUS_PRODUCT_SHA = $ProductSha
    $env:CARGO_TARGET_DIR = Join-Path $evidence 'target'
    $rustc = (& rustc -Vv) -join "`n"
    $head = (& git rev-parse HEAD).Trim()
    $cargoExe = (Get-Command cargo -ErrorAction Stop).Source
    $stdoutPath = Join-Path $evidence 'native-stdout.log'
    $stderrPath = Join-Path $evidence 'native-stderr.log'
    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = $cargoExe
    $startInfo.Arguments = 'run --locked -p mind-place-index --example qualify_corpus'
    $startInfo.WorkingDirectory = $repo
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $cargoProcess = [System.Diagnostics.Process]::Start($startInfo)
    $stdoutTask = $cargoProcess.StandardOutput.ReadToEndAsync()
    $stderrTask = $cargoProcess.StandardError.ReadToEndAsync()
    $cargoProcess.WaitForExit()
    $nativeExit = $cargoProcess.ExitCode
    [IO.File]::WriteAllText($stdoutPath, $stdoutTask.GetAwaiter().GetResult())
    [IO.File]::WriteAllText($stderrPath, $stderrTask.GetAwaiter().GetResult())
    $cargoProcess.Dispose()
    Get-Content -LiteralPath $stderrPath
    Get-Content -LiteralPath $stdoutPath
    $native = if (Test-Path -LiteralPath $env:MP_CORPUS_RECEIPT) {
        Get-Content -Raw -LiteralPath $env:MP_CORPUS_RECEIPT | ConvertFrom-Json
    } else { $null }
    $status = 'BLOCKED'
    if ($null -ne $native) {
        $status = $native.status
        if ($native.cases.Count -ne 2 -or ($status -eq 'PASS' -and $nativeExit -ne 0)) { $status = 'FAIL' }
    }
    $binary = Join-Path $env:CARGO_TARGET_DIR 'debug/examples/qualify_corpus.exe'
    [ordered]@{
        schema = 'mind-place/mp-4l2-run/v1'; task_id = 'MP-4L2'; status = $status
        product_sha = $ProductSha; harness_head = $head; native_exit = $nativeExit
        product_source_unchanged = $true; dependency_lock = 'EXACT'
        rustc = $rustc; os = [Environment]::OSVersion.VersionString
        architecture = $env:PROCESSOR_ARCHITECTURE; filesystem = 'NTFS'; drive = 'D'
        harness_sha256 = (Get-FileHash -LiteralPath (Join-Path $repo 'crates/index-core/examples/qualify_corpus.rs')).Hash
        runner_sha256 = (Get-FileHash -LiteralPath $PSCommandPath).Hash
        binary_sha256 = $(if (Test-Path -LiteralPath $binary) { (Get-FileHash -LiteralPath $binary).Hash } else { $null })
        native_receipt_sha256 = $(if ($null -ne $native) { (Get-FileHash -LiteralPath $env:MP_CORPUS_RECEIPT).Hash } else { $null })
        executed_at_utc = [DateTime]::UtcNow.ToString('o')
        installed_app = 'NOT RUN'; private_corpus = 'NOT ACCESSED'; fixture_cleanup = 'RETAINED; no recursive deletion'
    } | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $evidence 'run-metadata.json')
    Write-Output "MP-4L2 $status; evidence: $evidence"
    if ($status -ne 'PASS') { exit 1 }
}
finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
    $env:PATH = $savedPath
    Pop-Location
}
