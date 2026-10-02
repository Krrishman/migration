<#
.SYNOPSIS
  Build the portable Migration Assistant executable (Windows x64).

.EXAMPLE
  .\scripts\build-portable.ps1
  .\scripts\build-portable.ps1 -WebView2FixedRuntime C:\WebView2\Microsoft.WebView2.FixedVersionRuntime.130.0.2849.80.x64
#>
[CmdletBinding()]
param(
  [string]$WebView2FixedRuntime,
  [switch]$SkipTests
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host '==> Installing frontend dependencies (npm ci)'
npm ci
if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }

if (-not $SkipTests) {
  Write-Host '==> Running tests'
  npm run test:all
  if ($LASTEXITCODE -ne 0) { throw 'Tests failed; not building a release.' }
}

Write-Host '==> Building release executable (no installer)'
npm run tauri build -- --no-bundle
if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }

$exe = Join-Path $root 'src-tauri\target\release\MigrationAssistant.exe'
if (-not (Test-Path $exe)) { throw "Executable not found: $exe" }

$out = Join-Path $root 'dist-portable\MigrationAssistant'
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force -Path $out | Out-Null
Copy-Item $exe $out

if ($WebView2FixedRuntime) {
  if (-not (Test-Path (Join-Path $WebView2FixedRuntime 'msedgewebview2.exe'))) { throw 'Not a WebView2 fixed runtime folder.' }
  Write-Host '==> Copying fixed WebView2 runtime'
  Copy-Item -Recurse $WebView2FixedRuntime (Join-Path $out 'WebView2')
}

@"
Migration Assistant (portable)
==============================
Run MigrationAssistant.exe from this folder (for example on a USB drive).
- App data is kept in MigrationAssistantData\ beside the executable when the folder is writable.
- Backups are written to <destination>\migrations\ - by default this folder.
- No installer, no service, no network access, no telemetry.
Use only on computers you are authorized to migrate. See SECURITY.md and PRIVACY.md in the source repository.
"@ | Set-Content -Encoding UTF8 (Join-Path $out 'README.txt')

& (Join-Path $PSScriptRoot 'write-checksums.ps1') -Folder $out
Write-Host "==> Portable build ready: $out"
