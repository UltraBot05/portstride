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
