<#
.SYNOPSIS
  Authenticode-sign the portable executable (SHA-256 + RFC 3161 timestamp).
  Runs on the build machine only; the application itself never uses the network.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Path,
  [Parameter(Mandatory)][string]$Thumbprint,
  [string]$TimestampUrl = 'http://timestamp.digicert.com',
  [string]$SignTool = 'signtool.exe'
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Path)) { throw "File not found: $Path" }
& $SignTool sign /fd SHA256 /sha1 $Thumbprint /tr $TimestampUrl /td SHA256 /d 'Migration Assistant' $Path
if ($LASTEXITCODE -ne 0) { throw 'signtool sign failed' }
& $SignTool verify /pa /v $Path
if ($LASTEXITCODE -ne 0) { throw 'signature verification failed' }
& (Join-Path $PSScriptRoot 'write-checksums.ps1') -Folder (Split-Path -Parent (Resolve-Path $Path))
