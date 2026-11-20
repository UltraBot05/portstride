use std::{fs, process::Command};

fn harness(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_portstride-harness"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn generated_fixtures_are_reproducible_and_existing_roots_are_preserved() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-harness-fixture-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let a = root.join("a");
    let b = root.join("b");
    for path in [&a, &b] {
        assert!(
            harness(&["generate", path.to_str().unwrap(), "3", "1025"])
                .status
                .success()
        );
    }
    for i in 0..3 {
        let name = format!("file-{i:08}");
        assert_eq!(
            fs::read(a.join(&name)).unwrap(),
            fs::read(b.join(name)).unwrap()
        );
    }
    assert!(
        !harness(&["generate", a.to_str().unwrap(), "1", "1"])
            .status
            .success()
    );
    assert_eq!(fs::metadata(a.join("file-00000000")).unwrap().len(), 1025);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn benchmark_reports_each_complete_scan_and_rejects_missing_sources() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-harness-bench-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let source = root.join("source");
    let dest = root.join("destination");
    fs::create_dir(&dest).unwrap();
    assert!(
        harness(&["generate", source.to_str().unwrap(), "2", "10"])
            .status
            .success()
    );
    let result = harness(&[
        "benchmark",
        source.to_str().unwrap(),
        dest.to_str().unwrap(),
        "2",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["runs"].as_array().unwrap().len(), 2);
    assert_eq!(report["runs"][0]["files"], 2);
    assert_eq!(report["runs"][0]["logical_bytes"], 20);
    assert!(!String::from_utf8_lossy(&result.stdout).contains(source.to_str().unwrap()));
    assert!(
        !harness(&[
            "benchmark",
            root.join("missing").to_str().unwrap(),
            dest.to_str().unwrap(),
            "1"
        ])
        .status
        .success()
    );
    let failed = harness(&[
        "benchmark",
        root.join("missing").to_str().unwrap(),
        dest.to_str().unwrap(),
        "1",
    ]);
    let failure: serde_json::Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(failure["status"], "incomplete");
    assert!(failure["failed_run"]["elapsed_ns"].is_number());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn generator_rejects_unbounded_requests_before_creating_directory() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-harness-limit-{}", std::process::id()));
    let result = harness(&["generate", root.to_str().unwrap(), "1", "1073741824"]);
    assert!(!result.status.success());
    assert!(!root.exists());
}

#[test]
fn scenario_fixtures_and_benchmark_statistics_are_reported() {
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "portstride-harness-scenarios-{}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    for scenario in [
        "nested",
        "wide",
        "edge",
        "sparse",
        "simulated",
        "permission",
    ] {
        let path = root.join(scenario);
        let output = harness(&["generate", path.to_str().unwrap(), "2", "16", scenario]);
        assert!(
            output.status.success(),
            "{scenario}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["scenario"], scenario);
    }
    let output = harness(&[
        "benchmark",
        root.join("nested").to_str().unwrap(),
        root.to_str().unwrap(),
        "2",
        "1",
        "nested",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["warmups"].as_array().unwrap().len(), 1);
    assert_eq!(report["runs"].as_array().unwrap().len(), 2);
    assert!(report["statistics"]["median_ns"].is_number());
    assert_eq!(report["cache_state"], "uncontrolled");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_scenarios_and_symlinked_parents_are_rejected_before_creation() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-harness-invalid-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let path = root.join("invalid");
    assert!(
        !harness(&["generate", path.to_str().unwrap(), "2", "16", "unknown"])
            .status
            .success()
    );
    assert!(!path.exists());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&root, root.join("link")).unwrap();
        assert!(
            !harness(&[
                "generate",
                root.join("link/fixture").to_str().unwrap(),
                "1",
                "1"
            ])
            .status
            .success()
        );
        assert!(!root.join("fixture").exists());
        fs::create_dir(root.join("actual")).unwrap();
        std::os::unix::fs::symlink(root.join("actual"), root.join("intermediate")).unwrap();
        fs::create_dir(root.join("actual/child")).unwrap();
        assert!(
            !harness(&[
                "generate",
                root.join("intermediate/child/fixture").to_str().unwrap(),
                "1",
                "1"
            ])
            .status
            .success()
        );
        assert!(!root.join("actual/child/fixture").exists());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn edge_fixture_has_real_links_empty_directory_and_conflicting_names() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-edge-{}", std::process::id()));
    let output = harness(&["generate", root.to_str().unwrap(), "2", "8", "edge"]);
    assert!(output.status.success());
    assert_eq!(fs::read_dir(root.join("empty")).unwrap().count(), 0);
    assert!(root.join("input-a/conflict-λ").is_file());
    assert!(root.join("input-b/conflict-λ").is_file());
    #[cfg(unix)]
    {
        assert!(
            fs::symlink_metadata(root.join("valid-link"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(root.join("valid-link").exists());
        assert!(
            fs::symlink_metadata(root.join("broken-link"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!root.join("broken-link").exists());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_benchmark_parameters_and_missing_destination_are_rejected() {
    let root = std::env::temp_dir().canonicalize().unwrap();
    assert!(
        !harness(&[
            "benchmark",
            root.to_str().unwrap(),
            root.to_str().unwrap(),
            "0"
        ])
        .status
        .success()
    );
    assert!(
        !harness(&[
            "benchmark",
            root.to_str().unwrap(),
            root.join("not-a-portstride-destination").to_str().unwrap(),
            "1"
        ])
        .status
        .success()
    );
}

#[test]
fn fat_simulation_reports_metadata_only_blocker() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-simulated-{}", std::process::id()));
    let out = harness(&["generate", root.to_str().unwrap(), "1", "4", "simulated"]);
    assert!(out.status.success());
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["simulation"]["code"], "DEST_FILE_TOO_LARGE");
    assert_eq!(
        result["simulation"]["file_size"]["limit_kind"],
        "upper_bound"
    );
    assert_eq!(fs::metadata(root.join("file-00000000")).unwrap().len(), 4);
    fs::remove_dir_all(root).unwrap();
}
