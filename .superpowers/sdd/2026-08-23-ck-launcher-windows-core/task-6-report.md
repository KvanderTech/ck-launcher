# Task 6 report — Resumable verified download queue

## Status

Implemented the Task 6 download queue, its local-only HTTP tests, and the production adapter for Task 5's existing `RuntimeArchiveFetcher` boundary.

Commit: `feat: add resumable verified downloads`. The final hash is recorded in the task handoff because a Git commit cannot contain its own final hash.

## Delivered behavior

- Added `DownloadSpec`, `DownloadService::execute`, cancellation, typed progress, configurable finite timeouts, and injectable sleeper/jitter dependencies.
- Added a six-worker buffered queue with three total attempts, exponential backoff plus jitter, transient-only HTTP retry classification, and cancellation that drops active requests and never schedules waiting HTTP work.
- Added exact expected-size, SHA-1, and SHA-256 verification. Hashless entries still require exact size, verified existing files are skipped, and corrupt destinations are preserved until a verified replacement is ready.
- Added fixed `.part` resume files with HTTP Range only after an exact compatible `206 Content-Range`; a full `200` response restarts by truncating safely, and incompatible partial responses are discarded before retry.
- Added serialized progress containing operation id, total/completed bytes, and current destination, with aggregate progress clamped to the operation total.
- Added component-by-component destination, `.part`, lock, backup, and final-path validation through the hardened Task 2 path boundary immediately around creates and renames. Owned roots and nested Windows reparse points are rejected.
- Added Windows OS byte-range ownership for `.part.lock` markers. Concurrent writers fail cleanly, while an unlocked marker left by process termination can be reused so a valid `.part` remains resumable.
- Added Windows-safe replacement through a unique backup: an incorrect existing file is moved only after the part verifies, restoration is attempted on activation failure, and the backup is retained until the activated file verifies.
- Replaced Task 5's duplicate reqwest implementation with a narrow `DownloadHttpClient` adapter while preserving `RuntimeArchiveFetcher`, its fake-based installer tests, bounded in-memory archive reads, and the runtime checksum/extraction flow.
- Kept frontend capabilities unchanged and never includes download URLs or query strings in errors.

## TDD evidence

Red/green cycles observed during implementation:

1. The initial queue suite failed to compile because all download interfaces were absent. After the minimal module/API implementation, the focused suite compiled and exposed path and transport issues.
2. All local HTTP cases initially returned `invalid_path` because Windows canonical roots use a verbatim prefix. Normalizing only the trusted root comparison made atomic, retry, resume, cancellation, timeout, and concurrency tests pass without weakening containment.
3. The bounded-memory adapter test initially accepted an unsolicited `206` body. It now requires a full `200` and retries/rejects partial responses.
4. The trusted-root junction test initially showed that canonicalization followed a reparse-point root. Root validation now happens before accepting its canonical path.
5. The stale-lock test initially returned `download_in_progress` for an unlocked marker. Windows byte-range locking now distinguishes active ownership from a marker left after termination, while the concurrent-owner test proves a live writer remains exclusive.
6. Local instrumented servers cover exactly two requests after a corrupt response, exactly three transient attempts with injected 107/207 ms delays, one request for permanent 404, a measured maximum of six concurrent requests, Range compatibility, safe restart, idle timeout, and cancellation before queued files start.

## Verification

All commands were run from the Task 6 worktree with the bundled toolchains and no external test network.

- `cargo fmt --all --check`: passed.
- `cargo test --all-targets`: passed — 77 library tests plus 2 Task 5 integration tests, 0 failures. The 15 download tests use only loopback HTTP.
- `cargo clippy --all-targets -- -D warnings`: passed with 0 project warnings/errors. Cargo prints the existing environment-only `could not canonicalize C:\Users\kvand` notice.
- `npm test -- --run`: passed — 4 files, 5 tests, 0 failures (run outside the filesystem sandbox after esbuild config resolution was denied inside it).
- `npm run build`: passed — TypeScript and Vite production build completed.
- `git diff --check`: passed before the report; final diff checking is repeated immediately before commit.

## Self-review

- Queue limits are enforced by `buffer_unordered(6)` and each waiting future checks cancellation before opening a lock or making HTTP requests.
- Retried corrupt data is deleted before a subsequent attempt, so it is never used as a Range prefix. Cancellation is checked before final rename and only verified data reaches a final destination.
- Correct finals are checked both during planning and immediately before replacement. An existing incorrect final is not removed until the part has passed verification.
- Request, network, timeout, integrity, path, storage, cancellation, and HTTP-status failures use stable errors without URLs. A query-string secret fixture confirms the error does not expose it.
- The runtime installer still performs its pinned SHA-256 check before extraction and staged Java probing. Only its production HTTP transport changed.
- No Tauri command, frontend HTTP/filesystem permission, account flow, metadata interface, or secret boundary changed.

## Concerns and follow-up

- As documented for Task 2, path validation is performed at the last practical boundary but cannot completely remove a same-user TOCTOU race without a broader handle-relative/no-follow filesystem layer.
- Windows `.part.lock` files are intentionally persistent zero-byte ownership markers. The OS lock, not marker existence, identifies a live writer; keeping the marker avoids a deletion/recreation race and permits recovery after process termination.
- Task 5's established fetcher contract returns `Vec<u8>`, so runtime archives remain bounded in memory by its 512 MiB cap even though all request/retry/timeout logic now comes from the shared download client.
