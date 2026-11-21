use rustix::fs::{self, AtFlags, Mode, OFlags, Stat};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    io,
    os::{fd::OwnedFd, unix::ffi::OsStrExt},
};

const MAX_ITEMS: usize = 10_000;
const MAX_DEPTH: usize = 64;
const MAX_COMPONENT_BYTES: usize = 255;
const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

#[derive(Debug)]
pub(super) enum PlanError {
    Io(io::Error),
    UnsupportedName,
    UnknownNameBehavior,
    Collision,
    DestinationInsideSource,
    DirectoryAlias,
    EntryIo {
        path: Vec<OsString>,
        phase: &'static str,
        error: io::Error,
    },
    LimitExceeded,
}
impl From<rustix::io::Errno> for PlanError {
    fn from(value: rustix::io::Errno) -> Self {
        Self::Io(value.into())
    }
}

// This is an explicit fixture-volume assumption, not the result of a filesystem probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NameBehavior {
    Unknown,
    FixtureConventionalAssumption,
}
fn require_name_behavior(behavior: NameBehavior) -> Result<(), PlanError> {
    match behavior {
        NameBehavior::Unknown => Err(PlanError::UnknownNameBehavior),
        NameBehavior::FixtureConventionalAssumption => Ok(()),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UnsupportedKind {
    Symlink,
    HardLink,
    Special,
    SelectedDirectory,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ItemKind {
    Directory,
    File,
    Unsupported(UnsupportedKind),
}
#[derive(Debug)]
pub(super) struct PlannedItem {
    path: Vec<OsString>,
    source_path: Vec<OsString>,
    stat: Stat,
    kind: ItemKind,
}
#[derive(Debug)]
pub(super) struct Plan {
    items: Vec<PlannedItem>,
    source_directories: BTreeMap<Vec<OsString>, Stat>,
    source_root: Stat,
    destination_root: Stat,
}

impl PlannedItem {
    pub(super) fn path(&self) -> &[OsString] {
        &self.path
    }
    pub(super) fn source_path(&self) -> &[OsString] {
        &self.source_path
    }
    pub(super) fn stat(&self) -> &Stat {
        &self.stat
    }
    pub(super) fn kind(&self) -> ItemKind {
        self.kind
    }
}
impl Plan {
    pub(super) fn items(&self) -> &[PlannedItem] {
        &self.items
    }
    pub(super) fn source_directories(&self) -> &BTreeMap<Vec<OsString>, Stat> {
        &self.source_directories
    }
    pub(super) fn source_root(&self) -> &Stat {
        &self.source_root
    }
    pub(super) fn destination_root(&self) -> &Stat {
        &self.destination_root
    }
}

#[derive(Default)]
struct Mappings {
    paths: BTreeSet<Vec<Vec<u8>>>,
}
impl Mappings {
    fn insert(&mut self, path: &[OsString]) -> Result<(), PlanError> {
        let mut key = Vec::with_capacity(path.len());
        for part in path {
            let bytes = part.as_bytes();
            if bytes.len() > MAX_COMPONENT_BYTES
                || bytes.last() == Some(&b'.')
                || reserved_name(bytes)
                || !bytes.first().is_some_and(u8::is_ascii_alphanumeric)
                || !bytes
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            {
                return Err(PlanError::UnsupportedName);
            }
            key.push(bytes.iter().map(u8::to_ascii_lowercase).collect());
        }
        if !self.paths.insert(key) {
            return Err(PlanError::Collision);
        }
        Ok(())
    }
}
fn reserved_name(bytes: &[u8]) -> bool {
    let base = bytes.split(|b| *b == b'.').next().unwrap_or_default();
    let folded: Vec<u8> = base.iter().map(u8::to_ascii_uppercase).collect();
    matches!(
        folded.as_slice(),
        b"CON"
            | b"PRN"
            | b"AUX"
            | b"NUL"
            | b"COM1"
            | b"COM2"
            | b"COM3"
            | b"COM4"
            | b"COM5"
            | b"COM6"
            | b"COM7"
            | b"COM8"
            | b"COM9"
            | b"LPT1"
            | b"LPT2"
            | b"LPT3"
            | b"LPT4"
            | b"LPT5"
            | b"LPT6"
            | b"LPT7"
            | b"LPT8"
            | b"LPT9"
    )
}
fn identity(st: &Stat) -> (u64, u64) {
    (st.st_dev as u64, st.st_ino)
}
fn same_snapshot(a: &Stat, b: &Stat) -> bool {
    identity(a) == identity(b)
        && a.st_mode == b.st_mode
        && a.st_nlink == b.st_nlink
        && a.st_size == b.st_size
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
}
fn source_kind(st: &Stat) -> ItemKind {
    match fs::FileType::from_raw_mode(st.st_mode) {
        fs::FileType::Directory => ItemKind::Directory,
        fs::FileType::RegularFile if st.st_nlink == 1 => ItemKind::File,
        fs::FileType::RegularFile => ItemKind::Unsupported(UnsupportedKind::HardLink),
        fs::FileType::Symlink => ItemKind::Unsupported(UnsupportedKind::Symlink),
        _ => ItemKind::Unsupported(UnsupportedKind::Special),
    }
}
fn open_child(parent: &OwnedFd, name: &OsStr, expected: &Stat) -> Result<OwnedFd, PlanError> {
    let fd = fs::openat(parent, name, DIR_FLAGS, Mode::empty())?;
    if identity(&fs::fstat(&fd)?) != identity(expected) {
        return Err(PlanError::DirectoryAlias);
    }
    Ok(fd)
}
fn destination_outside(source: &OwnedFd, destination: &OwnedFd) -> Result<(), PlanError> {
    let source_id = identity(&fs::fstat(source)?);
    let mut current = fs::openat(destination, ".", DIR_FLAGS, Mode::empty())?;
    for _ in 0..=MAX_DEPTH {
        let id = identity(&fs::fstat(&current)?);
        if id == source_id {
            return Err(PlanError::DestinationInsideSource);
        }
        let parent = fs::openat(&current, "..", DIR_FLAGS, Mode::empty())?;
        if identity(&fs::fstat(&parent)?) == id {
            return Ok(());
        }
        current = parent;
    }
    Err(PlanError::LimitExceeded)
}
fn destination_names(destination: &OwnedFd) -> Result<BTreeSet<Vec<u8>>, PlanError> {
    let mut names = BTreeSet::new();
    let mut count = 0;
    for entry in fs::Dir::read_from(destination)? {
        let entry = entry?;
        let bytes = entry.file_name().to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        if bytes.len() > MAX_COMPONENT_BYTES || !bytes.is_ascii() {
            return Err(PlanError::UnsupportedName);
        }
        if count >= MAX_ITEMS {
            return Err(PlanError::LimitExceeded);
        }
        count += 1;
        names.insert(bytes.iter().map(u8::to_ascii_lowercase).collect());
    }
    Ok(names)
}
fn check_absent(names: &BTreeSet<Vec<u8>>, name: &OsStr) -> Result<(), PlanError> {
    let key: Vec<u8> = name.as_bytes().iter().map(u8::to_ascii_lowercase).collect();
    if names.contains(&key) {
        Err(PlanError::Collision)
    } else {
        Ok(())
    }
}
pub(super) fn recheck_absent(destination: &OwnedFd, name: &OsStr) -> Result<(), PlanError> {
    check_absent(&destination_names(destination)?, name)
}

fn add_item(
    parent: &OwnedFd,
    path: Vec<OsString>,
    mappings: &mut Mappings,
    items: &mut Vec<PlannedItem>,
) -> Result<(), PlanError> {
    if items.len() >= MAX_ITEMS || path.len() > MAX_DEPTH {
        return Err(PlanError::LimitExceeded);
    }
    mappings.insert(&path)?;
    let name = path.last().ok_or(PlanError::UnsupportedName)?;
    let stat = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
        PlanError::EntryIo {
            path: path.clone(),
            phase: "stat",
            error: error.into(),
        }
    })?;
    let kind = source_kind(&stat);
    items.push(PlannedItem {
        source_path: path.clone(),
        path,
        stat,
        kind,
    });
    Ok(())
}
pub(super) fn plan_files(
    source: &OwnedFd,
    destination: &OwnedFd,
    names: &[OsString],
    behavior: NameBehavior,
) -> Result<Plan, PlanError> {
    if names.len() > MAX_ITEMS
        || names
            .iter()
            .any(|name| name.as_bytes().len() > MAX_COMPONENT_BYTES)
    {
        return Err(PlanError::LimitExceeded);
    }
    let selections: Vec<Vec<OsString>> = names.iter().cloned().map(|name| vec![name]).collect();
    plan_selected(source, destination, &selections, behavior)
}
pub(super) fn plan_selected(
    source: &OwnedFd,
    destination: &OwnedFd,
    selections: &[Vec<OsString>],
    behavior: NameBehavior,
) -> Result<Plan, PlanError> {
    require_name_behavior(behavior)?;
    destination_outside(source, destination)?;
    let source_stat = fs::fstat(source)?;
    let destination_names = destination_names(destination)?;
    if selections.len() > MAX_ITEMS {
        return Err(PlanError::LimitExceeded);
    }
    let mut component_count = 0usize;
    for path in selections {
        component_count = component_count
            .checked_add(path.len())
            .ok_or(PlanError::LimitExceeded)?;
        if component_count > MAX_ITEMS {
            return Err(PlanError::LimitExceeded);
        }
        if path.is_empty() || path.len() > MAX_DEPTH {
            return Err(PlanError::LimitExceeded);
        }
        // Validate each component before copying or passing it to an at-style call.
        for component in path {
            Mappings::default().insert(std::slice::from_ref(component))?;
        }
    }
    let mut selections = selections.to_vec();
    selections.sort();
    let mut mappings = Mappings::default();
    let mut items = Vec::new();
    let mut source_directories = BTreeMap::new();
    let mut directory_ids = BTreeSet::from([identity(&source_stat)]);
    for path in selections {
        let mut current = fs::openat(source, ".", DIR_FLAGS, Mode::empty())?;
        let mut prefix = Vec::new();
        for name in &path[..path.len() - 1] {
            prefix.push(name.clone());
            let stat = fs::statat(&current, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
                PlanError::EntryIo {
                    path: prefix.clone(),
                    phase: "stat directory",
                    error: error.into(),
                }
            })?;
            if fs::FileType::from_raw_mode(stat.st_mode) != fs::FileType::Directory {
                return Err(PlanError::DirectoryAlias);
            }
            if let Some(previous) = source_directories.get(&prefix) {
                if !same_snapshot(previous, &stat) {
                    return Err(PlanError::DirectoryAlias);
                }
            } else {
                if !directory_ids.insert(identity(&stat)) {
                    return Err(PlanError::DirectoryAlias);
                }
                source_directories.insert(prefix.clone(), stat);
            }
            current = open_child(&current, name, &stat)?;
        }
        let name = path.last().ok_or(PlanError::UnsupportedName)?.clone();
        add_item(&current, vec![name.clone()], &mut mappings, &mut items)?;
        if let Some(item) = items.last_mut() {
            item.source_path = path;
            if item.kind == ItemKind::Directory {
                item.kind = ItemKind::Unsupported(UnsupportedKind::SelectedDirectory);
            }
        }
        check_absent(&destination_names, &name)?;
    }
    if !same_snapshot(&source_stat, &fs::fstat(source)?) {
        return Err(PlanError::DirectoryAlias);
    }
    Ok(Plan {
        items,
        source_directories,
        source_root: source_stat,
        destination_root: fs::fstat(destination)?,
    })
}
pub(super) fn plan_tree(
    source: &OwnedFd,
    destination: &OwnedFd,
    behavior: NameBehavior,
) -> Result<Plan, PlanError> {
    require_name_behavior(behavior)?;
    destination_outside(source, destination)?;
    let destination_names = destination_names(destination)?;
    let root = fs::openat(source, ".", DIR_FLAGS, Mode::empty())?;
    let root_stat = fs::fstat(source)?;
    let mut stack = vec![(root, Vec::<OsString>::new(), root_stat)];
    let mut directories = BTreeSet::from([identity(&fs::fstat(source)?)]);
    let mut mappings = Mappings::default();
    let mut items = Vec::new();
    while let Some((dir, prefix, expected)) = stack.pop() {
        if !same_snapshot(&expected, &fs::fstat(&dir)?) {
            return Err(PlanError::DirectoryAlias);
        }
        let mut names = Vec::new();
        for entry in fs::Dir::read_from(&dir).map_err(|error| PlanError::EntryIo {
            path: prefix.clone(),
            phase: "open directory",
            error: error.into(),
        })? {
            let entry = entry.map_err(|error| PlanError::EntryIo {
                path: prefix.clone(),
                phase: "read directory",
                error: error.into(),
            })?;
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            if bytes.len() > MAX_COMPONENT_BYTES {
                return Err(PlanError::UnsupportedName);
            }
            if names.len() + items.len() >= MAX_ITEMS {
                return Err(PlanError::LimitExceeded);
            }
            use std::os::unix::ffi::OsStringExt;
            names.push(OsString::from_vec(bytes.to_vec()));
        }
        if !same_snapshot(&expected, &fs::fstat(&dir)?) {
            return Err(PlanError::DirectoryAlias);
        }
        names.sort();
        let mut children = Vec::new();
        for name in names {
            let mut path = prefix.clone();
            path.push(name.clone());
            add_item(&dir, path.clone(), &mut mappings, &mut items)?;
            let item = items.last().ok_or(PlanError::LimitExceeded)?;
            if prefix.is_empty() {
                check_absent(&destination_names, &name)?;
            }
            if item.kind == ItemKind::Directory {
                if !directories.insert(identity(&item.stat)) {
                    return Err(PlanError::DirectoryAlias);
                }
                if stack.len() + children.len() >= MAX_DEPTH {
                    return Err(PlanError::LimitExceeded);
                }
                let child = open_child(&dir, &name, &item.stat).map_err(|error| match error {
                    PlanError::Io(error) => PlanError::EntryIo {
                        path: path.clone(),
                        phase: "open directory",
                        error,
                    },
                    other => other,
                })?;
                children.push((child, path, item.stat));
            }
        }
        children.reverse();
        stack.extend(children);
    }
    if !same_snapshot(&root_stat, &fs::fstat(source)?) {
        return Err(PlanError::DirectoryAlias);
    }
    Ok(Plan {
        items,
        source_directories: BTreeMap::new(),
        source_root: root_stat,
        destination_root: fs::fstat(destination)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_name_behavior_is_rejected() {
        assert!(matches!(
            require_name_behavior(NameBehavior::Unknown),
            Err(PlanError::UnknownNameBehavior)
        ));
        assert!(require_name_behavior(NameBehavior::FixtureConventionalAssumption).is_ok());
    }

    #[test]
    fn destination_recheck_rejects_case_alias() {
        let names = BTreeSet::from([b"report.txt".to_vec()]);
        assert!(matches!(
            check_absent(&names, OsStr::new("REPORT.TXT")),
            Err(PlanError::Collision)
        ));
        assert!(check_absent(&names, OsStr::new("other.txt")).is_ok());
        let _: fn(&OwnedFd, &OsStr) -> Result<(), PlanError> = recheck_absent;
    }

    #[test]
    fn case_and_duplicate_mappings_are_rejected() {
        let mut mappings = Mappings::default();
        mappings.insert(&[OsString::from("Report")]).unwrap();
        assert!(matches!(
            mappings.insert(&[OsString::from("report")]),
            Err(PlanError::Collision)
        ));
        assert!(matches!(
            mappings.insert(&[OsString::from("Report")]),
            Err(PlanError::Collision)
        ));
    }

    #[test]
    fn unknown_and_traversal_names_are_rejected() {
        for name in [
            "..",
            "../escape",
            "é",
            "e\u{301}",
            ".hidden",
            "name.",
            "con.txt",
            "LPT1",
        ] {
            assert!(matches!(
                Mappings::default().insert(&[OsString::from(name)]),
                Err(PlanError::UnsupportedName)
            ));
        }
    }
}
