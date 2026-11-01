# Build the release on Windows and package it as dist\ACXS-Manager-<version>-windows-x64.zip.
#
# The zip holds ACXS-Manager.exe, README.md and CHANGELOG.md at its root. The SHA-256 of the zip is
# printed and written to dist\SHA256SUMS.txt. Run it from anywhere:
#
#   powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1 [-DryRun]
param([switch]$DryRun)

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$manifest = Get-Content Cargo.toml -Raw
if ($manifest -notmatch '(?ms)^\[package\].*?^version\s*=\s*"([0-9A-Za-z.+-]+)"') {
    throw 'Could not read the package version from Cargo.toml.'
}
$version = $Matches[1]
$zipName = "ACXS-Manager-$version-windows-x64.zip"
if ($DryRun) {
    Write-Host "Would build and write dist\$zipName"
    return
}

$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (-not (Test-Path $cargo)) { $cargo = 'cargo' }
& $cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build --release failed.' }

$exe = 'target\release\acxs-manager.exe'
if (-not (Test-Path $exe)) { throw "$exe was not produced." }

$stage = Join-Path 'dist' "stage-$version"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $exe (Join-Path $stage 'ACXS-Manager.exe')
Copy-Item README.md, CHANGELOG.md $stage

$zip = Join-Path 'dist' $zipName
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Remove-Item $stage -Recurse -Force

$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $zipName" | Set-Content (Join-Path 'dist' 'SHA256SUMS.txt')
Write-Host "Wrote $zip"
Write-Host "SHA-256 $hash"
