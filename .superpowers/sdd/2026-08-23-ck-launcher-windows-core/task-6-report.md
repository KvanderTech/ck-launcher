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

## Fix Round 1

### Reviewer findings addressed

1. Resume is now disabled when neither SHA-1 nor SHA-256 is available. Any nonempty hashless `.part` is discarded before the request, preventing a stale prefix from a different artifact version from becoming a size-valid final file.
2. Planning now reserves every final, derived `.part`, and derived `.part.lock` path before checking whether a final can be skipped. Intersections are rejected using Windows ordinal case-insensitive comparison over validated absolute paths, covering direct case variants and final/part/lock aliases.
3. HTTP 416 for a Range request is handled as stale resume state: the part is discarded, progress is reset, and the next attempt starts at byte zero within the existing three-attempt policy. A non-Range 416 remains a permanent HTTP failure.
4. Aggregate progress mutation and sink emission now occur while holding the same mutex. Concurrent events therefore cannot be emitted in reverse counter order; the existing upper bound remains enforced.

### TDD evidence

- The mixed-version test first finalized `AAAABBBB` from a stale hashless `AAAA` prefix and a fresh `BBBB` suffix. It now sends no Range request and finalizes only `BBBBBBBB`.
- Planner tests first accepted `a` with `a.part`, `a` with `a.part.lock`, `A.PART` with `a.part`, and `File.bin` with `file.bin`. All now fail before filesystem or HTTP work with `download_spec_invalid`.
- The Range regression first returned permanent `download_http_status` after one 416. It now makes exactly two requests, with Range only on the first, and produces the verified final file.
- The deterministic progress test paused the first update after counter mutation, allowing the second update to emit first; the old implementation produced `2, 1`. With a single state/emission mutex it produces `1, 2`. The local six-worker test also asserts every concurrent progress sequence is nondecreasing and bounded by total bytes.

### Round verification

- `cargo test downloads -- --nocapture`: passed — 19 loopback-only download tests, 0 failures.
- `cargo fmt --all --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed with 0 project warnings/errors.
- `cargo test --all-targets`: passed — 81 library tests plus 2 Task 5 integration tests, 0 failures.

### Round concerns

- Hashless downloads deliberately trade resume efficiency for integrity: existing correct finals remain size-verifiable and skippable, but interrupted hashless parts always restart from zero.
- Windows case-insensitive collision checks use `CompareStringOrdinal` on already validated absolute paths. The existing same-user path-based TOCTOU limitation remains unchanged.

## Fix Round 2

### Reviewer findings addressed

1. Final destination filenames ending case-insensitively in `.part` or `.part.lock` are now rejected as a queue-internal namespace. This invariant applies independently to every plan, so one execution's final can never be another execution's part or ownership marker even though their derived locks would otherwise differ.
2. Collision planning no longer performs an allocation-heavy quadratic scan. Each validated final, part, and lock path is encoded into its Windows UTF-16 identity once, the identities are sorted with ordinal case-insensitive comparison, and only adjacent identities are checked. Collision detection is now O(n log n), and filesystem verification starts only after the entire namespace is proven collision-free.

### TDD evidence

- The separate-plan regression first accepted `a.part` as a standalone final after a plan for `a`, proving that plan-local intersection checks did not protect concurrent executions. The planner now rejects `.part`, `.PART`, `.part.lock`, and `.PART.LOCK` finals with `download_spec_invalid` in every plan.
- A 2,048-entry manifest test proves large unique plans remain valid and that a case-only alias appended at the end is still rejected. The existing final/part/lock and direct case-alias planner cases remain green.

### Round verification

- `cargo test downloads -- --nocapture`: passed — 21 loopback/planner download tests, 0 failures.
- `cargo fmt --all --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed with 0 project warnings/errors.
- `cargo test --all-targets`: passed — 83 library tests plus 2 Task 5 integration tests, 0 failures.

### Round concerns

- `.part` and `.part.lock` are intentionally unavailable as final filename suffixes. They are implementation-reserved across the owned download root; callers must choose a different final name.
- Windows path identity remains ordinal case-insensitive and path-based. The previously documented same-user TOCTOU limitation is unchanged.
