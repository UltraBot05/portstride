use portstride_engine::Capabilities;
use std::path::Path;

/// Probe a single opened destination directory without creating files.
#[cfg(target_os = "macos")]
pub fn probe(path: &Path) -> Capabilities {
    use std::{
        ffi::CStr,
        fs::OpenOptions,
        os::{
            fd::AsRawFd,
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
    };
    let mut caps = Capabilities::unknown();
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(e) => {
            caps.uncertainty
                .push(format!("Cannot open destination for probing: {}", e.kind()));
            return caps;
        }
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(e) => {
            caps.uncertainty
                .push(format!("Cannot identify destination: {}", e.kind()));
            return caps;
        }
    };
    caps.probe_identity = Some((metadata.dev() as u64, metadata.ino()));
    caps.volume_id = Some(format!("device:{}", metadata.dev()));
    let mut raw = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: file owns a live descriptor and fstatfs initializes raw on success.
    if unsafe { libc::fstatfs(file.as_raw_fd(), raw.as_mut_ptr()) } != 0 {
        caps.uncertainty.push(format!(
            "Mount probe failed: {}",
            std::io::Error::last_os_error().kind()
        ));
        return caps;
    }
    // SAFETY: fstatfs returned success above.
    let raw = unsafe { raw.assume_init() };
    // SAFETY: macOS statfs returns a NUL-terminated filesystem name.
    let fs_name = unsafe { CStr::from_ptr(raw.f_fstypename.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    caps.available_bytes = raw.f_bavail.checked_mul(u64::from(raw.f_bsize));
    caps.mount_read_only = Some(raw.f_flags & libc::MNT_RDONLY as u32 != 0);
    if raw.f_flags & libc::MNT_RDONLY as u32 != 0 {
        caps.writable = Some(false);
    }
    if fs_name.eq_ignore_ascii_case("msdos") {
        caps.max_file_bytes = Some(u32::MAX as u64);
        caps.max_file_bytes_kind = Some(portstride_engine::LimitKind::UpperBound);
        caps.uncertainty.push("The msdos mount may be FAT12 or FAT16, with a lower file-size limit than the reported upper bound.".into());
    }
    // SAFETY: file holds a valid descriptor; -1 is treated as unknown.
    let case = unsafe { libc::fpathconf(file.as_raw_fd(), libc::_PC_CASE_SENSITIVE) };
    caps.case_sensitive = match case {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    caps.filesystem = Some(fs_name);
    caps.uncertainty
        .push("Folder write permission, metadata and name semantics are unverified.".into());
    caps
}
#[cfg(not(target_os = "macos"))]
pub fn probe(_path: &Path) -> Capabilities {
    Capabilities::unknown()
}
