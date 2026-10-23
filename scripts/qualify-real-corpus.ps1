param(
    [Parameter(Mandatory = $true)][string]$ApprovedRoot,
    [Parameter(Mandatory = $true)][string]$EvidenceDirectory,
    [Parameter(Mandatory = $true)][string]$ManifestFile,
    [switch]$Release
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Native Windows required' }
if ((Get-Volume -DriveLetter D).FileSystem -ne 'NTFS') { throw 'D must be NTFS' }
$approved = [IO.Path]::GetFullPath($ApprovedRoot).TrimEnd('\')
$evidence = [IO.Path]::GetFullPath($EvidenceDirectory).TrimEnd('\')
$manifest = [IO.Path]::GetFullPath($ManifestFile)
if ($approved -cnotmatch '^D:\\' -or $evidence -cnotmatch '^D:\\') { throw 'Only D drive allowed' }
if (-not (Test-Path -LiteralPath $approved -PathType Container)) { throw 'Approved source missing' }
if (Test-Path -LiteralPath $evidence) { throw 'Evidence must be fresh' }
if ($evidence.StartsWith($approved + '\', [StringComparison]::OrdinalIgnoreCase) -and $evidence -notmatch '^D:\\MindPlaceQualification\\') {
    throw 'Evidence must not be within source repositories'
}
if ($evidence -notmatch '^D:\\MindPlaceQualification\\') { throw 'Use D:\MindPlaceQualification\unique-run as evidence' }
$ancestor = $evidence
while ($ancestor) {
    if (Test-Path -LiteralPath $ancestor) {
        if ((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Evidence path crosses reparse point'
        }
    }
    $parent = Split-Path -Parent $ancestor
    if ($parent -eq $ancestor) { break }
    $ancestor = $parent
}
$root = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot)).TrimEnd('\')
$head = (& git -C $root rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Git unavailable' }
$baseline = (& git -C $root merge-base HEAD '08b310b2b14639594edcf5210d9a28b8b677846f').Trim()
if ($baseline -ne '08b310b2b14639594edcf5210d9a28b8b677846f') { throw 'Wrong baseline' }
$items = Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json
if ($items.Count -lt 5 -or $items.Count -gt 50) { throw 'Manifest needs 5 to 50 samples' }
$allowed = @('.md','.txt','.rs','.ts','.tsx','.py','.pdf','.docx','.pptx','.xlsx')
$fileNames = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
$planned = @()
foreach ($item in $items) {
    $src = [IO.Path]::GetFullPath([string]$item.source)
    $dest = [string]$item.name
    if (-not $src.StartsWith($approved + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Source outside approved root' }
    if ($src -match '(?i)[\\/](node_modules|\.git|target|vendor|\.venv|\.cache|uploads|payment-proofs|secrets|credentials)[\\/]') { throw 'Sensitive or generated source not allowed' }
    if ($dest -cnotmatch '^[a-zA-Z0-9][a-zA-Z0-9_\-]*\.[a-zA-Z0-9]+$') { throw 'Invalid staged filename' }
    if (-not $fileNames.Add($dest)) { throw 'Duplicate staged name' }
    if ([IO.Path]::GetExtension($src).ToLowerInvariant() -ne [IO.Path]::GetExtension($dest).ToLowerInvariant()) { throw 'Extension mismatch' }
    if ($allowed -notcontains [IO.Path]::GetExtension($src).ToLowerInvariant()) { throw 'Unsupported extension' }
    $cursor = $src
    while ($cursor -and $cursor.Length -ge $approved.Length) {
        $entry = Get-Item -LiteralPath $cursor -Force
        if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse point in source path' }
        $cursor = Split-Path -Parent $cursor
    }
    $sourceFile = Get-Item -LiteralPath $src -Force
    if ($sourceFile.Length -gt 5MB) { throw '5 MiB per-file cap' }
    $planned += [pscustomobject]@{Source=$src;Dest=$dest;Size=$sourceFile.Length}
}
if ($fileNames.Contains('local_revision_probe.md')) { throw 'Marker collision' }
$sourceHashes = @($planned | ForEach-Object { (Get-FileHash -LiteralPath $_.Source -Algorithm SHA256).Hash })
New-Item -ItemType Directory -Path (Join-Path $evidence 'staging') -Force | Out-Null
foreach ($file in $planned) {
    Copy-Item -LiteralPath $file.Source -Destination (Join-Path (Join-Path $evidence 'staging') $file.Dest) -ErrorAction Stop
}
[IO.File]::WriteAllText((Join-Path $evidence 'staging\local_revision_probe.md'), "mp4l2olduniquemarker2026`n")
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:MP_REAL_STAGING = Join-Path $evidence 'staging'
$env:MP_REAL_EVIDENCE = $evidence
$env:MP_REAL_PRODUCT_SHA = $head
if (& git -C $root status --porcelain -- crates src src-tauri extension) { throw 'Commit product changes before qualification' }
$env:CARGO_TARGET_DIR = 'D:\MindPlaceQualification\mp4l2-real-cargo-target'
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Rust toolchain unavailable' }
$rust = (& rustc -Vv) -join "`n"
$summary = [pscustomobject]@{
    Schema = 'mp-4l2-real-staging-v1'; NativeOS = [Environment]::OSVersion.VersionString
    HarnessSha = $head; ProductBaseline = '08b310b2b14639594edcf5210d9a28b8b677846f'
    BuildProfile = $(if ($Release) { 'release' } else { 'debug' })
    Rust = $rust; SampleCount = $planned.Count; InputBytes = ($planned | Measure-Object -Property Size -Sum).Sum
    SourceRoots = 'D drive / owner-approved projects; private paths withheld'
}
$summary | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $evidence 'run-metadata.json')
Push-Location $root
try {
    $processInfo = New-Object System.Diagnostics.ProcessStartInfo
    $processInfo.FileName = (Get-Command cargo -ErrorAction Stop).Source
    $processInfo.Arguments = $(if ($Release) { 'run --locked --release -p mind-place-index --example qualify_real_corpus' } else { 'run --locked -p mind-place-index --example qualify_real_corpus' })
    $processInfo.WorkingDirectory = $root
    $processInfo.UseShellExecute = $false
    $processInfo.RedirectStandardOutput = $true
    $processInfo.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::Start($processInfo)
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $exitCode = $process.ExitCode
    [IO.File]::WriteAllText((Join-Path $evidence 'stdout.log'), $stdoutTask.GetAwaiter().GetResult())
    [IO.File]::WriteAllText((Join-Path $evidence 'stderr.log'), $stderrTask.GetAwaiter().GetResult())
    $process.Dispose()
} finally { Pop-Location }
$sourceUnchanged = $true
for ($i = 0; $i -lt $planned.Count; $i++) {
    if ((Get-FileHash -LiteralPath $planned[$i].Source -Algorithm SHA256).Hash -ne $sourceHashes[$i]) {
        $sourceUnchanged = $false
    }
}
$summary | Add-Member -NotePropertyName SourceHashesUnchanged -NotePropertyValue $sourceUnchanged
$summary | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath (Join-Path $evidence 'run-metadata.json')
if (-not $sourceUnchanged) { throw 'Source content changed during qualification' }
if (Test-Path (Join-Path $evidence 'real-receipt.json')) {
    $receipt = Get-Content -LiteralPath (Join-Path $evidence 'real-receipt.json') -Raw | ConvertFrom-Json
    Write-Host ('REAL_CORPUS=' + $receipt.status + ' FILES=' + $receipt.initial.files + ' FORMATS=' + ($receipt.formats.PSObject.Properties.Name -join ','))
    $receipt | ConvertTo-Json -Depth 9
}
if ($exitCode -ne 0) {
    Get-Content -LiteralPath (Join-Path $evidence 'stderr.log') -Tail 45
    throw ('Real corpus native process failed exit ' + $exitCode)
}
