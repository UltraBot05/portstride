#[path = "../../test-support/fixture.rs"]
mod test_fixture;
use std::{fs, process::Command};
use test_fixture::fixture;
#[test]
fn json_report_is_versioned_and_redacts_paths() {
    let root = fixture();
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
}

#[test]
fn explicit_path_display_identifies_nested_findings() {
    let root = fixture();
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
}

#[test]
fn observed_inspection_reports_readiness_separately() {
    let root = fixture();
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
}

#[test]
fn incomplete_and_invalid_invocation_have_distinct_exits() {
    let root = fixture();
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
}

#[cfg(target_os = "macos")]
#[test]
fn completed_inspection_with_conflict_returns_blocked() {
    let root = fixture();
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
    let root = fixture();
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
}

#[cfg(target_os = "macos")]
#[test]
fn denied_destination_is_reported_in_json_without_exposing_paths() {
    use std::os::unix::fs::PermissionsExt;
    let root = fixture();
    let dest = root.join("destination");
    fs::create_dir(&dest).unwrap();
    fs::write(root.join("source"), b"hello").unwrap();
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o500)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_portstride"))
        .args([
            "inspect",
            root.join("source").to_str().unwrap(),
            "--destination",
            dest.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o700)).unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["destination"]["writable"], false);
    assert!(
        report["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "ACCESS_DENIED")
    );
    assert!(!text.contains(root.to_str().unwrap()));
}
