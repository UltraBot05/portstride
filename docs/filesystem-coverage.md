# Filesystem coverage

The [machine-readable matrix](filesystem-coverage.json) records the current preflight test evidence. Synthetic capability tests are not filesystem integration tests.

| Filesystem | Disposable directory | Disposable volume | Simulated behavior |
|---|---|---|---|
| APFS | Current host | Case-insensitive ASCII collision and symlink inventory; case-sensitive distinct ASCII names | Case and capability logic |
| NTFS | Not tested | Not tested | Read-only capability |
| exFAT | Not tested | Case and Unicode lookup, collisions, symlinks and xattr observed; no copy | Generic capabilities |
| FAT32 | Not tested | Small source completed preflight; 4 GiB logical sparse source blocked; no copy | Per-file size bound |
| Other mounted | Not tested | Not tested | Unknown capability fallback |

For future integration testing, obtain explicit authorization for a disposable mounted volume, confirm its identity and that it contains no personal data, then run read-only inspection against separately prepared disposable fixtures. Record filesystem, mount flags, OS and exact results. This project does not create, mount, format, unmount or detach volumes automatically.

Fixture smoke tests on the current APFS host exercised flat, nested, wide and edge directories, including Unicode names and valid and broken symlinks. A sparse candidate was created using `set_len`; physical sparseness was not verified. Permission-denial scenarios were skipped. Generator-level FAT-family restriction checks use synthetic metadata. Existing synthetic engine tests use FAT-size metadata and do not themselves verify a FAT mount.

M0.6b partial run on macOS 27.2.1 (26B131), arm64: the case-insensitive image produced the expected ASCII collision and symlink inventory; the case-sensitive image produced no ASCII collision for distinct case variants. Twelve real observations were recorded across APFS and FAT32; these include filesystem behavior and preflight checks, not twelve transfer tests. The modern exFAT creation attempt failed with `LOCALIZED_STRING_UNAVAILABLE`; the documented hdiutil alternative returned `Operation not permitted`. Neither earlier attempt produced an exFAT image. A separate real FAT32 image was identified by the OS attachment plist. A small file completed preflight, and a 4,294,967,296-byte logical sparse APFS source, with zero reported allocated bytes, produced the expected `DEST_FILE_TOO_LARGE` finding against the `msdos` upper bound. No file was copied. All four images were identity-verified, detached and removed; sparse sources were removed; private manifests and reports were retained.

A further APFS image exercised an existing-destination blocker, composed/decomposed lookup, an xattr round trip, a hard-link inode match, an ACL entry, sparse logical versus allocated bytes, and partial inventory for a mode-000 directory. These do not establish metadata preservation during transfer. See [structured partial results](m06b-real-results.json).

### exFAT investigation and next attempt

The prior session's command transcript records two fresh 6 GiB creation attempts: `diskutil image create blank --plist --format RAW --size 6442450944 --volumeName PortstrideEx --fs ExFAT <new-private-image>` and `hdiutil create -size 6g -type UDIF -fs ExFAT -volname PortstrideEx -plist <different-new-private-image>`. The retained manifests record errors, but not original argument arrays, numeric exit codes or complete stderr; the transcript is the source for the command shapes. Installed utility help lists exFAT, and private run directories were user-owned with mode 0700. These facts do not establish the root cause or that exFAT is unsupported.

A future attempt should first investigate service and authorization state read-only, then request separate approval for a single fresh manifest-owned image and capture the exact arguments, numeric exit status and stderr. If the OS requires access or privileges, stop rather than changing security settings or using a physical device. A manual Disk Utility image workflow, if separately approved, must create a new bounded image file in a private directory, verify that the mounted volume is backed by that exact image before any fixture writes, and use the independent identity and cleanup gates in [the M0.6a plan](m06a-filesystem-integration-plan.md). Never select or erase an existing physical disk.

The additional APFS setup initially stopped after Python reported `os.setxattr` unavailable; its partial fixture was retained. A separately approved attempt with the macOS xattr utility produced the reported round trip. The approved 4 GiB logical sparse source was created on host APFS in a private run directory, not on the FAT32 image.

### M0.6c exFAT continuation

On macOS 27.2.1 (26B131), one fresh 6 GiB RAW exFAT image named `PS_EXFAT` was created and tested. The volume reported case insensitivity; ASCII collision and existing-destination blockers were observed; two symlinks were counted; decomposed lookup of a composed name and an xattr round trip succeeded. These are observations of this mount, not guarantees of portable metadata preservation. No transfer occurred. The image was identity-verified, detached and removed; private logs and reports were retained. See [structured results](m06c-exfat-results.json).

The earlier name `PortstrideEx` has 12 characters; [Apple documents an 11-character limit](https://support.apple.com/en-ca/guide/disk-utility/dskutl14027/mac) for MS-DOS and exFAT volume names. The shorter-name repeat succeeded using otherwise matching `diskutil` parameters. This supports a name-length explanation but does not prove it: original exit codes and complete stderr were not preserved and environmental changes cannot be excluded. The earlier blocked attempts remain part of the historical record. Read-only mount, ACL, hard link, sparse, permission, reattach and nested-mount behavior remain untested for exFAT.

M0 final-validation continuation: a separately approved disposable APFS image was attached read-only, detached and reattached read-only. Both inspections reported `DEST_READ_ONLY` and `transfer_readiness: not_assessed`. The reported device/inode happened to be equal across this run; it is not a durable identity. The image was detached and removed after verification. See [result](m0-readonly-reattach-result.json). Equivalent exFAT/FAT32 lifecycle and nested-mount cases remain untested.

M0 two-blocker continuation: a fresh macOS 27.2.1 exFAT image accepted all twelve tested names, including punctuation that FAT-family on-disk rules restrict. Portstride emits an unknown-confidence warning for a colon-containing source component, not a blocker. On that exFAT mount, `chmod 0500` left the observed mode at `0700`, so folder denial was not reproduced there. On a host APFS temporary directory, descriptor-bound effective-access observations were positive at mode `0700` and denied at `0500`. This is point-in-time evidence, not a future-write guarantee. The exFAT image was verified, detached and removed; no file was copied. See [structured evidence](m0-filename-permission-evidence.json).
