//! Retained private fixtures. Cleanup requires separate identity review.
#[cfg(unix)]
pub fn fixture() -> std::path::PathBuf {
    use rustix::fs::{self, AtFlags, Mode, OFlags};
    use std::io::Write;

    let parent_path = std::env::temp_dir().canonicalize().unwrap();
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let parent = fs::open(&parent_path, flags, Mode::empty()).unwrap();
    let parent_identity = fs::fstat(&parent).unwrap();
    assert_eq!(parent_identity.st_uid, rustix::process::geteuid().as_raw());
    assert_eq!(
        parent_identity.st_mode & 0o077,
        0,
        "temporary parent must be private"
    );
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).unwrap();
    let id: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let name = format!("portstride-test-{id}");
    fs::mkdirat(&parent, name.as_str(), Mode::from_bits_truncate(0o700)).unwrap();
    let named = fs::statat(&parent, name.as_str(), AtFlags::SYMLINK_NOFOLLOW).unwrap();
    let root = fs::openat(&parent, name.as_str(), flags, Mode::empty()).unwrap();
    let held = fs::fstat(&root).unwrap();
    assert_eq!((named.st_dev, named.st_ino), (held.st_dev, held.st_ino));
    let mut manifest = std::fs::File::from(
        fs::openat(
            &root,
            "manifest.json",
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .unwrap(),
    );
    let manifest_identity = fs::fstat(&manifest).unwrap();
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
    eprintln!(
        "Retained test fixture: {}",
        parent_path.join(&name).display()
    );
    parent_path.join(name)
}

#[cfg(not(unix))]
pub fn fixture() -> std::path::PathBuf {
    panic!("private fixture setup is unsupported on this platform")
}
