# ЦК Лаунчер

ЦК Лаунчер is an independent Windows desktop launcher for users who own a
legitimate copy of Minecraft: Java Edition. It provides Microsoft account
sign-in, local profile and instance management, game installation, mod-loader
support, and game launching.

> NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG
> OR MICROSOFT.

## Microsoft and Minecraft authentication

The launcher uses the Microsoft OAuth 2.0 Authorization Code flow with PKCE.
Authentication happens in the user's system browser, and the response is
received by a temporary loopback listener using the registered
`http://localhost` redirect with a dynamically selected local port.
The desktop application is a public client and does not use a client secret.

After Microsoft sign-in, the native Rust process performs the Xbox Live, XSTS,
and Minecraft Services exchanges required to verify ownership and retrieve the
authenticated Minecraft profile. The launcher does not bypass authentication,
ownership, licensing, parental controls, chat safety, or account security.

Application (Client) ID:
`69a61395-3c9e-485e-8662-dcb1bfa73472`

## Privacy and security

- Microsoft credentials are entered only on Microsoft's authorization pages.
- OAuth refresh tokens are stored locally in Windows Credential Manager.
- Access tokens remain in the native launcher process and are not exposed to
  the web interface.
- Tokens are not uploaded to a project server, sold, shared with third parties,
  or used for advertising or analytics.
- Known secrets are removed from launcher diagnostics before logs are exposed.
- The launcher downloads required game metadata and files from the appropriate
  official services and does not redistribute Minecraft.

Additional implementation notes are available in
[`app/docs/privacy-and-logs.md`](app/docs/privacy-and-logs.md) and
[`app/docs/windows-acceptance.md`](app/docs/windows-acceptance.md).

## Technology

- Tauri 2 and Rust
- React 19 and TypeScript
- SQLite for local launcher data
- Windows Credential Manager for OAuth refresh tokens

## Development

Prerequisites:

- Windows 10 or 11 x64
- Node.js and npm
- Rust toolchain with the Windows MSVC target
- WebView2 Runtime

From the `app` directory:

```powershell
npm install
npm run test
npm run tauri -- build
```

Release binaries are intentionally not committed to this repository.

## Legal

Minecraft is a trademark of Microsoft Corporation. This project is independently
developed and is not affiliated with, endorsed by, or sponsored by Mojang or
Microsoft. Users are responsible for complying with the Minecraft EULA and
Minecraft Usage Guidelines.
