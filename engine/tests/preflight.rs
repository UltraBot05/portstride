use portstride_engine::{Capabilities, inspect};
use std::fs;

#[path = "../../test-support/fixture.rs"]
mod test_fixture;
use test_fixture::fixture;

#[cfg(unix)]
fn bound_capabilities(destination: &std::path::Path) -> Capabilities {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(destination).unwrap();
    let mut caps = Capabilities::unknown();
    caps.probe_identity = Some((metadata.dev() as u64, metadata.ino()));
    caps
}

#[test]
fn nested_sources_are_counted_without_following_links() {
    let root = fixture();
    let source = root.join("source");
    let dest = root.join("dest");
    fs::create_dir_all(source.join("empty")).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(source.join("a"), b"hello").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("missing", source.join("broken")).unwrap();
    let report = inspect(&[source], &dest, Capabilities::unknown());
    assert_eq!(report.inventory.files, 1);
    assert_eq!(report.inventory.directories, 2);
    assert_eq!(report.inventory.logical_bytes, 5);
    assert!(report.inventory.complete);
    #[cfg(unix)]
    assert_eq!(report.inventory.symlinks, 1);
}

#[test]
fn missing_source_is_incomplete_and_blocked() {
    let root = fixture();
    let report = inspect(&[root.join("missing")], &root, Capabilities::unknown());
    assert!(!report.inventory.complete);
    assert!(report.issues.iter().any(|i| i.severity == "blocker"));
}

#[test]
fn fat32_limit_is_per_file_not_total() {
    use portstride_engine::file_size_issue;
    let max = u32::MAX as u64;
    assert!(file_size_issue(max, Some(max)).is_none());
    let finding = file_size_issue(8_804_682_956, Some(max)).unwrap();
    assert_eq!(finding.code, "DEST_FILE_TOO_LARGE");
    assert_eq!(finding.severity, "blocker");
    assert!(
        finding
            .remedies
            .iter()
            .any(|r| r.action == "backup_then_reformat" && r.risk == "destructive")
    );
    assert!(file_size_issue(8_804_682_956, None).is_none());
}

#[test]
fn nested_destination_and_readonly_are_blocked() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    let mut caps = bound_capabilities(&dest);
    caps.writable = Some(false);
    let report = inspect(std::slice::from_ref(&root), &dest, caps);
    assert!(report.issues.iter().any(|i| i.code == "ACCESS_DENIED"));
    assert!(report.issues.iter().any(|i| i.code == "PATH_UNSUPPORTED"));
}

#[test]
fn multiple_sources_capacity_and_name_collision_are_reported() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    fs::write(root.join("Foo"), b"123").unwrap();
    fs::write(root.join("foo"), b"456").ok();
    let mut caps = bound_capabilities(&dest);
    caps.available_bytes = Some(2);
    caps.case_sensitive = Some(false);
    let report = inspect(&[root.join("Foo"), root.join("Foo")], &dest, caps);
    assert_eq!(report.inventory.files, 2);
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == "INSUFFICIENT_CAPACITY")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == "CASE_NAME_COLLISION")
    );
}

#[test]
fn distinct_case_names_collide_on_case_insensitive_destination() {
    let root = fixture();
    fs::create_dir_all(root.join("one")).unwrap();
    fs::create_dir_all(root.join("two")).unwrap();
    fs::create_dir_all(root.join("dest")).unwrap();
    fs::write(root.join("one/Foo"), b"a").unwrap();
    fs::write(root.join("two/foo"), b"b").unwrap();
    let mut caps = bound_capabilities(&root.join("dest"));
    caps.case_sensitive = Some(false);
    let report = inspect(
        &[root.join("one/Foo"), root.join("two/foo")],
        &root.join("dest"),
        caps,
    );
    let collision = report
        .issues
        .iter()
        .find(|i| i.code == "CASE_NAME_COLLISION")
        .unwrap();
    assert_eq!(collision.affected_paths.len(), 2);
}

#[test]
fn aggregate_above_fat_limit_has_no_per_file_violation() {
    let sizes = [3_000_000_000, 3_000_000_000];
    assert!(sizes.iter().sum::<u64>() > u32::MAX as u64);
    assert!(
        sizes
            .iter()
            .all(|&n| portstride_engine::file_size_issue(n, Some(u32::MAX as u64)).is_none())
    );
}

#[test]
fn fat32_example_matches_specified_code_limit_and_risk() {
    let examples: serde_json::Value = serde_json::from_str(include_str!(
        "../../adaptive-transfer-spec/examples/preflight-examples.json"
    ))
    .unwrap();
    let case = &examples["cases"][0];
    let bytes = case["input"]["file_bytes"].as_u64().unwrap();
    let limit = case["expected"]["max_file_bytes"].as_u64().unwrap();
    let finding = portstride_engine::file_size_issue(bytes, Some(limit)).unwrap();
    assert_eq!(finding.code, case["expected"]["code"].as_str().unwrap());
    assert_eq!(finding.severity, "blocker");
    for expected in case["expected"]["remedies"].as_array().unwrap() {
        assert!(
            finding
                .remedies
                .iter()
                .any(|r| r.action == expected["action"].as_str().unwrap()
                    && r.risk == expected["risk"].as_str().unwrap())
        );
    }
}

#[cfg(unix)]
#[test]
fn mismatched_probe_volume_does_not_drive_capacity_or_readonly_findings() {
    let root = fixture();
    fs::create_dir_all(root.join("dest")).unwrap();
    fs::write(root.join("source"), b"data").unwrap();
    let mut caps = Capabilities::unknown();
    caps.volume_id = Some("device:999999999999999".into());
    caps.probe_identity = Some((999999999999999, 1));
    caps.writable = Some(false);
    caps.available_bytes = Some(0);
    let report = inspect(&[root.join("source")], &root.join("dest"), caps);
    assert!(report.issues.iter().any(|i| i.code == "FS_ERROR"));
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.code == "DEST_READ_ONLY" || i.code == "INSUFFICIENT_CAPACITY")
    );
    assert!(report.destination.volume_id.is_none());
}

#[test]
fn oversized_file_finding_counts_the_affected_file() {
    let finding = portstride_engine::file_size_issue(10, Some(5)).unwrap();
    assert_eq!(finding.affected_count, 1);
}

#[cfg(unix)]
#[test]
fn destination_entry_symlink_is_a_conflict_without_descendant_traversal() {
    use std::os::unix::fs::symlink;
    let root = fixture();
    fs::create_dir_all(root.join("source/child")).unwrap();
    fs::create_dir_all(root.join("dest")).unwrap();
    fs::create_dir_all(root.join("outside")).unwrap();
    fs::write(root.join("outside/child"), b"secret").unwrap();
    symlink(root.join("outside"), root.join("dest/source")).unwrap();
    let report = inspect(
        &[root.join("source")],
        &root.join("dest"),
        Capabilities::unknown(),
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == "PATH_UNSUPPORTED" && i.severity == "blocker")
    );
    assert_eq!(report.inventory.files, 0);
}

#[cfg(unix)]
#[test]
fn unopened_destination_discards_unbound_probe_results() {
    let root = fixture();
    fs::write(root.join("source"), b"data").unwrap();
    let mut caps = Capabilities::unknown();
    caps.writable = Some(false);
    caps.available_bytes = Some(0);
    let report = inspect(&[root.join("source")], &root.join("missing"), caps);
    assert!(report.issues.iter().any(|i| i.code == "FS_ERROR"));
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.code == "DEST_READ_ONLY" || i.code == "INSUFFICIENT_CAPACITY")
    );
}

#[test]
fn oversized_finding_does_not_claim_an_upper_bound_is_exact() {
    let finding = portstride_engine::file_size_issue(10, Some(5)).unwrap();
    assert!(finding.evidence.contains("upper bound"));
}

#[test]
fn complete_inspection_allows_unknown_optional_capabilities() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    fs::write(root.join("source"), b"hello").unwrap();
    let mut caps = bound_capabilities(&dest);
    caps.filesystem = Some("examplefs".into());
    caps.available_bytes = Some(100);
    caps.mount_read_only = Some(false);
    let report = inspect(&[root.join("source")], &dest, caps);
    assert!(report.inspection_complete);
    assert_eq!(report.status, "complete");
    assert_eq!(report.transfer_readiness, "not_assessed");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == "CAPABILITY_UNVERIFIED" && i.category == "unknown")
    );
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.code == "METADATA_UNSUPPORTED")
    );
}

#[test]
fn missing_required_probe_is_incomplete() {
    let root = fixture();
    fs::create_dir_all(root.join("dest")).unwrap();
    fs::write(root.join("source"), b"hello").unwrap();
    let report = inspect(
        &[root.join("source")],
        &root.join("dest"),
        bound_capabilities(&root.join("dest")),
    );
    assert!(!report.inspection_complete);
    assert_eq!(report.status, "incomplete");
}

#[test]
fn file_size_evidence_preserves_limit_kind() {
    use portstride_engine::{LimitKind, file_size_issue_with_kind};
    for kind in [LimitKind::Exact, LimitKind::UpperBound] {
        let issue = file_size_issue_with_kind(10, Some(5), Some(kind)).unwrap();
        let json = serde_json::to_value(issue).unwrap();
        assert_eq!(
            json["file_size"]["limit_kind"],
            serde_json::to_value(kind).unwrap()
        );
        assert_eq!(json["file_size"]["file_bytes"], 10);
    }
}

#[test]
fn unclassified_file_limit_does_not_create_a_blocker() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    fs::write(root.join("source"), b"hello").unwrap();
    let mut caps = bound_capabilities(&dest);
    caps.max_file_bytes = Some(2);
    let report = inspect(&[root.join("source")], &dest, caps);
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.code == "DEST_FILE_TOO_LARGE")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.category == "inspection_failure")
    );
}

#[test]
fn unclassified_file_size_cannot_emit_a_blocker() {
    assert!(portstride_engine::file_size_issue_with_kind(10, Some(5), None).is_none());
}

#[cfg(unix)]
#[test]
fn verified_symlink_support_does_not_report_it_as_unknown() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    std::os::unix::fs::symlink("missing", root.join("link")).unwrap();
    let mut caps = bound_capabilities(&dest);
    caps.supports_symlink = Some(true);
    let report = inspect(&[root.join("link")], &dest, caps);
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.code == "SYMLINK_CAPABILITY_UNVERIFIED")
    );
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == "SYMLINK_POLICY_UNRESOLVED")
    );
}

#[cfg(unix)]
#[test]
fn ascii_names_collide_under_non_ascii_parent_with_simulated_case_insensitivity() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    for parent in ["one", "two"] {
        fs::create_dir_all(root.join(parent).join("é")).unwrap();
    }
    fs::write(root.join("one/é/Foo"), b"a").unwrap();
    fs::write(root.join("two/é/foo"), b"b").unwrap();
    let mut caps = bound_capabilities(&dest);
    caps.case_sensitive = Some(false);
    let report = inspect(&[root.join("one/é"), root.join("two/é")], &dest, caps);
    assert!(report.issues.iter().any(|i| i.code == "CASE_NAME_COLLISION"
        && i.affected_paths.iter().all(|p| p.contains("/entry["))));
}

#[test]
fn generic_uncertainty_does_not_recommend_changing_destination() {
    let root = fixture();
    let report = inspect(&[root.join("missing")], &root, Capabilities::unknown());
    let warning = report
        .issues
        .iter()
        .find(|i| i.code == "CAPABILITY_UNVERIFIED")
        .unwrap();
    assert_eq!(warning.confidence, "unknown");
    assert!(
        !warning
            .remedies
            .iter()
            .any(|r| r.action == "choose_destination")
    );
}

#[cfg(unix)]
#[test]
fn fat_family_forbidden_component_is_reported_as_driver_dependent() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(root.join("source")).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(root.join("source/a:b"), b"data").unwrap();
    for filesystem in ["exfat", "msdos"] {
        let mut caps = bound_capabilities(&dest);
        caps.filesystem = Some(filesystem.into());
        let report = inspect(&[root.join("source")], &dest, caps);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == "FILENAME_RESTRICTION_UNVERIFIED"
                    && i.severity == "warning"
                    && i.confidence == "unknown"
                    && i.affected_paths == ["source[1]/entry[1]"])
        );
    }
}

#[cfg(unix)]
#[test]
fn fat_name_warning_does_not_apply_windows_reserved_names_or_other_filesystems() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    for name in ["CON", "valid.txt", "a:b"] {
        fs::write(root.join(name), b"data").unwrap();
    }
    for (filesystem, name) in [
        ("exfat", "CON"),
        ("msdos", "valid.txt"),
        ("apfs", "a:b"),
        ("unknownfs", "a:b"),
    ] {
        let mut caps = bound_capabilities(&dest);
        caps.filesystem = Some(filesystem.into());
        let report = inspect(&[root.join(name)], &dest, caps);
        assert!(
            !report
                .issues
                .iter()
                .any(|i| i.code == "FILENAME_RESTRICTION_UNVERIFIED"),
            "{filesystem} {name}"
        );
    }
}

#[cfg(unix)]
#[test]
fn windows_portability_risks_are_scoped_to_fat_family() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    for name in [
        "CON", "con.txt", "COM1.log", "LPT9", "a:b", "trail.", "trail ",
    ] {
        fs::write(root.join(name), b"data").unwrap();
        for filesystem in ["exfat", "msdos", "apfs"] {
            let mut caps = bound_capabilities(&dest);
            caps.filesystem = Some(filesystem.into());
            let report = inspect(&[root.join(name)], &dest, caps);
            if filesystem == "apfs" {
                assert!(
                    !report
                        .issues
                        .iter()
                        .any(|i| i.code == "WINDOWS_FILENAME_PORTABILITY"),
                    "{name}"
                );
                continue;
            }
            assert!(
                report
                    .issues
                    .iter()
                    .any(|i| i.code == "WINDOWS_FILENAME_PORTABILITY"
                        && i.severity == "warning"
                        && i.category == "unknown"
                        && i.confidence == "unknown"),
                "{name}"
            );
            assert!(
                !report.issues.iter().any(|i| i.severity == "blocker"),
                "{name}"
            );
        }
    }
    for name in ["console.txt", "COM10", "normal.txt"] {
        fs::write(root.join(name), b"data").unwrap();
        let mut caps = bound_capabilities(&dest);
        caps.filesystem = Some("exfat".into());
        let report = inspect(&[root.join(name)], &dest, caps);
        assert!(
            !report
                .issues
                .iter()
                .any(|i| i.code == "WINDOWS_FILENAME_PORTABILITY"),
            "{name}"
        );
    }
}

#[cfg(unix)]
#[test]
fn simulated_folder_access_and_mount_state_have_distinct_diagnostics() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    fs::write(root.join("source"), b"data").unwrap();
    for (mount, writable, expected) in [
        (Some(true), Some(false), Some("DEST_READ_ONLY")),
        (Some(false), Some(false), Some("ACCESS_DENIED")),
        (Some(false), Some(true), None),
        (Some(false), None, None),
    ] {
        let mut caps = bound_capabilities(&dest);
        caps.mount_read_only = mount;
        caps.writable = writable;
        let report = inspect(&[root.join("source")], &dest, caps);
        let access: Vec<_> = report
            .issues
            .iter()
            .filter(|i| matches!(i.code, "DEST_READ_ONLY" | "ACCESS_DENIED"))
            .collect();
        match expected {
            Some(code) => assert_eq!(access.iter().map(|i| i.code).collect::<Vec<_>>(), [code]),
            None => assert!(access.is_empty()),
        }
    }
}

#[cfg(unix)]
#[test]
fn windows_portability_warning_serializes_without_paths_or_blocking() {
    let root = fixture();
    let dest = root.join("dest");
    fs::create_dir_all(&dest).unwrap();
    fs::write(root.join("CON"), b"data").unwrap();
    let mut caps = bound_capabilities(&dest);
    caps.filesystem = Some("exfat".into());
    caps.available_bytes = Some(1000);
    caps.mount_read_only = Some(false);
    let report = inspect(&[root.join("CON")], &dest, caps);
    let text = serde_json::to_string(&report).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["status"], "complete");
    assert!(
        json["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "WINDOWS_FILENAME_PORTABILITY" && i["severity"] == "warning")
    );
    assert!(!text.contains(root.to_str().unwrap()));
}
