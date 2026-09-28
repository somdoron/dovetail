# Run on Windows after the release workflow has packaged dist/.
$ErrorActionPreference = 'Stop'
$assets = Join-Path $PSScriptRoot '../dist'
$installer = Join-Path $PSScriptRoot '../website/installers/install.ps1'
$temporary = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
$destination = Join-Path $temporary 'install dir'
$script:CorruptChecksum = $false

# Exercise the actual installer against locally built release assets.
function Invoke-WebRequest {
    param($Uri, $Headers, $OutFile, [switch]$UseBasicParsing)
    if ($Uri -notlike 'https://github.com/somdoron/dovetail/releases/download/v0.1.4/*') {
        throw "Unexpected download URL: $Uri"
    }
    $asset = ($Uri -split '/')[-1]
    if ($script:CorruptChecksum -and $asset.EndsWith('.sha256')) {
        Set-Content -LiteralPath $OutFile -Value (('0' * 64) + '  dovetail-x86_64-pc-windows-msvc.exe')
    } else {
        Copy-Item -LiteralPath (Join-Path $assets $asset) -Destination $OutFile
    }
}

try {
    & $installer -Version v0.1.4 -InstallDir $destination
    $installed = Join-Path $destination 'dovetail.exe'
    $expected = (Get-FileHash -LiteralPath (Join-Path $assets 'dovetail-x86_64-pc-windows-msvc.exe')).Hash
    if ((Get-FileHash -LiteralPath $installed).Hash -ne $expected) { throw 'Fresh install mismatch' }
    & $installer -Version v0.1.4 -InstallDir $destination
    if ((Get-FileHash -LiteralPath $installed).Hash -ne $expected) { throw 'Upgrade mismatch' }
    $script:CorruptChecksum = $true
    $rejected = $false
    try { & $installer -Version v0.1.4 -InstallDir $destination } catch {
        if ($_.Exception.Message -notlike '*Checksum mismatch*') { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw 'Corrupt checksum was accepted' }
    if ((Get-FileHash -LiteralPath $installed).Hash -ne $expected) { throw 'Failed install changed existing executable' }
    if (@(Get-ChildItem -LiteralPath $destination -Force -Filter '.dovetail-*').Count) { throw 'Staging files were left behind' }
    Write-Host 'Windows installer fresh install, upgrade, and checksum rejection passed.'
} finally {
    if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Recurse -Force }
}
