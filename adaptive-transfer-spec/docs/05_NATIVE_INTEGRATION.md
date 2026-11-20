# 05 — Native integration specifications

## Product goal

The user should remain in Finder (later Explorer), not have to use a separate full file browser. Native launch/feedback is a product requirement; intercepting an OS's entire copy engine is **not** a currently supported platform capability we can assume.

## macOS (first release)

**Supported baseline entry points:**

1. **Finder Quick Action / Services:** Selected files → “Smart Copy to…” or “Smart Move to…” → destination picker. The operation runs in our transfer manager; Finder remains unchanged. Ensure actions appear reliably in the appropriate Finder locations and contexts.
2. **Drag/drop:** Drop files or folders onto a small transfer destination target or app window.
3. **Smart Paste shortcut:** A dedicated, user-configurable shortcut, with a destination picker or Finder-window destination resolution where supported. Clipboard file URLs are inspected locally.
4. **Share/Services integration:** Only where platform API and selected-item context genuinely support it; do not depend on nonpublic Finder APIs.
5. **Menu bar background status (optional):** Number of active jobs, pause/resume and open transfer window.

**Finder Sync caveat:** Apple's Finder Sync extension is geared toward sync/status and contextual menu behaviors, not a general replacement for Finder's built-in Copy/Paste mechanism. Evaluate Finder Quick Actions and supported extension mechanisms first. Do not claim that installing a Finder extension reroutes native `Cmd+V` automatically.

**Future experimental seamless paste:** Optional opt-in shortcut ownership or event handling after a permission and conflict audit, no app-specific dependency. It must not double-paste, hijack text paste, steal a normal paste in unrelated apps, or conflict with users' key bindings. Unsupported if reliability cannot be demonstrated; retain the explicit Smart Paste workflow.

## macOS window design

- Main transfer popover/window appears for meaningful operations, but user can leave it in background.
- **Preflight sheet:** total files and logical size, destination format, proposed strategy, warnings, controls.
- **Progress:** throughput graph; active file workers; overall progress; inspect file-by-file; pause/resume/cancel; independently labeled verification phase.
- **Conflict decision:** “Replace / Keep both / Skip / Compare / Cancel” with a preview and apply-to-all scope.
- **Guided remedy:** Compatibility diagnosis; alternative destinations; safe backup/reformat steps; no dangerous confirm dialogs disguised as a routine transfer.
- **Completion:** elapsed time, bytes/files copied, errors/retries, verification result, metadata warnings, show in Finder.

SwiftUI for window layout, AppKit where needed for window control, Finder integration and services. Use macOS accessibility/keyboard conventions, keyboard navigation and native text. Match macOS appearance (light/dark and system accent) rather than designing a faux operating system.

### Sandboxing and permissions

- Determine signing/notarization/entitlements strategy before implementing persistent access or extensions.
- If sandboxed, use user-selected URLs and security-scoped bookmarks with explicit consent for persistent access; release access after use.
- Avoid requesting Full Disk Access or Accessibility for ordinary transfers if file selection/permissions suffice.
- Back-end engine should not require privileged root process in v1.
- Removable-volume permission and reconnect flow must work with real devices.

## Windows (later native release)

- App shell: WinUI 3 / Windows App SDK.
- Explorer modern context-menu entry: `IExplorerCommand` with appropriate package identity/registration; keep shell extension lightweight, no scanning or synchronous disk work on Explorer UI thread.
- Drag/drop, destination selector, notifications, shell progress integration where supported.
- Optional dedicated Smart Paste shortcut; do not claim universal interception of Explorer Paste without validated integration.
- Windows-specific metadata: ACLs, ADS, reparse points, copy flags; prefer native APIs where tested.

## Integration acceptance tests

- Finder Quick Action appears and passes exact selected paths, including Unicode, spaces, special characters and bundles.
- Copy/move works to APFS and exFAT with permissions; FAT32 blockers surface before copy.
- Clipboard text-only paste remains untouched, including in Ghostty, browsers and editors.
- Quick Action launch incurs no long Finder UI freeze; heavy scanning always async.
- Transfer keeps running with UI hidden, lock/sleep constraints explained, app can reopen and recover.
- In Windows version, Explorer keeps responding even with 100,000 selected files.

## Disclosure

“Native integration” means supported Finder/Explorer actions plus the optional user-controlled Smart Paste experience. Automatic takeover of the **existing** `Cmd+V` or `Ctrl+V` is a stretch goal, conditional on real engineering tests, not the release promise.
