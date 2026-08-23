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
- Added a globally serialized active-process registry for the shared `latest.log`, stable duplicate error, PID/exit tracking, one-shot started/exited/error events, cleanup on exit/spawn failure, and a 128-record terminal history bound.
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
- Global launch serialization intentionally trades simultaneous multi-profile play for deterministic ownership of the shared `logs/latest.log`. Per-profile concurrent launches require a future coordinated/per-profile log sink before they can be enabled safely.

## Round 1 review remediation

Addressed every Round 1 finding:

- Metadata JVM arguments now reject all Java alternate launch modes (`-jar`, `-m`, `--module`, `--module=...`) and every leading `@` argument-file form. The main class must match a strict dotted Java identifier grammar and cannot be an option, argument file, path, descriptor, or malformed qualified name.
- Logging accepts only Mojang's exact `-Dlog4j.configurationFile=${path}` property template. It substitutes one launcher-validated local log-configuration file and rejects raw/outside paths, URLs, extra controls, alternate properties, and repeated placeholders.
- Child waiting now returns the OS exit code together with an optional auxiliary output-capture error. A stdout/stderr drain or log-write failure is stored and emitted as a separately sanitized error while `launcher://game-exited` is still emitted exactly once with the real exit code.
- Launches are globally serialized while `logs/latest.log` is shared. A second profile receives the stable `game_already_running` error before context preparation or spawning.
- Immediate pre-spawn path validation is tested through an injected path inspector that deterministically simulates reparse rejection on every platform; the test no longer depends on Windows symbolic-link privilege.

Round 1 TDD evidence:

1. Malicious launch-mode and malformed-main-class regressions failed because the builder accepted them; the focused launcher suite passed after strict filtering and grammar validation.
2. Raw external logging paths initially built successfully; exact-template validation made the external path, URL, duplicate-placeholder, control-character, alternate-property, and traversal cases pass.
3. A different-profile launch initially reached context preparation and returned `internal_error`; global reservation now returns `game_already_running` and leaves the spawner at one command.
4. A post-exit output failure initially recorded no exit code. The outcome split now records exit code `23`, one auxiliary `game_log_unavailable` error, and exactly one matching exit event without serializing its test token.

Fresh Round 1 verification:

- `cargo fmt --manifest-path app/src-tauri/Cargo.toml -- --check`: passed.
- `cargo test --manifest-path app/src-tauri/Cargo.toml`: passed — 130 library tests and 2 integration tests, 0 failures.
- `cargo clippy --manifest-path app/src-tauri/Cargo.toml --all-targets -- -D warnings`: passed with 0 warnings/errors.

## Fix 2 review remediation

Closed the Java source-file-mode bypass by replacing the JVM denylist with an exact known-safe Mojang allowlist:

- The only retained metadata JVM options are Mojang's fixed Windows heap-dump option, `-Xss1M`, `-XstartOnFirstThread`, the three native work-directory properties bound exactly to the launcher-validated natives directory, and launcher brand/version properties bound exactly to backend values.
- Metadata's `-Djava.library.path` is accepted only when its value exactly equals the validated natives directory and is then discarded in favor of the backend-owned option.
- The only accepted two-token forms are the classpath aliases, whose operand must exactly equal the backend-built classpath and cannot begin with `@` or `-`. Missing operands remain a stable malformed-argument error; hostile or unexpected operands return `unsafe_launch_argument`.
- All unrecognized options and all non-option JVM operands before the backend main class are rejected. This explicitly covers `--source`, `--source=...`, Java source paths, premature class-name operands, numeric operands, arbitrary words, argument files, and unknown system properties.

Fix 2 TDD evidence:

1. The exact sequence `--source 21 ${game_directory}\\libraries\\evil\\Evil.java` initially produced a valid `PreparedLaunch`; the strict parser now rejects it before command construction.
2. Regression cases cover `--source=21`, absolute/substituted and relative `.java` operands, slash paths, main-class-like operands, numbers, arbitrary non-options, unknown properties, and classpath values beginning with `@`/`-` or differing from the verified classpath.
3. The modern builder fixture now exercises Mojang's valid heap-dump, stack, native-workdir, launcher identity, owned native-path, and exact classpath metadata forms and confirms they retain safe precedence.

Fresh Fix 2 verification:

- `cargo fmt --manifest-path app/src-tauri/Cargo.toml -- --check`: passed.
- `cargo test --quiet --manifest-path app/src-tauri/Cargo.toml`: passed — 131 library tests and 2 integration tests, 0 failures.
- `cargo clippy --manifest-path app/src-tauri/Cargo.toml --all-targets -- -D warnings`: passed with 0 warnings/errors.
