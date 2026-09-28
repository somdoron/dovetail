# Usage: .\install.ps1 [-Version v0.1.0] [-InstallDir "$env:LOCALAPPDATA\Dovetail\bin"]
[CmdletBinding()]
param(
    [string]$Version = 'latest',
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Dovetail\bin')
)
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Use install.sh on macOS or Linux.'
}
$architecture = [Environment]::GetEnvironmentVariable('PROCESSOR_ARCHITEW6432')
if (-not $architecture) { $architecture = $env:PROCESSOR_ARCHITECTURE }
if ($architecture -ne 'AMD64') { throw 'The Windows release supports x86_64 only.' }
if ([string]::IsNullOrWhiteSpace($InstallDir)) { throw 'Install directory must not be empty.' }
$InstallDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($InstallDir)
# Windows PowerShell 5.1 may otherwise negotiate an obsolete TLS version.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$headers = @{ 'User-Agent' = 'dovetail-installer' }
if ($Version -eq 'latest') {
    $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/somdoron/dovetail/releases/latest' -Headers $headers
    $Version = $release.tag_name
}
if ($Version -notmatch '^[a-zA-Z0-9_][a-zA-Z0-9._+-]*$') { throw 'Invalid release tag.' }
$base = "https://github.com/somdoron/dovetail/releases/download/$Version"
$scratch = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
$staged = $null
New-Item -ItemType Directory -Path $scratch | Out-Null
try {
    $asset = 'dovetail-x86_64-pc-windows-msvc.exe'
    $checksum = Join-Path $scratch 'checksum'
    $binary = Join-Path $scratch $asset
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset.sha256" -Headers $headers -OutFile $checksum
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -Headers $headers -OutFile $binary
    $matchesForAsset = @(Get-Content -LiteralPath $checksum | Where-Object { $_ -match ('^[0-9a-fA-F]{64}\s+' + [regex]::Escape($asset) + '$') })
    if ($matchesForAsset.Count -ne 1) { throw "Missing or ambiguous checksum for $asset" }
    $expected = ($matchesForAsset[0] -split '\s+')[0]
    if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash -ne $expected) {
        throw 'Checksum mismatch; existing installation was not changed.'
    }
    & $binary --version
    if ($LASTEXITCODE -ne 0) { throw 'Downloaded dovetail failed its version check.' }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $destination = Join-Path $InstallDir 'dovetail.exe'
    if (Test-Path -LiteralPath $destination -PathType Container) { throw 'Installation target dovetail.exe is a directory.' }
    $staged = Join-Path $InstallDir ('.dovetail-' + [Guid]::NewGuid().ToString() + '.exe')
    Copy-Item -LiteralPath $binary -Destination $staged
    if (Test-Path -LiteralPath $destination) {
        [IO.File]::Replace($staged, $destination, [NullString]::Value)
    } else {
        [IO.File]::Move($staged, $destination)
    }
    Write-Host "Installed Dovetail $Version to $InstallDir"
    if ($InstallDir -notin ($env:PATH -split ';')) {
        Write-Host "Add this directory to your user PATH: $InstallDir"
    }
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
    if ($staged -and (Test-Path -LiteralPath $staged)) { Remove-Item -LiteralPath $staged -Force }
}
