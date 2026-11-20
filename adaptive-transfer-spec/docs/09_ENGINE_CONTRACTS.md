# 09 — Proposed engine / frontend contracts

**Design proposal, not an existing API.** The Rust core should expose a stable, versioned schema usable by a macOS frontend, CLI, and later WinUI 3, without OS-specific assumptions in the decision layer.

## Domain model (Rust-like pseudo-types)

```rust
struct JobRequest {
    api_version: u32,
    sources: Vec<PathRef>,
    destination: PathRef,
    operation: Operation,           // Copy | Move
    policy: TransferPolicy,
}

struct VolumeCapabilities {
    volume_id: VolumeIdentity,
    filesystem: Option<String>,
    readable: Capability,           // Yes | No | Unknown(reason)
    writable: Capability,
    max_file_bytes: Option<u64>,
    available_bytes: Option<u64>,
    supports_clone: Capability,
    supports_atomic_rename: Capability,
    supports_symlink: Capability,
    supports_xattrs: Capability,
    supports_acl: Capability,
    case_sensitive: Option<bool>,
    // Extended metadata support flags, path limits and mount properties...
}

struct Issue {
    code: IssueCode,
    severity: Severity,             // Blocker | Warning | Information
    affected_paths: Vec<PathRef>,
    affected_count: u64,
    evidence: Evidence,
    remedies: Vec<Remedy>,
}

struct Remedy {
    action: RemedyAction,
    risk: Risk,                     // NonDestructive | BackupRequired | Destructive
    execution: ExecutionMode,       // AppSupported | OpensSystemTool | InstructionsOnly | NeverAutomatic
    preconditions: Vec<Condition>,
    consequences: Vec<Consequence>,
}

struct TransferPlan {
    plan_id: String,
    source_snapshot: Vec<SourceIdentity>,
    destination_volume: VolumeIdentity,
    items: TransferInventory,
    issues: Vec<Issue>,
    strategy: Strategy,
    reasons: Vec<String>,
    preflight_complete: bool,
}
```

**Never serialize raw secrets or all full local paths into diagnostic exports by default.** Path identity and user-display path are different concepts. Use integer-safe byte counts, not floating point.

## Minimal command verbs

- `inspect(request) -> plan`: read-only discovery; blockers/resolutions.
- `start(plan_id, approved_policies) -> job_id`: validates plan freshness; returns event stream.
- `pause(job_id)`, `resume(job_id)`, `cancel(job_id)`: idempotent operations.
- `resolve(job_id, issue_id, remedy_choice)`: explicit user decision; recompute plan as necessary.
- `retry(job_id, item_id)`: targeted retry where safe.
- `verify(job_id, scope) -> verification_report`.
- `list_recoverable_jobs()`, `recover(job_id)`.

State transitions are authorized by the job controller; frontend must not write journal files directly.

## Example preflight diagnosis event

```json
{
  "api_version": 1,
  "kind": "preflight_issue",
  "issue": {
    "code": "DEST_FILE_TOO_LARGE",
    "severity": "blocker",
    "display_path": "Movies/video.mkv",
    "file_bytes": 8804682956,
    "destination_fs": "FAT32",
    "max_file_bytes": 4294967295,
    "remedies": [
      {"action": "choose_destination", "risk": "non_destructive", "execution": "app_supported"},
      {"action": "backup_then_reformat", "target_fs": "exFAT", "risk": "destructive", "execution": "opens_system_tool"},
      {"action": "cancel", "risk": "non_destructive", "execution": "app_supported"}
    ]
  }
}
```

## Progress event design

`job_created`, `scan_progress`, `plan_ready`, `blocker`, `job_started`, `worker_update`, `item_committed`, `item_failed`, `job_paused`, `job_interrupted`, `verification_started`, `verification_passed`, `verification_failed`, `job_completed`, `job_cancelled`.

Each progress event includes: `job_id`, `sequence_number`, `timestamp`, `state`, `completed_file_bytes`, `committed_files`, `total_estimated_file_bytes` (nullable), `active_file_workers`, `rolling_bytes_per_second` (nullable), `issues`, `reason_codes`. Monotonic sequence numbers support robust UI reconnect. Discovery totals can increase; UI must not show false fixed percentages.

## Strategy explanations

Examples of reason codes:

- `SAME_VOLUME_RENAME_ELIGIBLE`
- `SAME_VOLUME_APFS_CLONE_SUPPORTED`
- `ONE_LARGE_FILE_SEQUENTIAL`
- `MANY_INDEPENDENT_FILES`
- `SOURCE_DEST_SHARE_PHYSICAL_DEVICE`
- `DEST_METADATA_LIMITATION`
- `WORKER_COUNT_REDUCED_LATENCY`
- `CAPABILITY_UNCERTAIN_FALLBACK`

Reason codes become stable analytic and test assertions. Localized UI descriptions are maintained outside core.

## Versioning and persistence

- Version event and journal schemas separately.
- Include engine version, path encoding policy, OS build and filesystem capability fingerprint in recovery records.
- Disallow unsupported journal version auto-recovery; offer safe restart instead.
- Use durable write semantics, authenticated local IPC if a background helper is deployed, and event replay/last-known state on GUI reconnect.
