# M0 acceptance review

Recommendation: **KEEP M0 OPEN**. This is an evidence review, not a transfer authorization or a claim that unknown optional capabilities are unsupported. The current v3 `inspection_complete` means the defined read-only checks completed; it is not an atomic snapshot and `transfer_readiness` remains `not_assessed`.

| M0 requirement | State | Evidence and remaining work |
|---|---|---|
| Versioned, redacted read-only CLI reports | VERIFIED | v3 CLI tests and real APFS, exFAT and FAT32 results; default paths redacted. |
| Multiple sources, file statistics, partial scans | VERIFIED | Regression suite and real APFS permission-denied observation. Totals are logical observations, not an atomic snapshot. |
| Destination filesystem, mount flags and capacity | LIMITED | Real APFS, exFAT and FAT32 probes; read-only APFS mount tested before and after reattach. Folder-level writability and future free space remain unknown. |
| FAT-family individual-file bound | LIMITED | Real FAT32 image and 4 GiB logical sparse APFS source produced a blocker; `msdos` is only a FAT-family upper bound and does not establish FAT subtype. Aggregate over 4 GiB has only synthetic coverage. |
| Exact and ASCII case-name collisions | LIMITED | Deterministic tests and real APFS case variants and exFAT. Full Unicode equivalence, filename restrictions, non-UTF-8 names and nested existing destination conflicts remain incomplete. These are M0 compatibility gaps, not silently deferred. |
| Symlink traversal safety and identity | LIMITED | Descriptor-relative no-follow descent and deterministic substitution tests. Initial path components, concurrently moved directories, mount identity reuse and changing files prevent snapshot or adversarial-race guarantees. |
| Metadata and link compatibility | LIMITED | Real APFS/exFAT observations, but metadata support and preservation policies remain unknown. Do not claim successful copying. |
| Mount boundaries | LIMITED | Device changes are reported as observations; same-device and network boundaries can be missed. Real nested mount behavior is not verified. |
| Diagnostic completeness and errors | LIMITED | Structured v3 findings and observed errors; permission and filesystem matrix coverage remains incomplete. Unknown capabilities are explicit and do not individually block M0. |
| Disposable fixture and scan benchmark harness | LIMITED | Bounded fixtures and release-mode scanner baselines in [m0-scanner-baselines.md](m0-scanner-baselines.md). Trusted-parent model and physical allocation uncertainty apply. |
| Additional read-only, permission and lifecycle matrix | BLOCKED | APFS read-only/reattach tested; equivalent exFAT/FAT32, live identity-change cases and nested mounts are not verified. They require separately approved disposable-volume operations or suitable environment. |
| NTFS macOS writable integration | BLOCKED | No supported writable NTFS environment was established. No driver or elevated configuration is assumed. |
| Sequential copy, transactional staging and verified transfer | DEFERRED | M1 and later. No source deletion or move support is authorized by this review. |
| Finder/ditto/rsync/rclone copy baseline timings | DEFERRED | The roadmap lists baseline timings in M0, but the requested M0 acceptance scope explicitly defers copy benchmarks until copying exists. Scanner timings are not a replacement or comparison claim. |
| File-level adaptive workers, journal, GUI and explorer integrations | DEFERRED | Roadmap M2 and later; outside this milestone. |

## Decision basis

The scanner's read-only foundation and limited real-filesystem checks are testable today, and unknown optional capabilities alone should not keep M0 open. However, M0 requested detection of applicable filesystem-specific invalid names and assessment of selected-folder write restrictions. The scanner currently does not implement a filesystem-specific filename restriction check or establish folder-level writability. For example, it cannot diagnose an exFAT-restricted name from source metadata before a write, and a writable mount flag does not establish that the selected folder permits writing. These missing checks are the concrete acceptance blockers; complete them or explicitly agree on a narrower M0 gate. Full coverage of optional metadata capabilities, writable NTFS, and every nested mount is not independently required for this decision. Complete the remaining achievable naming/permission cases or explicitly agree on a narrower documented M0 release gate before claiming acceptance. Do not interpret a future M0 acceptance as permission to start copying, moving or deleting sources.

The read-only APFS mount and reattachment observations are recorded in [m0-readonly-reattach-result.json](m0-readonly-reattach-result.json); the existing [coverage matrix](filesystem-coverage.md) and M0.6 reports retain earlier APFS, exFAT and FAT32 evidence. Controlled scanner measurements are in [m0-scanner-baselines.json](m0-scanner-baselines.json). No physical drives were used for these tests.
