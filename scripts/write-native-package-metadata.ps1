param([Parameter(Mandatory=$true)][string]$PackageRoot)
$ErrorActionPreference = 'Stop'
$packageRoot = [IO.Path]::GetFullPath($PackageRoot).TrimEnd([IO.Path]::DirectorySeparatorChar)
if (!(Test-Path -LiteralPath $packageRoot -PathType Container)) { throw 'Package directory is missing' }
if ((Get-Item -LiteralPath $packageRoot -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Package directory must not be a link' }

$entries = @(Get-ChildItem -LiteralPath $packageRoot -Recurse -Force)
if ($entries.Count -gt 20000) { throw 'Package entry limit exceeded' }
foreach ($entry in $entries) {
    if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Package entries must not be links' }
}
$checksumPath = Join-Path $packageRoot 'SHA256SUMS.txt'
$files = @($entries | Where-Object { !$_.PSIsContainer -and $_.FullName -ne $checksumPath } | Sort-Object FullName)
$names = @()
foreach ($file in $files) {
    $relative = $file.FullName.Substring($packageRoot.Length + 1).Replace('\', '/')
    # These names become fixed NSIS instructions, not an editable runtime deletion list.
    # Reject NSIS interpolation, traversal and wildcard syntax before compilation.
    if ($relative -notmatch '^[a-zA-Z0-9_.+ /-]+$' -or @($relative.Split('/') | Where-Object { $_ -eq '.' -or $_ -eq '..' -or $_ -eq '' }).Count) {
        throw "Unsupported package filename: $relative"
    }
    $names += $relative
}
$checksumLines = for ($i = 0; $i -lt $files.Count; $i++) {
    $hash = Get-FileHash -LiteralPath $files[$i].FullName -Algorithm SHA256
    "$($hash.Hash.ToLowerInvariant())  $($names[$i])"
}
$checksumLines | Set-Content -LiteralPath (Join-Path $packageRoot 'SHA256SUMS.txt') -Encoding utf8

$manifest = [Collections.Generic.List[string]]::new()
$manifest.Add('; Generated from this release package. Never use RMDir /r on an install directory.')
$manifest.Add('!macro CK_REMOVE_PACKAGE_FILES')
foreach ($relative in @($names + 'SHA256SUMS.txt' | Sort-Object -Unique)) {
    if ($relative -eq 'uninstall.exe') { continue }
    $manifest.Add('  Push "' + $relative.Replace('/', '\') + '"')
    $manifest.Add('  Call un.RemovePackageFile')
}
$manifest.Add('!macroend')
$manifest.Add('!macro CK_REMOVE_PACKAGE_DIRECTORIES')
$directories = @($entries | Where-Object PSIsContainer | ForEach-Object {
    $_.FullName.Substring($packageRoot.Length + 1).Replace('\', '/')
} | Sort-Object @{Expression={($_.Split('/')).Count};Descending=$true}, @{Expression={$_};Descending=$true})
foreach ($relative in $directories) {
    if ($relative -notmatch '^[a-zA-Z0-9_.+ /-]+$' -or @($relative.Split('/') | Where-Object { $_ -eq '.' -or $_ -eq '..' -or $_ -eq '' }).Count) { throw 'Invalid package directory name' }
    $manifest.Add('  Push "' + $relative.Replace('/', '\') + '"')
    $manifest.Add('  Call un.RemovePackageDirectory')
}
$manifest.Add('!macroend')
$manifest.Add('!macro CK_VALIDATE_INSTALL_PACKAGE_PATHS')
foreach ($relative in @($names + $directories + 'SHA256SUMS.txt' + 'uninstall.exe' | Sort-Object -Unique)) {
    $manifest.Add('  Push "' + $relative.Replace('/', '\') + '"')
    $manifest.Add('  Call CheckInstallPackagePath')
}
$manifest.Add('!macroend')
# Keep build instructions outside the ZIP and installed program directory.
$manifest | Set-Content -LiteralPath ($packageRoot + '.uninstall.nsh') -Encoding utf8
Write-Output "Package metadata: $packageRoot"
