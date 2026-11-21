# Portstride M0 preflight

This workspace contains a read-only inspection prototype. It does not copy, move, delete, overwrite, format, or open disk utilities. The engine is portable Rust; the macOS adapter reads mount information; the CLI produces an inspection report. `adaptive-transfer-spec/` remains the design baseline.

```sh
cargo run -p portstride -- inspect /path/to/source /path/to/another --destination /path/to/directory
cargo run -p portstride -- inspect /path/to/source --destination /path/to/directory --json
cargo run -p portstride -- inspect /path/to/source --destination /path/to/directory --json --show-paths
```

Use `--` after options to pass source names starting with a dash. The JSON v3 report is an M0 inspection report, not a transfer plan or stable public API. Absolute paths and names are omitted by default. `--show-paths` includes source-relative entry names and can expose sensitive information; non-Unicode names are displayed lossily. The source index follows CLI argument order.

Exit codes are 0 for a completed inspection with no known blockers, 1 for a completed inspection with blockers, 3 for an incomplete inspection, and 2 for invalid invocation or serialization failure. If a blocker and an incomplete check both occur, exit 3 and `status: incomplete` take precedence; inspect `issues` for blockers. Exit 0 does **not** authorize a transfer; `transfer_readiness` is always `not_assessed`.

`inventory.complete` means source enumeration reported no error, not that files remained unchanged. `inspection_complete` is true only if: source enumeration completed; destination was opened and rechecked; destination-entry checks did not fail; a matching, bound destination probe supplied a nonempty filesystem name, available bytes and mount read-only state; and a supplied file-size bound has a corresponding kind. Failed identity checks and any `FS_ERROR` make inspection incomplete. An observed blocker does not itself make inspection incomplete. Optional capability values can remain unknown. Counts are observations gathered over time, not an atomic snapshot.

## v2 to v3

- `api_version` is 3. `status` is `complete`, `blocked` or `incomplete`; `inspection_complete` can be true, independently of `transfer_readiness`.
- `destination.mount_read_only` is the observed mount flag; `writable` is still unknown for a writable mount. `destination.max_file_bytes_kind` is `exact`, `upper_bound` or null. A numeric bound without its kind cannot produce a file-size blocker and makes inspection incomplete.
- Each issue adds `category`: `incompatibility`, `observed_property`, `estimate`, `unknown` or `inspection_failure`. `file_size`, when present, contains integer `file_bytes`, `limit_bytes`, and `limit_kind`. `confidence` retains `observed`, `estimated` or `unknown`. Existing issue paths, counts, severity and remedy risks retain their meaning.
- `CAPABILITY_UNVERIFIED` replaces the unconditional `METADATA_UNSUPPORTED` warning. `SYMLINK_CAPABILITY_UNVERIFIED` reports unknown support; `SYMLINK_POLICY_UNRESOLVED` reports unresolved policy when support is known; `SYMLINK_UNSUPPORTED` remains for observed lack of support. `INSPECTION_INCOMPLETE` reports failed or missing required checks. These new codes avoid describing uncertainty as an incompatibility.
- Representative redacted reports are in `examples/preflight-v3-*.json`; device identity and capacity in those fixtures are illustrative.

The macOS mount probe opens the destination without following its final symlink and uses `fstatfs` and `fpathconf` on that descriptor. The engine compares the probe's device and inode with its opened destination and discards unbound or mismatched probe data. A read-only mount produces a blocker. Folder write permission remains unknown. Device and inode identifiers are ephemeral, not durable volume identities. Other platforms return unknown capabilities. On macOS `msdos` does not distinguish FAT12, FAT16 and FAT32; 4 GiB minus one is an upper bound for all three, and smaller limits can apply. The FAT32 per-file diagnosis is tested using synthetic lengths. The sample's reformat recommendation is instructions only: Portstride cannot open a utility or perform a reformat.

## Known limitations and decisions

- On Unix, enumeration holds directory descriptors and opens children relative to them with no-follow flags. It checks a directory's device and inode after opening and again after scanning. A changed or deleted directory is reported as an incomplete scan. It stops at cycles or a depth of 256. A rename after a directory is opened does not redirect the descriptor, but the original directory can be moved or modified while it is scanned. File content and metadata can change without detection. The scan is not an atomic snapshot, and not all concurrent changes are detectable.
- Initial source-parent and destination acquisition still resolves intermediate path components through the OS. Only the final component of each acquisition is opened with no-follow; a source parent or destination that is itself a symlink is rejected. A concurrent change before acquisition can select a different directory. The destination probe and scanner acquire separate descriptors and compare their device and inode. A mismatch discards the probe; inode reuse, concurrent renames, and remount races are not fully excluded. The selected destination path is rechecked against its descriptor at the end, but it can change again afterward. Descriptor-relative destination conflict checks inspect only each selected root name and stop at an existing entry, so they do not traverse conflicting destination subtrees. No result guarantees a later write.
- Inspection continues across nested filesystems. When an entry reports a device ID different from its parent, `SOURCE_DEVICE_CHANGE` records the observation without blocking or claiming a mount was identified. Equal device IDs do not prove there is no mount boundary; APFS volumes, same-device mounts and network filesystems need additional identification. Symlinks are not followed. This policy is read-only and is not a transfer policy.
- Exact path collisions are detected regardless of case sensitivity. ASCII case collisions are detected when the destination reports case insensitivity; ASCII components are compared separately even under non-ASCII parents. Unicode normalization, filesystem-specific case folding, invalid names, path limits and all existing-name equivalences are not fully checked. Non-Unicode filenames can be inspected but cannot be faithfully represented by `--show-paths`.
- Metadata, ACLs, xattrs, hard-link relationships, sparse allocation, mount boundaries, cloud placeholders, filename restrictions and exact destination write ability are not fully assessed. Unknown capabilities remain `null`; metadata preservation is never claimed.
- Scan and destination failures yield partial counts and findings. If the opened destination is encountered while scanning a source, traversal stops at that directory and reports a blocker. Concurrent renames and aliases can still defeat relationship checks. Exact identity and snapshot validation remain future work.
- Logical free-space comparisons are warnings because no sparse-copy or metadata policy has been selected. A detected single-file limit and a confirmed read-only mount are blockers.
- The example diagnosis suggests opening a system tool, but this milestone only provides instructions. Move verification policy, conflict policy, and transfer semantics remain open design decisions.
- The roadmap's complete benchmark harness, transfer baseline timings and physical filesystem matrix remain incomplete. No removable volume was modified or tested.

Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` for verification.

## Licensing

The project license has not yet been selected. No license grant is implied by publication.

## Scanner fixture and benchmark harness

The harness generates deterministic fixtures in a newly created directory and measures read-only inspection. It does not implement a copy benchmark. It refuses an existing output path and never removes a fixture. An interrupted or failed generation can leave a partial directory that the user must inspect before removing. Generation errors are emitted as JSON on standard error with status `error`; a partial directory is not automatically deleted.

```sh
mkdir -m 700 /private/tmp/portstride-work-new
mkdir /private/tmp/portstride-work-new/destination
cargo run -p portstride --bin portstride-harness -- generate /private/tmp/portstride-work-new/fixture 1000 128 flat
cargo run -p portstride --bin portstride-harness -- benchmark /private/tmp/portstride-work-new/fixture /private/tmp/portstride-work-new/destination 5 1 flat
```

Scenarios include flat files, nested and wide directories, empty directories, Unicode names, conflicting source names, valid and broken links, and a sparse-file candidate. Physical sparseness is not verified. The simulated scenario evaluates a synthetic FAT-family size limit without creating a large file or testing a filesystem. Permission-denial scenarios are reported as skipped where they cannot be demonstrated reliably. File count, depth and logical data are bounded; filesystem allocation granularity and metadata prevent a strict cap on physical disk use. Use a disposable, trusted parent that cannot be modified concurrently. It must be owned by the effective user and have no group or other write mode bits. Unix parent components are opened with no-follow flags; generation is unsupported on non-Unix. Mode checks do not examine ACLs or ensure ancestors cannot be modified. For this developer fixture generator only, the security assumption is that the parent, its ancestors and ACLs are trusted and no same-user process modifies them concurrently. These assumptions do not apply to the production transfer engine. The generator rejects an effective user ID of zero; other forms of elevated privilege are outside its checks. A process with write access can replace a newly created directory between creation and opening, redirecting exclusive creation into that directory. Existing files are not overwritten. Before creating the fixture, the generator checks available space against the logical bytes plus 16 KiB per file and 1 MiB; this estimate is neither an allocation bound nor a reservation. The report distinguishes warm-up and measured runs and records cache state as uncontrolled; timings are not copy throughput.

Disposable APFS, exFAT and FAT32 image preflight tests have been run; no removable-drive tests have been run. See [filesystem coverage](docs/filesystem-coverage.md) for the distinction between simulated and real tests.
