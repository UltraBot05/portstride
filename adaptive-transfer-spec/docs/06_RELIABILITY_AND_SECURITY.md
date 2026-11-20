# 06 — Reliability, data integrity, and security

## Core invariants

1. The source remains intact during **copy**. No source deletion during a failed cross-volume **move**.
2. A destination is never announced as complete before required verification and commit.
3. A failed or cancelled transfer cannot silently leave a partial file masquerading as a completed one.
4. Filesystem capability gaps must be reported, not silently papered over.
5. The app must never erase, repartition, format or convert a drive as a hidden part of resolving a transfer failure.
6. No path is trusted based solely on its string; validate resolved path, volume identity, file type and link behavior.

## Data copy transaction

For each ordinary file:

`preflight → validate source identity → reserve/stage destination → copy → flush → metadata → verify → atomic final rename if supported → mark complete`

Use a private temp file in the destination directory/volume to avoid cross-volume final renames and to preserve recovery state. If the final path already exists, enforce explicit conflict policy; **do not overwrite it early**. Robust atomic-overwrite semantics differ by OS and filesystem; implement them per adapter and test with crashes. A whole directory tree is not one atomic operation.

## Verification profiles

- **Quick:** size / expected byte count, source metadata consistency, destination handle completion and errors; low extra I/O. Clearly labeled as **not byte-for-byte cryptographic verification**.
- **Verified:** hash source and destination with a supported cryptographic digest, or compute source digest while copying and read back destination after commit/staging. Changes detected during copy cause retry or fail. Keep resource overhead visible.
- **Move:** stronger mandatory policy with flush/identity checks and verification before source deletion; cross-volume deletion is opt-in with a recoverable/error state. Decide exact verification guarantee in ADR before exposing move to users.
- **Metadata:** separate checklist for timestamps, modes, xattrs, resource forks, symlinks, ACLs, Windows alternate streams, and any documented degradation.

Hashes are useful for content equality but do not establish full metadata preservation. Storage firmware, network layers and caches can complicate durability claims; do not market "power-loss proof" without applicable platform guarantees and tests.

## Recovery and journaling

- Journal stores operation ID, source/destination volume identity, paths, plan version, metadata snapshot, chunk/file checkpoints, committed artifacts, errors and user decisions.
- Journals are versioned and crash-consistent (transactional database or atomic record append with durability policy).
- On launch, restore interrupted jobs; validate source size, mtime, file identity if available, destination partial size/content, mounted volume identity and journal version.
- If validation fails, **restart affected file from zero or request user resolution**; never blindly append stale partial data.
- Respect OS/volume-specific constraints: external volumes can be unplugged, renamed or remounted; volume display name alone is not a trustworthy identity.
- Implement bounded retry/backoff for transient OS failures; no infinite loops or retry storms.

## High-risk edge cases

- Symlink/hardlink escape, cycles, recursive destination-inside-source copy.
- File mutates during scan/copy; deleted files; cloud storage placeholders; package bundles.
- Case-folding collisions and Unicode normalization collisions.
- Sparse VM images with logical bytes far exceeding physical allocation; free-space claims may be wrong.
- Disk nearly full; read-only remount; device unplug; USB errors; filesystem corruption; partial sector write.
- Cross-volume moves with permissions different from source; destination verified but source removal fails.
- Metadata unsupported or rejected; cross-OS permission bits and resource forks.
- System files, protected directories, boot volumes, network shares, encrypted volumes, path-length limits.
- Simultaneous jobs referencing same destination; staged-file name collisions; overwrite race.
- Windows reparse points/alternate data streams; macOS `.app` bundles/AppleDouble sidecars.

## Threat model

- **Untrusted filenames and paths:** control characters, shell metacharacters, path traversal attempts; never build shell commands from unescaped names.
- **Malicious symlinks:** never traverse outside user selection without explicit policy.
- **Privilege:** least-privilege design, no background administrator/root process for standard operations.
- **Clipboard/shell automation:** opt-in, scoped, cannot steal generic paste input.
- **Sensitive local data:** collect minimal metadata, no unsolicited network calls or upload; diagnostics redact absolute paths by default when exported.
- **Drive remediation:** OS-owned tools; no silent direct access to disk formatting APIs.
- **Updates and plugins:** signed/reproducible delivery strategy; do not load arbitrary filesystem plugins without trust model.

## Copy versus sync versus backup

This tool is a **transfer manager**, not initially an incremental sync or backup product. A successful copy can still be a poor backup if the source/destination share a single failure domain. Do not imply that a transfer replaces a backup strategy.

## Safety release gate

No move-with-delete, auto conflict resolution or recovery resume shipped until fault injection proves the source survives relevant failure points and user-visible state remains accurate.
