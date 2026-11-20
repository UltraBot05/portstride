# 10 — Coding-agent handoff (Codex / Claude Code)

This is the entry point when starting implementation in a fresh AI coding session.

## Project intent (read before coding)

Build a **native macOS-first adaptive file-transfer manager**, later Windows, with a **shared Rust core**. It should scan files and target filesystem capabilities, then intelligently select sequential/clone/rename/parallel-*file* copying within a single job. It should offer actionable, **safe** resolutions for filesystem failures (including guidance for backup and reformat), without automatically performing destructive disk operations.

The user specifically rejects the idea that “parallel transfers” means simply running two unrelated transfer jobs at once. **The core requirement is file-level parallelism inside one job**, selectively enabled for suitable workloads. Job-level scheduling follows after the engine is correct and benchmarked.

## Non-negotiable constraints

1. macOS release before Windows. Native SwiftUI/AppKit on Mac; WinUI 3 on Windows; Rust is common.
2. First-class, tested targets: APFS, NTFS, exFAT, FAT32. Other mounted filesystems should get capability-driven fallback.
3. macOS NTFS is not automatically writable; Windows APFS not automatically available. Do not write raw filesystem drivers.
4. Single huge files generally should stay sequential unless evidence proves multi-stream beneficial. Large directories should use bounded concurrent file workers if beneficial.
5. No “faster than Finder” claims without controlled like-for-like benchmarks **and file verification**.
6. User wants native Finder/Explorer integration, **not** a new file browser. Finder Quick Actions and Windows `IExplorerCommand` are supported; silent replacement of system paste is experimental.
7. Drive reformatting **erases data**. In-place conversion cannot be generalized. Windows FAT/FAT32 → NTFS is a narrowly documented path, not a shortcut to exFAT/APFS.
8. Display ranked actionable resolutions. Never erase, format, repartition, or run conversion automatically.
9. Preserve source on failed copies/moves. Cross-volume move means copy, verify/commit, then remove original only under clear user policy.
10. Do not silently discard metadata, file conflicts, symlinks, sparse-file semantics or mismatched filename case.
11. Keep performance and failure reports reproducible and visible.
12. Do not create a GitHub repository, push commits, select a final brand name, or change the user's global macOS settings without explicit authorization.

## Read order

1. `01_PRODUCT_REQUIREMENTS.md`
2. `04_PREFLIGHT_AND_RESOLUTIONS.md`
3. `06_RELIABILITY_AND_SECURITY.md`
4. `02_ARCHITECTURE.md`
5. `03_FILESYSTEM_COMPATIBILITY.md`
6. `09_ENGINE_CONTRACTS.md`
7. `07_PERFORMANCE_AND_TESTS.md`
8. `05_NATIVE_INTEGRATION.md`
9. `08_ROADMAP.md`
10. `11_SOURCES_AND_OPEN_DECISIONS.md`

## First implementation slice (do not build GUI yet)

1. Scaffold a Rust workspace for `engine`, `platform-macos`, `cli`, and `integration-tests`. Avoid premature Windows-specific boilerplate.
2. Implement **read-only** `inspect` request/response from `09_ENGINE_CONTRACTS.md`.
3. Probe mounted destination volume and report read/write, filesystem name, capacity, FAT32 limit, and confidence/unknown fields.
4. Walk a directory tree without following unsafe symlinks; report exact count/size estimates and path hazards.
5. Emit sample `DEST_FILE_TOO_LARGE`, `DEST_READ_ONLY`, `INSUFFICIENT_CAPACITY`, `CASE_NAME_COLLISION` and `METADATA_UNSUPPORTED` findings.
6. Add golden JSON tests from `examples/preflight-examples.json` and edge-case fixtures.
7. Provide a short demonstration with an 8 GiB **sparse fixture** on a *disposable* test volume: preflight blocks FAT32 and explains remediation; nothing is copied or formatted. On a test FS that cannot represent a sparse file, use metadata-only mock fixtures.
8. Report tests and open issues; obtain approval before implementing deletion/move or system-level integrations.

## Recommended next agent prompt

> Read `README.md` and all `docs/` in this project. Implement milestone M0/M1 read-only preflight first, not a GUI. Follow the requirements, particularly native FS capabilities and the resolution safety model. Do not implement formatting, source deletion, or overwrite-by-default. Create focused unit tests and realistic fixtures, show the CLI JSON output, and list what has actually been verified on the machine. Do not push code or change system settings without asking.

## Things to verify on actual hardware

- macOS target version, signing/entitlement approach, Finder Quick Action behavior.
- The actual Samsung T7 filesystem and USB link speed; never assume it is APFS/exFAT solely by brand.
- Swift + Rust integration choice, compatibility with sandboxed file access.
- Disk Utility versions, read-only NTFS behavior, and platform-specific metadata semantics.
- Whether APFS clone or file-level concurrency improves actual end-to-end timings.

## Explicit permission boundaries

**Allowed by this handoff:** inspect repo files; run tests using disposable temporary directories; create scaffolding and implementation in an agreed workspace.

**Not authorized:** erase/reformat any disk, delete academic files, move source data destructively, modify user's existing project folders, create/push a public repo, install broad privileges or intercept global shortcuts without discussion.
