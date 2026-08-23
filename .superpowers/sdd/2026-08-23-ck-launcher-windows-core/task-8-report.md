# Task 8 report — Minecraft command construction and process lifecycle

## Status

Implemented Task 8: safe modern/legacy Minecraft argument resolution, deterministic Windows x64 classpath construction, internal account-token refresh, protected JVM argument precedence, direct child-process spawning, supervision, events, and bounded redacted logs.

Commit: `feat: build and supervise Minecraft process`. The final hash is recorded in the handoff because a commit cannot contain its own hash.

## Delivered behavior

- Added `LaunchCommand { executable, args, cwd }`; its custom `Debug` implementation never renders arguments or access tokens.
- Resolves modern argument arrays and legacy `minecraftArguments`, including conditional Mojang rules/features shared with the installer and all required player/version/directory/assets/natives/classpath substitutions.
- Rejects unresolved required placeholders, malformed conditional values, NULs, missing main classes, and protected/security-sensitive metadata JVM overrides.
- Removes Mojang's launcher-owned native/classpath pair and reconstructs safe precedence: metadata-safe JVM arguments, `-Xms512M`, backend-clamped `-Xmx`, natives, verified logging configuration, exact backend classpath, main class, then game arguments.
- Builds a deterministic `;`-separated Windows classpath from allowed ordinary artifacts plus the client JAR, with case-insensitive deduplication and no native classifier JARs.
- Validates executable, libraries, client JAR, native directory, logging configuration, assets root, and working directory; every prepared path is revalidated for reparse points immediately before the spawner receives the command.
- Keeps ordinary absolute classpath paths for Java 8 compatibility while using canonical trusted paths for containment/reparse validation.
- Added internal refresh-token exchange through the existing Microsoft/Xbox/XSTS/Minecraft chain. The active account and Minecraft access token never cross the DTO boundary; rotated refresh tokens return to Credential Manager.
- Added a direct `tokio::process::Command` spawner with an argument vector, null stdin, piped stdout/stderr, current directory, and `kill_on_drop`; no shell, `cmd.exe`, string join, or frontend process permission is used.
- Added a per-profile active-process registry, stable duplicate error, PID/exit tracking, one-shot started/exited/error events, cleanup on exit/spawn failure, and a 128-record terminal history bound.
- Captures stdout/stderr to `logs/latest.log`, redacts tokens across reader-chunk boundaries, caps the log at 1 MiB, and retains only three rotated predecessors.
- Registered Rust commands `launch` and `launch_status` and events `launcher://game-started`, `launcher://game-exited`, and `launcher://error`.

## TDD evidence

Observed red/green cycles:

1. Builder tests first failed because the launcher modules and API did not exist. The pure builder made modern/legacy substitution, Windows rules/classpath, unknown-placeholder, protected-JVM-argument, and safe-debug cases pass.
2. Lifecycle tests first failed because the spawner, child, event, log, registry, and `Launcher` interfaces did not exist. The supervisor implementation made exact-command, duplicate, exit-code, spawn-failure, event-cardinality, redaction, size, and immediate reparse checks pass.
3. A malicious `-cp C:\evil.jar;C:\other.jar` regression initially passed the filter. Exact equality with the backend-built classpath now rejects it with `unsafe_launch_argument`.
4. A token split between `access-` and `secret` reader chunks initially leaked. The streaming overlap boundary now retains any crossing match until it can redact the complete token.
5. A Windows compatibility regression first observed `\\?\` canonical prefixes in classpath arguments. The builder now validates canonical targets but passes ordinary absolute paths to Java.

Additional tests cover internal token refresh/rotation, log rotation, terminal-history expiry, legacy tokenization, native/library exclusion, classpath deduplication, memory clamping, logging arguments, and process-registry cleanup.

## Verification

Fresh verification from the Task 8 worktree:

- `cargo fmt --manifest-path app/src-tauri/Cargo.toml --check`: passed.
- `cargo test --manifest-path app/src-tauri/Cargo.toml`: passed — 125 library tests and 2 integration tests, 0 failures.
- `cargo clippy --manifest-path app/src-tauri/Cargo.toml --all-targets -- -D warnings`: passed with 0 warnings/errors.
- `git diff --check`: passed before writing this report and is repeated immediately before commit.

Frontend checks were not required because Task 8 adds Rust commands/events without changing TypeScript DTOs or frontend source.

## Self-review and concerns

- Immediate path/reparse validation substantially narrows the launch boundary but remains path-based; as in earlier tasks, a malicious same-user filesystem actor could theoretically race a check without a future handle-relative/no-follow Windows filesystem layer.
- The first release intentionally requires the profile game directory to equal the backend-owned game root. A future explicit custom-directory picker should persist a separately authorized root rather than trusting arbitrary frontend path text.
- Concurrent launches for different profiles are allowed by the per-profile rule, but all processes target the shared `logs/latest.log` convention. Task 9/10 should either serialize global launches or introduce a coordinated multi-process log sink before exposing simultaneous multi-profile play.
