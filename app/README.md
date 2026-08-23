# ЦК Лаунчер

Unsigned Windows x64 Tauri launcher for licensed Minecraft: Java Edition. It stores its
per-user data in `%APPDATA%\CKLauncher`.

## Prerequisites

- Windows 10 or 11 x64, with Microsoft Edge WebView2 Evergreen Runtime.
- Node.js with npm (the committed `package-lock.json` is used).
- Rust stable with the MSVC target and Visual Studio C++ Build Tools.

## Run and build

From `app/`:

```powershell
npm.cmd ci
npm.cmd run tauri dev

npm.cmd test
npm.cmd run build
Push-Location src-tauri
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets --quiet
Pop-Location
npm.cmd run tauri build
```

The direct executable is `src-tauri\target\release\ck-launcher.exe`. The NSIS installer
is written below `src-tauri\target\release\bundle\nsis\`.

## Microsoft sign-in configuration

`CK_LAUNCHER_MICROSOFT_CLIENT_ID` is a public Entra application (client) ID, not a
secret. No client ID is committed or invented by this project. Without it, selecting
Microsoft sign-in deliberately returns the clear `auth_not_configured` state.

Follow [the registration and privacy guide](docs/windows-acceptance.md) before providing
a production build to users. Set the value in the process environment that starts the
launcher, for example:

```powershell
$env:CK_LAUNCHER_MICROSOFT_CLIENT_ID = '<public-application-client-id>'
npm.cmd run tauri dev
```

For an installed build, set the same per-user environment variable, start a new shell or
sign in again, then launch the application. This value is read at runtime, so it is not
embedded by `tauri build`.

See [privacy-and-logs.md](docs/privacy-and-logs.md) for the local data and log policy.

The default Minecraft directory is `%APPDATA%\CKLauncher\game`. Settings can persist a
different absolute directory only through the native backend picker; install and launch use
that same validated profile directory. A nonzero game exit is shown as retryable
`game_exit`, with a backend-only action for opening the sanitized `latest.log`.
