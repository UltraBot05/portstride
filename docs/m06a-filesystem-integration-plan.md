# M0.6a: disposable filesystem integration test plan

Status: design only. No image was created, attached, formatted, erased or detached for this plan. No proposed command in this document is authorization to execute it. M0.6b requires separate explicit approval for image creation, formatting, attachment, fixture writes and verified cleanup. It does not authorize operations on physical drives, source deletion or M1 transfer work.

## Goals and evidence

Test the actual mounted filesystem and compare its observed behavior with Portstride's v3 JSON. Record real observations separately from simulated metadata; never present a mock as filesystem evidence. Unknown or unimplemented scanner capabilities remain unknown, not failures merely because they are unavailable. Report observed differences and fail on false claims or unsafe behavior. Filesystem observations are point-in-time and do not authorize a later transfer.

| Target | Real volume cases | Baseline and expected caveat |
|---|---|---|
| APFS | Case-insensitive and case-sensitive, each a separate image | Verify the actual case flag; do not infer from the label. Compare collision behavior and Unicode names. |
| exFAT | One image | Verify mounted driver, case behavior and metadata observations; no generic Unix metadata guarantee. |
| FAT32 | One image, confirm actual subtype | Per-file maximum is 4,294,967,295 bytes; `msdos` alone does not prove subtype. Large folder totals and individual files are separate cases. |
| FAT12/16 | Optional, separate approved images | Skip if size or tooling cannot establish the subtype reliably. |
| NTFS | Deferred | No write support assumed on macOS; a separately approved supported environment is required. |
| Unknown mounts | Mock only in this workflow | Confirm conservative fallback, not a real-filesystem claim. |

## Safety boundary and ownership

1. Run as an ordinary, non-root user; do not use `sudo`. Create a unique private directory with atomic creation and mode 0700 under a trusted parent on the internal filesystem. Verify ownership, mode, ancestors and ACLs; there must be no hostile same-user process modifying the test directory. The generator's trusted-parent assumptions apply only to developer fixtures, not the transfer engine.
2. The harness owns a run manifest created exclusively in that directory. Record its random run ID, canonical directory identity, image path and device/inode identity, creation result, attachment plist, and lifecycle state. Reject pre-existing image paths, symlinks, hard-linked or replaced artifacts, ambiguous ownership, and unexpected contents. A path under a temporary directory is not sufficient proof.
3. Create **one image at a time** using image-file creation with a filesystem option. Never invoke `diskutil eraseDisk`, `partitionDisk`, `newfs_*`, or any formatter on a device node. Do not accept a user-supplied `/dev/diskN`, existing image, volume label or mountpoint as a target. Run process APIs with argument arrays, not shell interpolation.
4. Parse the attach plist and independently correlate the live attachment's backing image with the manifest's file identity, image path and expected device tree. Validate that it is a disk-image-backed attachment, its mountpoint lies inside the run directory, and the relevant filesystem, volume and mount properties are as expected. APFS can expose multiple synthesized device nodes; never identify an image using a device number, display name, or device ID alone. If a trustworthy one-to-one backing-file mapping cannot be established, **stop**; do not write, detach or remove anything automatically.
5. Recheck identity immediately before every fixture write and before any detach. Keep descriptors where applicable, but recognize that separate tool invocations do not make identity checks atomic. Do not reuse a stale disk identifier after detach/reattach. User selections and external mounted disks must remain untouched.
6. No automatic destructive cleanup on any error, interruption, unexpected mount or verification mismatch. Record the last confirmed state and retain artifacts. Cleanup is a separate explicit action with fresh validation; if ambiguous or busy, stop and ask for manual review. Do not use force detach.

## Proposed command shapes, not an executable script

The installed `hdiutil create -help` on the inspected macOS 27.2.1 host lists `APFS`, `Case-sensitive APFS`, `ExFAT` and `MS-DOS FAT32`; it also warns that create/attach/detach are deprecated in favor of `diskutil image`. These templates are for review only. Replace placeholders only inside the M0.6b identity-checked harness; do not run them now.

```text
hdiutil create -size 6g -type UDIF -fs <approved-format> -volname <test-label> -plist <new-owned-image-path>
hdiutil attach -plist -nobrowse -noautoopen -mountroot <owned-existing-mount-directory> <verified-owned-image-path>
hdiutil info -plist
# Only after verifying the active attachment against the manifest immediately before cleanup:
hdiutil detach <verified-current-whole-image-device>
```

Never use `-ov`, `-srcdevice`, `-force`, or a pre-existing image. Parse plist output and handle multiple returned entities instead of relying on text parsing. `hdiutil info` and attach plist formats, whether an already attached image can be returned, and backing-image matching must be verified on the target OS before the harness permits writes or detachment. If correlation fails, leave the attachment alone. Check modern `diskutil image` equivalents during M0.6b; avoid `diskutil image info` as a presumed read-only lookup because the local manual says it can attach and eject. Apple's [APFS tools overview](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/ToolsandAPIs/ToolsandAPIs.html) documents image-file creation; Apple notes that [APFS volumes may share container space](https://support.apple.com/en-ie/guide/disk-utility/dskua9e6a110/mac), so physical-device equality is not a mount or volume identity.

## Space and authorization

Proposed maximum: one 6 GiB logical UDIF image attached at a time, one 6 GiB image file, no more than 64 MiB of ordinary fixture payload, and a separate 2 GiB host free-space reserve beyond the image's full logical size and fixture budget. Before image creation, require at least 8 GiB plus the fixture budget available on the host filesystem; otherwise skip. These are planning caps, not a claim that metadata, APFS snapshots or a filesystem allocation cannot exceed an estimate. Use a fixed-size image rather than relying on sparse host allocation to enforce a physical budget; monitor actual host usage and stop on unexpected growth. No test may fill the host filesystem. The 64 MiB logical fixture cap cannot demonstrate a real aggregate above 4 GiB, and FAT32 cannot itself hold an oversized source. Skip both real large-size cases under this budget; retain the existing synthetic 8.2 GB and aggregate cases as simulation, not filesystem proof. A separately approved later experiment could use sparse, metadata-only source fixtures on a separately verified source filesystem with its own logical and physical limits; never write 4 GiB of payload for this plan.

Local help confirms utilities exist, not that create, attach, filesystem setup or detach are unprivileged in this environment. `diskutil` could not contact DiskManagement during the read-only investigation. M0.6b must test those permissions only after approval, as the ordinary user, and report denial or an OS authorization prompt rather than escalating. Creating an image initializes a filesystem and writes a new file; attaching/detaching changes system state. Fixture setup writes inside the verified image. All require explicit project-owner approval under project policy. If administration is necessary, stop for a separate decision.

## Phases and test cases

**Prepare, approved writes:** validate the private run directory and budget; create one image; attach it; establish backing-image identity; collect OS build, architecture, utility versions, mounted filesystem name, mount flags, volume/device identifiers and available capacity. Only then create bounded fixture entries within the verified mount. Keep a separate source directory and a distinct empty destination directory; avoid selecting an existing destination child as a source when checking the no-conflict case. The current generator expects a non-elevated user and a trusted parent owned by that user without group/other write mode bits; some mounted filesystems may not expose these semantics. If setup is unsupported or refused, record `skip` or `unsupported`; do not bypass the guard.

**Execute, read-only inspector:** invoke `portstride inspect <fixture> --destination <verified-mount-directory> --json` using argument arrays and keep raw v3 output plus exit code. Validate `inspection_complete`, `inventory.complete`, `transfer_readiness: not_assessed`, redaction and findings independently. Do not equate exit 0 with transfer permission. Repeat after read-only remount only if separately approved and identity revalidated.

| Case | Fixture setup and real observation | Inspector assertion |
|---|---|---|
| Identity and mount | Capture attach plist, backing file relation, mount flags and capacity before/after | Probe agrees where comparable; identity is ephemeral; unknown stays null. |
| Exact/case collisions | Distinct source roots with same and ASCII case-variant names; exercise both APFS variants and portable targets | Detect supported exact/ASCII collisions; ensure case-sensitive controls do not produce false blockers. |
| Unicode/names | Create composed/decomposed and case variants where OS permits; test forbidden characters and long names without normalization assumptions | Record actual OS outcomes; current incomplete name coverage is explicit. Do not assert generic lowercase equals filesystem behavior. |
| FAT32 size | Real oversized-file and >4 GiB aggregate cases are skipped under the 64 MiB logical cap; use existing synthetic metadata cases | Confirm simulated per-file and aggregate logic, but do not claim real FAT32 boundary validation. A future larger logical sparse-source experiment needs separate approval. |
| Capacity | Compare v3 available bytes with platform observation at probe time | Allow changing free space and distinguish logical from allocated bytes; capacity finding remains an estimate. |
| Read-only/permissions | Separately approved read-only attach; ordinary-user private denial only if reliably reproducible | Read-only state observed; denied or unprobeable cases reported accurately, never infer permission from mode alone. |
| Links and metadata | Attempt valid/broken links, xattrs and ACLs; inspect return codes and preservation *availability*, not copy preservation | Symlink/capability unknown or unsupported as observed; never claim metadata survived a copy. |
| Sparse | Create small bounded candidate and compare logical and allocated sizes where OS reports them | Logical size is counted; allocated space and sparse support remain separately qualified. |
| Identity lifecycle | Record image and volume identity, detach and reattach only in approved lifecycle | Reprobe; compare observations without promising stable identity or trusting reused disk IDs. |
| Nested mount | Optional separately approved second image mounted inside test image, only if identity gates work | Observe device-change reporting and note same-device boundaries may be missed. Skip if verification is ambiguous. |

Result records should contain case ID, phase, `pass`/`fail`/`skip`/`unsupported`, real versus mock provenance, expected and observed values, v3 report or redacted reference, OS/build/architecture, filesystem and mount metadata, image identity evidence, timestamps and error category. Keep private absolute paths out of exported results by default; preserve full local provenance only in the private run directory. A failed prerequisite skips dependent cases rather than silently passing them.

## Cleanup and recovery, separate approved phase

Freeze the manifest and capture the current `hdiutil info -plist`. For each candidate, correlate current device tree and backing image with the original owned image and current file identity; check no unexpected sibling or mountpoint is affected. If any check fails, the image is busy, or device reuse is possible, stop without detaching. After a normal verified detach, independently confirm no attachment still references the image. Only then, with explicit cleanup approval, remove the exact manifest-owned image and private run artifacts by descriptor-relative operations after checking their identities. Never recursively delete an arbitrary path; unknown contents or changed identity require manual review. On crash or reboot, reload the manifest, rediscover from current state and revalidate from scratch; never blindly replay a saved device identifier.

## M0.6b implementation tasks and exit criteria

1. Obtain explicit authorization for image lifecycle, bounded fixture writes, and separately for identity-verified cleanup. Implement the manifest and fail-closed lifecycle; unit-test parsers and negative identity cases with mocks before any attach.
2. Confirm utility syntax, plist schemas, unprivileged behavior and filesystem subtype on a single approved disposable image. Record skips and OS prompts; do not change system settings or install drivers.
3. Add the minimal declarative case list and versioned JSON result writer. Reuse existing scanner fixtures and keep setup, read-only inspection and cleanup as separate commands. Do not build a generic plugin framework.
4. Run the matrix one image at a time and review every fail, skip and unsupported result. Verify no personal volume was selected and no physical device was formatted. Update the existing coverage matrix with actual evidence.
5. Preserve all existing tests and run fmt, Clippy and workspace tests. Defer future M1 copy verification until separately authorized; revalidate all source and destination identities and policies at transfer time.
