use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
pub struct Capabilities {
    pub volume_id: Option<String>,
    pub probe_identity: Option<(u64, u64)>,
    pub filesystem: Option<String>,
    pub writable: Option<bool>,
    pub mount_read_only: Option<bool>,
    pub available_bytes: Option<u64>,
    pub max_file_bytes: Option<u64>,
    pub max_file_bytes_kind: Option<LimitKind>,
    pub case_sensitive: Option<bool>,
    pub supports_symlink: Option<bool>,
    pub supports_xattrs: Option<bool>,
    pub supports_acl: Option<bool>,
    pub supports_clone: Option<bool>,
    pub supports_atomic_rename: Option<bool>,
    pub uncertainty: Vec<String>,
}
impl Capabilities {
    pub fn unknown() -> Self {
        Self {
            volume_id: None,
            probe_identity: None,
            filesystem: None,
            writable: None,
            mount_read_only: None,
            available_bytes: None,
            max_file_bytes: None,
            max_file_bytes_kind: None,
            case_sensitive: None,
            supports_symlink: None,
            supports_xattrs: None,
            supports_acl: None,
            supports_clone: None,
            supports_atomic_rename: None,
            uncertainty: vec!["Capabilities were not verified.".into()],
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitKind {
    Exact,
    UpperBound,
}

#[derive(Debug, Serialize)]
pub struct FileSizeEvidence {
    pub file_bytes: u64,
    pub limit_bytes: u64,
    pub limit_kind: Option<LimitKind>,
}

#[derive(Debug, Serialize)]
pub struct Remedy {
    pub action: &'static str,
    pub risk: &'static str,
    pub execution: &'static str,
    pub description: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Issue {
    pub code: &'static str,
    pub severity: &'static str,
    pub affected_paths: Vec<String>,
    pub affected_count: u64,
    pub evidence: String,
    pub confidence: &'static str,
    pub category: &'static str,
    pub file_size: Option<FileSizeEvidence>,
    pub remedies: Vec<Remedy>,
}
#[derive(Default, Debug, Serialize)]
pub struct Inventory {
    pub files: u64,
    pub directories: u64,
    pub symlinks: u64,
    pub special_files: u64,
    pub logical_bytes: u64,
    pub largest_file_bytes: u64,
    pub size_buckets: SizeBuckets,
    pub complete: bool,
}
#[derive(Default, Debug, Serialize)]
pub struct SizeBuckets {
    pub under_1_mib: u64,
    pub from_1_mib_to_1_gib: u64,
    pub at_least_1_gib: u64,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub api_version: u32,
    pub kind: &'static str,
    pub status: &'static str,
    pub inspection_complete: bool,
    pub transfer_readiness: &'static str,
    pub path_policy: &'static str,
    pub destination: Capabilities,
    pub inventory: Inventory,
    pub issues: Vec<Issue>,
}

fn remedy(action: &'static str, risk: &'static str, description: &'static str) -> Remedy {
    Remedy {
        action,
        risk,
        execution: "instructions_only",
        description,
    }
}
fn issue(code: &'static str, severity: &'static str, label: String, evidence: String) -> Issue {
    let mut remedies = vec![remedy(
        "choose_destination",
        "non_destructive",
        "Choose a compatible destination.",
    )];
    if matches!(
        code,
        "CAPABILITY_UNVERIFIED" | "INSPECTION_INCOMPLETE" | "SOURCE_DEVICE_CHANGE"
    ) {
        remedies.clear();
    }
    match code {
        "DEST_FILE_TOO_LARGE" => {
            remedies.push(remedy("backup_then_reformat", "destructive", "Back up and verify the backup first. Erasing and reformatting destroys existing data; check device compatibility."));
            remedies.push(remedy(
                "split_explicitly",
                "non_destructive",
                "Splitting is a separate future workflow and requires reassembly.",
            ));
        }
        "INSUFFICIENT_CAPACITY" => remedies.insert(
            0,
            remedy(
                "free_space",
                "non_destructive",
                "Free space or reduce the selection.",
            ),
        ),
        "ACCESS_DENIED" | "DEST_READ_ONLY" => remedies.push(remedy(
            "inspect_permissions",
            "non_destructive",
            "Check the selected folder's permissions and mount state.",
        )),
        "CASE_NAME_COLLISION" | "PATH_UNSUPPORTED" => remedies.push(remedy(
            "change_policy",
            "non_destructive",
            "Review names and conflicts before any transfer.",
        )),
        "FS_ERROR" => remedies.push(remedy(
            "inspect_permissions",
            "non_destructive",
            "Check the path, permissions, and storage connection; retry inspection.",
        )),
        _ => {}
    }
    if code != "SOURCE_DEVICE_CHANGE" {
        remedies.push(remedy("cancel", "non_destructive", "Cancel the operation."));
    }
    let affected_count = u64::from(!label.is_empty());
    Issue {
        code,
        severity,
        affected_paths: if label.is_empty() {
            vec![]
        } else {
            vec![label]
        },
        affected_count,
        evidence,
        confidence: "observed",
        category: match code {
            "FS_ERROR" | "INSPECTION_INCOMPLETE" => "inspection_failure",
            "DEST_READ_ONLY" | "SOURCE_DEVICE_CHANGE" => "observed_property",
            "INSUFFICIENT_CAPACITY" => "estimate",
            "CAPABILITY_UNVERIFIED"
            | "FILENAME_RESTRICTION_UNVERIFIED"
            | "SYMLINK_CAPABILITY_UNVERIFIED"
            | "SYMLINK_POLICY_UNRESOLVED" => "unknown",
            _ => "incompatibility",
        },
        file_size: None,
        remedies,
    }
}

/// Diagnose a known per-file limit from already observed metadata.
pub fn file_size_issue(bytes: u64, max: Option<u64>) -> Option<Issue> {
    file_size_issue_with_kind(bytes, max, Some(LimitKind::UpperBound))
}

pub fn file_size_issue_with_kind(
    bytes: u64,
    max: Option<u64>,
    kind: Option<LimitKind>,
) -> Option<Issue> {
    let max = max?;
    let kind = kind?;
    (bytes > max).then(|| {
        let mut finding = issue("DEST_FILE_TOO_LARGE", "blocker", String::new(), format!("A file is {bytes} bytes; the reported destination single-file limit or upper bound is {max} bytes. Aggregate folder size is a separate constraint."));
        finding.affected_count = 1;
        finding.file_size = Some(FileSizeEvidence { file_bytes: bytes, limit_bytes: max, limit_kind: Some(kind) });
        finding
    })
}

/// Inspect selected paths without writing or intentionally following symbolic links.
/// Results are a snapshot and must be revalidated before any future transfer.
pub fn inspect(sources: &[PathBuf], destination: &Path, capabilities: Capabilities) -> Report {
    inspect_with_paths(sources, destination, capabilities, PathDisplay::Redacted)
}

#[derive(Clone, Copy)]
pub enum PathDisplay {
    Redacted,
    RelativeNames,
}

pub fn inspect_with_paths(
    sources: &[PathBuf],
    destination: &Path,
    mut capabilities: Capabilities,
    display: PathDisplay,
) -> Report {
    #[cfg(unix)]
    let opened_destination = anchored::open_directory(destination)
        .and_then(|fd| anchored::stat_directory(&fd).map(|stat| (fd, stat)));
    #[cfg(unix)]
    let probe_mismatch = match (&opened_destination, capabilities.probe_identity) {
        (Ok((_, stat)), Some(id)) => id != anchored::identity(stat),
        _ => false,
    };
    #[cfg(unix)]
    let unbound_probe = capabilities.probe_identity.is_none()
        && (capabilities.volume_id.is_some()
            || capabilities.filesystem.is_some()
            || capabilities.writable.is_some()
            || capabilities.mount_read_only.is_some()
            || capabilities.available_bytes.is_some()
            || capabilities.max_file_bytes.is_some()
            || capabilities.max_file_bytes_kind.is_some()
            || capabilities.case_sensitive.is_some()
            || capabilities.supports_symlink.is_some()
            || capabilities.supports_xattrs.is_some()
            || capabilities.supports_acl.is_some()
            || capabilities.supports_clone.is_some()
            || capabilities.supports_atomic_rename.is_some());
    #[cfg(unix)]
    if probe_mismatch || unbound_probe || opened_destination.is_err() {
        capabilities = Capabilities::unknown();
        capabilities.uncertainty.push("Destination probe results could not be bound to the opened directory and were discarded.".into());
    }
    let mut scanner = Scanner {
        inventory: Inventory {
            complete: true,
            ..Inventory::default()
        },
        issues: Vec::new(),
        capabilities: &capabilities,
        #[cfg(unix)]
        destination: None,
        #[cfg(unix)]
        destination_identity: None,
        #[cfg(unix)]
        active: std::collections::HashSet::new(),
        display,
        names: BTreeMap::new(),
    };
    #[cfg(unix)]
    {
        if probe_mismatch || unbound_probe {
            scanner.issues.push(issue(
                "FS_ERROR",
                "blocker",
                "destination".into(),
                "Destination probe identity is missing or does not match the opened directory; retry.".into(),
            ));
        }
        match opened_destination {
            Ok((fd, stat)) => {
                scanner.destination_identity = Some(anchored::identity(&stat));
                scanner.destination = Some(fd);
            }
            Err(e) => scanner.issues.push(issue(
                "FS_ERROR",
                "blocker",
                "destination".into(),
                format!(
                    "Cannot open destination directory without following its final symlink: {}",
                    e.kind()
                ),
            )),
        }
    }
    #[cfg(not(unix))]
    scanner.issues.push(issue(
        "FS_ERROR",
        "blocker",
        "destination".into(),
        "Safe descriptor-relative inspection is unavailable on this platform.".into(),
    ));
    if sources.is_empty() {
        scanner.inventory.complete = false;
        scanner.issues.push(issue(
            "PATH_UNSUPPORTED",
            "blocker",
            String::new(),
            "No sources were selected.".into(),
        ));
    }
    if capabilities.mount_read_only == Some(true) {
        scanner.issues.push(issue(
            "DEST_READ_ONLY",
            "blocker",
            "destination".into(),
            "Destination mount is read-only.".into(),
        ));
    }
    if capabilities.mount_read_only != Some(true) && capabilities.writable == Some(false) {
        scanner.issues.push(issue(
            "ACCESS_DENIED",
            "blocker",
            "destination".into(),
            "The selected folder denied the effective access check at inspection time.".into(),
        ));
    }
    for (i, source) in sources.iter().enumerate() {
        let label = format!("source[{}]", i + 1);
        if let Some(name) = source.file_name() {
            #[cfg(unix)]
            {
                scanner.check_destination(name, &label);
                let parent_path = source
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                match anchored::open_directory(parent_path) {
                    Ok(parent) => scanner.walk(&parent, name, PathBuf::from(name), label),
                    Err(e) => scanner.failure(label, "Cannot open source parent", e),
                }
            }
            #[cfg(not(unix))]
            {
                let _ = name;
                scanner.inventory.complete = false;
            }
        } else {
            scanner.inventory.complete = false;
            scanner.issues.push(issue(
                "PATH_UNSUPPORTED",
                "blocker",
                label,
                "Source has no transferable final path component.".into(),
            ));
        }
    }
    #[cfg(unix)]
    if let Some(fd) = &scanner.destination {
        match anchored::path_still_identifies(destination, fd) {
            Ok(true) => {}
            _ => scanner.issues.push(issue(
                "FS_ERROR",
                "blocker",
                "destination".into(),
                "Destination path changed or became inaccessible during inspection; retry.".into(),
            )),
        }
    }
    if let Some(free) = capabilities.available_bytes
        && scanner.inventory.logical_bytes > free
    {
        let mut finding = issue(
            "INSUFFICIENT_CAPACITY",
            "warning",
            "destination".into(),
            format!(
                "Observed logical bytes ({}) exceed available bytes ({free}). Actual allocation depends on sparse-file and metadata policy.",
                scanner.inventory.logical_bytes
            ),
        );
        finding.confidence = "estimated";
        scanner.issues.push(finding);
    }
    let mut uncertainty = issue("CAPABILITY_UNVERIFIED", "warning", String::new(), "Metadata, Unicode normalization and case equivalence, non-UTF-8 names, filename restrictions, write permissions, source stability and transfer policies are not fully verified.".into());
    uncertainty.confidence = "unknown";
    scanner.issues.push(uncertainty);
    let complete = scanner.inventory.complete
        && !scanner.issues.iter().any(|i| i.code == "FS_ERROR")
        && capabilities.probe_identity.is_some()
        && capabilities
            .filesystem
            .as_ref()
            .is_some_and(|s| !s.is_empty())
        && capabilities.available_bytes.is_some()
        && capabilities.mount_read_only.is_some()
        && (capabilities.max_file_bytes.is_some() == capabilities.max_file_bytes_kind.is_some());
    if !complete {
        let mut finding = issue("INSPECTION_INCOMPLETE", "warning", String::new(), "One or more required M0 checks failed, were skipped, or lacked required probe observations.".into());
        finding.confidence = "unknown";
        scanner.issues.push(finding);
    }
    let blocked = scanner.issues.iter().any(|i| i.severity == "blocker");
    let Scanner {
        inventory, issues, ..
    } = scanner;
    Report {
        api_version: 3,
        kind: "preflight_report",
        status: if !complete {
            "incomplete"
        } else if blocked {
            "blocked"
        } else {
            "complete"
        },
        inspection_complete: complete,
        transfer_readiness: "not_assessed",
        path_policy: match display {
            PathDisplay::Redacted => "redacted source indices; no local names or absolute paths",
            PathDisplay::RelativeNames => {
                "source indices and lossy relative names; no absolute paths"
            }
        },
        destination: capabilities,
        inventory,
        issues,
    }
}

#[cfg(unix)]
fn comparison_key(relative: &Path, case_sensitive: Option<bool>) -> PathBuf {
    if case_sensitive != Some(false) {
        return relative.to_path_buf();
    }
    relative
        .components()
        .map(|component| {
            let name = component.as_os_str();
            match name.to_str() {
                Some(text) if text.is_ascii() => {
                    std::ffi::OsString::from(text.to_ascii_lowercase())
                }
                _ => name.to_os_string(),
            }
        })
        .collect()
}

#[cfg(unix)]
fn device_change_issue(parent_device: u64, child_device: u64, label: &str) -> Option<Issue> {
    (parent_device != child_device).then(|| issue("SOURCE_DEVICE_CHANGE", "information", label.into(), "A source entry reports a different filesystem device from its parent. Inspection continues; device identity does not identify every mount boundary.".into()))
}

#[cfg(all(test, unix))]
mod device_tests {
    #[test]
    fn simulated_unicode_and_non_utf8_names_are_not_folded() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};
        let a = PathBuf::from(OsString::from_vec(vec![0xff, b'A']));
        let b = PathBuf::from(OsString::from_vec(vec![0xff, b'a']));
        assert_ne!(
            super::comparison_key(&a, Some(false)),
            super::comparison_key(&b, Some(false))
        );
        assert_ne!(
            super::comparison_key(std::path::Path::new("é"), Some(false)),
            super::comparison_key(std::path::Path::new("É"), Some(false))
        );
        assert_ne!(
            super::comparison_key(std::path::Path::new("A"), None),
            super::comparison_key(std::path::Path::new("a"), None)
        );
    }

    #[test]
    fn simulated_device_transition_is_reported_without_claiming_a_mount() {
        let finding = super::device_change_issue(1, 2, "source[1]").unwrap();
        assert_eq!(finding.code, "SOURCE_DEVICE_CHANGE");
        assert_eq!(finding.category, "observed_property");
        assert!(super::device_change_issue(1, 1, "source[1]").is_none());
    }
}

struct Scanner<'a> {
    inventory: Inventory,
    issues: Vec<Issue>,
    capabilities: &'a Capabilities,
    #[cfg(unix)]
    destination: Option<std::os::fd::OwnedFd>,
    #[cfg(unix)]
    destination_identity: Option<(u64, u64)>,
    #[cfg(unix)]
    active: std::collections::HashSet<(u64, u64)>,
    names: BTreeMap<PathBuf, String>,
    display: PathDisplay,
}
#[cfg(unix)]
impl Scanner<'_> {
    fn failure(&mut self, label: String, context: &str, error: std::io::Error) {
        self.inventory.complete = false;
        self.issues.push(issue(
            "FS_ERROR",
            "blocker",
            label,
            format!("{context}: {}", error.kind()),
        ));
    }

    fn check_destination(&mut self, name: &std::ffi::OsStr, label: &str) {
        if let Some(dest) = &self.destination {
            match anchored::stat_child(dest, name) {
                Ok(_) => self.issues.push(issue(
                    "PATH_UNSUPPORTED",
                    "blocker",
                    label.into(),
                    "A destination entry already exists; an explicit conflict policy is required."
                        .into(),
                )),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => self.issues.push(issue(
                    "FS_ERROR",
                    "blocker",
                    label.into(),
                    format!("Cannot check destination entry: {}", e.kind()),
                )),
            }
        }
    }

    fn walk(
        &mut self,
        parent: &std::os::fd::OwnedFd,
        name: &std::ffi::OsStr,
        relative: PathBuf,
        label: String,
    ) {
        use rustix::fs::FileType;
        let metadata = match anchored::stat_child(parent, name) {
            Ok(m) => m,
            Err(e) => {
                self.failure(label, "Cannot inspect source entry", e);
                return;
            }
        };
        let parent_stat = match anchored::stat_directory(parent) {
            Ok(stat) => stat,
            Err(e) => {
                self.failure(label, "Cannot inspect source parent identity", e);
                return;
            }
        };
        if let Some(finding) = device_change_issue(
            anchored::identity(&parent_stat).0,
            anchored::identity(&metadata).0,
            &label,
        ) {
            self.issues.push(finding);
        }
        if self
            .capabilities
            .filesystem
            .as_deref()
            .is_some_and(|fs| fs.eq_ignore_ascii_case("exfat") || fs.eq_ignore_ascii_case("msdos"))
            && let Some(text) = name.to_str()
            && (text.encode_utf16().count() > 255
                || text.chars().any(|c| {
                    c <= '\u{1f}' || matches!(c, '"' | '*' | ':' | '<' | '>' | '?' | '\\' | '|')
                }))
        {
            let mut finding = issue(
                "FILENAME_RESTRICTION_UNVERIFIED",
                "warning",
                label.clone(),
                "This component conflicts with FAT-family on-disk filename rules; the mounted driver's name mapping and rejection behavior are unverified.".into(),
            );
            finding.confidence = "unknown";
            self.issues.push(finding);
        }
        let key = comparison_key(&relative, self.capabilities.case_sensitive);
        if let Some(previous) = self.names.insert(key, label.clone()) {
            let mut finding = issue(
                "CASE_NAME_COLLISION",
                "blocker",
                label.clone(),
                "Multiple source entries map to the same destination name.".into(),
            );
            finding.affected_paths.insert(0, previous);
            finding.affected_count = 2;
            self.issues.push(finding);
        }
        match FileType::from_raw_mode(metadata.st_mode) {
            FileType::Symlink => {
                self.inventory.symlinks += 1;
                let severity = if self.capabilities.supports_symlink == Some(false) {
                    "blocker"
                } else {
                    "warning"
                };
                let code = match self.capabilities.supports_symlink {
                    Some(false) => "SYMLINK_UNSUPPORTED",
                    Some(true) => "SYMLINK_POLICY_UNRESOLVED",
                    None => "SYMLINK_CAPABILITY_UNVERIFIED",
                };
                let mut finding = issue(code, severity, label, "Symbolic links are not followed; preservation and target safety require a policy.".into());
                if severity == "warning" {
                    finding.confidence = "unknown";
                }
                self.issues.push(finding);
            }
            FileType::RegularFile => {
                let Ok(bytes) = u64::try_from(metadata.st_size) else {
                    self.failure(
                        label,
                        "Invalid file length",
                        std::io::Error::other("negative size"),
                    );
                    return;
                };
                self.inventory.files += 1;
                match self.inventory.logical_bytes.checked_add(bytes) {
                    Some(n) => self.inventory.logical_bytes = n,
                    None => {
                        self.failure(
                            label.clone(),
                            "Logical byte total overflow",
                            std::io::Error::other("overflow"),
                        );
                    }
                }
                self.inventory.largest_file_bytes = self.inventory.largest_file_bytes.max(bytes);
                if bytes < 1024 * 1024 {
                    self.inventory.size_buckets.under_1_mib += 1;
                } else if bytes < 1024 * 1024 * 1024 {
                    self.inventory.size_buckets.from_1_mib_to_1_gib += 1;
                } else {
                    self.inventory.size_buckets.at_least_1_gib += 1;
                }
                if self.capabilities.max_file_bytes_kind.is_some()
                    && let Some(mut finding) = file_size_issue_with_kind(
                        bytes,
                        self.capabilities.max_file_bytes,
                        self.capabilities.max_file_bytes_kind,
                    )
                {
                    finding.affected_paths.push(label);
                    self.issues.push(finding);
                }
            }
            FileType::Directory => {
                let id = anchored::identity(&metadata);
                if Some(id) == self.destination_identity {
                    self.inventory.complete = false;
                    self.issues.push(issue(
                        "PATH_UNSUPPORTED",
                        "blocker",
                        label,
                        "Destination directory was encountered inside a source.".into(),
                    ));
                    return;
                }
                if self.active.contains(&id) || self.active.len() >= 256 {
                    self.failure(
                        label,
                        "Directory cycle or depth limit",
                        std::io::Error::other("cycle or depth limit"),
                    );
                    return;
                }
                let fd = match anchored::open_verified_directory(parent, name, &metadata) {
                    Ok(fd) => fd,
                    Err(e) => {
                        self.failure(label, "Cannot safely open source directory", e);
                        return;
                    }
                };
                self.inventory.directories += 1;
                self.active.insert(id);
                match anchored::names(&fd) {
                    Ok(children) => {
                        for (i, child) in children.into_iter().enumerate() {
                            let child_label = match self.display {
                                PathDisplay::Redacted => format!("{label}/entry[{}]", i + 1),
                                PathDisplay::RelativeNames => format!("{label}/{child:?}"),
                            };
                            self.walk(&fd, &child, relative.join(&child), child_label);
                        }
                    }
                    Err(e) => self.failure(label.clone(), "Cannot enumerate source directory", e),
                }
                self.active.remove(&id);
                match anchored::stat_child(parent, name) {
                    Ok(after)
                        if anchored::identity(&after) == id
                            && FileType::from_raw_mode(after.st_mode) == FileType::Directory => {}
                    _ => self.failure(
                        label,
                        "Source directory was renamed, removed or replaced during inspection",
                        std::io::Error::other("identity changed"),
                    ),
                }
            }
            _ => {
                self.inventory.special_files += 1;
                self.issues.push(issue(
                    "PATH_UNSUPPORTED",
                    "blocker",
                    label,
                    "Special files require an explicit future transfer policy.".into(),
                ));
            }
        }
    }
}

#[cfg(unix)]
mod anchored;
