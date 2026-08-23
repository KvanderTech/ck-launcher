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
