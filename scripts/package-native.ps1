param(
    [Parameter(Mandatory=$true)][string]$QtBin,
    [Parameter(Mandatory=$true)][string]$BuildDir,
    [Parameter(Mandatory=$true)][string]$ServicePath,
    [ValidateSet('modern','legacy')][string]$Channel = 'modern'
)
$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent $PSScriptRoot
$packageRoot = Join-Path $workspaceRoot "dist/$Channel"
New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $BuildDir 'ck-launcher-qt.exe') -Destination $packageRoot -Force
Copy-Item -LiteralPath $ServicePath -Destination $packageRoot -Force
& (Join-Path $QtBin 'windeployqt.exe') --release --compiler-runtime --no-translations --dir $packageRoot (Join-Path $packageRoot 'ck-launcher-qt.exe')
if ($LASTEXITCODE -ne 0) { throw 'Qt deployment failed' }
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'docs/NATIVE_BUILD.md') -Destination (Join-Path $packageRoot 'README.md') -Force
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'docs/THIRD_PARTY.md') -Destination $packageRoot -Force
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'licenses') -Destination $packageRoot -Recurse -Force
python (Join-Path $PSScriptRoot 'collect-licenses.py') $packageRoot
if ($LASTEXITCODE -ne 0) { throw 'Dependency notices collection failed' }
Get-ChildItem -LiteralPath $packageRoot -File | ForEach-Object { $hash=Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256; "$($hash.Hash.ToLower())  $($_.Name)" } | Set-Content -LiteralPath (Join-Path $packageRoot 'SHA256SUMS.txt') -Encoding ascii
Write-Output "Native package: $packageRoot"
