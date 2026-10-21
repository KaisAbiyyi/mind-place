param([Parameter(Mandatory)][string]$Archive, [Parameter(Mandatory)][string]$Destination, [Parameter(Mandatory)][string]$PinFile)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$pin = Get-Content -LiteralPath $PinFile -Raw | ConvertFrom-Json
$selected = @($pin.executable.name) + @($pin.resources.PSObject.Properties.Name)
$allowed = $selected + @($pin.excluded_archive_members)
$zip = [IO.Compression.ZipFile]::OpenRead($Archive)
try {
    $seen = @{}
    if ($zip.Entries.Count -ne $allowed.Count) { throw 'Unexpected helper archive members' }
    foreach ($entry in $zip.Entries) {
        if ($entry.FullName -cnotin $allowed -or $seen.ContainsKey($entry.FullName) -or $entry.Length -gt 64MB -or (($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) { throw 'Unsafe helper archive member' }
        $seen[$entry.FullName] = $true
        if ($entry.FullName -cnotin $selected) { continue }
        $folder = if ($entry.FullName -ceq $pin.executable.name) { $Destination } else { Join-Path $Destination 'resources' }
        [IO.Directory]::CreateDirectory($folder) | Out-Null
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $folder $entry.FullName), $true)
    }
} finally { $zip.Dispose() }
