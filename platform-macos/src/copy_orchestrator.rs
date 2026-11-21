//! Sequential orchestration reserved for privately authorized disposable fixtures.
use crate::{
    copy_transaction::{self, Authorization, AuthorizationRequest, MetadataPolicy, Request},
    job_plan::{ItemKind, Plan, recheck_absent},
};
use rustix::fs::{self, AtFlags, Mode, OFlags, Stat};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Write},
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
fn open_dir(path: &Path) -> io::Result<OwnedFd> {
    Ok(fs::open(path, DIR_FLAGS, Mode::empty())?)
}
fn id(stat: &Stat) -> (u64, u64) {
    (stat.st_dev as u64, stat.st_ino)
}
fn source_same(a: &Stat, b: &Stat) -> bool {
    id(a) == id(b)
        && a.st_mode == b.st_mode
        && a.st_nlink == b.st_nlink
        && a.st_size == b.st_size
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
}
fn checked_dir(parent: &OwnedFd, name: &OsStr, expected: &Stat) -> io::Result<OwnedFd> {
    let named = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    let dir = fs::openat(parent, name, DIR_FLAGS, Mode::empty())?;
    if fs::FileType::from_raw_mode(named.st_mode) != fs::FileType::Directory
        || id(&named) != id(expected)
        || id(&fs::fstat(&dir)?) != id(expected)
    {
        return Err(io::Error::other("directory identity changed"));
    }
    Ok(dir)
}
fn walk(
    root: &OwnedFd,
    path: &[OsString],
    expected: &BTreeMap<Vec<OsString>, Stat>,
    check_source: bool,
) -> io::Result<OwnedFd> {
    let mut dir = fs::openat(root, ".", DIR_FLAGS, Mode::empty())?;
    for i in 0..path.len() {
        let stat = expected
            .get(&path[..=i])
            .ok_or_else(|| io::Error::other("unbound directory"))?;
        dir = checked_dir(&dir, &path[i], stat)?;
        if check_source && !source_same(stat, &fs::fstat(&dir)?) {
            return Err(io::Error::other("source directory changed"));
        }
    }
    Ok(dir)
}
#[derive(Debug)]
enum Outcome {
    DirectoryCreated,
    DirectoryCreationUncertain { error: String },
    Committed(copy_transaction::Completed, bool, Option<String>),
    Unsupported(crate::job_plan::UnsupportedKind),
    Failed { phase: &'static str, error: String },
    TransactionFailed(copy_transaction::CopyFailure),
    CancelledTransaction(copy_transaction::CopyFailure),
    Cancelled,
    NotAttempted,
}
fn mkdir_failure(error: rustix::io::Errno) -> Outcome {
    if error == rustix::io::Errno::EXIST {
        Outcome::Failed {
            phase: "create directory",
            error: error.to_string(),
        }
    } else {
        Outcome::DirectoryCreationUncertain {
            error: error.to_string(),
        }
    }
}
#[derive(Debug)]
struct ItemResult {
    source_path: Vec<OsString>,
    path: Vec<OsString>,
    outcome: Outcome,
}
#[derive(Debug)]
enum JobState {
    Ready,
    Rejected { phase: &'static str, error: String },
    Cancelled,
}
#[derive(Debug)]
struct JobResult {
    state: JobState,
    outcomes: Vec<ItemResult>,
}
#[derive(Debug, Default, PartialEq, Eq)]
struct Summary {
    directories_created: usize,
    committed: usize,
    failed: usize,
    cancelled: usize,
    unsupported: usize,
    not_attempted: usize,
    written: u128,
    verified: u128,
    ledger_incomplete: usize,
}
impl JobResult {
    fn summary(&self) -> Summary {
        let mut summary = Summary::default();
        for item in &self.outcomes {
            match &item.outcome {
                Outcome::DirectoryCreated => summary.directories_created += 1,
                Outcome::Committed(done, ledger_complete, _) => {
                    summary.committed += 1;
                    if !ledger_complete {
                        summary.ledger_incomplete += 1;
                    }
                    summary.written += u128::from(done.progress.written);
                    summary.verified += u128::from(done.progress.verified);
                }
                Outcome::Failed { .. } | Outcome::DirectoryCreationUncertain { .. } => {
                    summary.failed += 1
                }
                Outcome::TransactionFailed(failure) | Outcome::CancelledTransaction(failure) => {
                    if matches!(item.outcome, Outcome::CancelledTransaction(_)) {
                        summary.cancelled += 1;
                    } else {
                        summary.failed += 1;
                    }
                    summary.written += u128::from(failure.progress.written);
                    summary.verified += u128::from(failure.progress.verified);
                }
                Outcome::Cancelled => summary.cancelled += 1,
                Outcome::Unsupported(_) => summary.unsupported += 1,
                Outcome::NotAttempted => summary.not_attempted += 1,
            }
        }
        summary
    }
    fn committed(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|x| matches!(x.outcome, Outcome::Committed(..)))
            .count()
    }
    fn success(&self) -> bool {
        matches!(self.state, JobState::Ready)
            && self.outcomes.iter().all(|x| {
                matches!(
                    x.outcome,
                    Outcome::Committed(_, true, _) | Outcome::DirectoryCreated
                )
            })
    }
}
// Only the fixture harness can create this capability. It does not grant general copy access.
struct FixtureJob {
    root: PathBuf,
    fixture: OwnedFd,
    manifest: File,
    source: OwnedFd,
    destination: OwnedFd,
    plan: Plan,
    metadata: MetadataPolicy,
}
impl FixtureJob {
    fn check_manifest(&self) -> io::Result<()> {
        let manifest = fs::fstat(&self.manifest)?;
        let named = fs::statat(&self.fixture, "manifest.json", AtFlags::SYMLINK_NOFOLLOW)?;
        if fs::FileType::from_raw_mode(named.st_mode) != fs::FileType::RegularFile
            || id(&manifest) != id(&named)
            || manifest.st_nlink != 1
            || manifest.st_uid != rustix::process::geteuid().as_raw()
            || manifest.st_mode & 0o077 != 0
        {
            return Err(io::Error::other("fixture manifest changed"));
        }
        Ok(())
    }
    fn record(&self, event: &str, path: &[OsString], stat: Option<&Stat>) -> io::Result<()> {
        self.check_manifest()?;
        let path = path
            .iter()
            .map(|component| component.to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let identity = stat
            .map(|s| format!("[{},{}]", s.st_dev as u64, s.st_ino))
            .unwrap_or_else(|| "null".to_owned());
        writeln!(
            &self.manifest,
            "{{\"event\":\"{event}\",\"path\":\"{path}\",\"identity\":{identity}}}"
        )?;
        self.manifest.sync_all()
    }
    fn check_roots(&self) -> io::Result<()> {
        let named_root = open_dir(&self.root)?;
        let held = fs::fstat(&self.fixture)?;
        if id(&fs::fstat(&named_root)?) != id(&held)
            || held.st_uid != rustix::process::geteuid().as_raw()
            || held.st_mode & 0o077 != 0
        {
            return Err(io::Error::other("fixture changed or is not private"));
        }
        checked_dir(&self.fixture, OsStr::new("source"), self.plan.source_root())?;
        checked_dir(
            &self.fixture,
            OsStr::new("destination"),
            self.plan.destination_root(),
        )?;
        if !source_same(&fs::fstat(&self.source)?, self.plan.source_root())
            || id(&fs::fstat(&self.destination)?) != id(self.plan.destination_root())
        {
            return Err(io::Error::other("job root changed"));
        }
        Ok(())
    }
    fn run(self, cancel: &AtomicBool) -> JobResult {
        self.run_with_hook(cancel, |_, _| Ok(()))
    }
    fn run_with_hook(
        self,
        cancel: &AtomicBool,
        mut hook: impl FnMut(usize, &str) -> io::Result<()>,
    ) -> JobResult {
        let state = if matches!(self.metadata, MetadataPolicy::Declined) {
            JobState::Rejected {
                phase: "authorization",
                error: "bytes-only metadata acknowledgement required".into(),
            }
        } else if cancel.load(Ordering::Relaxed) {
            JobState::Cancelled
        } else if let Err(error) = self.check_roots().and_then(|()| self.check_manifest()) {
            JobState::Rejected {
                phase: "revalidate",
                error: error.to_string(),
            }
        } else {
            JobState::Ready
        };
        if !matches!(state, JobState::Ready) {
            let outcomes = self
                .plan
                .items()
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let outcome = if index == 0 {
                        match &state {
                            JobState::Rejected { phase, error } => Outcome::Failed {
                                phase,
                                error: error.clone(),
                            },
                            JobState::Cancelled => Outcome::Cancelled,
                            JobState::Ready => unreachable!(),
                        }
                    } else {
                        Outcome::NotAttempted
                    };
                    ItemResult {
                        source_path: item.source_path().to_vec(),
                        path: item.path().to_vec(),
                        outcome,
                    }
                })
                .collect();
            return JobResult { state, outcomes };
        }
        let mut outcomes = Vec::with_capacity(self.plan.items().len());
        let mut source_dirs = self.plan.source_directories().clone();
        let mut dest_dirs = BTreeMap::new();
        let mut stopped = false;
        for (index, item) in self.plan.items().iter().enumerate() {
            if stopped {
                outcomes.push(ItemResult {
                    source_path: item.source_path().to_vec(),
                    path: item.path().to_vec(),
                    outcome: Outcome::NotAttempted,
                });
                continue;
            }
            if cancel.load(Ordering::Relaxed) {
                outcomes.push(ItemResult {
                    source_path: item.source_path().to_vec(),
                    path: item.path().to_vec(),
                    outcome: Outcome::Cancelled,
                });
                stopped = true;
                continue;
            }
            let operation = (|| -> Result<Outcome, (&'static str, String)> {
                if matches!(self.metadata, MetadataPolicy::Declined) {
                    return Err((
                        "authorization",
                        "bytes-only metadata acknowledgement required".into(),
                    ));
                }
                hook(index, "before_item").map_err(|e| ("before item", e.to_string()))?;
                self.check_roots()
                    .map_err(|e| ("revalidate", e.to_string()))?;
                let (name, parents) = item
                    .path()
                    .split_last()
                    .ok_or(("revalidate", "empty path".into()))?;
                let (source_name, source_parents) = item
                    .source_path()
                    .split_last()
                    .ok_or(("revalidate", "empty source path".into()))?;
                let source_parent = walk(&self.source, source_parents, &source_dirs, true)
                    .map_err(|e| ("source directory", e.to_string()))?;
                let current = fs::statat(&source_parent, source_name, AtFlags::SYMLINK_NOFOLLOW)
                    .map_err(|e| ("source", e.to_string()))?;
                if !source_same(item.stat(), &current) {
                    return Err(("source", "source changed since planning".into()));
                }
                if let ItemKind::Unsupported(reason) = item.kind() {
                    return Ok(Outcome::Unsupported(reason));
                }
                let dest_parent = walk(&self.destination, parents, &dest_dirs, false)
                    .map_err(|e| ("destination directory", e.to_string()))?;
                recheck_absent(&dest_parent, name)
                    .map_err(|e| ("destination mapping", format!("{e:?}")))?;
                if item.kind() == ItemKind::Directory {
                    self.record("mkdir_attempt", item.path(), None)
                        .map_err(|e| ("fixture ledger", e.to_string()))?;
                    if let Err(error) =
                        fs::mkdirat(&dest_parent, name, Mode::from_bits_truncate(0o700))
                    {
                        return Ok(mkdir_failure(error));
                    }
                    if let Err(error) = hook(index, "directory_created") {
                        return Ok(Outcome::DirectoryCreationUncertain {
                            error: error.to_string(),
                        });
                    }
                    let created = match fs::statat(&dest_parent, name, AtFlags::SYMLINK_NOFOLLOW) {
                        Ok(created) => created,
                        Err(error) => {
                            return Ok(Outcome::DirectoryCreationUncertain {
                                error: error.to_string(),
                            });
                        }
                    };
                    if created.st_uid != rustix::process::geteuid().as_raw()
                        || created.st_mode & 0o077 != 0
                    {
                        return Ok(Outcome::DirectoryCreationUncertain {
                            error: "directory is not private".into(),
                        });
                    }
                    if let Err(error) = checked_dir(&dest_parent, name, &created) {
                        return Ok(Outcome::DirectoryCreationUncertain {
                            error: error.to_string(),
                        });
                    }
                    if let Err(error) =
                        self.record("directory_created", item.path(), Some(&created))
                    {
                        return Ok(Outcome::DirectoryCreationUncertain {
                            error: error.to_string(),
                        });
                    }
                    source_dirs.insert(item.path().to_vec(), *item.stat());
                    dest_dirs.insert(item.path().to_vec(), created);
                    return Ok(Outcome::DirectoryCreated);
                }
                let source = File::from(
                    fs::openat(
                        &source_parent,
                        source_name,
                        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(|e| ("open source", e.to_string()))?,
                );
                if !source_same(
                    item.stat(),
                    &fs::fstat(&source).map_err(|e| ("open source", e.to_string()))?,
                ) {
                    return Err(("open source", "source changed since planning".into()));
                }
                let dest_path = parents
                    .iter()
                    .fold(self.root.join("destination"), |path, part| path.join(part));
                let component = if let Some(last) = parents.last() {
                    last.to_str()
                        .ok_or(("destination", "invalid component".into()))?
                } else {
                    "destination"
                };
                let grandparent = if parents.is_empty() {
                    fs::openat(&self.fixture, ".", DIR_FLAGS, Mode::empty())
                        .map_err(|e| ("destination", e.to_string()))?
                } else {
                    walk(
                        &self.destination,
                        &parents[..parents.len() - 1],
                        &dest_dirs,
                        false,
                    )
                    .map_err(|e| ("destination", e.to_string()))?
                };
                let auth = Authorization::authorize(AuthorizationRequest {
                    source_dir: &source_parent,
                    source: &source,
                    source_name,
                    dir: &dest_parent,
                    parent: &grandparent,
                    component,
                    root: &dest_path,
                    final_name: name,
                    metadata: self.metadata,
                })
                .map_err(|e| ("authorize file", format!("{e:?}")))?;
                let mut nonce = [0u8; 16];
                getrandom::getrandom(&mut nonce).map_err(|e| ("staging name", e.to_string()))?;
                let stage = format!(
                    ".portstride-{}",
                    nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
                );
                self.record("file_attempt", item.path(), None)
                    .map_err(|e| ("fixture ledger", e.to_string()))?;
                let request = Request::new(auth, &stage);
                let ledger_complete = std::cell::Cell::new(false);
                let observer_error = std::cell::RefCell::new(None);
                match copy_transaction::copy_with_hook(
                    &request,
                    self.metadata,
                    cancel,
                    &mut |phase, stage_file| {
                        let observation: io::Result<()> = (|| {
                            hook(index, phase)?;
                            if phase == "stage_created" || phase == "committed" {
                                let identity = fs::fstat(stage_file)?;
                                let mut ledger_path = parents.to_vec();
                                ledger_path.push(if phase == "stage_created" {
                                    OsString::from(&stage)
                                } else {
                                    name.to_os_string()
                                });
                                self.record(phase, &ledger_path, Some(&identity))?;
                            }
                            if phase == "before_commit" {
                                self.check_roots()?;
                                walk(&self.source, source_parents, &source_dirs, true)?;
                                let current_destination =
                                    walk(&self.destination, parents, &dest_dirs, false)?;
                                recheck_absent(&current_destination, name)
                                    .map_err(|e| io::Error::other(format!("{e:?}")))?;
                            }
                            Ok(())
                        })();
                        if phase == "committed" {
                            match &observation {
                                Ok(()) => ledger_complete.set(true),
                                Err(error) => {
                                    *observer_error.borrow_mut() = Some(error.to_string())
                                }
                            }
                        }
                        observation
                    },
                ) {
                    Ok(done) => Ok(Outcome::Committed(
                        done,
                        ledger_complete.get(),
                        observer_error.into_inner(),
                    )),
                    Err(error) if error.kind == copy_transaction::FailureKind::Cancelled => {
                        Ok(Outcome::CancelledTransaction(error))
                    }
                    Err(error) => Ok(Outcome::TransactionFailed(error)),
                }
            })();
            let outcome = match operation {
                Ok(result) => result,
                Err((phase, error)) => Outcome::Failed { phase, error },
            };
            if matches!(
                outcome,
                Outcome::Failed { .. }
                    | Outcome::DirectoryCreationUncertain { .. }
                    | Outcome::TransactionFailed(_)
                    | Outcome::CancelledTransaction(_)
                    | Outcome::Committed(_, false, _)
                    | Outcome::Cancelled
            ) {
                stopped = true;
            }
            outcomes.push(ItemResult {
                source_path: item.source_path().to_vec(),
                path: item.path().to_vec(),
                outcome,
            });
        }
        JobResult { state, outcomes }
    }
}
#[cfg(test)]
fn fixture_manifest(fixture: &OwnedFd) -> io::Result<File> {
    Ok(File::from(fs::openat(
        fixture,
        "manifest.json",
        OFlags::WRONLY | OFlags::APPEND | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?))
}
#[cfg(test)]
fn run_fixture_job(
    root: &Path,
    source: OwnedFd,
    destination: OwnedFd,
    plan: Plan,
    cancel: &AtomicBool,
) -> JobResult {
    let fixture = open_dir(root).unwrap();
    FixtureJob {
        root: root.to_path_buf(),
        manifest: fixture_manifest(&fixture).unwrap(),
        fixture,
        source,
        destination,
        plan,
        metadata: MetadataPolicy::BytesOnlyAcknowledged,
    }
    .run(cancel)
}

#[cfg(test)]
#[path = "../../test-support/fixture.rs"]
mod fixtures;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_plan::{NameBehavior, plan_files};
    const CONVENTIONAL: NameBehavior = NameBehavior::FixtureConventionalAssumption;
    use std::{ffi::OsString, io::Write, sync::atomic::AtomicBool};

    fn record_fixture_entry(root: &Path, path: &Path, kind: &str, identity: &Stat) {
        let fixture = open_dir(root).unwrap();
        let mut manifest = fixture_manifest(&fixture).unwrap();
        let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
        writeln!(manifest, "{{\"event\":\"fixture_created\",\"path\":\"{relative}\",\"type\":\"{kind}\",\"identity\":[{},{}]}}", identity.st_dev as u64, identity.st_ino).unwrap();
        manifest.sync_all().unwrap();
    }
    fn create_fixture_dir(root: &Path, path: &Path) {
        let parent = open_dir(path.parent().unwrap()).unwrap();
        let name = path.file_name().unwrap();
        fs::mkdirat(&parent, name, Mode::from_bits_truncate(0o700)).unwrap();
        let named = fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW).unwrap();
        let dir = checked_dir(&parent, name, &named).unwrap();
        record_fixture_entry(root, path, "directory", &fs::fstat(&dir).unwrap());
    }
    fn create_fixture_file(root: &Path, path: &Path, bytes: impl AsRef<[u8]>) {
        let parent = open_dir(path.parent().unwrap()).unwrap();
        let file = fs::openat(
            &parent,
            path.file_name().unwrap(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .unwrap();
        let mut file = File::from(file);
        record_fixture_entry(root, path, "file", &fs::fstat(&file).unwrap());
        file.write_all(bytes.as_ref()).unwrap();
    }
    fn mutate_fixture_file(root: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let parent = open_dir(
            path.parent()
                .ok_or_else(|| io::Error::other("missing fixture parent"))?,
        )?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::other("missing fixture name"))?;
        let named = fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
        let mut file = File::from(fs::openat(
            &parent,
            name,
            OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let held = fs::fstat(&file)?;
        if fs::FileType::from_raw_mode(named.st_mode) != fs::FileType::RegularFile
            || id(&named) != id(&held)
        {
            return Err(io::Error::other("fixture file changed"));
        }
        record_fixture_entry(root, path, "intentional-mutation", &held);
        file.set_len(0)?;
        file.write_all(bytes)
    }
    fn setup() -> (PathBuf, OwnedFd, OwnedFd) {
        use std::os::unix::fs::PermissionsExt;
        let root = fixtures::fixture();
        create_fixture_dir(&root, &root.join("source"));
        create_fixture_dir(&root, &root.join("destination"));
        std::fs::set_permissions(
            root.join("destination"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let source = open_dir(&root.join("source")).unwrap();
        let destination = open_dir(&root.join("destination")).unwrap();
        (root, source, destination)
    }
    #[test]
    fn nested_tree_and_empty_directory_are_created() {
        let (root, source, destination) = setup();
        create_fixture_dir(&root, &root.join("source/nested"));
        create_fixture_dir(&root, &root.join("source/nested/empty"));
        let bytes: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
        create_fixture_file(&root, &root.join("source/nested/file"), &bytes);
        let plan = crate::job_plan::plan_tree(&source, &destination, CONVENTIONAL).unwrap();
        let result = run_fixture_job(&root, source, destination, plan, &AtomicBool::new(false));
        assert!(result.success(), "{result:?}");
        assert!(root.join("destination/nested/empty").is_dir());
        assert_eq!(
            std::fs::read(root.join("destination/nested/file")).unwrap(),
            bytes
        );
        let digest = |path: &Path| {
            let output = std::process::Command::new("/usr/bin/shasum")
                .arg("-a")
                .arg("256")
                .arg(path)
                .output()
                .unwrap();
            assert!(output.status.success());
            output.stdout[..64].to_vec()
        };
        assert_eq!(
            digest(&root.join("source/nested/file")),
            digest(&root.join("destination/nested/file"))
        );
    }
    #[test]
    fn conflict_after_plan_stops_after_prior_commit() {
        let (root, source, destination) = setup();
        for name in ["a", "b", "c"] {
            create_fixture_file(&root, &root.join("source").join(name), name);
        }
        let names = ["a", "b", "c"].map(OsString::from);
        let plan = plan_files(&source, &destination, &names, CONVENTIONAL).unwrap();
        create_fixture_file(&root, &root.join("destination/b"), b"keep");
        let result = run_fixture_job(&root, source, destination, plan, &AtomicBool::new(false));
        assert_eq!(result.committed(), 1, "{result:?}");
        assert_eq!(
            result.summary(),
            Summary {
                committed: 1,
                failed: 1,
                not_attempted: 1,
                written: 1,
                verified: 1,
                ..Summary::default()
            }
        );
        assert!(matches!(result.outcomes[1].outcome, Outcome::Failed { .. }));
        assert!(matches!(result.outcomes[2].outcome, Outcome::NotAttempted));
        assert_eq!(std::fs::read(root.join("destination/b")).unwrap(), b"keep");
        assert!(!root.join("destination/c").exists());
        for name in ["a", "b", "c"] {
            assert_eq!(
                std::fs::read(root.join("source").join(name)).unwrap(),
                name.as_bytes()
            );
        }
    }
    #[test]
    fn pre_cancel_and_changed_source_stop_without_writes() {
        let (root, source, destination) = setup();
        create_fixture_file(&root, &root.join("source/a"), b"original");
        let names = [OsString::from("a")];
        let plan = plan_files(&source, &destination, &names, CONVENTIONAL).unwrap();
        let result = run_fixture_job(&root, source, destination, plan, &AtomicBool::new(true));
        assert!(matches!(result.outcomes[0].outcome, Outcome::Cancelled));
        assert!(!root.join("destination/a").exists());
        let source = open_dir(&root.join("source")).unwrap();
        let destination = open_dir(&root.join("destination")).unwrap();
        let plan = plan_files(&source, &destination, &names, CONVENTIONAL).unwrap();
        mutate_fixture_file(&root, &root.join("source/a"), b"changed").unwrap();
        let result = run_fixture_job(&root, source, destination, plan, &AtomicBool::new(false));
        assert!(
            matches!(result.outcomes[0].outcome, Outcome::Failed { .. }),
            "{result:?}"
        );
        assert!(!root.join("destination/a").exists());
    }
    #[test]
    fn selected_files_from_separate_directories_are_copied() {
        let (root, source, destination) = setup();
        for (dir, name, data) in [
            ("one", "alpha", b"a".as_slice()),
            ("two", "beta", b"b".as_slice()),
        ] {
            create_fixture_dir(&root, &root.join("source").join(dir));
            create_fixture_file(&root, &root.join("source").join(dir).join(name), data);
        }
        let paths = vec![
            vec![OsString::from("one"), OsString::from("alpha")],
            vec![OsString::from("two"), OsString::from("beta")],
        ];
        let plan =
            crate::job_plan::plan_selected(&source, &destination, &paths, CONVENTIONAL).unwrap();
        let result = run_fixture_job(&root, source, destination, plan, &AtomicBool::new(false));
        assert_eq!(result.committed(), 2, "{result:?}");
        assert_eq!(std::fs::read(root.join("destination/alpha")).unwrap(), b"a");
        assert_eq!(std::fs::read(root.join("destination/beta")).unwrap(), b"b");
    }
    #[test]
    fn cancellation_inside_transaction_reports_remaining_items() {
        let (root, source, destination) = setup();
        for name in ["a", "b", "c"] {
            create_fixture_file(&root, &root.join("source").join(name), vec![7u8; 4096]);
        }
        let plan = plan_files(
            &source,
            &destination,
            &["a", "b", "c"].map(OsString::from),
            CONVENTIONAL,
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let fixture = open_dir(&root).unwrap();
        let result = FixtureJob {
            root: root.clone(),
            manifest: fixture_manifest(&fixture).unwrap(),
            fixture,
            source,
            destination,
            plan,
            metadata: MetadataPolicy::BytesOnlyAcknowledged,
        }
        .run_with_hook(&cancel, |index, phase| {
            if index == 1 && phase == "before_write" {
                cancel.store(true, Ordering::Relaxed);
            }
            Ok(())
        });
        assert_eq!(result.committed(), 1, "{result:?}");
        assert!(matches!(
            result.outcomes[1].outcome,
            Outcome::CancelledTransaction(_)
        ));
        assert!(matches!(result.outcomes[2].outcome, Outcome::NotAttempted));
        assert!(!root.join("destination/b").exists());
        assert_eq!(
            std::fs::read(root.join("source/b")).unwrap(),
            vec![7u8; 4096]
        );
    }
    #[test]
    fn declined_metadata_prevents_directory_creation() {
        let (root, source, destination) = setup();
        create_fixture_dir(&root, &root.join("source/empty"));
        let plan = crate::job_plan::plan_tree(&source, &destination, CONVENTIONAL).unwrap();
        let fixture = open_dir(&root).unwrap();
        let result = FixtureJob {
            root: root.clone(),
            manifest: fixture_manifest(&fixture).unwrap(),
            fixture,
            source,
            destination,
            plan,
            metadata: MetadataPolicy::Declined,
        }
        .run(&AtomicBool::new(false));
        assert!(matches!(
            result.outcomes[0].outcome,
            Outcome::Failed {
                phase: "authorization",
                ..
            }
        ));
        assert!(!root.join("destination/empty").exists());
    }
    #[test]
    fn directory_created_but_unidentified_is_reported() {
        let (root, source, destination) = setup();
        create_fixture_dir(&root, &root.join("source/empty"));
        create_fixture_file(&root, &root.join("source/later"), b"x");
        let plan = crate::job_plan::plan_tree(&source, &destination, CONVENTIONAL).unwrap();
        let fixture = open_dir(&root).unwrap();
        let result = FixtureJob {
            root: root.clone(),
            manifest: fixture_manifest(&fixture).unwrap(),
            fixture,
            source,
            destination,
            plan,
            metadata: MetadataPolicy::BytesOnlyAcknowledged,
        }
        .run_with_hook(&AtomicBool::new(false), |_, phase| {
            if phase == "directory_created" {
                Err(io::Error::other("simulated identification failure"))
            } else {
                Ok(())
            }
        });
        assert!(
            matches!(
                result.outcomes[0].outcome,
                Outcome::DirectoryCreationUncertain { .. }
            ),
            "{result:?}"
        );
        assert!(matches!(result.outcomes[1].outcome, Outcome::NotAttempted));
        assert!(root.join("destination/empty").is_dir());
        assert!(!root.join("destination/later").exists());
    }
    #[test]
    fn hundreds_of_small_files_and_large_binary_are_verified() {
        let (root, source, destination) = setup();
        for i in 0..200 {
            create_fixture_file(
                &root,
                &root.join("source").join(format!("file{i:03}")),
                [i as u8],
            );
        }
        let large = vec![0xa5; 1024 * 1024 + 17];
        create_fixture_file(&root, &root.join("source/large"), &large);
        let plan = crate::job_plan::plan_tree(&source, &destination, CONVENTIONAL).unwrap();
        let result = run_fixture_job(&root, source, destination, plan, &AtomicBool::new(false));
        assert_eq!(result.committed(), 201, "{result:?}");
        assert_eq!(result.summary().verified, 200 + large.len() as u128);
        for i in 0..200 {
            assert_eq!(
                std::fs::read(root.join("destination").join(format!("file{i:03}"))).unwrap(),
                [i as u8]
            );
        }
        assert_eq!(
            std::fs::read(root.join("destination/large")).unwrap(),
            large
        );
    }
    #[test]
    fn symlink_is_reported_without_following_it() {
        let (root, source, destination) = setup();
        std::os::unix::fs::symlink("missing", root.join("source/link")).unwrap();
        let source_parent = open_dir(&root.join("source")).unwrap();
        let link = fs::statat(&source_parent, "link", AtFlags::SYMLINK_NOFOLLOW).unwrap();
        record_fixture_entry(
            &root,
            &root.join("source/link"),
            "symlink-no-follow-stat",
            &link,
        );
        let plan = crate::job_plan::plan_tree(&source, &destination, CONVENTIONAL).unwrap();
        let result = run_fixture_job(&root, source, destination, plan, &AtomicBool::new(false));
        assert!(matches!(
            result.outcomes[0].outcome,
            Outcome::Unsupported(crate::job_plan::UnsupportedKind::Symlink)
        ));
        assert!(!result.success());
        assert!(!root.join("destination/link").exists());
        assert_eq!(
            std::fs::read_link(root.join("source/link")).unwrap(),
            Path::new("missing")
        );
    }
    #[test]
    fn destination_replacement_before_commit_aborts() {
        let (root, source, destination) = setup();
        create_fixture_file(&root, &root.join("source/a"), b"source");
        let plan = plan_files(&source, &destination, &[OsString::from("a")], CONVENTIONAL).unwrap();
        let fixture = open_dir(&root).unwrap();
        let result = FixtureJob {
            root: root.clone(),
            manifest: fixture_manifest(&fixture).unwrap(),
            fixture,
            source,
            destination,
            plan,
            metadata: MetadataPolicy::BytesOnlyAcknowledged,
        }
        .run_with_hook(&AtomicBool::new(false), |_, phase| {
            if phase == "before_commit" {
                let old = open_dir(&root.join("destination"))?;
                std::fs::rename(root.join("destination"), root.join("old-destination"))?;
                record_fixture_entry(
                    &root,
                    &root.join("old-destination"),
                    "renamed-directory",
                    &fs::fstat(&old)?,
                );
                create_fixture_dir(&root, &root.join("destination"));
            }
            Ok(())
        });
        assert!(
            matches!(result.outcomes[0].outcome, Outcome::TransactionFailed(_)),
            "{result:?}"
        );
        assert!(!root.join("destination/a").exists());
        assert!(!root.join("old-destination/a").exists());
        assert_eq!(std::fs::read(root.join("source/a")).unwrap(), b"source");
    }
    #[test]
    fn ambiguous_mkdir_error_is_reported_as_possible_creation() {
        assert!(matches!(
            mkdir_failure(rustix::io::Errno::IO),
            Outcome::DirectoryCreationUncertain { .. }
        ));
        assert!(matches!(
            mkdir_failure(rustix::io::Errno::EXIST),
            Outcome::Failed {
                phase: "create directory",
                ..
            }
        ));
    }
    #[test]
    fn case_alias_appearing_after_plan_is_rejected() {
        let (root, source, destination) = setup();
        create_fixture_file(&root, &root.join("source/Report"), b"source");
        let plan = plan_files(
            &source,
            &destination,
            &[OsString::from("Report")],
            CONVENTIONAL,
        )
        .unwrap();
        create_fixture_file(&root, &root.join("destination/report"), b"existing");
        let result = run_fixture_job(&root, source, destination, plan, &AtomicBool::new(false));
        assert!(
            matches!(
                result.outcomes[0].outcome,
                Outcome::Failed {
                    phase: "destination mapping",
                    ..
                }
            ),
            "{result:?}"
        );
        assert_eq!(
            std::fs::read(root.join("destination/report")).unwrap(),
            b"existing"
        );
        assert_eq!(
            std::fs::read(root.join("source/Report")).unwrap(),
            b"source"
        );
    }
    #[test]
    fn case_alias_appearing_before_commit_is_rejected() {
        let (root, source, destination) = setup();
        create_fixture_file(&root, &root.join("source/Report"), b"source");
        let plan = plan_files(
            &source,
            &destination,
            &[OsString::from("Report")],
            CONVENTIONAL,
        )
        .unwrap();
        let fixture = open_dir(&root).unwrap();
        let result = FixtureJob {
            root: root.clone(),
            manifest: fixture_manifest(&fixture).unwrap(),
            fixture,
            source,
            destination,
            plan,
            metadata: MetadataPolicy::BytesOnlyAcknowledged,
        }
        .run_with_hook(&AtomicBool::new(false), |_, phase| {
            if phase == "before_commit" {
                create_fixture_file(&root, &root.join("destination/report"), b"existing");
            }
            Ok(())
        });
        assert!(
            matches!(result.outcomes[0].outcome, Outcome::TransactionFailed(_)),
            "{result:?}"
        );
        assert_eq!(
            std::fs::read(root.join("destination/report")).unwrap(),
            b"existing"
        );
    }
    #[test]
    fn ledger_failure_after_commit_preserves_commit_and_stops() {
        let (root, source, destination) = setup();
        for name in ["a", "b"] {
            create_fixture_file(&root, &root.join("source").join(name), name);
        }
        let plan = plan_files(
            &source,
            &destination,
            &["a", "b"].map(OsString::from),
            CONVENTIONAL,
        )
        .unwrap();
        let fixture = open_dir(&root).unwrap();
        let result = FixtureJob {
            root: root.clone(),
            manifest: fixture_manifest(&fixture).unwrap(),
            fixture,
            source,
            destination,
            plan,
            metadata: MetadataPolicy::BytesOnlyAcknowledged,
        }
        .run_with_hook(&AtomicBool::new(false), |_, phase| {
            if phase == "committed" {
                Err(io::Error::other("simulated ledger failure"))
            } else {
                Ok(())
            }
        });
        assert!(
            matches!(
                result.outcomes[0].outcome,
                Outcome::Committed(_, false, Some(_))
            ),
            "{result:?}"
        );
        assert!(matches!(result.outcomes[1].outcome, Outcome::NotAttempted));
        assert_eq!(result.summary().ledger_incomplete, 1);
        assert!(!result.success());
        let single = JobResult {
            state: JobState::Ready,
            outcomes: vec![result.outcomes.into_iter().next().unwrap()],
        };
        assert!(!single.success());
        assert_eq!(std::fs::read(root.join("destination/a")).unwrap(), b"a");
    }
    #[test]
    fn empty_jobs_validate_acknowledgement_cancellation_and_roots() {
        for case in ["declined", "cancelled", "replaced"] {
            let (root, source, destination) = setup();
            let plan = crate::job_plan::plan_tree(&source, &destination, CONVENTIONAL).unwrap();
            let fixture = open_dir(&root).unwrap();
            if case == "replaced" {
                let old = open_dir(&root.join("destination")).unwrap();
                std::fs::rename(root.join("destination"), root.join("old-destination")).unwrap();
                record_fixture_entry(
                    &root,
                    &root.join("old-destination"),
                    "renamed-directory",
                    &fs::fstat(&old).unwrap(),
                );
                create_fixture_dir(&root, &root.join("destination"));
            }
            let job = FixtureJob {
                root,
                manifest: fixture_manifest(&fixture).unwrap(),
                fixture,
                source,
                destination,
                plan,
                metadata: if case == "declined" {
                    MetadataPolicy::Declined
                } else {
                    MetadataPolicy::BytesOnlyAcknowledged
                },
            };
            let result = job.run(&AtomicBool::new(case == "cancelled"));
            assert!(
                !result.success(),
                "empty {case} job must not succeed: {result:?}"
            );
            assert!(result.outcomes.is_empty());
            assert!(matches!(
                result.state,
                JobState::Rejected { .. } | JobState::Cancelled
            ));
        }
    }
    #[test]
    fn planner_rejects_destination_inside_source_before_mutation() {
        let (root, source, _) = setup();
        create_fixture_dir(&root, &root.join("source/inside"));
        let nested_destination = open_dir(&root.join("source/inside")).unwrap();
        let result = crate::job_plan::plan_tree(&source, &nested_destination, CONVENTIONAL);
        assert!(
            matches!(
                result,
                Err(crate::job_plan::PlanError::DestinationInsideSource)
            ),
            "{result:?}"
        );
        assert_eq!(
            std::fs::read_dir(root.join("source/inside"))
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn hardlinks_and_fifo_are_individually_reported() {
        let (root, source, destination) = setup();
        create_fixture_file(&root, &root.join("source/original"), b"sentinel");
        std::fs::hard_link(root.join("source/original"), root.join("source/hardlink")).unwrap();
        let hardlink_parent = open_dir(&root.join("source")).unwrap();
        let hardlink = File::from(
            fs::openat(
                &hardlink_parent,
                "hardlink",
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .unwrap(),
        );
        record_fixture_entry(
            &root,
            &root.join("source/hardlink"),
            "hardlink",
            &fs::fstat(&hardlink).unwrap(),
        );
        let fifo = std::process::Command::new("/usr/bin/mkfifo")
            .arg(root.join("source/fifo"))
            .status()
            .unwrap();
        assert!(fifo.success());
        let source_parent = open_dir(&root.join("source")).unwrap();
        let fifo_fd = fs::openat(
            &source_parent,
            "fifo",
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .unwrap();
        record_fixture_entry(
            &root,
            &root.join("source/fifo"),
            "fifo",
            &fs::fstat(&fifo_fd).unwrap(),
        );
        let plan = crate::job_plan::plan_tree(&source, &destination, CONVENTIONAL).unwrap();
        let result = run_fixture_job(&root, source, destination, plan, &AtomicBool::new(false));
        let unsupported = |name: &str, expected| {
            result.outcomes.iter().any(|item| {
                item.source_path == [OsString::from(name)]
                    && matches!(item.outcome, Outcome::Unsupported(reason) if reason == expected)
            })
        };
        assert!(unsupported(
            "hardlink",
            crate::job_plan::UnsupportedKind::HardLink
        ));
        assert!(unsupported(
            "original",
            crate::job_plan::UnsupportedKind::HardLink
        ));
        assert!(unsupported(
            "fifo",
            crate::job_plan::UnsupportedKind::Special
        ));
        assert_eq!(result.summary().unsupported, 3);
        assert!(!result.success());
        assert_eq!(
            std::fs::read(root.join("source/original")).unwrap(),
            b"sentinel"
        );
        assert_eq!(
            std::fs::read_dir(root.join("destination")).unwrap().count(),
            0
        );
    }
    #[test]
    fn source_mutation_after_first_commit_stops_remaining_files() {
        let (root, source, destination) = setup();
        for name in ["a", "b", "c"] {
            create_fixture_file(&root, &root.join("source").join(name), name);
        }
        let plan = plan_files(
            &source,
            &destination,
            &["a", "b", "c"].map(OsString::from),
            CONVENTIONAL,
        )
        .unwrap();
        let fixture = open_dir(&root).unwrap();
        let result = FixtureJob {
            root: root.clone(),
            manifest: fixture_manifest(&fixture).unwrap(),
            fixture,
            source,
            destination,
            plan,
            metadata: MetadataPolicy::BytesOnlyAcknowledged,
        }
        .run_with_hook(&AtomicBool::new(false), |index, phase| {
            if index == 1 && phase == "before_item" {
                mutate_fixture_file(&root, &root.join("source/b"), b"changed by test")?;
            }
            Ok(())
        });
        assert!(
            matches!(result.outcomes[0].outcome, Outcome::Committed(..)),
            "{result:?}"
        );
        assert!(
            matches!(
                result.outcomes[1].outcome,
                Outcome::Failed {
                    phase: "source",
                    ..
                }
            ),
            "{result:?}"
        );
        assert!(matches!(result.outcomes[2].outcome, Outcome::NotAttempted));
        assert_eq!(std::fs::read(root.join("destination/a")).unwrap(), b"a");
        assert_eq!(std::fs::read(root.join("source/a")).unwrap(), b"a");
        assert_eq!(
            std::fs::read(root.join("source/b")).unwrap(),
            b"changed by test"
        );
        assert!(!root.join("destination/b").exists());
        assert!(!root.join("destination/c").exists());
    }
    #[test]
    fn sequential_files_report_commits_and_preserve_sources() {
        let root = fixtures::fixture();
        create_fixture_dir(&root, &root.join("source"));
        create_fixture_dir(&root, &root.join("destination"));
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.join("destination"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        create_fixture_file(&root, &root.join("source/a"), b"first\0");
        create_fixture_file(&root, &root.join("source/b"), b"");
        let source = open_dir(&root.join("source")).unwrap();
        let destination = open_dir(&root.join("destination")).unwrap();
        let plan = plan_files(
            &source,
            &destination,
            &[OsString::from("a"), OsString::from("b")],
            CONVENTIONAL,
        )
        .unwrap();
        let result = run_fixture_job(&root, source, destination, plan, &AtomicBool::new(false));
        assert_eq!(result.committed(), 2, "{result:?}");
        assert!(result.success());
        for name in ["a", "b"] {
            assert_eq!(
                std::fs::read(root.join("source").join(name)).unwrap(),
                std::fs::read(root.join("destination").join(name)).unwrap()
            );
        }
        let mut manifest = std::fs::OpenOptions::new()
            .append(true)
            .open(root.join("manifest.json"))
            .unwrap();
        writeln!(manifest, "{{\"cleanup\":\"manual review required\"}}").unwrap();
    }
}
