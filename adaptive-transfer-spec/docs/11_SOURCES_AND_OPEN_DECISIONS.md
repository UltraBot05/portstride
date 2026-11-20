# 11 — Official references and design decisions

**Verified source review:** 20 November 2026. The references support OS/API constraints, not any claim that this product has been implemented. Recheck when coding against shipping SDKs.

## Apple

- [Finder Sync programming guide](https://developer.apple.com/library/archive/documentation/General/Conceptual/ExtensibilityPG/Finder.html): Finder extension responsibilities and contextual menu capabilities.
- [macOS app extensions and Quick Actions](https://developer.apple.com/documentation/appkit/app-extensions): Finder Quick Action integration.
- [Apple File System overview](https://developer.apple.com/documentation/foundation/about-apple-file-system): clone behavior and native filesystem model.
- [URLResourceKey](https://developer.apple.com/documentation/foundation/urlresourcekey): volume properties, available capacity, file-clone capability.
- [File system formats in Disk Utility](https://support.apple.com/guide/disk-utility/file-system-formats-dsku19ed921c/mac): APFS, exFAT, FAT and intended usage.
- [Erase and reformat storage on Mac](https://support.apple.com/en-ae/guide/disk-utility/-dskutl14079/mac): destructive workflow guidance.
- [App sandbox and security-scoped bookmarks](https://developer.apple.com/documentation/professional-video-applications/enabling-security-scoped-bookmark-and-url-access): permissions for persistent app file access.

## Microsoft

- [File Explorer context menu integration](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/integrate-packaged-app-with-file-explorer): Windows 11 modern Explorer `IExplorerCommand`.
- [IExplorerCommand reference](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-iexplorercommand): UI-thread considerations.
- [Filesystem functionality comparison](https://learn.microsoft.com/en-us/windows/win32/fileio/filesystem-functionality-comparison): FAT32/exFAT/NTFS features and size limits.
- [FAT/FAT32 → NTFS conversion](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/autoconv): documented conversion direction and restrictions; doesn't generalize to exFAT.
- [Windows format command](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/format): formatting capabilities; **not a basis for auto-formatting**.
- [CopyFile2](https://learn.microsoft.com/en-us/windows/desktop/api/WinBase/nf-winbase-copyfile2): Windows native copy API and progress behavior.
- [CopyFile2 progress callbacks](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nc-winbase-pcopyfile2_progress_routine): cancellation and restart-specific flags.

## Agreed design decisions

| ID | Decision | Rationale |
|---|---|---|
| D-001 | macOS first, Windows second | User's immediate machine is Apple Silicon Mac |
| D-002 | Shared Rust engine | Avoid rewriting scheduler, verification, and issue model for Windows |
| D-003 | Native SwiftUI/AppKit + WinUI 3 | OS-consistent interactions and file manager extensions |
| D-004 | APFS/NTFS/exFAT/FAT32 are tested targets | Real everyday drive mix and portability |
| D-005 | File-level adaptive scheduling comes before job queues | User's actual performance requirement |
| D-006 | Guided remedies including filesystem choices | Prevent cryptic failure on unsupported operations |
| D-007 | Reformat is never automatic in v1 | Irreversible data-loss risk |
| D-008 | Native supported explorer action first; default paste takeover optional | Platform API limitations and user control |
| D-009 | Integrity and crash tests gate source deletion | Avoid catastrophic move failures |

## Decisions intentionally open

- Product name, branding, icon and license (do not copy prior project identities without consent).
- Minimum macOS and Windows versions; App Store vs direct distribution, signing/notarization costs and requirements.
- Rust ↔ Swift/WinUI interface (C ABI vs local IPC), after prototyping.
- Exact move verification policy (always re-read destination hash vs user-adjustable; safety gate still applies).
- Whether robust copy + verification support should include portable metadata archive/sidecars as an opt-in later.
- Default threshold for showing a full progress window; infer via tests rather than arbitrary 1 GB rule.
- Default adaptive worker range and tuning logic; benchmark rather than assume.
- Extent of Finder toolbar/Services UI and optional Smart Paste shortcut integration.
- Post-v1 features: cloud/network shares, automatic sync, cryptographic integrity manifests, split archives, job-level scheduling.

## Important non-claim

A recommended target filesystem is **not** a promise that an existing volume can be converted in place. Most paths require backing up and **erasing** the destination first. The product should always explain that difference.
