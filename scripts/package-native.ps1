param(
    [Parameter(Mandatory=$true)][string]$QtBin,
    [Parameter(Mandatory=$true)][string]$BuildDir,
    [Parameter(Mandatory=$true)][string]$ServicePath,
    [ValidateSet('modern','legacy')][string]$Channel = 'modern'
)
$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent $PSScriptRoot
$distRoot = [IO.Path]::GetFullPath((Join-Path $workspaceRoot 'dist'))
$packageRoot = [IO.Path]::GetFullPath((Join-Path $distRoot $Channel))
if (!$packageRoot.StartsWith($distRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid package path' }
foreach ($directory in @($distRoot, $packageRoot)) {
    if ((Test-Path -LiteralPath $directory) -and ((Get-Item -LiteralPath $directory -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Package path must not be a link' }
}
# Only the generated directory for this validated channel is replaced.
if (Test-Path -LiteralPath $packageRoot) { Remove-Item -LiteralPath $packageRoot -Recurse -Force }
New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $BuildDir 'ck-launcher-qt.exe') -Destination $packageRoot -Force
Copy-Item -LiteralPath $ServicePath -Destination $packageRoot -Force
# Qt 5's MinGW debug heuristic rejects its own release plugins when --release is forced.
# Auto detection uses the linked Qt5Core.dll and accepts the matching MinGW plugins.
$deployArguments = @('--compiler-runtime', '--no-translations', '--dir', $packageRoot)
if ($Channel -eq 'modern') { $deployArguments += '--release' }
else { $deployArguments += @('--no-angle', '--no-opengl-sw', '--no-system-d3d-compiler') }
# Widgets uses raster rendering; do not ship the host's Windows 8+ D3D compiler in Legacy.
$deployArguments += (Join-Path $packageRoot 'ck-launcher-qt.exe')
& (Join-Path $QtBin 'windeployqt.exe') @deployArguments
if ($LASTEXITCODE -ne 0) { throw 'Qt deployment failed' }
# Headless smoke tests must use a shipped plugin, never one from the developer's Qt install.
$platformRoot = Join-Path $packageRoot 'platforms'
New-Item -ItemType Directory -Path $platformRoot -Force | Out-Null
Copy-Item -LiteralPath (Join-Path (Split-Path -Parent $QtBin) 'plugins/platforms/qoffscreen.dll') -Destination $platformRoot -Force
@('[Paths]', 'Prefix=.', 'Plugins=.') | Set-Content -LiteralPath (Join-Path $packageRoot 'qt.conf') -Encoding ascii

Copy-Item -LiteralPath (Join-Path $workspaceRoot 'docs/NATIVE_BUILD.md') -Destination (Join-Path $packageRoot 'README.md') -Force
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'docs/THIRD_PARTY.md') -Destination $packageRoot -Force
Copy-Item -LiteralPath (Join-Path $workspaceRoot 'licenses') -Destination $packageRoot -Recurse -Force
python (Join-Path $PSScriptRoot 'collect-licenses.py') $packageRoot
if ($LASTEXITCODE -ne 0) { throw 'Dependency notices collection failed' }
$checksumPath = Join-Path $packageRoot 'SHA256SUMS.txt'
Get-ChildItem -LiteralPath $packageRoot -File -Recurse | Where-Object { $_.FullName -ne $checksumPath } | Sort-Object FullName | ForEach-Object {
    $hash = Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256
    $relative = $_.FullName.Substring($packageRoot.Length + 1).Replace('\', '/')
    "$($hash.Hash.ToLower())  $relative"
} | Set-Content -LiteralPath $checksumPath -Encoding utf8
Write-Output "Native package: $packageRoot"
