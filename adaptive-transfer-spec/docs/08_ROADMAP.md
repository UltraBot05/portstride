# 08 — Implementation roadmap

The engine and correctness tests come first. Mac builds prove the architecture before work on a Windows GUI. Every milestone has a shippable or testable outcome; design documents are not evidence that the app exists.

| Stage | Scope | Exit gate |
|---|---|---|
| **M0 — Research & harness** | Rust workspace, fixture generator, CLI job manifest, benchmark harness; probe filesystem capabilities | Deterministic test inputs and baseline Finder/ditto/rsync/rclone timings |
| **M1 — Safe copy MVP** | Sequential copy, local scanner, preflight blockers, temporary destination staging, plain progress | End-to-end hash-valid copies; FAT32 4 GiB guard; no overwrite without policy |
| **M2 — Smart file-level concurrency** | Bounded worker pool, device topology signals, adaptive tuning, clear fallback | Verified gains on suitable many-file workloads without major single-file regression |
| **M3 — Reliability** | Durable journal, pause/resume, retry, conflict management, metadata preservation, crash recovery | Kill/unplug/full-disk tests pass; no silent omissions/source loss |
| **M4 — macOS alpha UI** | SwiftUI/AppKit app, Finder Quick Action, destination picker, progress/pause/conflict/remedy sheets | Usable without CLI; APFS/exFAT/FAT32 and readable NTFS cases exercised |
| **M5 — macOS beta** | Improve packaging, accessibility, permissions, high-file-count UX, signature and comparison reports | External testers and controlled benchmark matrix; onboarding docs |
| **M6 — macOS stable** | Polish, error analytics local only, localization-ready copy, optional Smart Paste integration | Reliability and security review; support matrix published |
| **M7 — Windows native alpha** | WinUI 3 shell, Windows adapter, Explorer `IExplorerCommand`, NTFS semantics | Same core fixture suite and Windows-specific ACL/ADS/collision tests |
| **M8 — Windows stable** | Windows recovery/performance testing, installer/update design | Production reliability, accessibility and compatibility matrix |

## Ordered engineering tickets (initial backlog)

1. Define `VolumeCapabilities`, `Job`, `Issue`, `Remedy`, `Plan`, `ProgressEvent` schemas with versioning.
2. Build Rust CLI `inspect <sources> --destination <dir>` returning JSON plan *without modifications*.
3. Implement scanner with symbolic-link safety, cross-filesystem boundaries and progressive data-size estimation.
4. Add FAT32 size blocker, read-only destination blocker, space check, conflict discovery, and remedy cards to JSON.
5. Build sequential copy using temp files and hashes in test mode; prove crash behavior.
6. Implement metadata policy per filesystem, accounting for losses and unknowns.
7. Implement concurrent file worker pool and adjustable internal test knobs.
8. Benchmark sequential/2/4/8/16 and adaptive; document failures and speedups honestly.
9. Add persistent journal and resume; produce negative/crash test matrix.
10. Add safe cross-volume move only when documented integrity gate passes.
11. Build SwiftUI progress app and Finder Quick Action, with typed async bridge.
12. Add guided reformat/format choice explanations via OS-owned utilities (never auto-format).
13. Design Windows adapter and WinUI prototype using same manifests and issue schema.

## Deliverable gates

- **Demo 1:** `inspect` correctly explains why 8 GB file cannot be copied to FAT32 without changing anything.
- **Demo 2:** one job with thousands of files uses dynamic bounded file-level concurrency and reports correct verification.
- **Demo 3:** process killed halfway through; resume safely handles partial file and no source loss.
- **Demo 4:** Finder selection → native transfer UI → file-level adaptive engine → completion.
- **Demo 5:** Windows Explorer context action uses same core policy with Windows-specific volume features.

## Explicit deferrals

- Automatic takeover of normal `Cmd+V` / `Ctrl+V` until it can be shown compatible with native paste and user keybindings.
- Full reformat/conversion wizard execution (only guidance/OS-tool links initially).
- Networking/cloud sync, archive packing and chunk splitting unless separately approved.
- Multi-job saturation/priority algorithms until single-job file-level concurrency is measured and reliable.
