# Task 11 report — Windows hardening, package, and acceptance

## Status

DONE_WITH_CONCERNS

The unsigned Windows x64 executable and NSIS installer were built and passed a bounded
clean-profile startup smoke test. Automated checks passed. Real Microsoft OAuth and a
licensed Minecraft launch are intentionally **BLOCKED_EXTERNAL** because this build has no
provided Entra public-client ID or licensed test account.

## Hardening completed

- Closed the final integration gaps: Entra uses an exact
  `http://localhost:<ephemeral-port>/callback` redirect with an IPv4 loopback listener;
  cancellation reaches OAuth, metadata HTTP, Java discovery/download/extraction, and the
  Minecraft installer; all pre-spawn cancellations release their operation reservation.
- Nonzero or unavailable game exit codes now terminate as retryable `game_exit` failures,
  preserve the exit code and sanitized log path, and expose only a no-argument backend action
  for opening `%APPDATA%\CKLauncher\logs\latest.log`.
- The backend-owned folder picker is the only way to change `gameDir`. Selected absolute
  directories are created, canonicalized, persisted, and checked component-by-component for
  links/reparse points; direct install, orchestrated install, and launch share the profile root.
- Active-account switching now takes the same `AccountMutationCoordinator` lock used by
  login/removal and launch token snapshots.

- Replaced Tauri's broad `core:default` capability with only event `listen`/`unlisten` and
  the four custom-title-bar window actions used by the frontend. No shell, HTTP,
  filesystem, dialog, or opener plugin capability is granted to the webview.
- Added a release CSP. The webview can use its local assets, Tauri IPC, and HTTPS account
  head images, but has no general frontend network connection permission.
- Kept the requested `ru.cklauncher.app` identifier, NSIS target, undecorated window,
  product name, icons, and added product description/Cargo metadata. The Cargo package now
  produces `ck-launcher.exe` rather than the template `app.exe`.
- Confirmed `%APPDATA%\CKLauncher` creates `launcher.sqlite3`, `runtime`, `game`, and
  `logs` at startup. Updated `.gitignore` for generated application/runtime data,
  screenshots, and partial downloads without excluding source assets or migrations.
- Added `app/README.md`, `app/docs/windows-acceptance.md`, and
  `app/docs/privacy-and-logs.md`, including exact Entra public-client registration,
  runtime environment injection, artifact locations, data handling, and log-sharing
  guidance. No client ID was invented or committed; a missing value continues to return
  `auth_not_configured`.

## Verification

- `npm.cmd test` — PASS: 8 files, 31 tests.
- `npm.cmd run build` — PASS: TypeScript check and Vite production build (51 modules).
- `cargo fmt --all --check` — PASS.
- `cargo clippy --all-targets -- -D warnings` — PASS.
- `cargo test --all-targets --quiet` — PASS: 160 library tests and 2 integration tests,
  0 failures.
- `npm.cmd run tauri -- build` — PASS: release `ck-launcher.exe` and NSIS bundle created.
- `git diff --check` — PASS before commit.

The requested literal `pnpm tauri build` was attempted first. This npm-lockfile project has
no committed pnpm lockfile; pnpm 11 tried to repair `node_modules` and rejected esbuild's
postinstall pending interactive approval. Its generated temporary pnpm files were removed.
The artifact was therefore packaged through the repository's declared npm lockfile and the
same local Tauri CLI (`npm.cmd run tauri -- build`).

## Artifacts

- `app/src-tauri/target/release/ck-launcher.exe` — 18,724,352 bytes —
  SHA-256 `41B9A1FF55B919F9936CEF5FDC77C581060A746F0738AA4BE22677D3250477F3`
- `app/src-tauri/target/release/bundle/nsis/ЦК Лаунчер_0.1.0_x64-setup.exe` —
  5,238,832 bytes — SHA-256
  `5D3D3D4AF95B5BE9379D8B3968221B4C0DD3F3DD2954098AA50143D03B3C6704`

## Clean-profile smoke

The release executable was launched once with a new temporary `APPDATA` directory. After
six seconds, the exact launched PID was alive with a nonzero main-window handle, and SQLite
plus `runtime`, `game`, and `logs` all existed under `CKLauncher`. Only that PID was stopped;
the temporary profile was then removed. A final check confirmed neither the smoke process nor
the temporary directory remained.

## Concerns and acceptance blockers

- **PASS:** automated checks, package creation, and local clean-profile startup.
- **BLOCKED_EXTERNAL:** real OAuth needs an Entra public-client registration and test
  Microsoft account; Minecraft launch also needs a licensed Java Edition account, network
  downloads, and an installed compatible runtime. None was supplied, so no real login or
  game launch is claimed.
- Tauri warns that the required identifier ends in `.app`, which can conflict with macOS
  conventions. This Windows-only release retains the binding-spec identifier.
- The local Cargo wrapper emitted a non-failing `could not canonicalize path C:\Users\kvand`
  warning under sandboxed verification. All Rust commands exited successfully using the
  checked-in local toolchain.
