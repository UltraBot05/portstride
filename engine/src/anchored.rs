use rustix::fs::{self, AtFlags, Mode, OFlags, Stat};
use std::{
    ffi::{OsStr, OsString},
    io,
    os::{fd::OwnedFd, unix::ffi::OsStringExt},
    path::Path,
};

pub fn open_directory(path: &Path) -> io::Result<OwnedFd> {
    fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(Into::into)
}

pub fn stat_child(parent: &OwnedFd, name: &OsStr) -> io::Result<Stat> {
    fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(Into::into)
}

pub fn open_verified_directory(
    parent: &OwnedFd,
    name: &OsStr,
    expected: &Stat,
) -> io::Result<OwnedFd> {
    let fd = fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let actual = fs::fstat(&fd).map_err(io::Error::from)?;
    if identity(&actual) != identity(expected) {
        return Err(io::Error::other("Directory changed during inspection"));
    }
    Ok(fd)
}

pub fn identity(stat: &Stat) -> (u64, u64) {
    (stat.st_dev as u64, stat.st_ino)
}
pub fn stat_directory(fd: &OwnedFd) -> io::Result<Stat> {
    fs::fstat(fd).map_err(Into::into)
}

pub fn path_still_identifies(path: &Path, fd: &OwnedFd) -> io::Result<bool> {
    let current = open_directory(path)?;
    Ok(identity(&stat_directory(fd)?) == identity(&stat_directory(&current)?))
}

pub fn names(fd: &OwnedFd) -> io::Result<Vec<OsString>> {
    let dir = fs::Dir::read_from(fd).map_err(io::Error::from)?;
    let mut names = Vec::new();
    for entry in dir {
        let entry = entry.map_err(io::Error::from)?;
        let name = entry.file_name().to_bytes();
        if name != b"." && name != b".." {
            names.push(OsString::from_vec(name.to_vec()));
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write, os::unix::fs::symlink};

    // Test artifacts are deliberately retained: their entries are not ledgered for cleanup.
    fn fixture() -> std::path::PathBuf {
        use rustix::fs::{self as rfs, AtFlags, Mode, OFlags};
        let parent_path = std::env::temp_dir().canonicalize().unwrap();
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let parent = rfs::open(&parent_path, flags, Mode::empty()).unwrap();
        let parent_identity = rfs::fstat(&parent).unwrap();
        assert_eq!(parent_identity.st_uid, rustix::process::geteuid().as_raw());
        assert_eq!(
            parent_identity.st_mode & 0o077,
            0,
            "temporary parent must be private"
        );
        let mut nonce = [0u8; 16];
        getrandom::getrandom(&mut nonce).unwrap();
        let id: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        let name = format!("portstride-anchored-{id}");
        rfs::mkdirat(&parent, name.as_str(), Mode::from_bits_truncate(0o700)).unwrap();
        let named = rfs::statat(&parent, name.as_str(), AtFlags::SYMLINK_NOFOLLOW).unwrap();
        let dir = rfs::openat(&parent, name.as_str(), flags, Mode::empty()).unwrap();
        let held = rfs::fstat(&dir).unwrap();
        assert_eq!((named.st_dev, named.st_ino), (held.st_dev, held.st_ino));
        let mut manifest = fs::File::from(
            rfs::openat(
                &dir,
                "manifest.json",
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_bits_truncate(0o600),
            )
            .unwrap(),
        );
        let manifest_identity = rfs::fstat(&manifest).unwrap();
        writeln!(
            manifest,
            "{}",
            serde_json::json!({
                "version": 1, "id": id,
                "parent": [parent_identity.st_dev as u64, parent_identity.st_ino],
                "root": [held.st_dev as u64, held.st_ino],
                "manifest": [manifest_identity.st_dev as u64, manifest_identity.st_ino],
                "cleanup": "manual review required"
            })
        )
        .unwrap();
        manifest.sync_all().unwrap();
        parent_path.join(name)
    }

    #[test]
    fn replacing_directory_with_symlink_cannot_redirect_open() {
        let root = fixture();
        fs::create_dir_all(root.join("parent/child")).unwrap();
        fs::create_dir_all(root.join("outside")).unwrap();
        fs::write(root.join("outside/secret"), b"secret").unwrap();
        let parent = open_directory(&root.join("parent")).unwrap();
        let before = stat_child(&parent, std::ffi::OsStr::new("child")).unwrap();
        fs::rename(root.join("parent/child"), root.join("moved")).unwrap();
        symlink(root.join("outside"), root.join("parent/child")).unwrap();
        assert!(open_verified_directory(&parent, std::ffi::OsStr::new("child"), &before).is_err());
    }

    #[test]
    fn replaced_directory_identity_is_rejected() {
        let root = fixture();
        fs::create_dir_all(root.join("parent/child")).unwrap();
        let parent = open_directory(&root.join("parent")).unwrap();
        let before = stat_child(&parent, std::ffi::OsStr::new("child")).unwrap();
        fs::rename(root.join("parent/child"), root.join("old")).unwrap();
        fs::create_dir(root.join("parent/child")).unwrap();
        assert!(open_verified_directory(&parent, std::ffi::OsStr::new("child"), &before).is_err());
    }
    #[test]
    fn renamed_open_directory_stays_anchored() {
        let root = fixture();
        fs::create_dir_all(root.join("parent/child")).unwrap();
        fs::write(root.join("parent/child/original"), b"a").unwrap();
        let parent = open_directory(&root.join("parent")).unwrap();
        let before = stat_child(&parent, OsStr::new("child")).unwrap();
        let child = open_verified_directory(&parent, OsStr::new("child"), &before).unwrap();
        fs::rename(root.join("parent/child"), root.join("old")).unwrap();
        fs::create_dir(root.join("parent/child")).unwrap();
        fs::write(root.join("parent/child/replacement"), b"b").unwrap();
        assert_eq!(names(&child).unwrap(), vec![OsString::from("original")]);
        assert_ne!(
            identity(&stat_child(&parent, OsStr::new("child")).unwrap()),
            identity(&before)
        );
    }

    #[test]
    fn deleted_entry_reports_an_error() {
        let root = fixture();
        fs::create_dir_all(root.join("parent/child")).unwrap();
        let parent = open_directory(&root.join("parent")).unwrap();
        let before = stat_child(&parent, OsStr::new("child")).unwrap();
        fs::remove_dir(root.join("parent/child")).unwrap();
        assert!(open_verified_directory(&parent, OsStr::new("child"), &before).is_err());
    }
    #[test]
    fn replacing_destination_path_is_detected() {
        let root = fixture();
        fs::create_dir_all(root.join("destination")).unwrap();
        let destination = open_directory(&root.join("destination")).unwrap();
        fs::rename(root.join("destination"), root.join("old")).unwrap();
        fs::create_dir(root.join("destination")).unwrap();
        assert!(!path_still_identifies(&root.join("destination"), &destination).unwrap());
    }
}
