param([Parameter(Mandatory)][string]$Folder)
$ErrorActionPreference = 'Stop'
$lines = Get-ChildItem -Path $Folder -File -Recurse |
  Where-Object { $_.Name -ne 'SHA256SUMS.txt' } |
  Sort-Object FullName |
  ForEach-Object {
    $rel = $_.FullName.Substring((Resolve-Path $Folder).Path.Length + 1) -replace '\\', '/'
    '{0}  {1}' -f (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLower(), $rel
  }
$lines | Set-Content -Encoding ASCII (Join-Path $Folder 'SHA256SUMS.txt')
Write-Host "Wrote $(Join-Path $Folder 'SHA256SUMS.txt')"
