# 07 — Performance, benchmarks and test plan

## What success means

Performance is **measured end-to-end wall time plus integrity**, not a count of CPU threads or a momentary MB/s peak. The app should be fast on varied realistic workloads, avoid needless overhead on single large files and explain when copy performance is storage-bound.

## Controlled benchmark matrix

| Workload | Purpose | Suggested generated fixtures |
|---|---|---|
| Single giant file | Detect multithread overhead; large sequential I/O | 10–50 GB fixture; optional larger user-controlled test |
| Ten large files | File-level concurrency benefit | 10 × 1–4 GB |
| Mid-sized game directory | Real mixed file parallelism | 2,000 files, 64 KiB–500 MiB distribution |
| Source repository | Metadata/dir traversal overhead | 50k+ small files with deep trees, Git-style paths |
| Tiny file storm | Directory and creation latency | 100k files, 0–8 KiB |
| Sparse VM image | Allocation amplification and data integrity | Controlled sparse disk fixture |
| APFS same-volume | Clone/rename detection | Large file and folder; clone eligibility |
| Cross-filesystem | Metadata and case collision | APFS → exFAT, exFAT → APFS, FAT32 guards |
| Failure injection | Crash-safe recovery | Pause, SIGKILL, disk unplug, full drive, permission change |

Test machines/targets initially: Apple Silicon Mac internal APFS; external Samsung T7 over verified 10 Gbps path; exFAT and FAT32 sacrificial test volumes; NTFS source read-only on macOS. Later Windows NVMe→NTFS/exFAT/FAT32 and attached APFS when suitable driver exists.

## Baseline competitors

- Finder, `ditto`, macOS `rsync` (record version and flags), rclone (`--transfers` values recorded).
- On Windows: Explorer, Robocopy `/MT:n`, and platform APIs as baseline; test equivalent metadata semantics.
- Avoid comparing a verified+hash job to an unverified competitor without labeling the difference.

## Experimental controls

- Record OS build, drive model, USB link speed, filesystem, cluster/sector size, free space, encryption and firmware if available.
- Separate **cold/cache-influenced** runs from warmed repeated runs; randomize tool order and repeat at least 3–5 times for larger workloads.
- Record **wall time**, sustained throughput, p50/p95 per-file latency, CPU, RAM, disk read/write, worker count, failures, bytes/metadata preserved, verification duration.
- Benchmark 1/2/4/8/16 workers (configurable internal test mode), and adaptive mode. Measure with isolated job: no competing download or benchmark on same device.
- Compare same source/destination physical devices. Normal SSD thermal behavior and SLC cache saturation must be recorded; ignore claims based on one 30-second sample.
- Always check destination hashes against golden fixtures after benchmark. Keep fixtures disposable.

## Adaptive scheduling acceptance

- With a single huge file, scheduler selects default sequential path unless a validated device-profile override reliably wins.
- For many independent files, scheduler may increase concurrency; if throughput plateaus or degrades, reduce it with hysteresis.
- Worker count constrained by memory, open-file descriptors, destination queue depth and platform limits.
- A fully sequential baseline must remain available as fallback.
- Performance choices are reproducible/explainable, e.g. `reason_codes=["MANY_INDEPENDENT_FILES", "EXTERNAL_SSD", "WORKER_TUNE"]`.

## Functional test matrix

**Filesystems:** APFS standard/case-sensitive; exFAT; FAT32; NTFS macOS read-only source; NTFS Windows read/write; unknown mounted volume fallback.

**Names:** long UTF-8 paths, Unicode normalization, case conflicts, trailing dots/spaces on Windows, reserved device names, embedded whitespace, leading dashes.

**Metadata:** timestamps, executable mode, xattrs/resource forks, symlinks, hard links, sparse files, Windows ACL/ADS, bundles, special files.

**Storage:** same filesystem, same physical disk/different volume, two independent disks, USB SSD, network filesystems as best-effort fallback later, missing volume, low free space.

**User actions:** pause/resume, cancel, reboot, crash/restart, conflict policy, read-only target, permission denied, choose new drive, system reformat workflow return.

**Safety:** failure while writing data, flushing, hashing, renaming, and source deletion; no silent source loss. Reformat recommendation must never initiate destructive action by itself.

## QA gates and observability

- Unit tests on capability classification, policy decision, cost function, resolution safety ranking, and copy state transitions.
- Property-based tests for path normalization and collision mapping.
- Integration tests using temporary directories and loopback/file-backed disk images; real device tests before release.
- Fuzz untrusted file/path manifests and journal parser.
- Report actual performance relative to baseline, including scenarios where our engine loses.

## Release metrics (proposed thresholds)

- Mandatory integrity test cases: **100% pass**.
- No reproducible source data loss in move fault-injection suite.
- At least **4 out of 6** primary workload classes competitive with or faster than baseline in same-mode comparisons; no >10% regressions without documented tradeoff.
- UI operations remain responsive during scan and copy; no unbounded memory growth.

These are engineering targets, not marketing claims or predictions.
