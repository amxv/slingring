param([Parameter(Mandatory=$true)][string]$Root)
$ErrorActionPreference = 'Stop'
if (-not [IO.Path]::IsPathFullyQualified($Root)) { throw 'Slingring state path must be absolute' }
$version = '0.41.1'
$asset = 'agent-browser-browse-windows-x86_64.tar.gz'
$base = Join-Path $Root 'browser'
$target = Join-Path $base $version
$native = Join-Path $target 'agent-browser-browse-native.exe'
New-Item -ItemType Directory -Force -Path $base | Out-Null
$stage = Join-Path $base ('.install-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    if (-not (Test-Path $native -PathType Leaf) -or
        ((& $native --version) -ne "agent-browser $version")) {
        if (Test-Path $target) { throw 'Invalid existing browser installation; refusing to overwrite it' }
        $url = "https://github.com/amxv/agent-browser-browse/releases/download/v$version"
        $archive = Join-Path $stage $asset
        Invoke-WebRequest "$url/$asset" -OutFile $archive
        $checksums = Join-Path $stage 'SHA256SUMS'
        Invoke-WebRequest "$url/SHA256SUMS" -OutFile $checksums
        $rows = @(Get-Content $checksums | Where-Object { $_ -match "^([a-fA-F0-9]{64})\s+$([regex]::Escape($asset))$" })
        if ($rows.Count -ne 1) { throw 'Missing or ambiguous browser SHA256' }
        $expected = ($rows[0] -split '\s+')[0]
        if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne $expected) { throw 'Browser SHA256 mismatch' }
        $members = & tar.exe -tzf $archive
        if ($LASTEXITCODE -ne 0) { throw 'Invalid browser archive' }
        foreach ($member in $members) {
            $name = $member -replace '^\./', ''
            if ($name -notin @('', 'agent-browser-browse-native.exe', 'LICENSE.upstream', 'UPSTREAM_VERSION', 'UPSTREAM_COMMIT', 'PATCH_SHA256')) {
                throw "Unsafe browser archive entry: $member"
            }
        }
        $unpacked = Join-Path $stage 'unpacked'
        New-Item -ItemType Directory -Path $unpacked | Out-Null
        & tar.exe -xzf $archive -C $unpacked
        if ($LASTEXITCODE -ne 0) { throw 'Browser extraction failed' }
        $candidate = Join-Path $unpacked 'agent-browser-browse-native.exe'
        if (-not (Test-Path $candidate -PathType Leaf)) { throw 'Browser executable missing' }
        if ((Get-Content (Join-Path $unpacked 'UPSTREAM_VERSION') -Raw).Trim() -ne $version) { throw 'Browser version mismatch' }
        $stream = [IO.File]::OpenRead($candidate)
        try {
            $reader = [IO.BinaryReader]::new($stream)
            if ($reader.ReadUInt16() -ne 0x5A4D) { throw 'Not a PE executable' }
            $stream.Position = 0x3c
            $offset = $reader.ReadInt32()
            if ($offset -lt 0x40 -or $offset -gt ($stream.Length - 6)) { throw 'Invalid PE header' }
            $stream.Position = $offset
            if ($reader.ReadUInt32() -ne 0x00004550 -or $reader.ReadUInt16() -ne 0x8664) { throw 'Not a Windows x64 executable' }
        } finally { $stream.Dispose() }
        if ((& $candidate --version) -ne "agent-browser $version") { throw 'Browser executable version mismatch' }
        Move-Item $unpacked $target
    }
    $config = Join-Path $base 'config.json'
    if (-not (Test-Path $config)) { [IO.File]::WriteAllText($config, "{}" + [Environment]::NewLine) }
    Write-Output 'Bundled browser helper installed. No separate upstream CLI needed.'
} finally {
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
}
