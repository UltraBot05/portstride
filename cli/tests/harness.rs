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
        assert!(report["planned_creation_calls"].as_u64().unwrap() >= 2);
        assert!(report["estimated_required_space"].as_u64().unwrap() >= 1024 * 1024);
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
    let parent = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-edge-{}", std::process::id()));
    fs::create_dir(&parent).unwrap();
    let root = parent.join("fixture");
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
    fs::remove_dir_all(parent).unwrap();
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
    let parent = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-simulated-{}", std::process::id()));
    fs::create_dir(&parent).unwrap();
    let root = parent.join("fixture");
    let out = harness(&["generate", root.to_str().unwrap(), "1", "4", "simulated"]);
    assert!(out.status.success());
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["simulation"]["code"], "DEST_FILE_TOO_LARGE");
    assert_eq!(
        result["simulation"]["file_size"]["limit_kind"],
        "upper_bound"
    );
    assert_eq!(fs::metadata(root.join("file-00000000")).unwrap().len(), 4);
    fs::remove_dir_all(parent).unwrap();
}

#[cfg(unix)]
#[test]
fn generator_rejects_shared_writable_parent_and_accounts_for_extra_files() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-unsafe-parent-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
    let target = root.join("fixture");
    let result = harness(&["generate", target.to_str().unwrap(), "1", "1"]);
    assert!(!result.status.success());
    assert!(!target.exists());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let result = harness(&["generate", target.to_str().unwrap(), "9999", "1", "edge"]);
    assert!(!result.status.success());
    assert!(!target.exists());
    let result = harness(&[
        "generate",
        target.to_str().unwrap(),
        "1",
        "22369622",
        "edge",
    ]);
    assert!(!result.status.success());
    assert!(!target.exists());
    fs::remove_dir(root).unwrap();
}

#[cfg(unix)]
#[test]
fn existing_real_or_symlink_output_is_never_modified() {
    use std::os::unix::fs::symlink;
    let parent = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-existing-output-{}", std::process::id()));
    fs::create_dir(&parent).unwrap();
    let existing = parent.join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("sentinel"), b"unchanged").unwrap();
    let result = harness(&["generate", existing.to_str().unwrap(), "1", "10"]);
    assert!(!result.status.success());
    let link = parent.join("link");
    symlink(&existing, &link).unwrap();
    let result = harness(&["generate", link.to_str().unwrap(), "1", "10"]);
    assert!(!result.status.success());
    assert_eq!(fs::read(existing.join("sentinel")).unwrap(), b"unchanged");
    assert_eq!(fs::read_dir(&existing).unwrap().count(), 1);
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn fixture_layouts_match_reported_logical_limits() {
    let parent = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-layout-{}", std::process::id()));
    fs::create_dir(&parent).unwrap();
    for scenario in ["flat", "nested", "wide", "edge"] {
        let root = parent.join(scenario);
        let output = harness(&["generate", root.to_str().unwrap(), "7", "11", scenario]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut dirs = vec![root.clone()];
        if scenario == "nested" {
            for _ in 0..4 {
                dirs.push(dirs.last().unwrap().join("nested"));
            }
        } else if scenario == "wide" {
            for i in 0..4 {
                dirs.push(root.join(format!("dir-{i}")));
            }
        }
        for (i, dir) in dirs.iter().enumerate() {
            assert!(dir.is_dir(), "missing directory {i}");
        }
        for i in 0..7 {
            assert_eq!(
                fs::metadata(dirs[i % dirs.len()].join(format!("file-{i:08}")))
                    .unwrap()
                    .len(),
                11
            );
        }
        let expected_files = if scenario == "edge" { 9 } else { 7 };
        assert_eq!(report["max_files"], expected_files);
        assert_eq!(report["max_logical_bytes"], expected_files * 11);
        if scenario == "edge" {
            for name in ["input-a", "input-b"] {
                assert_eq!(
                    fs::metadata(root.join(name).join("conflict-λ"))
                        .unwrap()
                        .len(),
                    11
                );
            }
            #[cfg(unix)]
            {
                assert_eq!(
                    fs::read_link(root.join("valid-link")).unwrap(),
                    std::path::Path::new("input-a/conflict-λ")
                );
                assert_eq!(
                    fs::read_link(root.join("broken-link")).unwrap(),
                    std::path::Path::new("missing")
                );
            }
        }
    }
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn benchmark_statistics_match_measured_runs_only() {
    let parent = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-stats-{}", std::process::id()));
    fs::create_dir(&parent).unwrap();
    let source = parent.join("source");
    assert!(
        harness(&["generate", source.to_str().unwrap(), "2", "4"])
            .status
            .success()
    );
    for count in ["3", "4"] {
        let output = harness(&[
            "benchmark",
            source.to_str().unwrap(),
            parent.to_str().unwrap(),
            count,
            "2",
            "flat",
        ]);
        assert!(output.status.success());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["warmups"].as_array().unwrap().len(), 2);
        let mut timings: Vec<f64> = report["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|run| run["elapsed_ns"].as_f64().unwrap())
            .collect();
        assert_eq!(timings.len(), count.parse::<usize>().unwrap());
        timings.sort_by(f64::total_cmp);
        let mean = timings.iter().sum::<f64>() / timings.len() as f64;
        let median = if timings.len().is_multiple_of(2) {
            (timings[timings.len() / 2 - 1] + timings[timings.len() / 2]) / 2.0
        } else {
            timings[timings.len() / 2]
        };
        let stddev =
            (timings.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / timings.len() as f64).sqrt();
        let stats = &report["statistics"];
        assert_eq!(stats["min_ns"].as_f64().unwrap(), timings[0]);
        assert_eq!(stats["max_ns"].as_f64().unwrap(), *timings.last().unwrap());
        assert_eq!(stats["median_ns"].as_f64().unwrap(), median);
        assert!((stats["standard_deviation_ns"].as_f64().unwrap() - stddev).abs() < 0.000001);
        let text = String::from_utf8(output.stdout).unwrap().to_lowercase();
        assert!(!text.contains("throughput"));
    }
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn generator_failure_has_structured_error() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("portstride-error-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let output = harness(&["generate", root.to_str().unwrap(), "1", "1"]);
    assert!(!output.status.success());
    let failure: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(failure["schema_version"], 1);
    assert_eq!(failure["kind"], "fixture_generation");
    assert_eq!(failure["status"], "incomplete");
    assert!(failure["error"].as_str().unwrap().contains("Cannot create"));
    fs::remove_dir(root).unwrap();
}
