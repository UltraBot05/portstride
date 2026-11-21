//! Private, disposable-only execution experiment. This is not a public copy API.
use crate::copy_transaction::{
    Authorization, AuthorizationRequest, BUFFER, CommitState, Completed, CopyFailure, Durability,
    FailureKind, MetadataPolicy, MetadataProfile, Request, StageDisposition, check_limits,
    read_chunk, write_chunk,
};
use rustix::fs::{self, AtFlags, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::{fd::OwnedFd, unix::fs::MetadataExt},
    path::PathBuf,
};

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

struct Fixture {
    root: PathBuf,
    dir: OwnedFd,
    parent: OwnedFd,
    component: String,
    stage: String,
    source: File,
    manifest: File,
    committed: std::cell::RefCell<Option<File>>,
    cleanup_eligible: std::cell::Cell<bool>,
}
impl Fixture {
    fn new(bytes: &[u8]) -> io::Result<Self> {
        let mut nonce = [0u8; 16];
        getrandom::getrandom(&mut nonce).map_err(|e| io::Error::other(e.to_string()))?;
        let name = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let parent = std::env::temp_dir().canonicalize()?;
        let mut parent_fd = fs::open("/", DIR_FLAGS, Mode::empty())?;
        for component in parent.components() {
            match component {
                std::path::Component::RootDir => {}
                std::path::Component::Normal(name) => {
                    parent_fd = fs::openat(&parent_fd, name, DIR_FLAGS, Mode::empty())?;
                }
                _ => return Err(io::Error::other("unsupported fixture parent")),
            }
        }
        let parent_stat = fs::fstat(&parent_fd)?;
        if parent_stat.st_uid != rustix::process::geteuid().as_raw()
            || parent_stat.st_mode & 0o022 != 0
        {
            return Err(io::Error::other(
                "fixture parent is not private to the user",
            ));
        }
        let component = format!("portstride-copy-{name}");
        fs::mkdirat(
            &parent_fd,
            component.as_str(),
            Mode::from_bits_truncate(0o700),
        )?;
        let expected = fs::statat(&parent_fd, component.as_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        let dir = fs::openat(&parent_fd, component.as_str(), DIR_FLAGS, Mode::empty())?;
        let actual = fs::fstat(&dir)?;
        if expected.st_dev != actual.st_dev
            || expected.st_ino != actual.st_ino
            || actual.st_uid != rustix::process::geteuid().as_raw()
            || actual.st_mode & 0o077 != 0
        {
            return Err(io::Error::other(
                "fixture directory identity or permissions changed",
            ));
        }
        let root = parent.join(&component);
        let mut manifest = File::from(fs::openat(
            &dir,
            "manifest.json",
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )?);
        let manifest_identity = manifest.metadata()?;
        let mut source = File::from(fs::openat(
            &dir,
            "source",
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )?);
        source.write_all(bytes)?;
        let source_identity = source.metadata()?;
        serde_json::to_writer(
            &mut manifest,
            &serde_json::json!({
                "version": 1, "id": name,
                "parent": [parent_stat.st_dev as u64, parent_stat.st_ino],
                "root": [actual.st_dev as u64, actual.st_ino],
                "manifest": [manifest_identity.dev(), manifest_identity.ino()],
                "source": [source_identity.dev(), source_identity.ino()]
            }),
        )?;
        manifest.sync_all()?;
        let mut stage_nonce = [0u8; 16];
        getrandom::getrandom(&mut stage_nonce).map_err(|e| io::Error::other(e.to_string()))?;
        let stage = format!(
            ".portstride-{}",
            stage_nonce
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        Ok(Self {
            root,
            dir,
            parent: parent_fd,
            component,
            stage,
            source,
            manifest,
            committed: std::cell::RefCell::new(None),
            cleanup_eligible: std::cell::Cell::new(true),
        })
    }

    fn authorization(&self) -> io::Result<Authorization<'_>> {
        self.cleanup_eligible.set(false);
        Authorization::authorize(self.authorization_request())
            .map_err(|error| io::Error::other(format!("{error:?}")))
    }

    fn authorization_request(&self) -> AuthorizationRequest<'_> {
        AuthorizationRequest {
            source_dir: &self.dir,
            source: &self.source,
            source_name: std::ffi::OsStr::new("source"),
            dir: &self.dir,
            parent: &self.parent,
            component: &self.component,
            root: &self.root,
            final_name: std::ffi::OsStr::new("destination"),
            metadata: MetadataPolicy::BytesOnlyAcknowledged,
        }
    }

    // Only an untouched fixture or a recorded successful commit can be cleaned.
    fn cleanup(&self) -> io::Result<()> {
        if !self.cleanup_eligible.replace(false) {
            return Err(io::Error::other("fixture ledger is incomplete"));
        }
        let held = fs::fstat(&self.dir)?;
        let named = fs::statat(
            &self.parent,
            self.component.as_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        )?;
        if fs::FileType::from_raw_mode(named.st_mode) != fs::FileType::Directory
            || held.st_dev != named.st_dev
            || held.st_ino != named.st_ino
        {
            return Err(io::Error::other("fixture root changed"));
        }
        let mut names = Vec::new();
        for entry in fs::Dir::read_from(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name().to_bytes();
            if name != b"." && name != b".." {
                names.push(name.to_vec());
            }
        }
        names.sort();
        let committed = self.committed.borrow();
        let mut files = vec![("source", &self.source), ("manifest.json", &self.manifest)];
        if let Some(file) = committed.as_ref() {
            files.insert(0, ("destination", file));
        }
        let mut expected_names: Vec<Vec<u8>> = files
            .iter()
            .map(|(name, _)| name.as_bytes().to_vec())
            .collect();
        expected_names.sort();
        if names != expected_names {
            return Err(io::Error::other("unexpected fixture entry"));
        }
        for (name, file) in &files {
            let expected = file.metadata()?;
            let current = fs::statat(&self.dir, *name, AtFlags::SYMLINK_NOFOLLOW)?;
            if fs::FileType::from_raw_mode(current.st_mode) != fs::FileType::RegularFile
                || current.st_dev as u64 != expected.dev()
                || current.st_ino != expected.ino()
            {
                return Err(io::Error::other("fixture entry changed"));
            }
        }
        for (name, file) in &files {
            let expected = file.metadata()?;
            let current = fs::statat(&self.dir, *name, AtFlags::SYMLINK_NOFOLLOW)?;
            if current.st_dev as u64 != expected.dev()
                || current.st_ino != expected.ino()
                || fs::FileType::from_raw_mode(current.st_mode) != fs::FileType::RegularFile
            {
                return Err(io::Error::other("fixture entry changed during cleanup"));
            }
            fs::unlinkat(&self.dir, *name, AtFlags::empty())?;
        }
        let named = fs::statat(
            &self.parent,
            self.component.as_str(),
            AtFlags::SYMLINK_NOFOLLOW,
        )?;
        if fs::FileType::from_raw_mode(named.st_mode) != fs::FileType::Directory
            || held.st_dev != named.st_dev
            || held.st_ino != named.st_ino
        {
            return Err(io::Error::other("fixture root changed during cleanup"));
        }
        fs::unlinkat(&self.parent, self.component.as_str(), AtFlags::REMOVEDIR)?;
        Ok(())
    }
}

#[test]
fn authorization_rejects_a_mismatched_directory_binding() {
    let fixture = Fixture::new(b"source").unwrap();
    let mut request = fixture.authorization_request();
    request.component = "..";
    assert!(Authorization::authorize(request).is_err());
    fixture.cleanup().unwrap();
}

#[test]
fn provider_rejects_traversal_and_preserves_source() {
    let fixture = Fixture::new(b"sentinel").unwrap();
    let mut request = fixture.authorization_request();
    request.final_name = std::ffi::OsStr::new("../escape");
    let result = Authorization::authorize(request);
    assert!(matches!(
        result,
        Err(super::AuthorizationError::InvalidName)
    ));
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"sentinel"
    );
    fixture.cleanup().unwrap();
}

#[test]
fn provider_requires_known_writable_mount_and_folder() {
    assert!(super::destination_access_allowed(Some(true), Some(false)));
    for (writable, read_only) in [
        (None, Some(false)),
        (Some(false), Some(false)),
        (Some(true), None),
        (Some(true), Some(true)),
    ] {
        assert!(!super::destination_access_allowed(writable, read_only));
    }
}

#[test]
fn provider_rejects_directory_without_write_access() {
    let fixture = Fixture::new(b"source").unwrap();
    fs::fchmod(&fixture.dir, Mode::from_bits_truncate(0o500)).unwrap();
    let result = Authorization::authorize(fixture.authorization_request());
    fs::fchmod(&fixture.dir, Mode::from_bits_truncate(0o700)).unwrap();
    assert!(matches!(
        result,
        Err(super::AuthorizationError::DestinationAccessUnknownOrDenied)
    ));
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"source"
    );
    fixture.cleanup().unwrap();
}

#[test]
fn provider_rejects_unknown_access_without_modifying_source() {
    let fixture = Fixture::new(b"source").unwrap();
    let result = Authorization::authorize_with_probe(fixture.authorization_request(), |path| {
        let mut capabilities = crate::probe(path);
        capabilities.writable = None;
        capabilities
    });
    assert!(matches!(
        result,
        Err(super::AuthorizationError::DestinationAccessUnknownOrDenied)
    ));
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"source"
    );
    assert!(!fixture.root.join(&fixture.stage).exists());
    fixture.cleanup().unwrap();
}

#[test]
fn target_appearing_at_commit_is_not_replaced() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| {
            if phase == "commit_call" {
                std::fs::write(fixture.root.join("destination"), b"sentinel")?;
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(failure.commit, CommitState::NotCommitted);
    assert_eq!(failure.staging, StageDisposition::Removed);
    assert_eq!(
        std::fs::read(fixture.root.join("destination")).unwrap(),
        b"sentinel"
    );
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"source"
    );
}

#[test]
fn provider_rejects_stage_spelling_and_case_alias_before_staging() {
    let fixture = Fixture::new(b"source").unwrap();
    let alias = fixture.stage.to_ascii_uppercase();
    for name in [fixture.stage.as_str(), alias.as_str()] {
        let mut request = fixture.authorization_request();
        request.final_name = std::ffi::OsStr::new(name);
        assert!(matches!(
            Authorization::authorize(request),
            Err(super::AuthorizationError::InvalidName)
        ));
        assert!(!fixture.root.join(&fixture.stage).exists());
    }
    fixture.cleanup().unwrap();
}

#[test]
fn provider_checks_metadata_source_and_destination_identity() {
    let fixture = Fixture::new(b"source").unwrap();
    let other = Fixture::new(b"other").unwrap();
    let mut request = fixture.authorization_request();
    request.metadata = MetadataPolicy::Declined;
    assert!(matches!(
        Authorization::authorize(request),
        Err(super::AuthorizationError::MetadataNotAcknowledged)
    ));
    let mut request = fixture.authorization_request();
    request.source = &other.source;
    assert!(matches!(
        Authorization::authorize(request),
        Err(super::AuthorizationError::SourceChanged)
    ));
    let mut request = fixture.authorization_request();
    request.dir = &other.dir;
    assert!(matches!(
        Authorization::authorize(request),
        Err(super::AuthorizationError::DestinationChanged)
    ));
    fixture.cleanup().unwrap();
    other.cleanup().unwrap();
}

#[test]
fn provider_rejects_existing_final_entry_and_preserves_it() {
    let fixture = Fixture::new(b"source").unwrap();
    std::fs::write(fixture.root.join("destination"), b"sentinel").unwrap();
    assert!(matches!(
        Authorization::authorize(fixture.authorization_request()),
        Err(super::AuthorizationError::DestinationConflict)
    ));
    assert_eq!(
        std::fs::read(fixture.root.join("destination")).unwrap(),
        b"sentinel"
    );
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"source"
    );
}

#[test]
fn provider_copies_between_separate_controlled_directories() {
    let source = Fixture::new(b"separate source").unwrap();
    let destination = Fixture::new(b"unrelated destination source").unwrap();
    let mut request = destination.authorization_request();
    request.source_dir = &source.dir;
    request.source = &source.source;
    let authorization = Authorization::authorize(request).unwrap();
    let request = Request::new(authorization, &destination.stage);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    crate::copy_transaction::copy(request, MetadataPolicy::BytesOnlyAcknowledged, &cancel).unwrap();
    assert_eq!(
        std::fs::read(destination.root.join("destination")).unwrap(),
        b"separate source"
    );
    assert_eq!(
        std::fs::read(source.root.join("source")).unwrap(),
        b"separate source"
    );
    assert_eq!(
        std::fs::read(destination.root.join("source")).unwrap(),
        b"unrelated destination source"
    );
}

#[test]
fn restricted_executor_copies_through_its_normal_entry_point() {
    let fixture = Fixture::new(b"executor").unwrap();
    let request = Request::new(fixture.authorization().unwrap(), &fixture.stage);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let result =
        crate::copy_transaction::copy(request, MetadataPolicy::BytesOnlyAcknowledged, &cancel)
            .unwrap();
    assert_eq!(result.progress.verified, 8);
    assert_eq!(
        std::fs::read(fixture.root.join("destination")).unwrap(),
        b"executor"
    );
}

#[test]
fn authorization_rejects_source_replaced_before_execution() {
    let fixture = Fixture::new(b"original").unwrap();
    let authorization = fixture.authorization().unwrap();
    std::fs::rename(fixture.root.join("source"), fixture.root.join("original")).unwrap();
    std::fs::write(fixture.root.join("source"), b"replacement").unwrap();
    let request = Request::new(authorization, &fixture.stage);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure =
        crate::copy_transaction::copy(request, MetadataPolicy::BytesOnlyAcknowledged, &cancel)
            .unwrap_err();
    assert_eq!(failure.staging, StageDisposition::NotCreated);
    assert_eq!(
        std::fs::read(fixture.root.join("original")).unwrap(),
        b"original"
    );
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"replacement"
    );
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn simulated_ambiguous_commit_after_real_rename_preserves_destination() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| {
            if phase == "simulate_unknown_after_rename" {
                Err(io::Error::other("simulated ambiguous result"))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(failure.commit, CommitState::Unknown);
    assert_eq!(failure.staging, StageDisposition::UnknownAfterCommit);
    assert_eq!(
        std::fs::read(fixture.root.join("destination")).unwrap(),
        b"source"
    );
}

#[test]
fn simulated_ambiguous_commit_before_rename_preserves_stage() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| {
            if phase == "simulate_unknown_before_rename" {
                Err(io::Error::other("simulated ambiguous result"))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(failure.commit, CommitState::Unknown);
    assert_eq!(failure.staging, StageDisposition::UnknownAfterCommit);
    assert_eq!(
        std::fs::read(fixture.root.join(&fixture.stage)).unwrap(),
        b"source"
    );
    assert!(!fixture.root.join("destination").exists());
}

fn copy_fixture(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let fixture = Fixture::new(bytes)?;
    let outcome = copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged)
        .map_err(|failure| io::Error::other(format!("{failure:?}")))?;
    assert_eq!(outcome.progress.written, bytes.len() as u64);
    assert_eq!(outcome.progress.verified, outcome.progress.written);
    assert!(outcome.progress.flushed);
    assert_eq!(outcome.metadata, MetadataProfile::BytesOnly);
    assert_eq!(
        outcome.durability,
        Durability::StagingSyncSucceededNoPowerLossGuarantee
    );
    assert_eq!(outcome.digest.as_slice(), Sha256::digest(bytes).as_slice());
    assert_eq!(std::fs::read(fixture.root.join("source"))?, bytes);
    std::fs::read(fixture.root.join("destination"))
}

fn copy(fixture: &Fixture, policy: MetadataPolicy) -> Result<Completed, CopyFailure> {
    copy_cancellable(fixture, policy, &std::sync::atomic::AtomicBool::new(false))
}

fn copy_cancellable(
    fixture: &Fixture,
    policy: MetadataPolicy,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Completed, CopyFailure> {
    copy_with_hook(fixture, policy, cancel, &mut |_, _| Ok(()))
}

fn copy_with_hook(
    fixture: &Fixture,
    policy: MetadataPolicy,
    cancel: &std::sync::atomic::AtomicBool,
    hook: &mut impl FnMut(&str, &File) -> io::Result<()>,
) -> Result<Completed, CopyFailure> {
    fixture.cleanup_eligible.set(false);
    let request = Request {
        authorization: fixture.authorization()?,
        source_name: std::ffi::OsStr::new("source"),
        final_name: std::ffi::OsStr::new("destination"),
        stage: &fixture.stage,
        buffer_size: BUFFER,
    };
    let mut staged: Option<File> = None;
    crate::copy_transaction::copy_with_hook(
        &request,
        policy,
        cancel,
        &mut |phase, file| match phase {
            "stage_created" => {
                let identity = file.metadata()?;
                let held = file.try_clone()?;
                writeln!(
                    &fixture.manifest,
                    "\n{{\"stage\":\"{}\",\"device\":{},\"inode\":{}}}",
                    fixture.stage,
                    identity.dev(),
                    identity.ino()
                )?;
                fixture.manifest.sync_all()?;
                staged = Some(held);
                Ok(())
            }
            "committed" => {
                let held = staged
                    .take()
                    .ok_or_else(|| io::Error::other("missing stage record"))?;
                writeln!(&fixture.manifest, "{{\"committed\":\"destination\"}}")?;
                fixture.manifest.sync_all()?;
                *fixture.committed.borrow_mut() = Some(held);
                fixture.cleanup_eligible.set(true);
                Ok(())
            }
            _ => hook(phase, file),
        },
    )
}

#[test]
fn copies_fixture_bytes_without_changing_source() {
    let source = b"first\0real\xffcopy";
    assert_eq!(copy_fixture(source).unwrap(), source);
}

#[test]
fn replaced_fixture_root_is_rejected_before_source_or_stage_access() {
    let fixture = Fixture::new(b"source").unwrap();
    let moved = fixture.root.with_extension("moved-before-copy");
    std::fs::rename(&fixture.root, &moved).unwrap();
    std::fs::create_dir(&fixture.root).unwrap();
    std::fs::write(fixture.root.join("sentinel"), b"untouched").unwrap();
    let failure = copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).unwrap_err();
    assert_eq!(failure.staging, StageDisposition::NotCreated);
    assert_eq!(
        std::fs::read(fixture.root.join("sentinel")).unwrap(),
        b"untouched"
    );
    assert!(!moved.join(&fixture.stage).exists());
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn copies_empty_and_larger_than_buffer() {
    assert_eq!(copy_fixture(b"").unwrap(), b"");
    let bytes: Vec<u8> = (0..BUFFER + 23).map(|i| (i % 251) as u8).collect();
    assert_eq!(copy_fixture(&bytes).unwrap(), bytes);
}

#[test]
fn destination_conflict_does_not_overwrite_and_stage_is_removed() {
    let fixture = Fixture::new(b"source").unwrap();
    std::fs::write(fixture.root.join("destination"), b"existing").unwrap();
    assert!(copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).is_err());
    assert_eq!(
        std::fs::read(fixture.root.join("destination")).unwrap(),
        b"existing"
    );
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"source"
    );
    assert!(!fixture.root.join(&fixture.stage).exists());
}

#[test]
fn source_symlink_and_hardlink_are_rejected() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new(b"source").unwrap();
    std::fs::rename(fixture.root.join("source"), fixture.root.join("real")).unwrap();
    symlink("real", fixture.root.join("source")).unwrap();
    assert!(copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).is_err());
    std::fs::remove_file(fixture.root.join("source")).unwrap();
    std::fs::hard_link(fixture.root.join("real"), fixture.root.join("source")).unwrap();
    assert!(copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).is_err());
    assert!(!fixture.root.join(&fixture.stage).exists());
}

#[test]
fn final_symlink_conflict_preserves_target() {
    let fixture = Fixture::new(b"source").unwrap();
    let outside = Fixture::new(b"sentinel").unwrap();
    std::os::unix::fs::symlink(
        outside.root.join("source"),
        fixture.root.join("destination"),
    )
    .unwrap();
    let failure = copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).unwrap_err();
    assert_eq!(failure.commit, CommitState::NotCommitted);
    assert_eq!(
        std::fs::read(outside.root.join("source")).unwrap(),
        b"sentinel"
    );
    assert!(
        std::fs::symlink_metadata(fixture.root.join("destination"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn intermediate_symlink_output_is_rejected_before_staging() {
    let fixture = Fixture::new(b"source").unwrap();
    let outside = Fixture::new(b"sentinel").unwrap();
    std::os::unix::fs::symlink(&outside.root, fixture.root.join("link")).unwrap();
    let request = Request {
        authorization: fixture.authorization().unwrap(),
        source_name: std::ffi::OsStr::new("source"),
        final_name: std::ffi::OsStr::new("link/escape"),
        stage: &fixture.stage,
        buffer_size: BUFFER,
    };
    let failure = crate::copy_transaction::copy_with_hook(
        &request,
        MetadataPolicy::BytesOnlyAcknowledged,
        &std::sync::atomic::AtomicBool::new(false),
        &mut |_, _| Ok(()),
    )
    .unwrap_err();
    assert_eq!(failure.staging, StageDisposition::NotCreated);
    assert!(!outside.root.join("escape").exists());
    assert_eq!(
        std::fs::read(outside.root.join("source")).unwrap(),
        b"sentinel"
    );
}

#[test]
fn source_name_substitution_is_rejected_before_commit() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| {
            if phase == "before_commit" {
                std::fs::rename(fixture.root.join("source"), fixture.root.join("original"))?;
                std::fs::write(fixture.root.join("source"), b"replacement")?;
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(failure.commit, CommitState::NotCommitted);
    assert_eq!(
        std::fs::read(fixture.root.join("original")).unwrap(),
        b"source"
    );
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"replacement"
    );
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn cancellation_before_staging_leaves_no_destination() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    assert!(copy_cancellable(&fixture, MetadataPolicy::BytesOnlyAcknowledged, &cancelled).is_err());
    assert!(!fixture.root.join(&fixture.stage).exists());
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn staging_collision_preserves_existing_file() {
    let fixture = Fixture::new(b"source").unwrap();
    std::fs::write(fixture.root.join(&fixture.stage), b"unrelated").unwrap();
    assert!(copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).is_err());
    assert_eq!(
        std::fs::read(fixture.root.join(&fixture.stage)).unwrap(),
        b"unrelated"
    );
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn directory_source_is_rejected_without_staging() {
    let fixture = Fixture::new(b"source").unwrap();
    std::fs::remove_file(fixture.root.join("source")).unwrap();
    std::fs::create_dir(fixture.root.join("source")).unwrap();
    assert!(copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).is_err());
    assert!(!fixture.root.join(&fixture.stage).exists());
}

#[test]
fn corrupted_staging_is_rejected() {
    use std::os::unix::fs::FileExt;
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let result = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, stage| {
            if phase == "before_verify" {
                stage.write_at(b"X", 0)?;
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(!fixture.root.join("destination").exists());
    assert!(!fixture.root.join(&fixture.stage).exists());
}

#[test]
fn cancellation_before_commit_removes_stage() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| {
            if phase == "before_commit" {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(failure.cause.to_string(), "cancelled");
    assert_eq!(failure.kind, FailureKind::Cancelled);
    assert_eq!(failure.staging, StageDisposition::Removed);
    assert_eq!(failure.commit, CommitState::NotCommitted);
    assert!(!fixture.root.join("destination").exists());
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"source"
    );
}

#[test]
fn modified_source_is_detected_before_commit() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| {
            if phase == "before_verify" {
                std::fs::write(fixture.root.join("source"), b"change")?;
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(failure.staging, StageDisposition::Removed);
    assert_eq!(failure.commit, CommitState::NotCommitted);
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn replaced_destination_directory_is_detected() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let moved = fixture.root.with_extension("moved");
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| {
            if phase == "before_commit" {
                std::fs::rename(&fixture.root, &moved)?;
                std::fs::create_dir(&fixture.root)?;
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(failure.commit, CommitState::NotCommitted);
    assert!(!moved.join("destination").exists());
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn capacity_and_limit_reject_only_known_excess() {
    assert!(check_limits(101, Some(100), None).is_err());
    assert!(check_limits(101, None, Some(100)).is_err());
    assert!(check_limits(100, Some(100), Some(100)).is_ok());
    assert!(check_limits(u64::MAX, None, None).is_ok());
}

#[test]
fn injected_phase_failures_report_cleanup_and_preserve_source() {
    for failing_phase in [
        "before_write",
        "before_sync",
        "before_verify",
        "before_commit",
    ] {
        let fixture = Fixture::new(b"source").unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let failure = copy_with_hook(
            &fixture,
            MetadataPolicy::BytesOnlyAcknowledged,
            &cancel,
            &mut |phase, _| {
                if phase == failing_phase {
                    return Err(io::Error::other("injected phase failure"));
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(failure.kind, FailureKind::Error);
        assert_eq!(failure.staging, StageDisposition::Removed);
        assert_eq!(failure.commit, CommitState::NotCommitted);
        assert!(!fixture.root.join("destination").exists());
        assert_eq!(
            std::fs::read(fixture.root.join("source")).unwrap(),
            b"source"
        );
    }
}

#[test]
fn swapped_stage_is_not_removed() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| {
            if phase == "before_commit" {
                std::fs::rename(
                    fixture.root.join(&fixture.stage),
                    fixture.root.join("old-stage"),
                )?;
                std::fs::write(fixture.root.join(&fixture.stage), b"unrelated")?;
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(failure.staging, StageDisposition::LeftUnidentified);
    assert_eq!(
        std::fs::read(fixture.root.join(&fixture.stage)).unwrap(),
        b"unrelated"
    );
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn cancellation_interrupts_short_write_loop() {
    struct Partial<'a>(&'a std::sync::atomic::AtomicBool, Vec<u8>);
    impl Write for Partial<'_> {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.1.push(data[0]);
            self.0.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(1)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut writer = Partial(&cancel, Vec::new());
    assert!(write_chunk(&mut writer, b"abc", &cancel, &mut 0).is_err());
    assert_eq!(writer.1, b"a");
}

#[test]
fn read_retries_interruption_and_returns_short_read() {
    struct Reader(bool);
    impl Read for Reader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.0 {
                self.0 = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            buf[0] = 42;
            Ok(1)
        }
    }
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut buf = [0u8; 4];
    assert_eq!(
        read_chunk(&mut Reader(false), &mut buf, &cancel).unwrap(),
        1
    );
    assert_eq!(buf[0], 42);
}

#[test]
fn writes_retry_interruption_and_short_writes() {
    struct Writer {
        first: bool,
        bytes: Vec<u8>,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.first {
                self.first = false;
                return Err(io::ErrorKind::Interrupted.into());
            }
            self.bytes.push(bytes[0]);
            Ok(1)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut writer = Writer {
        first: true,
        bytes: Vec::new(),
    };
    write_chunk(&mut writer, b"abc", &cancel, &mut 0).unwrap();
    assert_eq!(writer.bytes, b"abc");
}

#[test]
fn write_zero_and_io_errors_are_not_retried() {
    struct Broken(bool);
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            if self.0 {
                Ok(0)
            } else {
                Err(io::Error::other("injected write error"))
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let cancel = std::sync::atomic::AtomicBool::new(false);
    assert_eq!(
        write_chunk(&mut Broken(true), b"a", &cancel, &mut 0)
            .unwrap_err()
            .kind(),
        io::ErrorKind::WriteZero
    );
    assert_eq!(
        write_chunk(&mut Broken(false), b"a", &cancel, &mut 0)
            .unwrap_err()
            .to_string(),
        "injected write error"
    );
}

#[test]
fn declined_metadata_policy_refuses_before_staging() {
    let fixture = Fixture::new(b"source").unwrap();
    let failure = copy(&fixture, MetadataPolicy::Declined).unwrap_err();
    assert_eq!(failure.staging, StageDisposition::NotCreated);
    assert!(!fixture.root.join(&fixture.stage).exists());
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn committed_file_never_has_group_or_world_permissions() {
    let fixture = Fixture::new(b"source").unwrap();
    copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).unwrap();
    let mode = std::fs::metadata(fixture.root.join("destination"))
        .unwrap()
        .mode();
    assert_eq!(mode & 0o177, 0);
}

#[test]
fn fifo_source_is_rejected_without_blocking_or_staging() {
    let fixture = Fixture::new(b"source").unwrap();
    std::fs::remove_file(fixture.root.join("source")).unwrap();
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .arg(fixture.root.join("source"))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(matches!(
        Authorization::authorize(fixture.authorization_request()),
        Err(super::AuthorizationError::UnsupportedSource)
    ));
    let failure = copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).unwrap_err();
    assert_eq!(failure.staging, StageDisposition::NotCreated);
    assert!(!fixture.root.join(&fixture.stage).exists());
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn system_sha256_confirms_real_fixture_copy() {
    let bytes: Vec<u8> = (0..BUFFER + 17).map(|index| (index % 251) as u8).collect();
    let fixture = Fixture::new(&bytes).unwrap();
    let completed = copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).unwrap();
    let digest = |name: &str| {
        let output = std::process::Command::new("/usr/bin/shasum")
            .arg("-a")
            .arg("256")
            .arg(fixture.root.join(name))
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()[..64].to_owned()
    };
    let expected = completed
        .digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(digest("source"), digest("destination"));
    assert_eq!(digest("source"), expected);
    assert_eq!(
        std::fs::read(fixture.root.join("destination")).unwrap(),
        bytes
    );
}

#[test]
fn rejects_non_component_request_names() {
    for invalid in ["", ".", "..", "../escape", "a/b", "/absolute", "a/", "a\0b"] {
        assert!(
            !crate::copy_transaction::valid_name(std::ffi::OsStr::new(invalid)),
            "accepted {invalid:?}"
        );
    }
    assert!(crate::copy_transaction::valid_name(std::ffi::OsStr::new(
        "normal-name"
    )));
}

#[test]
fn failure_reports_written_and_flushed_without_claiming_verification() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| {
            if phase == "before_verify" {
                return Err(io::Error::other("injected verification failure"));
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(failure.progress.written, 6);
    assert!(failure.progress.flushed);
    assert_eq!(failure.progress.verified, 0);
}

#[test]
fn cleanup_failure_retains_original_and_cleanup_errors() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| match phase {
            "before_verify" => Err(io::Error::other("original failure")),
            "before_cleanup" => Err(io::Error::other("cleanup failure")),
            _ => Ok(()),
        },
    )
    .unwrap_err();
    assert_eq!(failure.cause.to_string(), "original failure");
    assert_eq!(failure.staging, StageDisposition::CleanupFailed);
    assert_eq!(
        failure.cleanup_error.unwrap().to_string(),
        "cleanup failure"
    );
    assert!(fixture.root.join(&fixture.stage).exists());
}

#[test]
fn error_before_commit_call_reports_not_committed_and_removes_stage() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = copy_with_hook(
        &fixture,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |phase, _| {
            if phase == "commit_call" {
                return Err(io::Error::other("injected before commit"));
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(failure.commit, CommitState::NotCommitted);
    assert_eq!(failure.staging, StageDisposition::Removed);
    assert_eq!(failure.progress.verified, 6);
    assert!(!fixture.root.join(&fixture.stage).exists());
    assert!(!fixture.root.join("destination").exists());
}

#[test]
fn multiple_bounded_buffers_copy_the_same_bytes() {
    let bytes: Vec<u8> = (0..8193).map(|n| (n % 251) as u8).collect();
    for buffer_size in [1, 17, 4096, BUFFER] {
        let fixture = Fixture::new(&bytes).unwrap();
        let request = Request {
            authorization: fixture.authorization().unwrap(),
            source_name: std::ffi::OsStr::new("source"),
            final_name: std::ffi::OsStr::new("destination"),
            stage: &fixture.stage,
            buffer_size,
        };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let completed = crate::copy_transaction::copy_with_hook(
            &request,
            MetadataPolicy::BytesOnlyAcknowledged,
            &cancel,
            &mut |_, _| Ok(()),
        )
        .unwrap();
        assert_eq!(completed.progress.verified, bytes.len() as u64);
        assert_eq!(
            std::fs::read(fixture.root.join("destination")).unwrap(),
            bytes
        );
    }
}

#[test]
fn stage_equal_to_final_is_rejected_before_staging() {
    let fixture = Fixture::new(b"source").unwrap();
    let request = Request {
        authorization: fixture.authorization().unwrap(),
        source_name: std::ffi::OsStr::new("source"),
        final_name: std::ffi::OsStr::new(&fixture.stage),
        stage: &fixture.stage,
        buffer_size: BUFFER,
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let failure = crate::copy_transaction::copy_with_hook(
        &request,
        MetadataPolicy::BytesOnlyAcknowledged,
        &cancel,
        &mut |_, _| Ok(()),
    )
    .unwrap_err();
    assert_eq!(failure.staging, StageDisposition::NotCreated);
    assert!(!fixture.root.join(&fixture.stage).exists());
}

#[test]
fn injected_output_paths_leave_external_sentinel_unchanged() {
    let fixture = Fixture::new(b"source").unwrap();
    let outside = Fixture::new(b"sentinel").unwrap();
    let absolute = outside.root.join("source");
    let escape = outside.root.join("escape");
    let traversal = std::path::Path::new("..")
        .join(outside.root.file_name().unwrap())
        .join("escape");
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for name in [
        traversal.as_os_str(),
        absolute.as_os_str(),
        std::ffi::OsStr::new(""),
        std::ffi::OsStr::new("."),
        std::ffi::OsStr::new(".."),
        std::ffi::OsStr::new("dir/file"),
    ] {
        let request = Request {
            authorization: fixture.authorization().unwrap(),
            source_name: std::ffi::OsStr::new("source"),
            final_name: name,
            stage: &fixture.stage,
            buffer_size: BUFFER,
        };
        let failure = crate::copy_transaction::copy_with_hook(
            &request,
            MetadataPolicy::BytesOnlyAcknowledged,
            &cancel,
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
        assert_eq!(failure.staging, StageDisposition::NotCreated);
        assert_eq!(std::fs::read(&absolute).unwrap(), b"sentinel");
        assert!(!escape.exists());
        assert!(!fixture.root.join(&fixture.stage).exists());
    }
}

#[test]
fn invalid_request_fields_are_rejected_before_staging() {
    let fixture = Fixture::new(b"source").unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for field in 0..4 {
        let request = Request {
            authorization: fixture.authorization().unwrap(),
            source_name: std::ffi::OsStr::new(if field == 0 { "\0" } else { "source" }),
            final_name: std::ffi::OsStr::new(if field == 1 { "\0" } else { "destination" }),
            stage: if field == 2 { "\0" } else { &fixture.stage },
            buffer_size: if field == 3 { 0 } else { BUFFER },
        };
        let failure = crate::copy_transaction::copy_with_hook(
            &request,
            MetadataPolicy::BytesOnlyAcknowledged,
            &cancel,
            &mut |_, _| Ok(()),
        )
        .unwrap_err();
        assert_eq!(
            failure.staging,
            StageDisposition::NotCreated,
            "field {field}"
        );
        assert!(!fixture.root.join(&fixture.stage).exists());
        assert!(!fixture.root.join("destination").exists());
    }
}

#[test]
fn cleanup_removes_only_the_recorded_untouched_fixture() {
    let fixture = Fixture::new(b"source").unwrap();
    let root = fixture.root.clone();
    fixture.cleanup().unwrap();
    assert!(!root.exists());
}

#[test]
fn cleanup_rejects_unexpected_entry_without_removing_source() {
    let fixture = Fixture::new(b"source").unwrap();
    std::fs::write(fixture.root.join("unrelated"), b"keep").unwrap();
    assert!(fixture.cleanup().is_err());
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"source"
    );
    assert_eq!(
        std::fs::read(fixture.root.join("unrelated")).unwrap(),
        b"keep"
    );
}

#[test]
fn cleanup_rejects_replaced_source_without_removing_it() {
    let fixture = Fixture::new(b"source").unwrap();
    use std::os::unix::fs::FileExt;
    std::fs::remove_file(fixture.root.join("source")).unwrap();
    std::fs::write(fixture.root.join("source"), b"replacement").unwrap();
    assert!(fixture.cleanup().is_err());
    assert_eq!(
        std::fs::read(fixture.root.join("source")).unwrap(),
        b"replacement"
    );
    let mut original = [0; 6];
    fixture.source.read_exact_at(&mut original, 0).unwrap();
    assert_eq!(&original, b"source");
}

#[test]
fn committed_copy_has_recorded_identity_and_can_be_cleaned() {
    let bytes = b"fixture lifecycle copy";
    let fixture = Fixture::new(bytes).unwrap();
    let completed = copy(&fixture, MetadataPolicy::BytesOnlyAcknowledged).unwrap();
    assert_eq!(
        completed.digest.as_slice(),
        Sha256::digest(bytes).as_slice()
    );
    assert_eq!(std::fs::read(fixture.root.join("source")).unwrap(), bytes);
    assert_eq!(
        std::fs::read(fixture.root.join("destination")).unwrap(),
        bytes
    );
    let digest = |name: &str| {
        let output = std::process::Command::new("/usr/bin/shasum")
            .arg("-a")
            .arg("256")
            .arg(fixture.root.join(name))
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()[..64].to_owned()
    };
    assert_eq!(digest("source"), digest("destination"));
    let expected = completed
        .digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(digest("destination"), expected);
    let root = fixture.root.clone();
    fixture.cleanup().unwrap();
    assert!(!root.exists());
}
