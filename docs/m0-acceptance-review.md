# M0 acceptance review

Recommendation: **ACCEPT M0 under the final scoped acceptance criteria**. This is an evidence review, not a transfer authorization or a claim that unknown optional capabilities are unsupported. The current v3 `inspection_complete` means the defined read-only checks completed; it is not an atomic snapshot and `transfer_readiness` remains `not_assessed`.

| M0 requirement | State | Evidence and remaining work |
|---|---|---|
| Versioned, redacted read-only CLI reports | VERIFIED | v3 CLI tests and real APFS, exFAT and FAT32 results; default paths redacted. |
| Multiple sources, file statistics, partial scans | VERIFIED | Regression suite and real APFS permission-denied observation. Totals are logical observations, not an atomic snapshot. |
| Destination filesystem, mount flags and capacity | LIMITED | Real APFS, exFAT and FAT32 probes; read-only APFS mount tested before and after reattach. The descriptor-bound effective-access check is a point-in-time observation; future writes and free space remain uncertain. |
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

The final requested M0 gate accepts known read-only incompatibilities and accurately qualified uncertainty. The scanner detects established conflicts and per-file upper-bound violations; it warns rather than blocks when mounted-driver behavior or Windows portability is uncertain. The selected-folder access check is descriptor-bound and point-in-time, not a future-write guarantee. Real FAT32, case-sensitive APFS and case-insensitive APFS tests show that macOS can accept tested names despite on-disk or Windows portability restrictions. These observations do not prove broader driver behavior. Under the explicitly requested final scope, filename and permission checks remain LIMITED rather than absent; optional unknowns and later transfer requirements do not prevent M0 acceptance. No M0 acceptance authorizes copying, moving or source deletion.

The read-only APFS mount and reattachment observations are recorded in [m0-readonly-reattach-result.json](m0-readonly-reattach-result.json); the existing [coverage matrix](filesystem-coverage.md) and M0.6 reports retain earlier APFS, exFAT and FAT32 evidence. Controlled scanner measurements are in [m0-scanner-baselines.json](m0-scanner-baselines.json). No physical drives were used for these tests.

M0 two-blocker continuation: descriptor-bound macOS effective-access observations and FAT-family component warnings are implemented. On a disposable exFAT image, the macOS driver accepted every tested punctuation name, including characters prohibited by the on-disk format, so these rules cannot safely be blockers based on filesystem label alone. The final scoped acceptance criteria permit driver-specific unknowns as LIMITED with transfer-time revalidation. Permission observations remain limited to what the non-mutating OS check can establish. See [real observations](m0-filename-permission-evidence.json).

Final filename validation: [per-name observations](m0-final-name-observations.json) record actual submission, creation result, enumeration, lookup and scanner diagnostics for FAT32 and both APFS case variants. The scanner adds `WINDOWS_FILENAME_PORTABILITY` as an unknown-confidence warning for FAT-family destinations; it does not claim the selected destination rejects the name or implement a complete Windows validator.
