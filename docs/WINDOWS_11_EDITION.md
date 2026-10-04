# Kvanth Launcher — Windows 10/11 edition

The unified Windows 10/11 package is branded Kvanth Launcher. Its executable is
`kvanth-launcher.exe`; the standalone blue K is used for Windows icons and
the white text-only wordmark is used on loading/error screens, with the blue K
in the sidebar. The existing
`ru.cklauncher.win11` identity is retained for the WebView profile. NSIS uses the
product name for its installation and uninstall registry keys, so the renamed
package installs separately from the old ЦК package. User data and Credential
Manager keys are unchanged; do not delete the shared CKLauncher data directory.

The Kvanth edition is a Tauri/WebView2 package backed by the same
React application and `launcher-core` crate as the standard launcher. It has a
separate application identifier (`ru.cklauncher.win11`), so both editions can
be installed side by side without duplicating launcher features or user data.

## Development

From `app`:

```powershell
npm run tauri:win11:dev
```

## Installer and updates

From the repository root, supply the private Tauri updater signing key:

```powershell
.\scripts\build-win11-curseforge.ps1 -SigningKeyPath <private-key-path>
```

The mode-specific build uses `.env.win11` and the overlay configuration at
`src-tauri/tauri.win11.conf.json`. Ordinary `npm run tauri dev` and
`npm run tauri build` continue to produce the standard edition.

The installer includes the offline WebView2 Evergreen runtime for Windows 10
systems without it; Windows 11 systems with WebView2 already installed skip
that step. Tauri produces a signed NSIS updater artifact. The release must
publish the installer, matching `.sig`, and `latest.json` with the same
signature for **Settings → Check for updates** to work. Do not publish an
unsigned installer or a manifest for another executable.

The two packages intentionally share `%AppData%\CKLauncher`: accounts,
profiles, managed Java runtimes, builds, and settings therefore remain
available in either shell. Secrets stay in Windows Credential Manager and are
never exposed to WebView2.
