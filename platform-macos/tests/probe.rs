#[cfg(target_os = "macos")]
#[path = "../../test-support/fixture.rs"]
mod test_fixture;
#[test]
fn probes_existing_directory_without_writing() {
    let caps = portstride_platform_macos::probe(std::path::Path::new("/"));
    #[cfg(target_os = "macos")]
    {
        assert!(caps.filesystem.is_some());
        assert!(caps.available_bytes.is_some());
        assert!(caps.volume_id.is_some());
    }
}

#[cfg(target_os = "macos")]
#[test]
fn probes_case_sensitivity_for_root_volume() {
    let caps = portstride_platform_macos::probe(std::path::Path::new("/"));
    assert!(caps.case_sensitive.is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn observes_permissions_on_selected_directory() {
    use std::{
        fs,
        os::unix::fs::{MetadataExt, PermissionsExt},
    };

    let path = test_fixture::fixture();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let metadata = fs::metadata(&path).unwrap();
    let allowed = portstride_platform_macos::probe(&path);
    assert_eq!(
        allowed.probe_identity,
        Some((metadata.dev() as u64, metadata.ino()))
    );
    assert_eq!(allowed.writable, Some(true));

    fs::set_permissions(&path, fs::Permissions::from_mode(0o500)).unwrap();
    let denied = portstride_platform_macos::probe(&path);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(denied.probe_identity, allowed.probe_identity);
    assert_eq!(denied.writable, Some(false));
}
