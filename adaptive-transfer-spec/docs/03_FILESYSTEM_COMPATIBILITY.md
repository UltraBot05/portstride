# 03 — Filesystem capabilities and cross-platform compatibility

## Target matrix (mounted volumes; read/write depends on OS and driver)

| Filesystem | macOS baseline | Windows baseline | Strengths / caveats |
|---|---|---|---|
| APFS | Native read/write | No built-in general-purpose read/write | macOS-focused, cloning where supported, richer metadata; case-sensitive and insensitive variants |
| NTFS | Native reading; do **not** assume ordinary write support without a supported driver | Native read/write | Windows-focused, ACLs, alternate data streams, reparse points, large files |
| exFAT | Native read/write | Native read/write | Cross-platform portable drives, large files; fewer Unix/NTFS metadata capabilities |
| FAT32 | Native read/write | Native read/write | Broad legacy device compatibility; maximum single file **4 GiB minus 1 byte**, limited metadata |
| Other mounted FS | Probe and use conservative fallback | Probe and use conservative fallback | Don't reject solely because not named above |

**Important:** The app interacts with **OS-mounted volumes**, not raw NTFS/APFS drivers. A read-only NTFS mount is allowed as a source but not a destination. A third-party APFS/NTFS driver is not automatically trusted to support every metadata or durability guarantee.

## Preflight capability fields

- `filesystem_kind`: known label or `unknown`.
- `mount_readable`, `mount_writable`, and whether the *selected folder* is accessible.
- `max_file_bytes`: known, unavailable, or conservative best-effort; FAT32 exact bound.
- `free_bytes`, `available_for_important_usage`, allocation cluster overhead, and source/destination volume IDs.
- `supports_clone`, `supports_atomic_rename`, `supports_sparse`, `supports_hardlink`, `supports_symlink`, `supports_xattrs`, `supports_resource_forks`, `supports_acl`, `supports_ads`.
- `case_sensitive`, case-preserving behavior, Unicode normalization and name/path limitations.
- `removable`, `network_or_virtual`, known physical-device ID, read-only state and media lifecycle.
- **Per-operation** capability; cloning may work between two directories on one volume but not across volume boundaries.

Never equate *filesystem detected* with *capability verified*. Use volume APIs and safe probes, avoid probing production data destructively, and record uncertainty.

## Metadata and compatibility hazards

1. **Case collisions:** `A.txt` and `a.txt` may coexist on case-sensitive APFS but conflict on case-insensitive destinations. Preflight must detect mapped-name collisions.
2. **Unicode normalization:** Different filesystems may treat visually identical names differently; warn and apply deterministic collision mapping, not silent renaming.
3. **Symlinks:** Do not automatically follow links out of a requested tree. Preserve when target supports; otherwise present options and warnings.
4. **Hard links:** Preserve relationship where feasible; otherwise warn about duplication/semantics. Do not simply count multiple paths as independent physical bytes.
5. **macOS resource forks/xattrs:** exFAT/FAT32 may require platform-specific sidecar representation. Sidecars are not universally portable or understood; show preservation loss/sidecar policy.
6. **Windows ACLs and alternate streams:** NTFS-specific data is not generally representable in exFAT/FAT32; warn before copying if present.
7. **Sparse files and VM disk images:** Logical size is not allocated size. Copying can expand allocated bytes substantially; verify destination space assumptions.
8. **Bundles/packages:** A macOS `.app` is a directory; preserve its internals. File extensions alone do not define transfer strategy.
9. **Special files:** sockets, FIFOs, devices, mount points, cloud placeholders: never naively copy or dereference; show unsupported or explicitly controlled mode.
10. **Filesystem health:** Errors may signal hardware or corruption. Do not recommend format as a generic fix for every I/O failure.

## Recommended target filesystem by use

- **Mac-only external SSD:** APFS, unless device/vendor requirements dictate otherwise.
- **Mac + Windows portable storage:** exFAT generally practical for ordinary files; warn about permissions, resilience, file metadata, and safe eject practices.
- **Windows-only drive:** NTFS.
- **Legacy camera/console/device requiring FAT32:** FAT32, but large files are impossible unchanged.

These are *recommendations*, not automatic choices. Ask about the intended devices and whether existing data must remain before suggesting a reformat.

## FAT32 correctness example

A folder can exceed 4 GiB in aggregate if its constituent files each fit and the volume has capacity. Conversely, an individual 8 GiB file exceeds the FAT32 limit even on an otherwise empty 1 TB drive.

## Drive conversion versus reformat

- On Windows, the documented `convert` path supports **FAT/FAT32 → NTFS** conversion while keeping existing files, though backups are still essential and environment restrictions apply.
- **exFAT → NTFS/APFS, NTFS → exFAT/APFS, FAT32 → exFAT, and most other requested transitions** should be described as *backup → erase/reformat → restore*, unless a separately supported, proven in-place route exists for that exact OS/volume. Never imply they're nondestructive.
- APFS/NTFS are **not inherently incompatible data containers**; the OS's available driver determines access. Avoid claiming software can just change formats with a toggle.
- Formatting can affect not just the filesystem but partition schemes, boot behavior, external devices and applications. Warn about those dependencies.

See `04_PREFLIGHT_AND_RESOLUTIONS.md` for required wording and safety rules.
