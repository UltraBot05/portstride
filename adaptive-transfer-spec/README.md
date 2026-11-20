# Adaptive Transfer — project specification

**Status:** design baseline, **not an implemented product**  
**Working name:** Adaptive Transfer (placeholder; naming not decided)  
**Planning date:** 20 November 2026  
**Release sequence:** macOS first; Windows second  
**Core:** shared Rust transfer engine; native SwiftUI/AppKit and WinUI 3 shells

## North star

A reliable, genuinely native file-transfer utility that inspects a transfer *before* choosing how to copy, uses the fastest **safe** strategy for the actual storage and file mix, offers understandable fixes when an operation is unsupported, and integrates with Finder (later Windows Explorer) without replacing either file manager.

**Example:** A single 200 GB video ordinarily receives a sequential transfer. A directory of thousands of independent files may use bounded concurrent file workers. An APFS-to-APFS same-volume copy should prefer copy-on-write cloning where supported. A 9 GB file aimed at FAT32 should be blocked **before copying**, with useful alternatives (choose another volume, use exFAT if cross-platform, or back up and reformat through the OS's disk tool). Reformatting must never happen automatically.

## Principles

1. **Correctness > speed.** No silent data loss, invisible skips, or source deletion before verified completion of a cross-volume move.
2. **Workload-aware, not extension-driven.** Scan sizes, counts, mount capabilities, topologies, and measured throughput. File type is an optional hint, never the primary scheduler rule.
3. **One job can use many file workers.** File-level parallelism is the primary performance feature; whole-job scheduling comes afterwards.
4. **Native on each OS.** A shared engine, two separate native application frontends and explorer integrations.
5. **Actionable errors.** Distinguish nondestructive workarounds, backup-required in-place conversions, and destructive reformats.
6. **Capability-based filesystem support.** APFS, NTFS, exFAT, and FAT32 are the initial **tested** matrix, not an artificial allowlist.
7. **Nothing destructive by default.** Formatting/partitioning is out of the initial application; show instructions and open OS-owned tools with explicit consent only.
8. **Visible control.** Pause, resume, retry, verify, skip, inspect conflict details, and choose policies.

## Document map

| File | Why it exists |
|---|---|
| [01 — Product requirements](docs/01_PRODUCT_REQUIREMENTS.md) | User flows, requirements, definition of done |
| [02 — System architecture](docs/02_ARCHITECTURE.md) | Rust services, platform adapters, scheduler, lifecycle |
| [03 — Filesystem compatibility](docs/03_FILESYSTEM_COMPATIBILITY.md) | APFS/NTFS/exFAT/FAT32 capability matrix and metadata rules |
| [04 — Preflight & resolutions](docs/04_PREFLIGHT_AND_RESOLUTIONS.md) | The guided remediation engine, conversion/reformat safety and UX |
| [05 — Native integration](docs/05_NATIVE_INTEGRATION.md) | Finder and Explorer entry points, supported vs experimental flows |
| [06 — Reliability & security](docs/06_RELIABILITY_AND_SECURITY.md) | Data-integrity guarantees, journal, conflicts, threat model |
| [07 — Performance & QA](docs/07_PERFORMANCE_AND_TESTS.md) | Benchmarks, test matrix, acceptance and regression gates |
| [08 — Roadmap](docs/08_ROADMAP.md) | macOS-first implementation sequence, Windows gate |
| [09 — Engine interfaces](docs/09_ENGINE_CONTRACTS.md) | Proposed type contracts and event examples |
| [10 — Coding-agent handoff](docs/10_AGENT_HANDOFF.md) | Implementation instructions for Codex / Claude Code |
| [11 — Sources & decisions](docs/11_SOURCES_AND_OPEN_DECISIONS.md) | Official API references, resolved assumptions, remaining decisions |
| [Sample diagnosis](examples/preflight-examples.json) | Structured examples to drive UI and tests |

## Initial user journeys

- **Normal:** Select items in Finder → Smart Copy/Move action or Smart Paste → immediate plan → copy progress → optional verification → completion.
- **Large single file:** The engine recognizes the large-file topology, avoids wasting concurrency on one sequential stream, and reports actual throughput.
- **Many files:** A single transfer job uses a bounded number of concurrent file workers, tuned to the source/destination.
- **Unsupported FAT32 file:** Preflight stops, identifies the largest offending file and recommends a different volume or backup-and-reformat to exFAT (cross-platform) or an OS-native format. It never quietly splits, deletes, converts, or reformats anything.
- **Interrupted copy:** Resume from a durable journal after validating source identity and the partially copied destination; otherwise restart the affected file safely.

## What this package is not

No source code or runnable app is claimed. No native Finder paste interception has been demonstrated. No benchmark speedups are claimed. No filesystem conversion is promised as lossless except the narrowly documented Windows FAT/FAT32 → NTFS path, which still needs a backup and explicit user consent. Open design decisions are recorded rather than silently invented.

**Start implementation with** `docs/10_AGENT_HANDOFF.md`, then the reliability and preflight specs. Do **not** build the polished UI first.
