use std::{fs, process::Command};
#[test]
fn json_report_is_versioned_and_redacts_paths() {
    let root = std::env::temp_dir().join(format!("portstride-cli-test-{}", std::process::id()));
    fs::create_dir_all(root.join("destination")).unwrap();
    fs::write(root.join("secret-name"), b"hello").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_portstride"))
        .args([
            "inspect",
            root.join("secret-name").to_str().unwrap(),
            "--destination",
            root.join("destination").to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["api_version"], 3);
    assert_eq!(report["inventory"]["logical_bytes"], 5);
    assert!(!text.contains("secret-name"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_path_display_identifies_nested_findings() {
    let root =
        std::env::temp_dir().join(format!("portstride-cli-path-test-{}", std::process::id()));
    fs::create_dir_all(root.join("source")).unwrap();
    fs::create_dir_all(root.join("dest")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("missing", root.join("source/identifiable-link")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_portstride"))
        .args([
            "inspect",
            root.join("source").to_str().unwrap(),
            "--destination",
            root.join("dest").to_str().unwrap(),
            "--json",
            "--show-paths",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    #[cfg(unix)]
    assert!(text.contains("identifiable-link"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn observed_inspection_reports_readiness_separately() {
    let root = std::env::temp_dir().join(format!("portstride-cli-status-{}", std::process::id()));
    fs::create_dir_all(root.join("dest")).unwrap();
    fs::write(root.join("source"), b"data").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_portstride"))
        .args([
            "inspect",
            root.join("source").to_str().unwrap(),
            "--destination",
            root.join("dest").to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    #[cfg(target_os = "macos")]
    {
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(report["inspection_complete"], true);
    }
    #[cfg(not(target_os = "macos"))]
    {
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(report["inspection_complete"], false);
    }
    assert_eq!(report["api_version"], 3);
    assert_eq!(report["transfer_readiness"], "not_assessed");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn incomplete_and_invalid_invocation_have_distinct_exits() {
    let root =
        std::env::temp_dir().join(format!("portstride-cli-incomplete-{}", std::process::id()));
    fs::create_dir_all(root.join("dest")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_portstride"))
        .args([
            "inspect",
            root.join("missing").to_str().unwrap(),
            "--destination",
            root.join("dest").to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "incomplete");
    assert_eq!(report["inventory"]["complete"], false);
    let invalid = Command::new(env!("CARGO_BIN_EXE_portstride"))
        .arg("inspect")
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn completed_inspection_with_conflict_returns_blocked() {
    let root = std::env::temp_dir().join(format!("portstride-cli-blocked-{}", std::process::id()));
    fs::create_dir_all(root.join("dest")).unwrap();
    fs::write(root.join("source"), b"a").unwrap();
    fs::write(root.join("dest/source"), b"b").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_portstride"))
        .args([
            "inspect",
            root.join("source").to_str().unwrap(),
            "--destination",
            root.join("dest").to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "blocked");
    assert_eq!(report["inspection_complete"], true);
    assert_eq!(report["transfer_readiness"], "not_assessed");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn representative_v3_fixtures_cover_statuses() {
    for (raw, status) in [
        (
            include_str!("../../examples/preflight-v3-complete.json"),
            "complete",
        ),
        (
            include_str!("../../examples/preflight-v3-blocked.json"),
            "blocked",
        ),
        (
            include_str!("../../examples/preflight-v3-incomplete.json"),
            "incomplete",
        ),
    ] {
        let report: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(report["api_version"], 3);
        assert_eq!(report["status"], status);
        assert_eq!(report["transfer_readiness"], "not_assessed");
    }
}

#[test]
fn human_output_does_not_imply_transfer_authorization() {
    let root = std::env::temp_dir().join(format!("portstride-cli-human-{}", std::process::id()));
    fs::create_dir_all(root.join("dest")).unwrap();
    fs::write(root.join("source"), b"a").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_portstride"))
        .args([
            "inspect",
            root.join("source").to_str().unwrap(),
            "--destination",
            root.join("dest").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Transfer readiness: not assessed"));
    fs::remove_dir_all(root).unwrap();
}
