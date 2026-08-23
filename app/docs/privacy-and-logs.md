# Privacy and logs

## Local data

The launcher stores per-user data under `%APPDATA%\CKLauncher`:

- `launcher.sqlite3` holds public account summaries, profiles, settings, and installation
  state.
- `game` holds downloaded Minecraft files and assets.
- `runtime` holds managed Java runtimes.
- `logs` holds bounded launch diagnostics.

OAuth refresh tokens are stored in Windows Credential Manager, keyed to the account, rather
than in SQLite. Access tokens remain in the Rust process only and are not sent to React.

## Logs and support sharing

Launch logs are bounded and rotated. The logging layer redacts known access-token,
refresh-token, authorization-code, and session-secret values before writing child process
output. Error DTOs returned to the interface use stable codes and sanitized messages.

Logs can still contain Minecraft/Java diagnostic output, local file paths, version IDs,
and a Minecraft account name. Review a copy before sharing it. Never add an OAuth code,
access token, refresh token, client secret, or full credential-manager export to a bug
report. This application does not use a client secret; the Entra client ID is public
configuration rather than a credential.

## Network boundaries

The Rust core, not the webview, contacts Microsoft/Xbox/Minecraft services and official
Minecraft metadata/download endpoints. The webview has no shell, HTTP, filesystem, dialog,
or opener capability. Its only native permissions are the progress-event subscriptions and
the custom window controls it renders.
