param([string]$EvidenceDirectory = 'outputs/mp9c-windows')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'Windows native qualification only' }
$env:RUSTUP_TOOLCHAIN = '1.90.0'
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_BUILD_JOBS = '2'
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path (Get-Location) 'target' }
if (Test-Path -LiteralPath 'C:/Strawberry/perl/bin/perl.exe') { $env:PATH = 'C:/Strawberry/perl/bin;' + $env:PATH }
$env:npm_config_fetch_retries = '0'
$env:npm_config_fetch_timeout = '15000'
$env:MP_SETTINGS_EVIDENCE_DIR = Join-Path (Get-Location) "$EvidenceDirectory/settings"
New-Item -ItemType Directory -Force -Path $EvidenceDirectory | Out-Null
$sourceSha = (& git rev-parse HEAD).Trim()
$sourceTree = (& git rev-parse 'HEAD^{tree}').Trim()
$before = @(& git status --porcelain --untracked-files=no)
if ($LASTEXITCODE -ne 0 -or $before.Count -ne 0) { throw 'Qualification requires a clean tracked exact-HEAD source' }
$rust = (& rustc --version).Trim()
$nodeVersion = (& node --version).Trim()
if ($rust -notmatch '^rustc 1\.90\.0 ' -or $nodeVersion -notmatch '^v24\.') { throw 'Requires Rust 1.90.0 and Node 24' }

function Run-Native([string]$Command, [string[]]$Arguments) {
    Write-Output ('COMMAND: ' + $Command + ' ' + ($Arguments -join ' '))
    # Windows PowerShell 5 turns ordinary native stderr (including Cargo's
    # progress/warnings) into ErrorRecords. The native exit code is decisive.
    $previousPreference = $ErrorActionPreference
    try { $ErrorActionPreference = 'Continue'; & $Command @Arguments; $code = $LASTEXITCODE }
    finally { $ErrorActionPreference = $previousPreference }
    if ($code -ne 0) { throw "$Command failed with exit code $code" }
}
function Require-Tests([string[]]$Arguments, [string]$Prefix, [int]$Minimum) {
    $previousPreference = $ErrorActionPreference
    try { $ErrorActionPreference = 'Continue'; $listing = @(& cargo @Arguments -- --list 2>&1); $code = $LASTEXITCODE }
    finally { $ErrorActionPreference = $previousPreference }
    if ($code -ne 0) { throw 'Required test enumeration failed' }
    $count = @($listing | Select-String -Pattern ([regex]::Escape($Prefix) + '.+: test$')).Count
    if ($count -lt $Minimum) { throw "Required $Prefix test inventory missing: $count < $Minimum" }
    Write-Output "REQUIRED TEST INVENTORY: $Prefix $count"
}

# Same 14 Windows Product steps, seven SQLCipher/native legs, and seven MP-9C
# groups described in PR151 qualification 6890523935. Each group fails if any
# command fails. Continue subsequent groups to retain the complete failure map.
$steps = @(
    @{ Id='A1'; Name='Install dependencies'; Run={ Run-Native npm.cmd @('ci') } },
    @{ Id='A2'; Name='Rust formatting'; Run={ Run-Native cargo @('fmt','--all','--','--check') } },
    @{ Id='A3'; Name='Repository contract'; Run={ Run-Native npm.cmd @('run','verify:repo') } },
    @{ Id='A4'; Name='Typecheck'; Run={ Run-Native npm.cmd @('run','typecheck') } },
    @{ Id='A5'; Name='Settings visual scroll and footer'; Run={ Run-Native node @('scripts/verify-settings-render.mjs') } },
    @{ Id='A6'; Name='Bundled SQLite runtime'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-state','bundled_sqlite_version_is_supported','--','--nocapture') } },
    @{ Id='A7'; Name='Full npm baseline'; Run={ Run-Native npm.cmd @('test') } },
    @{ Id='A8'; Name='Native sidecar, helper, notices and web staging'; Run={
        Run-Native cargo @('build','--locked','-p','mind-place-capture-host')
        New-Item -ItemType Directory -Force src-tauri/binaries | Out-Null
        Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'debug/mind-place-capture-host.exe') -Destination src-tauri/binaries/mind-place-capture-host-x86_64-pc-windows-msvc.exe
        Run-Native node @('scripts/tunnel-helper.mjs')
        Run-Native node @('scripts/generate-notices.mjs')
        Run-Native npm.cmd @('run','build:web')
    } },
    @{ Id='A9'; Name='Phase 3E MCP'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-integration','--test','mcp','--','--nocapture') } },
    @{ Id='A10'; Name='Hosted tunnel desktop boundaries'; Run={ Run-Native cargo @('test','--locked','-p','mind-place','--bin','mind-place','hosted_tunnel::tests','--','--nocapture') } },
    @{ Id='A11'; Name='Owner import commands'; Run={
        $testArgs = @('test','--locked','-p','mind-place','--bin','mind-place','import_commands::tests')
        Require-Tests $testArgs 'import_commands::tests::' 14
        Run-Native cargo ($testArgs + @('--','--nocapture'))
    } },
    @{ Id='A12'; Name='Native capture IPC'; Run={
        $testArgs = @('test','--locked','-p','mind-place','--bin','mind-place','native_capture_ipc::qualification')
        Require-Tests $testArgs 'native_capture_ipc::qualification::' 2
        Run-Native cargo ($testArgs + @('--','--nocapture'))
    } },
    @{ Id='A13'; Name='Desktop application build'; Run={ Run-Native npm.cmd @('run','build') } },
    @{ Id='A14'; Name='Distribution license'; Run={ Run-Native npm.cmd @('run','verify:distribution-license') } },
    @{ Id='B1'; Name='Full encrypted state/index, including offline owner migration'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-state','-p','mind-place-index','--features','sqlcipher-preview','--','--nocapture') } },
    @{ Id='B2'; Name='Keyed MCP vault'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-integration','--features','vault-mcp','--test','mcp_vault','--','--nocapture') } },
    @{ Id='B3'; Name='Complete security suite'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-security','--','--nocapture') } },
    @{ Id='B4'; Name='Capture and coherent backup'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-index','--features','sqlcipher-preview','--test','vault_capture_encrypted','--test','vault_online_backup','--test','vault_backup_consistency','--test','sqlcipher_index_backup','--','--nocapture') } },
    @{ Id='B5'; Name='First-run and bootstrap'; Run={
        Run-Native cargo @('test','--locked','-p','mind-place-security','vault_first_run','--','--nocapture')
        Run-Native cargo @('test','--locked','-p','mind-place-state','--features','sqlcipher-preview','vault_bootstrap','--','--nocapture')
    } },
    @{ Id='B6'; Name='Encrypted migration restart'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-index','--features','sqlcipher-preview','--test','vault_migration_restart','--','--nocapture') } },
    @{ Id='B7'; Name='Native shutdown and unlock'; Run={ Run-Native cargo @('test','--locked','-p','mind-place','--bin','mind-place','shutdown_race_regression_tests','--','--nocapture') } },
    @{ Id='C1'; Name='Privacy fences'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-security','--lib','vault_fence','--','--nocapture') } },
    @{ Id='C2'; Name='Full state-core including plaintext refusal'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-state','--','--nocapture') } },
    @{ Id='C3'; Name='Encrypted redaction, forget and identity'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-state','--features','sqlcipher-preview','--test','source_redaction','--test','source_forget','--test','identity','--','--nocapture') } },
    @{ Id='C4'; Name='Vault privacy races'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-index','--features','sqlcipher-preview','--lib','privacy_capture_race_tests','--','--nocapture') } },
    @{ Id='C5'; Name='MCP clients'; Run={ Run-Native cargo @('test','--locked','-p','mind-place-integration','--test','clients','--','--nocapture') } },
    @{ Id='C6'; Name='Native owner forget commands'; Run={
        $testArgs = @('test','--locked','-p','mind-place','--bin','mind-place','forget_commands::tests')
        Require-Tests $testArgs 'forget_commands::tests::' 6
        Run-Native cargo ($testArgs + @('--','--nocapture'))
    } },
    @{ Id='C7'; Name='Complete desktop binary'; Run={ Run-Native cargo @('test','--locked','-p','mind-place','--bin','mind-place','--','--nocapture') } }
)
if ($steps.Count -ne 28) { throw 'Qualification inventory must contain all 28 groups' }
$results = @()
$startedUtc = (Get-Date).ToUniversalTime().ToString('o')
foreach ($step in $steps) {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $log = Join-Path $EvidenceDirectory ($step.Id + '.log')
    $outcome = 'PASS'
    $errorText = $null
    $headBefore = (& git rev-parse HEAD).Trim()
    $treeBefore = (& git rev-parse 'HEAD^{tree}').Trim()
    $changesBefore = @(& git status --porcelain --untracked-files=no)
    $sourceValidBefore = $headBefore -eq $sourceSha -and $treeBefore -eq $sourceTree -and $changesBefore.Count -eq 0
    try {
        if (-not $sourceValidBefore) { throw 'Source HEAD/tree or tracked bytes drifted before this group' }
        & $step.Run *> $log
    } catch { $outcome = 'FAIL'; $errorText = $_.Exception.Message; Add-Content -LiteralPath $log -Value $errorText }
    $watch.Stop()
    $after = @(& git status --porcelain --untracked-files=no)
    $headAfter = (& git rev-parse HEAD).Trim()
    $treeAfter = (& git rev-parse 'HEAD^{tree}').Trim()
    $sourceValidAfter = $headAfter -eq $sourceSha -and $treeAfter -eq $sourceTree -and $after.Count -eq 0
    if (-not $sourceValidAfter) { $outcome = 'FAIL'; $errorText = 'Source HEAD/tree or tracked bytes drifted during this group' }
    $results += [pscustomobject]@{ id=$step.Id; name=$step.Name; status=$outcome; seconds=[Math]::Round($watch.Elapsed.TotalSeconds,3); log=($step.Id + '.log'); error=$errorText; head_before=$headBefore; head_after=$headAfter; tree_before=$treeBefore; tree_after=$treeAfter }
    $receipt = @{ source_sha=$sourceSha; source_tree=$sourceTree; rust=$rust; node=$nodeVersion; platform='Windows native synthetic'; started_utc=$startedUtc; tracked_changes_before=$before; tracked_changes_after=$after; results=$results }
    $receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'results.json') -Encoding utf8
    Write-Output "$($step.Id) $outcome $($watch.Elapsed.TotalSeconds.ToString('F1'))s - $($step.Name)"
    if (-not $sourceValidBefore -or -not $sourceValidAfter) { throw 'Qualification stopped: source drift invalidates exact-HEAD evidence' }
}
if (@($results | Where-Object status -ne 'PASS').Count -ne 0 -or $after.Count -ne 0) { exit 1 }
