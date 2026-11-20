# 04 — Preflight diagnosis and guided resolutions

**This is a core feature, not an error-dialog afterthought.** The engine must offer *safe, ranked options* when a transfer cannot proceed or cannot preserve requested semantics.

## Preflight flow

1. **Discover:** Traverse input paths incrementally, inspect metadata, classify special files and estimate total count/bytes.
2. **Probe:** Check destination volume identity, filesystem/driver and actual folder permissions, capacity, max file size, metadata support.
3. **Diagnose:** Return structured blockers and warnings: every finding needs a path/sample count, reason code, evidence and confidence.
4. **Rank remedies:** Prefer nondestructive fixes; explain impact on portability, performance, metadata, existing data and user effort.
5. **User chooses:** Proceed with supported modifications, choose another destination, request help, or cancel. Re-run preflight after destination/format changes.
6. **Transfer:** If a newly discovered item violates a constraint, pause the affected operation and present the same diagnosis rather than silently skipping.

Preflight is not a complete read of all file contents. A very large or changing tree may be analyzed progressively; disclose when the scan is partial and revalidate critical facts before commit.

## Severity and safety taxonomy

| Field | Values | Meaning |
|---|---|---|
| `severity` | `blocker`, `warning`, `information` | Can the operation continue under chosen policy? |
| `remedy.risk` | `non_destructive`, `backup_required`, `destructive` | User-facing risk, **not** ease of implementation |
| `remedy.action` | `choose_destination`, `change_policy`, `free_space`, `backup_then_reformat`, `in_place_convert_documented`, `split_explicitly`, `inspect_permissions`, `repair_or_replace_media`, `cancel` | Typed, localized choices |
| `automation` | `supported`, `opens_system_tool`, `instructions_only`, `never_automatic` | What app is actually allowed to do |

**UX wording rule:** Never call a *reformat* a *conversion*. Do not offer a generic **Convert drive** button if the operation erases data. Say **Back up, erase and reformat**. If an in-place conversion is documented, name its exact source and target formats, OS, backup requirement and limitations.

## Issue/remedy catalogue

| Issue code | Condition | Recommended resolutions (in priority order) |
|---|---|---|
| `DEST_FILE_TOO_LARGE` | File exceeds destination single-file limit (e.g., FAT32) | Choose an exFAT/NTFS/APFS destination; if drive may be erased, back up → reformat → restore; split into parts only by explicit user choice and with reconstruction instructions |
| `DEST_READ_ONLY` | Volume or folder cannot be written | Choose writable destination; check permissions; if NTFS on Mac, use a supported driver or write on Windows / choose exFAT portable drive; never suggest SIP bypass |
| `INSUFFICIENT_CAPACITY` | Estimated requirement exceeds available space | Free space, another destination, smaller selection, supported compression/archive as *separate* feature; distinguish logical from allocated bytes |
| `CASE_NAME_COLLISION` | Multiple source paths map to same target name | Rename selected files with preview; choose case-sensitive target; cancel; never silently overwrite |
| `METADATA_UNSUPPORTED` | ACLs, streams, symlinks or forks unrepresentable | Choose compatible target; accept explicit loss with exact report; optionally create verified archive for preservation (future feature) |
| `SYMLINK_UNSUPPORTED` | Cannot preserve link safely | Choose compatible target; ask to copy link targets only after assessing escapes/cycles; cancel |
| `SOURCE_CHANGED` | File changed since preflight/checkpoint | Retry file from start with new snapshot; skip; cancel; never resume from stale offset |
| `DEST_DISCONNECTED` | Drive removed midtransfer | Pause; await reconnection of same volume identity; revalidate; resume when safe |
| `PATH_UNSUPPORTED` | Destination rejects path/name | Preview safe renames or choose another volume; show all collisions |
| `ACCESS_DENIED` | OS permission / sandbox restriction | Grant per-folder access or select another location; avoid escalating privileges unless essential and documented |
| `FS_ERROR` | Read/write or corruption errors | Retry bounded times, check drive health and cable, stop potentially destructive activity; do **not** prescribe formatting as first response |

## Detailed drive-format guidance

### Case A — FAT32 and a single file above 4 GiB minus 1 byte

**Blocker message:** “`video.mkv` is 8.2 GB. This drive uses FAT32, which cannot store a single file this large. The drive may have free space, but this operation cannot succeed unchanged.”

**Remedy 1 — Use another compatible destination** (`non_destructive`). “Choose a drive formatted as exFAT, APFS or NTFS that this computer can write to.”

**Remedy 2 — Back up and reformat this drive** (`destructive`). “For use on both Mac and Windows, exFAT is usually suitable. **Reformatting erases existing data on the selected volume.** Back up and independently verify the backup first. Check whether a console, camera or other device requires FAT32.” Button: **View steps in Disk Utility** (macOS) or **Open Disk Management** (Windows), not **Convert**.

**Remedy 3 — Split into multiple parts** (`non_destructive`, *optional feature*). Explain that no ordinary single-file app can use the segments until rejoined, the target device may not support it, and a reassembly tool is required. Do not silently split.

### Case B — NTFS volume mounted read-only on macOS

**Blocker message:** “The destination is mounted read-only. macOS can usually read NTFS volumes, but this mount does not allow writing.”

Remedies: choose writable exFAT/APFS destination; copy on Windows; use a trusted, compatible NTFS driver if user already has one; back up and reformat to exFAT only if cross-platform sharing is desired. No claims that driver installation is mandatory or security settings must be disabled.

### Case C — Windows FAT32 → NTFS request

The Windows system utility supports a specific **in-place FAT/FAT32 → NTFS** path. It is *not* general FAT32 → exFAT conversion, and it is one-way via this utility. Show **Backup required** even though it is intended to retain existing files. Supply official instructions for the user's platform, warn that macOS generally cannot write NTFS without extra support, and **never run the conversion from a routine file transfer**.

### Case D — exFAT → APFS/NTFS request

Explain that no generally supported built-in nondestructive conversion is being promised. Recommend **verified backup → erase/reformat → restore**, then re-run transfer preflight. Never auto-erase the selected disk.

## Destructive-action safeguards

- **Phase 1 product:** cannot format, partition, wipe or invoke unattended conversion commands. It can show guidance and, with explicit click, open OS disk utilities.
- Must show **physical disk vs volume** selection distinctions, volume name, capacity, identifier, affected data, backup status and target OS compatibility.
- Never encourage reformatting internal boot/system volumes or managed/encrypted drives in a transfer dialog.
- Avoid one-click irreversible remediation; backup may be impossible to verify automatically and must not be implied complete.
- On return from Disk Utility/Windows tools, **reprobe** filesystem, volume identity, contents and free space rather than assuming change succeeded.
- If user refuses, provide other safe routes and let them cancel.

## Ranking logic

Prefer, in order: (1) choose existing compatible writable destination; (2) change non-destructive copy policy; (3) explicit segmented packaging when it makes sense; (4) backup-required documented in-place conversion; (5) destructive reformat workflow **only when benefits are clear and consent explicit**. Never rank reformat as default for generic I/O errors.

## UX requirements for every resolution card

- “Why this failed” in everyday language; actual path, size and filesystem.
- What the alternative changes, what it preserves, what it loses.
- Risk label and whether a backup is needed.
- OS compatibility and use-case impact (Mac-only, Windows-only, both, legacy devices).
- Concrete next action and a way to cancel.
- No promise that selecting an alternative automatically changes a volume's filesystem.
