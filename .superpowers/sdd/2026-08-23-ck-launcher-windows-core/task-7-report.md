# Task 7 report — Verified Vanilla Minecraft installer

## Status

Implemented the Task 7 Vanilla installer, secure native extraction, persistent installation state, operation registry, Tauri commands, and progress event bridge.

Commit: `feat: install verified Vanilla Minecraft files`. The final hash is recorded in the task handoff because a Git commit cannot contain its own hash.

## Delivered behavior

- Added `Installer::plan` and `Installer::install`, with injectable version, verified-download, installation-store, and progress boundaries for deterministic tests.
- Plans the resolved version JSON, client JAR, logging configuration, asset index, verified asset objects, ordinary libraries, and only the selected Windows x64 native classifier.
- Applies Mojang rules with last-matching-action semantics for Windows x64, deterministic feature defaults, OS architecture/version matching, and no-OS rules.
- Supports safe Maven `group:name:version[:classifier][@ext]` paths, explicit artifact metadata paths/URLs, and legacy libraries by obtaining a finite HEAD `Content-Length` before passing the file to Task 6's exact-size verified queue.
- Downloads the asset index in the first verified queue phase and expands content-addressed objects only after its size/SHA-1 and JSON structure verify. Object hashes must be exactly 40 hexadecimal characters.
- Materializes Task 4's already-verified resolved version JSON through a unique temporary file and rollback-capable replacement; all network artifacts use Task 6's `.part`, integrity, retry, cancellation, reserved-namespace, and replacement behavior.
- Extracts native ZIPs into unique version-specific staging directories, rejects traversal, absolute/prefixed paths, Windows alternate streams/trailing-dot names, symbolic links, oversized entries, and reparse boundaries, and excludes `META-INF` plus metadata `extract.exclude` prefixes.
- Recovers stale native staging/backup directories, preserves the previous native destination until complete extraction, rolls back activation failure, and never exposes a partial destination.
- Persists `installing`, `failed`, `cancelled`, or `verified`; `verified_at` is present only for the verified state, which is written after every download, asset expansion, and native activation succeeds.
- Added a concurrent operation registry that rejects duplicate installs of one version, makes cancellation idempotent, retains queryable terminal status, and embeds the operation id in progress.
- Added `install_version`, `cancel_operation`, and `installation_status`; install returns its operation id before spawning work on the Tauri Tokio runtime. `launcher://progress` exposes only a destination filename, never a local path or URL/query secret.
- Kept frontend capabilities and account/credential boundaries unchanged.

## TDD evidence

Observed red/green cycles:

1. The fixture planning suite first failed because the installer module did not exist. The minimal planning implementation made the six rule/path/asset tests pass.
2. The native/operation suite first failed on missing extraction and registry interfaces. Transactional extraction, recovery, duplicate prevention, and idempotent cancellation made all focused cases pass.
3. Installer failure/cancellation tests first failed on missing dependency boundaries and `Installer`; the implementation now propagates the original retry-exhaustion error and never records `verified` on either path.
4. The local legacy-library test initially failed with `download_network_failed`: Reqwest's semantic HEAD body length was zero even though the response header carried the artifact size. Parsing the explicit `Content-Length` header made the HEAD-preflight plus real Task 6 GET pass.

The suite also covers fixture contents, explicit artifact paths, malformed coordinates/hashes/version ids, modern and legacy argument metadata preservation, local HTTP success, retry-error propagation, native excludes/traversal/link rollback, stale staging recovery, cancellation, duplicate operations, sanitized progress, and installation-state persistence.

## Verification

Fresh verification from the Task 7 worktree:

- `cargo fmt --all --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed with 0 project warnings/errors.
- `cargo test --all-targets --quiet`: passed — 101 library tests and 2 integration tests, 0 failures. Installer network tests use loopback HTTP only.
- `git diff --check`: passed before the report; repeated immediately before commit.

Frontend checks were not required because Task 7 registers Rust commands/events without changing the existing TypeScript DTO boundary or frontend code.

## Self-review and concerns

- Path/reparse validation occurs immediately before every installer create, extract, rename, restore, and delete boundary. As documented in Task 2, path-based validation cannot completely eliminate a malicious same-user TOCTOU swap without a future handle-relative/no-follow filesystem layer.
- Legacy Mojang libraries without size/hash metadata require the artifact host to support HEAD and return a valid `Content-Length`; bodies still download only through Task 6 and must match that exact size.
- The installed version JSON is the resolved, normalized Task 4 representation rather than a byte-for-byte copy of a child metadata document. Its source document has already passed Task 4's SHA/cache checks, and the normalized bytes are independently hashed and safely replaced.
