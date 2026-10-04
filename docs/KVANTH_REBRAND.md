# Kvanth Launcher — Windows 10/11 / WebView2

## Branding

- Windows application, shortcut, installer and uninstaller icons: approved blue/cyan standalone K.
- In-app sidebar: standalone blue/cyan K. Loading/error screens: approved white Kvanth / LAUNCHER wordmark.
- Window title, executable metadata, installer and file association labels: Kvanth Launcher.
- Executable: `kvanth-launcher.exe`.
- Minecraft launcher brand and content/image request user agents: KvanthLauncher.
- Installer publisher: Kvanth.

## Compatibility

The existing `%AppData%\CKLauncher` directory, Windows Credential Manager keys,
community links, source repository URL, and `ru.cklauncher.win11` WebView identity
are intentionally unchanged. Renaming those identifiers without migration would
disconnect existing data or break links.

NSIS registers installations by product name. The new setup creates a Kvanth
installation rather than automatically removing the earlier ЦК installation.
Both use the same launcher data. Do not remove the shared data directory.

The Kvanth WebView edition uses its own Tauri updater signing key and a signed
`latest.json` asset on the newest GitHub release. The older Qt and ЦК update
artifacts are not compatible with this installer. Keep the private signing key
outside Git and back it up securely: losing it prevents future in-app updates.

## Rebuild

Use `scripts/build-win11-curseforge.ps1 -SigningKeyPath <private-key-path>`.
The resulting single NSIS installer targets Windows 10 and 11 x64 and contains
the offline WebView2 Evergreen installer, which runs only when the runtime is
missing. The `win11` suffix remains in internal build paths for compatibility.

Artwork export: run `scripts/generate-kvanth-assets.cjs` with the approved asset
directory and a Node environment containing `sharp`. Generated assets are already
checked into the working tree; ordinary builds do not require this export step.

## Verification

React tests, including the text-only Windows 11 brand test; production frontend
build; Rust launcher argument/process tests; release executable metadata and
generated NSIS script checked. Installation on a clean Windows 11 machine and
an authenticated Minecraft launch are still manual acceptance checks.
