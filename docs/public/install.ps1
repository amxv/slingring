# Install or upgrade the Windows x64 Slingring CLI and private browse helper.
# Current Windows build has CLI/MCP and per-user startup, not a tray UI.
param(
    [switch]$Upgrade,
    [switch]$Check,
    [string]$Target = '',
    [int]$WaitForPid = 0
)
$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitOperatingSystem) { throw 'Windows x64 required' }
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\Slingring'
if (-not $Target) { $Target = Join-Path $installDir 'slingring.exe' }
if (-not [IO.Path]::IsPathFullyQualified($Target)) { throw 'Installation path must be absolute' }
$installDir = Split-Path -Parent $Target
if ($Check) {
    if (Test-Path $Target) { & $Target --version } else { Write-Output 'Slingring is not installed' }
    exit
}
if ($Upgrade -and -not (Test-Path $Target -PathType Leaf)) { throw 'No installed CLI to upgrade' }
if (-not $Upgrade -and (Test-Path $Target)) { throw 'Already installed; run slingring upgrade' }

$version = $env:CLI_VERSION
if (-not $version) {
    if (Get-Command gh.exe -ErrorAction SilentlyContinue) {
        $version = (& gh.exe api repos/amxv/slingring/releases/latest --jq .tag_name 2>$null)
    }
    if (-not $version) {
        $release = Invoke-RestMethod 'https://api.github.com/repos/amxv/slingring/releases/latest'
        $version = $release.tag_name
    }
}
$version = $version -replace '^v', ''
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Invalid release version' }
$asset = 'slingring-x86_64-pc-windows-msvc.zip'
$base = "https://github.com/amxv/slingring/releases/download/v$version"
$stage = Join-Path $env:TEMP ('slingring-install-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
function Download-Release([string]$Name, [string]$Output) {
    if (Get-Command gh.exe -ErrorAction SilentlyContinue) {
        & gh.exe release download "v$version" --repo amxv/slingring --pattern $Name --output $Output
        if ($LASTEXITCODE -eq 0) { return }
    }
    Invoke-WebRequest "$base/$Name" -OutFile $Output
}
try {
    $archive = Join-Path $stage $asset
    Download-Release $asset $archive
    $sum = Join-Path $stage 'checksum'
    Download-Release "$asset.sha256" $sum
    $expected = ((Get-Content $sum -Raw).Trim() -split '\s+')[0]
    if ($expected -notmatch '^[a-fA-F0-9]{64}$') { throw 'Invalid release SHA256' }
    if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne $expected) {
        throw 'Release SHA256 mismatch; installation unchanged'
    }
    Add-Type -AssemblyName System.IO.Compression
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $names = @($zip.Entries | ForEach-Object { $_.FullName })
        if (@($names | Where-Object { $_ -eq 'slingring.exe' }).Count -ne 1) {
            throw 'Release archive is missing the CLI'
        }
        foreach ($name in $names) {
            if ($name -notin @('slingring.exe', 'LICENSE', 'README.md')) {
                throw "Unexpected archive member: $name"
            }
        }
    } finally { $zip.Dispose() }
    $unpacked = Join-Path $stage 'unpacked'
    Expand-Archive -LiteralPath $archive -DestinationPath $unpacked
    $candidate = Join-Path $unpacked 'slingring.exe'
    if ((& $candidate --version) -ne "slingring $version") {
        throw 'Release binary version mismatch'
    }
    # Install and verify the pinned browse helper first. The existing CLI is
    # untouched if Chrome helper installation fails or its release is missing.
    & $candidate browser install
    if ($LASTEXITCODE -ne 0) { throw 'Browser helper setup failed; existing CLI preserved' }
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    if ($Upgrade) {
        $status = & $Target status --json | ConvertFrom-Json
        if ($status.active_tasks -gt 0) { throw 'Active tasks detected; refusing upgrade' }
        if ($status.running) {
            & $Target service stop | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'Could not stop existing daemon' }
        }
    }
    if ($WaitForPid -gt 0) {
        Wait-Process -Id $WaitForPid -ErrorAction SilentlyContinue
    }
    $backup = Join-Path $stage 'previous.exe'
    if (Test-Path $Target) { Move-Item -LiteralPath $Target -Destination $backup }
    try {
        Move-Item -LiteralPath $candidate -Destination $Target -ErrorAction Stop
        if ((& $Target --version) -ne "slingring $version") { throw 'Installed CLI failed validation' }
    } catch {
        Remove-Item $Target -Force -ErrorAction SilentlyContinue
        if (Test-Path $backup) { Move-Item $backup $Target }
        throw
    }
    Write-Output "Installed Slingring $version (Windows x64) to $Target"
    Write-Output 'Run slingring profile list, then slingring setup --profile NAME.'
} finally {
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
    if ($WaitForPid -gt 0) { Remove-Item $MyInvocation.MyCommand.Path -Force -ErrorAction SilentlyContinue }
}
