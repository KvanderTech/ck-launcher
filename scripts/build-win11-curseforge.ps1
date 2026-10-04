param(
    [Parameter(Mandatory = $true)]
    [string]$SigningKeyPath
)

$ErrorActionPreference = 'Stop'
$appDirectory = Join-Path $PSScriptRoot '..\app'
$resolvedSigningKey = (Resolve-Path -LiteralPath $SigningKeyPath -ErrorAction Stop).Path
if (-not (Test-Path -LiteralPath $resolvedSigningKey -PathType Leaf)) {
    throw 'The updater signing key file is missing.'
}
$secret = Read-Host 'CurseForge Core API key (hidden; not saved to files)' -AsSecureString
$pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secret)
try {
    $env:TAURI_SIGNING_PRIVATE_KEY = $resolvedSigningKey
    $env:CK_CURSEFORGE_API_KEY = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)
    if ([string]::IsNullOrWhiteSpace($env:CK_CURSEFORGE_API_KEY)) {
        throw 'CurseForge API key is required.'
    }
    Push-Location $appDirectory
    try {
        npm run tauri:win11:build
        if ($LASTEXITCODE -ne 0) { throw "Installer build failed ($LASTEXITCODE)." }
    } finally {
        Pop-Location
    }
} finally {
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:CK_CURSEFORGE_API_KEY -ErrorAction SilentlyContinue
    [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)
    $secret.Dispose()
}
