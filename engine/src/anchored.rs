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
    use std::{fs, os::unix::fs::symlink};

    #[test]
    fn replacing_directory_with_symlink_cannot_redirect_open() {
        let root = std::env::temp_dir().join(format!("portstride-race-{}", std::process::id()));
        fs::create_dir_all(root.join("parent/child")).unwrap();
        fs::create_dir_all(root.join("outside")).unwrap();
        fs::write(root.join("outside/secret"), b"secret").unwrap();
        let parent = open_directory(&root.join("parent")).unwrap();
        let before = stat_child(&parent, std::ffi::OsStr::new("child")).unwrap();
        fs::rename(root.join("parent/child"), root.join("moved")).unwrap();
        symlink(root.join("outside"), root.join("parent/child")).unwrap();
        assert!(open_verified_directory(&parent, std::ffi::OsStr::new("child"), &before).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replaced_directory_identity_is_rejected() {
        let root = std::env::temp_dir().join(format!("portstride-replace-{}", std::process::id()));
        fs::create_dir_all(root.join("parent/child")).unwrap();
        let parent = open_directory(&root.join("parent")).unwrap();
        let before = stat_child(&parent, std::ffi::OsStr::new("child")).unwrap();
        fs::rename(root.join("parent/child"), root.join("old")).unwrap();
        fs::create_dir(root.join("parent/child")).unwrap();
        assert!(open_verified_directory(&parent, std::ffi::OsStr::new("child"), &before).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn renamed_open_directory_stays_anchored() {
        let root = std::env::temp_dir().join(format!("portstride-rename-{}", std::process::id()));
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
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleted_entry_reports_an_error() {
        let root = std::env::temp_dir().join(format!("portstride-deleted-{}", std::process::id()));
        fs::create_dir_all(root.join("parent/child")).unwrap();
        let parent = open_directory(&root.join("parent")).unwrap();
        let before = stat_child(&parent, OsStr::new("child")).unwrap();
        fs::remove_dir(root.join("parent/child")).unwrap();
        assert!(open_verified_directory(&parent, OsStr::new("child"), &before).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn replacing_destination_path_is_detected() {
        let root =
            std::env::temp_dir().join(format!("portstride-destination-{}", std::process::id()));
        fs::create_dir_all(root.join("destination")).unwrap();
        let destination = open_directory(&root.join("destination")).unwrap();
        fs::rename(root.join("destination"), root.join("old")).unwrap();
        fs::create_dir(root.join("destination")).unwrap();
        assert!(!path_still_identifies(&root.join("destination"), &destination).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
