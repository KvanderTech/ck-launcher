# Windows acceptance and Microsoft sign-in configuration

## Scope and build

This release targets Windows 10 and 11 x64. It uses an undecorated Tauri window with
custom minimize, maximize, and close controls; the NSIS bundle and direct executable are
produced by `npm.cmd run tauri build` from `app/`. WebView2 is checked before the Tauri
webview is created and a missing runtime receives an Evergreen Runtime instruction.

The expected paths for a release build are:

- `src-tauri\target\release\ck-launcher.exe`
- `src-tauri\target\release\bundle\nsis\*.exe`

## Entra public-client registration

The application uses Authorization Code with PKCE in the system browser. Its listener binds
only to IPv4 `127.0.0.1` on a random free port, while both OAuth requests use the exact
redirect form `http://localhost:<random-port>`. Microsoft Entra ignores the port
when matching a `localhost` loopback redirect. Register
a **public client** in Microsoft Entra ID as follows:

1. Create an App registration for the Microsoft account population your release supports.
   This implementation uses the `consumers` authority, so select **Personal Microsoft
   accounts** (or an account type that includes them).
2. In **Authentication**, add the platform **Mobile and desktop applications** and add the
   redirect URI `http://localhost`. Do not register `127.0.0.1`, a fixed port, or
   a client secret for this desktop public-client flow.
3. Enable public client flows only if the tenant policy asks for it. The implemented flow
   is authorization-code + PKCE, not a client-secret flow.
4. Copy the **Application (client) ID**. It is public configuration, but it must still be
   the ID of the registration whose redirect and account type match the steps above.
5. Provide it to the launched process as `CK_LAUNCHER_MICROSOFT_CLIENT_ID`. For a local
   acceptance run:

   ```powershell
   $env:CK_LAUNCHER_MICROSOFT_CLIENT_ID = '<public-application-client-id>'
   npm.cmd run tauri dev
   ```

   For an installed release, a deployment owner can set it per user before starting the
   app:

   ```powershell
   [Environment]::SetEnvironmentVariable(
     'CK_LAUNCHER_MICROSOFT_CLIENT_ID',
     '<public-application-client-id>',
     'User'
   )
   ```

   Start a new user session (or a new process inheriting the updated environment) before
   launching `ck-launcher.exe`. The ID is read at runtime and is not bundled into the
   executable.

With no environment value, sign-in must remain unavailable with the stable
`auth_not_configured` error. That is the expected acceptance state for an unsigned build
without a release registration.

Cancelling sign-in interrupts the pending loopback callback and any in-flight token exchange;
it does not leave an OAuth listener reserved for a later attempt.

## Bounded local smoke acceptance

Run the direct release executable with a temporary, empty `APPDATA` directory. Verify that
the `ck-launcher` process and its main window are alive, then verify creation of:

- `%APPDATA%\CKLauncher\launcher.sqlite3`
- `%APPDATA%\CKLauncher\runtime`
- `%APPDATA%\CKLauncher\game`
- `%APPDATA%\CKLauncher\logs`

Close the window or terminate only the PID started for the smoke run. Do not use that
temporary profile as a real user profile.

The default game directory is `%APPDATA%\CKLauncher\game`. A user can choose another
absolute local directory in Settings through the backend-owned folder picker. The backend
creates and canonicalizes that selection, persists it in the active profile, rejects links
and Windows reparse points component by component, and uses the same directory for install,
verification, and launch.

## External acceptance blockers

Real Microsoft OAuth needs the registration above and a test Microsoft account. A real
Minecraft launch additionally needs an account licensed for Minecraft: Java Edition, the
network downloads, and an installed compatible Java runtime. Those external checks are
blocked, not inferred, when no client ID/account is supplied.
