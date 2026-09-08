param(
    [Parameter(Mandatory=$true)][string]$PackageRoot,
    [Parameter(Mandatory=$true)][string]$Makensis,
    [Parameter(Mandatory=$true)][string]$ThemePluginDir,
    [Parameter(Mandatory=$true)][string]$OutFile
)
$ErrorActionPreference = 'Stop'
$workspaceRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$packageRoot = [IO.Path]::GetFullPath($PackageRoot).TrimEnd([IO.Path]::DirectorySeparatorChar)
if (!$packageRoot.StartsWith($workspaceRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Build package must be inside this workspace' }
$stubRoot = Join-Path $workspaceRoot 'build/installer-stubs'
New-Item -ItemType Directory -Path $stubRoot -Force | Out-Null
$emit = Join-Path $stubRoot ((Split-Path -Leaf $packageRoot) + '-emit-uninstaller.exe')
& (Join-Path $PSScriptRoot 'write-native-package-metadata.ps1') -PackageRoot $packageRoot
$arguments = @('/INPUTCHARSET', 'UTF8', "/DTHEME_PLUGIN_DIR=$ThemePluginDir", "/DPACKAGE=$packageRoot")
& $Makensis @arguments '/DCK_UNINSTALL_EMIT' "/DOUTFILE=$emit" (Join-Path $PSScriptRoot 'native-installer.nsi')
if ($LASTEXITCODE -ne 0) { throw 'Uninstaller emitter compilation failed' }
# This special build only writes uninstall.exe to its compiled-in dist path,
# then quits in .onInit. It cannot execute installation or registry sections.
$process = Start-Process -FilePath $emit -ArgumentList '/S' -WindowStyle Hidden -Wait -PassThru
if ($process.ExitCode -ne 0) { throw "Uninstaller emission failed: $($process.ExitCode)" }
$uninstaller = Join-Path $packageRoot 'uninstall.exe'
if (!(Test-Path -LiteralPath $uninstaller -PathType Leaf) -or (Get-Item -LiteralPath $uninstaller).Length -lt 1024) { throw 'Uninstaller was not emitted' }
# ZIP updates must replace old unsafe uninstallers too, with a matching checksum.
& (Join-Path $PSScriptRoot 'write-native-package-metadata.ps1') -PackageRoot $packageRoot
& $Makensis @arguments "/DOUTFILE=$([IO.Path]::GetFullPath($OutFile))" (Join-Path $PSScriptRoot 'native-installer.nsi')
if ($LASTEXITCODE -ne 0) { throw 'Native installer compilation failed' }
Write-Output "Installer and update-compatible uninstaller: $OutFile"
