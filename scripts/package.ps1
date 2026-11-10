#Requires -Version 5.1
<#
.SYNOPSIS
    Builds the Windows release binary and packages it for GitHub Releases.

.DESCRIPTION
    Produces three files in the output directory (dist by default):

      designer.exe                              portable single-file application
      designer-<version>-windows-x64.zip        executable plus README and license
      SHA256SUMS.txt                            checksums for both downloads

    The same script is used locally and by .github/workflows/release.yml, so the
    artifacts attached to a GitHub release are always reproducible.

.PARAMETER OutputDirectory
    Directory that receives the artifacts. Relative to the repository root.
#>
[CmdletBinding()]
param(
    [string]$OutputDirectory = "dist"
)

$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $root
try {
    $manifest = Select-String -Path "Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $manifest) {
        throw "Could not read the package version from Cargo.toml"
    }
    $version = $manifest.Matches[0].Groups[1].Value

    Write-Host "Building designer $version for Windows x64..."
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }

    $exe = Join-Path $root "target\release\designer.exe"
    if (-not (Test-Path $exe)) {
        throw "Expected $exe to exist after the build"
    }

    $output = Join-Path $root $OutputDirectory
    New-Item -ItemType Directory -Path $output -Force | Out-Null

    $archiveName = "designer-$version-windows-x64"
    $stage = Join-Path $output $archiveName
    if (Test-Path $stage) {
        Remove-Item $stage -Recurse -Force
    }
    New-Item -ItemType Directory -Path $stage -Force | Out-Null

    Copy-Item $exe $stage
    Copy-Item (Join-Path $root "README.md") $stage
    Copy-Item (Join-Path $root "LICENSE") $stage

    # Compress-Archive in Windows PowerShell 5.1 can fail on freshly copied
    # files, so the .NET API is used directly and the archive is built under a
    # temporary name before being moved into place.
    $zip = Join-Path $output "$archiveName.zip"
    $zipTemp = "$zip.tmp"
    if (Test-Path $zipTemp) {
        Remove-Item $zipTemp -Force
    }
    Get-ChildItem $stage -File | ForEach-Object { $_.IsReadOnly = $false }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::CreateFromDirectory(
        $stage,
        $zipTemp,
        [System.IO.Compression.CompressionLevel]::Optimal,
        $false
    )
    Move-Item -Force $zipTemp $zip
    Remove-Item $stage -Recurse -Force

    $portable = Join-Path $output "designer.exe"
    Copy-Item $exe $portable -Force

    $zipHash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    $exeHash = (Get-FileHash $portable -Algorithm SHA256).Hash.ToLowerInvariant()
    @(
        "$zipHash  $archiveName.zip"
        "$exeHash  designer.exe"
    ) | Set-Content (Join-Path $output "SHA256SUMS.txt") -Encoding ascii

    Write-Host ""
    Write-Host "Artifacts written to ${output}:"
    Write-Host "  $portable"
    Write-Host "  $zip"
    Write-Host "  $(Join-Path $output 'SHA256SUMS.txt')"
}
finally {
    Pop-Location
}
