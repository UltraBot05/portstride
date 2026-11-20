# 01 — Product requirements (PRD)

**Status:** proposed v0.1 baseline. Mac-first product; Windows planned. The name is provisional.

## Problem

Finder and Explorer provide basic transfer operations but limited workload-aware tuning and often unclear preflight diagnoses. Robocopy-style multi-file parallelism, native UI, resilient controls and helpful filesystem remediation are scattered across CLI utilities and third-party apps. The user wants **one transfer job that intelligently copies its own constituent files in parallel**, when useful—not merely several unrelated jobs at once.

## Target users and scenarios

- Developers moving source trees, build assets, project folders, VMs, and mixed metadata between internal disks and external SSDs.
- Gamers transferring folders with thousands of small/medium files or one giant archive.
- Cross-platform users moving data among APFS, NTFS, exFAT, FAT32 and later other filesystem types.
- Users who need a transfer manager, not a full replacement file browser.

## Primary UX requirements

**FR-001 — Native entry:** From Finder selection, launch *Smart Copy to…* and *Smart Move to…* by supported Finder mechanisms. Later mirror in Explorer; drag/drop and optional Smart Paste shortcut supported.

**FR-002 — Local metadata preflight:** Discover file count, logical/apparent bytes, size histogram, source/destination volume, filesystem, writable state, free capacity, path/case/metadata hazards, and relevant device topology *before committing to a plan*. Provide progressive discovery and safe re-planning for enormous trees.

**FR-003 — Workload optimization:** Select clone, metadata-only rename, sequential stream, or bounded concurrent file workers based on filesystem capabilities, number/size of files and measured behavior. Do not assume parallel I/O is faster for a single giant file.

**FR-004 — File-level concurrency:** Multiple independent files in **one job** may copy concurrently. Separate-job concurrency is a later scheduling feature and must not starve an active heavy job.

**FR-005 — Preflight explanations:** Show actionable, prioritized resolutions when a job cannot proceed or will lose metadata. Resolution cards state whether actions are nondestructive, backup-required, or destructive.

**FR-006 — Safe conversion guidance:** Recommend appropriate target formats (typically exFAT for Mac/Windows sharing, APFS for Mac-focused drives, NTFS for Windows-focused drives) *with compatibility caveats*. Guide the user to system tools; never auto-format or assume any conversion preserves data.

**FR-007 — Transfer controls:** Pause, resume, cancel, retry failed file, prioritize/queue jobs, view per-file and aggregate progress; report verification status separately from copying status.

**FR-008 — Integrity:** Preserve file bytes; preserve supported metadata or show exactly what cannot be preserved. Never delete originals on cross-volume moves until all mandatory checks complete. No silent skips.

**FR-009 — Conflict handling:** Before overwriting, choose skip, keep both, overwrite, compare, or ask. Apply policy once/per conflict; preserve meaningful source and destination identities.

**FR-010 — Recovery:** Persist an operation journal; resume safely across app restart, removable-volume disconnect, and recoverable errors, after checking whether the source changed.

**FR-011 — Observability:** Actual throughput, rolling throughput, bytes remaining, files completed, failures, active worker count, estimated time **with confidence**, and reason for selected strategy; avoid fabricated precision.

**FR-012 — Native feel:** SwiftUI/AppKit Mac interface, macOS notifications, accessibility and keyboard commands; later WinUI 3 and Windows notifications. GUI should not require always opening a file browser.

**FR-013 — Consent and privacy:** Scan local metadata only by default; no upload or telemetry by default; avoid requiring Full Disk Access or Accessibility except where demonstrably needed for optional functions.

## Example acceptance flows

### 200 GB `.mkv` from internal SSD to T7

- Preflight sees one large file and a cross-volume destination; evaluates destination max file size/capacity.
- Uses a sequential path unless empirical tests demonstrate a benefit to segmented I/O for this topology.
- Displays progress and verification controls, preserving the original source.

### Source repository with 40,000 files

- Preflight shows file count, sizes and likely metadata limitations.
- Uses bounded file-worker parallelism, not 40,000 threads or competing whole-job streams.
- Applies per-volume backpressure and updates worker count based on measured performance.

### FAT32 destination with 8 GB single file

- Blocks before copying that file. Shows FAT32 limit, affected path, and resolutions: use a different volume, back up and reformat to exFAT for cross-platform use, or consider NTFS/APFS for OS-specific use.
- Does not mislabel reformatting as nondestructive conversion.
- Never launches a formatting command without a separate, explicit action—and the initial release performs no formatting itself.

### Moving a folder across physical volumes

- Copies into controlled temporary destinations; verifies mandatory results; commits; only then removes source with explicitly selected move semantics.
- On failure, leaves original intact and exposes recovery state.

## Performance and quality targets (targets, not current claims)

- 100% byte correctness across verified fixtures; zero silent omissions; deterministic fail behavior.
- No regression greater than 10% against the best tested safe baseline in the top reference workloads without a documented justification; aspiration, not a universal guarantee.
- Keep UI responsive during preflight, long transfers and media disconnects.
- Workload routing decisions explained and benchmarked; at least 1, 4, 8 and 16 worker settings in test mode.
- Copy failures identify file, operation, OS error and proposed next step.
- Crash/forced-exit recovery test suite passes before shipping move support.

## Explicit non-goals for first release

- Replacing Finder/Explorer as a file browser.
- Implementing custom APFS/NTFS filesystem drivers.
- Fully seamless interception of Finder's or Explorer's default Paste **without** dedicated opt-in integration and testing.
- Silent formatting, repartitioning, or in-place conversion; storage recovery software.
- Guaranteed accelerated copy speed on every device or file set.
- Sync engine/cloud backup, deduplication, compression archive creation, or remote file protocols in v1.

## Release gates

- **Mac alpha:** CLI core + native UI + Finder action; tested APFS, exFAT, FAT32, NTFS read-only source; no destructive moves until recovery validated.
- **Mac stable:** performance, verified metadata degradation, recovery, supported integration, accessibility and crash tests.
- **Windows alpha/stable:** same engine, Windows adapter, WinUI 3/Explorer integration, NTFS/exFAT/FAT32 tested; APFS when mounted via third-party driver is conditional and capability-tested.
