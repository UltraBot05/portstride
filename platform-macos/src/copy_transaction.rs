//! Restricted transaction for trusted source and destination directories.
//!
//! The caller must establish an explicit copy intention and trust both directory
//! chains, their ACLs and the filesystem to prevent concurrent substitutions,
//! mount changes and hostile name equivalence. The selected source descriptor
//! must be the user's intended file. Identity checks detect observed changes; they
//! do not establish those assumptions or sandbox hostile code in this process.
//! Final names use a conservative ASCII subset disjoint from staging names.
use rustix::fs::{self, AtFlags, Mode, OFlags, RenameFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
};

pub(super) const BUFFER: usize = 1024 * 1024;
const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

// The caller must establish trust in the parent and ancestors, including ACLs,
// and prevent concurrent entry or mount changes. Identity checks cannot prove it.
pub(super) struct Authorization<'a> {
    source_dir: &'a std::os::fd::OwnedFd,
    source_name: &'a std::ffi::OsStr,
    final_name: &'a std::ffi::OsStr,
    dir: &'a std::os::fd::OwnedFd,
    parent: &'a std::os::fd::OwnedFd,
    component: &'a str,
    root: &'a std::path::Path,
    source_identity: std::fs::Metadata,
}
pub(super) struct AuthorizationRequest<'a> {
    pub(super) source_dir: &'a std::os::fd::OwnedFd,
    pub(super) source: &'a File,
    pub(super) source_name: &'a std::ffi::OsStr,
    pub(super) dir: &'a std::os::fd::OwnedFd,
    pub(super) parent: &'a std::os::fd::OwnedFd,
    pub(super) component: &'a str,
    pub(super) root: &'a std::path::Path,
    pub(super) final_name: &'a std::ffi::OsStr,
    pub(super) metadata: MetadataPolicy,
}
#[derive(Debug)]
pub(super) enum AuthorizationError {
    InvalidName,
    MetadataNotAcknowledged,
    UnsupportedSource,
    SourceChanged,
    DestinationChanged,
    DestinationConflict,
    DestinationAccessUnknownOrDenied,
    Io(io::Error),
}
impl From<io::Error> for AuthorizationError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<rustix::io::Errno> for AuthorizationError {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Io(error.into())
    }
}
impl<'a> Authorization<'a> {
    pub(super) fn authorize(request: AuthorizationRequest<'a>) -> Result<Self, AuthorizationError> {
        Self::authorize_with_probe(request, crate::probe)
    }

    fn authorize_with_probe(
        request: AuthorizationRequest<'a>,
        probe: impl FnOnce(&std::path::Path) -> portstride_engine::Capabilities,
    ) -> Result<Self, AuthorizationError> {
        if !valid_name(std::ffi::OsStr::new(request.component))
            || !valid_name(request.source_name)
            || !valid_final_name(request.final_name)
        {
            return Err(AuthorizationError::InvalidName);
        }
        if matches!(request.metadata, MetadataPolicy::Declined) {
            return Err(AuthorizationError::MetadataNotAcknowledged);
        }
        let entry = fs::statat(
            request.source_dir,
            request.source_name,
            AtFlags::SYMLINK_NOFOLLOW,
        )?;
        if fs::FileType::from_raw_mode(entry.st_mode) != fs::FileType::RegularFile
            || entry.st_nlink != 1
        {
            return Err(AuthorizationError::UnsupportedSource);
        }
        let source_identity = request.source.metadata()?;
        if !source_identity.is_file()
            || source_identity.nlink() != 1
            || source_identity.dev() != entry.st_dev as u64
            || source_identity.ino() != entry.st_ino
        {
            return Err(AuthorizationError::SourceChanged);
        }
        let authorization = Self {
            source_dir: request.source_dir,
            source_name: request.source_name,
            final_name: request.final_name,
            dir: request.dir,
            parent: request.parent,
            component: request.component,
            root: request.root,
            source_identity,
        };
        check_fixture(&authorization).map_err(|_| AuthorizationError::DestinationChanged)?;
        let held = fs::fstat(authorization.dir)?;
        let capabilities = probe(authorization.root);
        if capabilities.probe_identity != Some((held.st_dev as u64, held.st_ino)) {
            return Err(AuthorizationError::DestinationChanged);
        }
        if !destination_access_allowed(capabilities.writable, capabilities.mount_read_only) {
            return Err(AuthorizationError::DestinationAccessUnknownOrDenied);
        }
        match fs::statat(
            authorization.dir,
            authorization.final_name,
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(_) => return Err(AuthorizationError::DestinationConflict),
            Err(rustix::io::Errno::NOENT) => {}
            Err(error) => return Err(error.into()),
        }
        Ok(authorization)
    }
}
pub(super) struct Request<'a> {
    authorization: Authorization<'a>,
    source_name: &'a std::ffi::OsStr,
    final_name: &'a std::ffi::OsStr,
    stage: &'a str,
    buffer_size: usize,
}
impl<'a> Request<'a> {
    // The caller must generate an unpredictable, fresh stage name for each request.
    pub(super) fn new(authorization: Authorization<'a>, stage: &'a str) -> Self {
        Self {
            source_name: authorization.source_name,
            final_name: authorization.final_name,
            authorization,
            stage,
            buffer_size: BUFFER,
        }
    }
}
#[derive(Debug)]
pub(super) struct Completed {
    pub(super) progress: Progress,
    pub(super) digest: [u8; 32],
    pub(super) metadata: MetadataProfile,
    pub(super) durability: Durability,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum MetadataProfile {
    BytesOnly,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Durability {
    StagingSyncSucceededNoPowerLossGuarantee,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StageDisposition {
    NotCreated,
    Removed,
    LeftUnidentified,
    CleanupFailed,
    UnknownAfterCommit,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum CommitState {
    NotCommitted,
    Unknown,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum FailureKind {
    Cancelled,
    Error,
}
#[derive(Debug)]
struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}
impl std::error::Error for Cancelled {}
fn failure_kind(cause: &io::Error) -> FailureKind {
    if cause.get_ref().is_some_and(|inner| inner.is::<Cancelled>()) {
        FailureKind::Cancelled
    } else {
        FailureKind::Error
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Progress {
    pub(super) written: u64,
    pub(super) readback: u64,
    pub(super) verified: u64,
    pub(super) flushed: bool,
}
#[derive(Debug)]
pub(super) struct CopyFailure {
    pub(super) progress: Progress,
    pub(super) cleanup_error: Option<io::Error>,
    pub(super) kind: FailureKind,
    pub(super) cause: io::Error,
    pub(super) staging: StageDisposition,
    pub(super) commit: CommitState,
}
impl From<io::Error> for CopyFailure {
    fn from(cause: io::Error) -> Self {
        Self {
            progress: Progress::default(),
            cleanup_error: None,
            kind: failure_kind(&cause),
            cause,
            staging: StageDisposition::NotCreated,
            commit: CommitState::NotCommitted,
        }
    }
}
impl From<rustix::io::Errno> for CopyFailure {
    fn from(cause: rustix::io::Errno) -> Self {
        io::Error::from(cause).into()
    }
}

fn same(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.nlink() == b.nlink()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}

fn destination_access_allowed(writable: Option<bool>, read_only: Option<bool>) -> bool {
    writable == Some(true) && read_only == Some(false)
}

pub(super) fn check_limits(
    size: u64,
    available: Option<u64>,
    limit: Option<u64>,
) -> io::Result<()> {
    if available.is_some_and(|free| size > free) {
        return Err(io::Error::other("insufficient capacity"));
    }
    if limit.is_some_and(|max| size > max) {
        return Err(io::Error::other("file too large"));
    }
    Ok(())
}

fn check_cancel(cancel: &std::sync::atomic::AtomicBool) -> io::Result<()> {
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        Err(io::Error::other(Cancelled))
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(super) enum MetadataPolicy {
    BytesOnlyAcknowledged,
    Declined,
}

pub(super) fn valid_name(name: &std::ffi::OsStr) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes != b"."
        && bytes != b".."
        && !bytes.contains(&b'/')
        && !bytes.contains(&0)
}

// Keep final names disjoint from the dot-prefixed ASCII staging namespace.
fn valid_final_name(name: &std::ffi::OsStr) -> bool {
    let bytes = name.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn check_fixture(fixture: &Authorization<'_>) -> io::Result<()> {
    let held = fs::fstat(fixture.dir)?;
    let named = fs::statat(fixture.parent, fixture.component, AtFlags::SYMLINK_NOFOLLOW)?;
    if fs::FileType::from_raw_mode(named.st_mode) != fs::FileType::Directory
        || held.st_dev != named.st_dev
        || held.st_ino != named.st_ino
        || held.st_uid != rustix::process::geteuid().as_raw()
        || held.st_mode & 0o077 != 0
    {
        return Err(io::Error::other("fixture directory changed"));
    }
    Ok(())
}

pub(super) fn copy(
    request: Request<'_>,
    policy: MetadataPolicy,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Completed, CopyFailure> {
    copy_with_hook(&request, policy, cancel, &mut |_, _| Ok(()))
}

pub(super) fn copy_with_hook(
    request: &Request<'_>,
    policy: MetadataPolicy,
    cancel: &std::sync::atomic::AtomicBool,
    hook: &mut impl FnMut(&str, &File) -> io::Result<()>,
) -> Result<Completed, CopyFailure> {
    let stage_suffix = request.stage.strip_prefix(".portstride-");
    let valid_stage = stage_suffix.is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if request.source_name != request.authorization.source_name
        || request.final_name != request.authorization.final_name
        || !valid_stage
        || request.buffer_size == 0
        || request.buffer_size > BUFFER
    {
        return Err(io::Error::other("invalid filename component").into());
    }
    if matches!(policy, MetadataPolicy::Declined) {
        return Err(io::Error::other("bytes-only metadata policy not acknowledged").into());
    }
    check_cancel(cancel)?;
    check_fixture(&request.authorization)?;
    let initial_entry = fs::statat(
        request.authorization.source_dir,
        request.source_name,
        AtFlags::SYMLINK_NOFOLLOW,
    )?;
    if fs::FileType::from_raw_mode(initial_entry.st_mode) != fs::FileType::RegularFile
        || initial_entry.st_nlink != 1
    {
        return Err(io::Error::other("unsupported source").into());
    }
    let mut source = File::from(fs::openat(
        request.authorization.source_dir,
        request.source_name,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    let initial = source.metadata()?;
    if !initial.is_file()
        || initial.nlink() != 1
        || initial.dev() != initial_entry.st_dev as u64
        || initial.ino() != initial_entry.st_ino
        || !same(&request.authorization.source_identity, &initial)
    {
        return Err(io::Error::other("source changed").into());
    }
    let initial_dir = fs::fstat(request.authorization.dir)?;
    let capabilities = crate::probe(request.authorization.root);
    if capabilities.probe_identity != Some((initial_dir.st_dev as u64, initial_dir.st_ino)) {
        return Err(io::Error::other("destination probe identity unavailable or changed").into());
    }
    if !destination_access_allowed(capabilities.writable, capabilities.mount_read_only) {
        return Err(io::Error::other("destination is not writable").into());
    }
    let volume = fs::fstatvfs(request.authorization.dir)?;
    let available = volume
        .f_bavail
        .checked_mul(volume.f_frsize)
        .filter(|_| volume.f_frsize != 0);
    check_limits(initial.len(), available, capabilities.max_file_bytes)?;
    check_cancel(cancel)?;
    check_fixture(&request.authorization)?;
    let mut stage = File::from(fs::openat(
        request.authorization.dir,
        request.stage,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )?);
    let stage_identity = stage.metadata().map_err(|cause| CopyFailure {
        progress: Progress::default(),
        cleanup_error: None,
        kind: failure_kind(&cause),
        cause,
        staging: StageDisposition::LeftUnidentified,
        commit: CommitState::NotCommitted,
    })?;
    let mut commit_unknown = false;
    let mut progress = Progress::default();
    let result: io::Result<Completed> = (|| {
        hook("stage_created", &stage)?;
        let mut buffer = vec![0u8; request.buffer_size];
        let mut source_hash = Sha256::new();
        loop {
            check_cancel(cancel)?;
            let count = read_chunk(&mut source, &mut buffer, cancel)?;
            if count == 0 {
                break;
            }
            source_hash.update(&buffer[..count]);
            check_cancel(cancel)?;
            hook("before_write", &stage)?;
            write_chunk(&mut stage, &buffer[..count], cancel, &mut progress.written)?;
        }
        check_cancel(cancel)?;
        hook("before_sync", &stage)?;
        stage.sync_all()?;
        progress.flushed = true;
        hook("before_verify", &stage)?;
        stage.seek(SeekFrom::Start(0))?;
        let mut staged_hash = Sha256::new();
        loop {
            check_cancel(cancel)?;
            let count = read_chunk(&mut stage, &mut buffer, cancel)?;
            if count == 0 {
                break;
            }
            staged_hash.update(&buffer[..count]);
            progress.readback += count as u64;
        }
        let digest: [u8; 32] = source_hash.finalize().into();
        let staged_digest: [u8; 32] = staged_hash.finalize().into();
        if digest != staged_digest
            || progress.written != progress.readback
            || progress.written != initial.len()
        {
            return Err(io::Error::other("verification mismatch"));
        }
        progress.verified = progress.readback;
        hook("before_commit", &stage)?;
        check_fixture(&request.authorization)?;
        let held_dir = fs::fstat(request.authorization.dir)?;
        let reopened_dir = fs::open(request.authorization.root, DIR_FLAGS, Mode::empty())?;
        let named_dir = fs::fstat(&reopened_dir)?;
        let current_capabilities = crate::probe(request.authorization.root);
        if current_capabilities.probe_identity != capabilities.probe_identity
            || current_capabilities.volume_id != capabilities.volume_id
            || !destination_access_allowed(
                current_capabilities.writable,
                current_capabilities.mount_read_only,
            )
        {
            return Err(io::Error::other("destination capabilities changed"));
        }
        check_limits(initial.len(), None, current_capabilities.max_file_bytes)?;
        if held_dir.st_dev != named_dir.st_dev || held_dir.st_ino != named_dir.st_ino {
            return Err(io::Error::other("destination changed"));
        }
        let entry = fs::statat(
            request.authorization.source_dir,
            request.source_name,
            AtFlags::SYMLINK_NOFOLLOW,
        )?;
        if !same(&initial, &source.metadata()?)
            || entry.st_dev as u64 != initial.dev()
            || entry.st_ino != initial.ino()
        {
            return Err(io::Error::other("source changed"));
        }
        let stage_entry = fs::statat(
            request.authorization.dir,
            request.stage,
            AtFlags::SYMLINK_NOFOLLOW,
        )?;
        if stage_entry.st_dev as u64 != stage_identity.dev()
            || stage_entry.st_ino != stage_identity.ino()
        {
            return Err(io::Error::other("stage changed"));
        }
        // A final spelling can alias the stage on case-folding filesystems.
        match fs::statat(
            request.authorization.dir,
            request.final_name,
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "destination exists",
                ));
            }
            Err(rustix::io::Errno::NOENT) => {}
            Err(error) => return Err(error.into()),
        }
        check_cancel(cancel)?;
        hook("commit_call", &stage)?;
        #[cfg(test)]
        if let Err(error) = hook("simulate_unknown_before_rename", &stage) {
            commit_unknown = true;
            return Err(error);
        }
        if let Err(error) = fs::renameat_with(
            request.authorization.dir,
            request.stage,
            request.authorization.dir,
            request.final_name,
            RenameFlags::NOREPLACE,
        ) {
            commit_unknown =
                error != rustix::io::Errno::EXIST && error != rustix::io::Errno::NOTSUP;
            return Err(error.into());
        }
        #[cfg(test)]
        if let Err(error) = hook("simulate_unknown_after_rename", &stage) {
            commit_unknown = true;
            return Err(error);
        }
        // A ledger failure after successful rename must not turn a committed copy into failure.
        let _ = hook("committed", &stage);
        Ok(Completed {
            progress,
            digest,
            metadata: MetadataProfile::BytesOnly,
            durability: Durability::StagingSyncSucceededNoPowerLossGuarantee,
        })
    })();
    match result {
        Ok(completed) => Ok(completed),
        Err(cause) if commit_unknown => Err(CopyFailure {
            progress,
            cleanup_error: None,
            kind: failure_kind(&cause),
            cause,
            staging: StageDisposition::UnknownAfterCommit,
            commit: CommitState::Unknown,
        }),
        Err(cause) => {
            let (staging, cleanup_error) = match hook("before_cleanup", &stage) {
                Err(error) => (StageDisposition::CleanupFailed, Some(error)),
                Ok(()) => match fs::statat(
                    request.authorization.dir,
                    request.stage,
                    AtFlags::SYMLINK_NOFOLLOW,
                ) {
                    Ok(entry)
                        if entry.st_dev as u64 == stage_identity.dev()
                            && entry.st_ino == stage_identity.ino() =>
                    {
                        match fs::unlinkat(
                            request.authorization.dir,
                            request.stage,
                            AtFlags::empty(),
                        ) {
                            Ok(()) => (StageDisposition::Removed, None),
                            Err(error) => (StageDisposition::CleanupFailed, Some(error.into())),
                        }
                    }
                    Ok(_) => (StageDisposition::LeftUnidentified, None),
                    Err(error) => (StageDisposition::LeftUnidentified, Some(error.into())),
                },
            };
            Err(CopyFailure {
                progress,
                cleanup_error,
                kind: failure_kind(&cause),
                cause,
                staging,
                commit: CommitState::NotCommitted,
            })
        }
    }
}

pub(super) fn write_chunk(
    writer: &mut impl Write,
    mut bytes: &[u8],
    cancel: &std::sync::atomic::AtomicBool,
    written: &mut u64,
) -> io::Result<()> {
    while !bytes.is_empty() {
        check_cancel(cancel)?;
        match writer.write(bytes) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "short write")),
            Ok(count) => {
                *written = written
                    .checked_add(count as u64)
                    .ok_or_else(|| io::Error::other("byte count overflow"))?;
                bytes = &bytes[count..];
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) fn read_chunk(
    reader: &mut impl Read,
    buffer: &mut [u8],
    cancel: &std::sync::atomic::AtomicBool,
) -> io::Result<usize> {
    loop {
        check_cancel(cancel)?;
        match reader.read(buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

#[cfg(test)]
#[path = "copy_fixture.rs"]
mod copy_fixture;
