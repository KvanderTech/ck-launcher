# Task 5 report — Java selection, installation, and settings

## Status

Implemented the Task 5 scope for Java 8, 17, 21, and 25 detection, validation, managed installation, Tauri commands/state, and the memory/Java settings components.

Commit: `feat: manage Java runtimes and memory`. The final hash is recorded in the task handoff because a Git commit cannot contain its own final hash.

## Delivered behavior

- Added `RuntimeManager::resolve(JavaRequirement, Option<PathBuf>)` with managed → manual → system priority and exact-major validation.
- Added a process-runner boundary and a production Tokio runner. `java -version` is invoked directly with an argument array, has a five-second timeout, reads stderr/stdout, and rejects timeout/nonzero/malformed results.
- Added a versioned Windows x64 Temurin manifest with pinned HTTPS archive URLs and SHA-256 values for Java 8/17/21/25.
- Added a focused, bounded reqwest archive fetcher behind `RuntimeArchiveFetcher`; it is deliberately not a second general download queue.
- Added checksum-before-extraction, ZIP entry/size limits, traversal/absolute/prefix/link rejection, unique private staging, staged Java probing, and Windows-safe previous/candidate/final rename ordering.
- Added persisted manual overrides. The native backend picker returns a path only to Rust; Rust canonicalizes and probes it, and persistence happens only after an exact-major success.
- Added `runtime_statuses`, `detect_runtime`, `install_runtime`, and `choose_runtime_path`, registered the runtime state, and left the Tauri capability set unchanged.
- Added narrow TypeScript runtime DTOs/API plus `MemorySettings` and `JavaSettings`. The memory slider emits exact 512 MB steps against the backend-provided maximum; Java cards render valid/missing/installing/invalid and disable only the active major's actions.

## TDD evidence

Red/green cycles observed during implementation:

1. `cargo test --test runtime_task5` failed with unresolved `app_lib::runtime`; after the detection API and implementation were added, both detection/priority tests passed.
2. TypeScript `tsc --noEmit` failed because `JavaRuntimeStatus`, `MemorySettings`, and `JavaSettings` did not exist; after the minimal components/DTOs were added, type-checking passed and both focused Vitest files passed.
3. Runtime archive tests failed to compile because `extract_zip_archive`, `RuntimeArchiveManifest`, and `verify_archive_checksum` were absent; after the minimal secure extraction/manifest/checksum implementation, the runtime tests passed.
4. Unsupported-major deserialization initially failed its assertion (`99` was accepted); after custom `JavaRequirement` deserialization, `99` was rejected and `21` remained accepted.
5. Additional fake-runner/fetcher fixtures cover nonzero/timeout probes, all four major versions, priority ordering, checksum failure, unsafe archive entries, failed staged probes, successful replacement, and manual persistence without host Java or network access.

## Verification

All commands were run from the isolated Task 5 worktree with its bundled Rust/Node toolchains.

- `cargo fmt --all`: passed.
- `cargo test --all-targets`: passed — 50 library tests plus 2 Task 5 integration tests, 0 failures.
- `cargo clippy --all-targets -- -D warnings`: passed, 0 warnings/errors from the project (Cargo prints an environment-only `could not canonicalize C:\Users\kvand` notice).
- `npm test`: passed — 4 files, 5 tests, 0 failures.
- `npm run build`: passed — TypeScript and Vite production build completed.
- `git diff --check`: passed; only Git's existing LF/CRLF checkout notices were printed.

## Self-review

- Security boundaries remain backend-owned: no secrets or account/metadata behavior changed, no shell command strings were introduced, and frontend filesystem permissions were not added.
- Archive writes revalidate paths immediately before filesystem operations. Extraction uses a freshly created private directory, rejects links, and opens files with `create_new` so archive entries cannot overwrite existing files.
- The previous managed runtime is untouched until download, checksum, extraction, and staged Java probe have all succeeded. A failed final rename attempts restoration of the previous directory.
- Optional Rust status fields are omitted during serialization, matching the optional TypeScript DTO fields.
- Existing `LauncherApi` account consumers were preserved; runtime commands use a separate narrow `RuntimeApi` interface.

## Concerns and follow-up

- The bundled Temurin artifacts are intentionally pinned. Their URLs and SHA-256 values require an explicit manifest update when upgrading Java security releases.
- Cleanup of a replaced backup directory is best-effort after the new runtime is live. A file lock can therefore leave a hidden `.backup-java-*` directory, but does not invalidate or roll back the verified runtime.
- Tests intentionally use fake process runners/fetchers and in-memory ZIP fixtures, so no external network or host Java installation was exercised.

## Fix Round 1

### Reviewer findings addressed

1. Memory limits are now backend-owned. `MemorySettingsStatus` is returned by the narrow `memory_status` command and contains the current clamped value, minimum, safe maximum, and step. `ProfileService` derives all four fields through Task 4's `PhysicalMemory` abstraction and `clamp_memory`. The React component loads this DTO through `SettingsApi`; callers no longer provide an arbitrary maximum or step.
2. Task 2 path inspection now rejects every Windows object with `FILE_ATTRIBUTE_REPARSE_POINT`, covering symbolic links, junctions, mount points, and other reparse tags. Launcher directory and database paths are revalidated at the write/open boundary. Tests cover a deterministic non-symlink reparse classification, a real junction, and refusal to create launcher directories through an existing junction.
3. Managed runtime swaps now use a fixed discoverable `.backup-java-{major}` recovery point and an injectable swap filesystem. Activation failure with successful rollback restores the probe-valid old runtime. If rollback itself fails, the stable `runtime_state_inconsistent` error is returned and the backup remains discoverable. Startup restores a missing final directory structurally; pre-install recovery probes final/backup runtimes, keeps a valid activation, or replaces an invalid activation with the probe-valid backup.

### TDD evidence

- The deterministic reparse-point test first failed because `PathKind` had no general reparse classification; after Windows attribute-based inspection it and the real-junction tests passed.
- The launcher-directory junction test first demonstrated that `create_directories` followed an existing junction; after safe-join checks immediately around creation it passed.
- The rollback test first failed to compile because no injectable swap filesystem/recovery API existed; after the swap protocol was introduced, injected activation+rollback failure returned `runtime_state_inconsistent` and retained `.backup-java-17`.
- Invalid-final recovery first returned `runtime_state_inconsistent`; after probing and activating the valid backup it restored the old runtime and passed.
- Rust and TypeScript memory tests first failed because `memory_status`, `SettingsApi`, and the backend DTO did not exist; after adding the narrow boundary both focused tests passed with a backend maximum of 12,288 MB and a 512 MB step.

### Round verification

- `cargo fmt --all`: passed.
- `cargo test --all-targets`: passed — 59 library tests plus 2 Task 5 integration tests, 0 failures.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `npm test`: passed — 4 files, 5 tests, 0 failures.
- `npm run build`: passed.
- `git diff --check`: passed; only Git's LF/CRLF checkout notices were printed.

### Round concerns

- Path validation is performed at the last practical boundary and the runtime staging root is launcher-private. As already documented by Task 2, path-based checks cannot completely eliminate a same-user TOCTOU race without a broader Windows handle-relative/no-follow filesystem layer.
- A locked obsolete backup may remain after a successful activation, but it is now intentionally named, detected, probed, and recovered or cleaned on the next pre-install pass.

## Fix Round 2

### Reviewer findings addressed

1. Recovery now derives each `bin/java.exe` probe target as a root-relative path and passes the complete runtime-home, `bin`, and executable component chain through hardened `AppPaths::safe_join` immediately before invoking the process runner. A reparse point on the runtime directory, `bin`, or `java.exe` therefore returns the stable `invalid_path` error instead of probing through it.
2. Backup discovery no longer accepts arbitrary prefix matches. It recognizes only the current exact `.backup-java-{major}` name and the validated legacy `.backup-java-{major}-{u64}` grammar. Textual, empty, compound, and numeric-overflow suffix lookalikes are ignored.

### TDD evidence

- A Windows regression test first showed that recovery successfully followed `.backup-java-17/bin` when `bin` was a junction to an external executable. After full executable-path validation was added, the same recovery attempt returned `invalid_path` without probing through the junction.
- A discovery regression test first selected `.backup-java-17-attacker`. After the grammar was restricted, attacker, empty, compound, and out-of-range numeric suffixes were ignored, while the exact current name and maximum valid legacy `u64` suffix remained discoverable.

### Round verification

- `cargo fmt --all`: passed.
- `cargo test --all-targets`: passed — 62 library tests plus 2 Task 5 integration tests, 0 failures.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `git diff --check`: passed; only Git's LF/CRLF checkout notices were printed.

### Round concerns

- Complete component validation is performed immediately before each recovery probe, but the existing documented same-user path-based TOCTOU limitation still applies until a broader Windows handle-relative/no-follow process-launch layer exists.
