use std::{
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if std::env::args_os()
                .nth(1)
                .is_some_and(|arg| arg == "generate")
            {
                eprintln!("{}", generation_error(&error));
            } else {
                eprintln!("{error}");
            }
            ExitCode::from(2)
        }
    }
}

fn generation_error(error: &str) -> serde_json::Value {
    serde_json::json!({"schema_version": 1, "kind": "fixture_generation", "status": "incomplete", "error": error})
}

#[cfg(unix)]
fn validate_generator_uid(uid: u32) -> Result<(), String> {
    if uid == 0 {
        Err("Fixture generation must not run with elevated privileges.".into())
    } else {
        Ok(())
    }
}

#[cfg(all(test, unix))]
type TestHook = Box<dyn FnMut(&str) -> Result<(), String>>;

#[cfg(all(test, unix))]
thread_local! {
    static TEST_HOOK: std::cell::RefCell<Option<TestHook>> = std::cell::RefCell::new(None);
}

#[cfg(all(test, unix))]
fn test_hook(stage: &str) -> Result<(), String> {
    TEST_HOOK.with(|hook| match hook.borrow_mut().as_mut() {
        Some(hook) => hook(stage),
        None => Ok(()),
    })
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command, root, files, size] if command == "generate" => {
            generate_request(Path::new(root), files, size, "flat")
        }
        [command, root, files, size, scenario] if command == "generate" => {
            let scenario = scenario.to_str().ok_or("Invalid scenario.")?;
            generate_request(Path::new(root), files, size, scenario)
        }
        [command, source, destination, runs] if command == "benchmark" => {
            benchmark_request(source, destination, runs, 0, "unspecified")
        }
        [command, source, destination, runs, warmups, scenario] if command == "benchmark" => {
            benchmark_request(source, destination, runs, number(warmups, "warm-up count")?, scenario.to_str().ok_or("Invalid scenario.")?)
        }
        _ => Err("Usage: portstride-harness generate <new-directory> <file-count> <bytes-per-file> [flat|nested|wide|edge|sparse|simulated|permission] | benchmark <source> <destination-directory> <runs> [warmups scenario]".into()),
    }
}

fn number(value: &std::ffi::OsStr, name: &str) -> Result<u64, String> {
    value
        .to_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("Invalid {name}."))
}

fn generate_request(
    root: &Path,
    files: &std::ffi::OsStr,
    size: &std::ffi::OsStr,
    scenario: &str,
) -> Result<(), String> {
    let count = number(files, "file count")?;
    let size = number(size, "file size")?;
    if ![
        "flat",
        "nested",
        "wide",
        "edge",
        "sparse",
        "simulated",
        "permission",
    ]
    .contains(&scenario)
    {
        return Err("Unknown fixture scenario.".into());
    }
    let extra = match scenario {
        "edge" => 2,
        "sparse" => 1,
        _ => 0,
    };
    let max_files = count.checked_add(extra).ok_or("File count overflow.")?;
    let total = max_files
        .checked_mul(size)
        .ok_or("Total size exceeds u64.")?;
    if max_files > 10_000 || size > 64 * 1024 * 1024 || total > 64 * 1024 * 1024 {
        return Err(
            "Fixture limit: at most 10,000 files, 64 MiB per file and 64 MiB logical bytes.".into(),
        );
    }
    let estimated_space = total + max_files * 16 * 1024 + 1024 * 1024;
    let writes_per_file = size.div_ceil(8192);
    let written_files = count + if scenario == "edge" { 2 } else { 0 };
    let max_operations = max_files
        + written_files * writes_per_file
        + 5
        + if scenario == "edge" { 2 } else { 0 }
        + if scenario == "sparse" { 1 } else { 0 };
    generate(root, count, size, scenario, estimated_space)?;
    let coverage = match scenario {
        "sparse" => "set_len requested; physical sparseness not verified",
        "simulated" => {
            "simulated metadata and FAT-family upper bound; no filesystem capability or format was modified"
        }
        "permission" => {
            "skipped: permission errors are not reliably reproducible under elevated privileges"
        }
        "edge" if !cfg!(unix) => "symlink creation skipped: unsupported platform",
        _ => "real directory fixture",
    };
    let simulation = if scenario == "simulated" {
        let finding = portstride_engine::file_size_issue_with_kind(
            8_804_682_956,
            Some(u32::MAX as u64),
            Some(portstride_engine::LimitKind::UpperBound),
        );
        serde_json::to_value(finding).map_err(|e| format!("Cannot serialize simulation: {e}"))?
    } else {
        serde_json::Value::Null
    };
    println!(
        "{}",
        serde_json::json!({"simulation": simulation, "scenario": scenario, "requested_files": count, "bytes_per_file": size, "max_files": max_files, "max_logical_bytes": total, "max_directories": 5, "max_depth": 5, "max_symlinks": if scenario == "edge" && cfg!(unix) { 2 } else { 0 }, "planned_creation_calls": max_operations, "estimated_required_space": estimated_space, "safety_assumption": "Parent and ancestors, including ACLs, are trusted against modification by other users; no concurrent same-user modification", "symlinks_followed": false, "coverage": coverage})
    );
    Ok(())
}

#[cfg(unix)]
fn generate(
    root: &Path,
    count: u64,
    size: u64,
    scenario: &str,
    estimated_space: u64,
) -> Result<(), String> {
    use rustix::fs::{CWD, Mode, OFlags, fstat, fstatvfs, mkdirat, openat, symlinkat};
    use std::path::Component;

    validate_generator_uid(rustix::process::geteuid().as_raw())?;
    let name = root
        .file_name()
        .ok_or("Fixture path must end with a directory name.")?;
    let parent = root.parent().unwrap_or(Path::new("."));
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut parent_fd = openat(
        CWD,
        if parent.is_absolute() { "/" } else { "." },
        flags,
        Mode::empty(),
    )
    .map_err(|e| format!("Cannot open fixture parent: {e}"))?;
    for component in parent.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => {
                parent_fd = openat(&parent_fd, part, flags, Mode::empty()).map_err(|e| {
                    format!("Cannot open fixture parent without following symlinks: {e}")
                })?;
            }
            _ => return Err("Parent directory traversal is not supported.".into()),
        }
    }
    let parent_stat =
        fstat(&parent_fd).map_err(|e| format!("Cannot inspect fixture parent: {e}"))?;
    if parent_stat.st_uid != rustix::process::geteuid().as_raw() || parent_stat.st_mode & 0o022 != 0
    {
        return Err(
            "Fixture parent must be owned by the effective user and not group- or world-writable."
                .into(),
        );
    }
    let space =
        fstatvfs(&parent_fd).map_err(|e| format!("Cannot inspect fixture free space: {e}"))?;
    if space.f_frsize == 0 {
        return Err("Filesystem did not report a usable allocation unit.".into());
    }
    let available = space
        .f_bavail
        .checked_mul(space.f_frsize)
        .ok_or("Available space overflow.")?;
    if available < estimated_space {
        return Err(format!(
            "Insufficient available space for fixture estimate: need {estimated_space} bytes, have {available} bytes."
        ));
    }
    let mode = Mode::from_bits_truncate(0o700);
    mkdirat(&parent_fd, name, mode)
        .map_err(|e| format!("Cannot create new fixture directory: {e}"))?;
    #[cfg(test)]
    test_hook("root-created")?;
    let root_fd = openat(&parent_fd, name, flags, Mode::empty())
        .map_err(|e| format!("Cannot open new fixture directory: {e}"))?;
    let root_stat =
        fstat(&root_fd).map_err(|e| format!("Cannot inspect fixture directory: {e}"))?;
    if root_stat.st_uid != rustix::process::geteuid().as_raw() || root_stat.st_mode & 0o077 != 0 {
        return Err("New fixture directory is not private and owned by the effective user.".into());
    }
    let mut dirs = vec![root_fd];
    match scenario {
        "nested" => {
            for _ in 0..4 {
                let parent = dirs.last().ok_or("Missing root")?;
                mkdirat(parent, "nested", mode)
                    .map_err(|e| format!("Cannot create fixture directory: {e}"))?;
                let next = openat(parent, "nested", flags, Mode::empty())
                    .map_err(|e| format!("Cannot open fixture directory: {e}"))?;
                dirs.push(next);
            }
        }
        "wide" => {
            for i in 0..4 {
                let name = format!("dir-{i}");
                mkdirat(&dirs[0], &name, mode)
                    .map_err(|e| format!("Cannot create fixture directory: {e}"))?;
                let next = openat(&dirs[0], &name, flags, Mode::empty())
                    .map_err(|e| format!("Cannot open fixture directory: {e}"))?;
                dirs.push(next);
            }
        }
        "edge" => {
            for name in ["empty", "input-a", "input-b"] {
                mkdirat(&dirs[0], name, mode)
                    .map_err(|e| format!("Cannot create fixture directory: {e}"))?;
                let dir = openat(&dirs[0], name, flags, Mode::empty())
                    .map_err(|e| format!("Cannot open fixture directory: {e}"))?;
                if name != "empty" {
                    write_file(&dir, "conflict-λ", size, 1)?;
                }
            }
            symlinkat("input-a/conflict-λ", &dirs[0], "valid-link")
                .map_err(|e| format!("Cannot create valid symlink: {e}"))?;
            symlinkat("missing", &dirs[0], "broken-link")
                .map_err(|e| format!("Cannot create broken symlink: {e}"))?;
        }
        "sparse" => {
            let file = create_file(&dirs[0], "sparse-file")?;
            file.set_len(size)
                .map_err(|e| format!("Cannot create sparse candidate: {e}"))?;
        }
        _ => {}
    }
    for index in 0..count {
        #[cfg(test)]
        test_hook(&format!("file-{index}"))?;
        let dir = &dirs[(index as usize) % dirs.len()];
        write_file(
            dir,
            &format!("file-{index:08}"),
            size,
            index.wrapping_add(1),
        )?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn generate(
    _root: &Path,
    _count: u64,
    _size: u64,
    _scenario: &str,
    _estimated_space: u64,
) -> Result<(), String> {
    Err("Safe fixture generation is unsupported on this platform.".into())
}

#[cfg(unix)]
fn create_file(dir: &std::os::fd::OwnedFd, name: &str) -> Result<std::fs::File, String> {
    use rustix::fs::{Mode, OFlags, openat};
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    openat(dir, name, flags, Mode::from_bits_truncate(0o600))
        .map(std::fs::File::from)
        .map_err(|e| format!("Cannot create fixture file: {e}"))
}

#[cfg(unix)]
fn write_file(
    dir: &std::os::fd::OwnedFd,
    name: &str,
    size: u64,
    mut state: u64,
) -> Result<(), String> {
    let mut file = create_file(dir, name)?;
    let mut remaining = size;
    let mut buffer = [0u8; 8192];
    while remaining > 0 {
        let len = remaining.min(buffer.len() as u64) as usize;
        for byte in &mut buffer[..len] {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        file.write_all(&buffer[..len])
            .map_err(|e| format!("Cannot write fixture file: {e}"))?;
        remaining -= len as u64;
    }
    Ok(())
}

fn benchmark_request(
    source: &std::ffi::OsStr,
    destination: &std::ffi::OsStr,
    runs: &std::ffi::OsStr,
    warmups: u64,
    scenario: &str,
) -> Result<(), String> {
    let runs = number(runs, "run count")?;
    if runs == 0 || runs > 100 || warmups > 100 {
        return Err("Run count must be 1 to 100 and warm-ups at most 100.".into());
    }
    benchmark(
        Path::new(source),
        Path::new(destination),
        runs,
        warmups,
        scenario,
    )
}

fn benchmark(
    source: &Path,
    destination: &Path,
    count: u64,
    warmup_count: u64,
    scenario: &str,
) -> Result<(), String> {
    benchmark_with_writer(
        source,
        destination,
        count,
        warmup_count,
        scenario,
        &mut std::io::stdout().lock(),
    )
}

fn benchmark_with_writer(
    source: &Path,
    destination: &Path,
    count: u64,
    warmup_count: u64,
    scenario: &str,
    output: &mut impl Write,
) -> Result<(), String> {
    if !destination.is_dir() {
        return Err("Destination must be an existing directory.".into());
    }
    let capabilities = portstride_platform_macos::probe(destination);
    let mut runs = Vec::new();
    let mut warmups = Vec::new();
    let mut expected_inventory = None;
    for index in 0..(count + warmup_count) {
        #[cfg(all(test, unix))]
        test_hook(&format!("scan-{index}"))?;
        let start = Instant::now();
        let report =
            portstride_engine::inspect(&[PathBuf::from(source)], destination, capabilities.clone());
        let elapsed = start.elapsed();
        if !report.inspection_complete
            || !report.inventory.complete
            || report.issues.iter().any(|issue| issue.code == "FS_ERROR")
        {
            let failure = serde_json::json!({"schema_version": 1, "kind": "scanner_benchmark", "status": "incomplete", "reason": "inspection incomplete", "warmups": warmups, "runs": runs, "failed_run": {"elapsed_ns": elapsed.as_nanos(), "inspection_complete": report.inspection_complete, "inventory_complete": report.inventory.complete, "issue_codes": report.issues.iter().map(|issue| issue.code).collect::<Vec<_>>()}});
            writeln!(output, "{failure}").map_err(|e| format!("Cannot write benchmark: {e}"))?;
            return Err("Scan was incomplete; partial measurements were reported.".into());
        }
        let inventory = (
            report.inventory.files,
            report.inventory.directories,
            report.inventory.symlinks,
            report.inventory.special_files,
            report.inventory.logical_bytes,
        );
        if expected_inventory.is_some_and(|expected| expected != inventory) {
            let failure = serde_json::json!({"schema_version": 1, "kind": "scanner_benchmark", "status": "incomplete", "reason": "inventory changed", "warmups": warmups, "runs": runs, "failed_run": {"elapsed_ns": elapsed.as_nanos(), "files": report.inventory.files, "directories": report.inventory.directories, "symlinks": report.inventory.symlinks, "logical_bytes": report.inventory.logical_bytes}});
            writeln!(output, "{failure}").map_err(|e| format!("Cannot write benchmark: {e}"))?;
            return Err(
                "Inventory changed between runs; partial measurements were reported.".into(),
            );
        }
        expected_inventory = Some(inventory);
        let entries = report.inventory.files
            + report.inventory.directories
            + report.inventory.symlinks
            + report.inventory.special_files;
        let per_second = if elapsed.is_zero() {
            None
        } else {
            Some(entries as f64 / elapsed.as_secs_f64())
        };
        let measurement = serde_json::json!({
            "entries": entries,
            "entries_per_second": per_second,
            "elapsed_ns": elapsed.as_nanos(),
            "files": report.inventory.files,
            "directories": report.inventory.directories,
            "symlinks": report.inventory.symlinks,
            "logical_bytes": report.inventory.logical_bytes,
            "inspection_complete": report.inspection_complete,
            "status": report.status,
            "transfer_readiness": report.transfer_readiness,
        });
        if index < warmup_count {
            warmups.push(measurement);
        } else {
            runs.push(measurement);
        }
    }
    let mut timings: Vec<u64> = runs
        .iter()
        .filter_map(|run| run["elapsed_ns"].as_u64())
        .collect();
    timings.sort_unstable();
    let min = timings[0];
    let max = timings[timings.len() - 1];
    let median = if timings.len().is_multiple_of(2) {
        (timings[timings.len() / 2 - 1] as f64 + timings[timings.len() / 2] as f64) / 2.0
    } else {
        timings[timings.len() / 2] as f64
    };
    let mean = timings.iter().map(|&v| v as f64).sum::<f64>() / timings.len() as f64;
    let stddev = (timings
        .iter()
        .map(|&v| (v as f64 - mean).powi(2))
        .sum::<f64>()
        / timings.len() as f64)
        .sqrt();
    let rust_version = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    let source_filesystem = portstride_platform_macos::probe(source).filesystem;
    let report = serde_json::json!({"schema_version": 1, "kind": "scanner_benchmark", "os": std::env::consts::OS, "architecture": std::env::consts::ARCH, "destination_filesystem": capabilities.filesystem, "source_filesystem": source_filesystem, "scenario": scenario, "fixture_configuration": "See generator report; not inferred from existing files", "warmups": warmups, "statistics": {"min_ns": min, "max_ns": max, "median_ns": median, "mean_ns": mean, "standard_deviation_ns": stddev}, "rust_version": rust_version, "logical_cpus": std::thread::available_parallelism().ok().map(|n| n.get()), "command_arguments": ["portstride-harness".to_owned(), "benchmark".to_owned(), "<source redacted>".to_owned(), "<destination redacted>".to_owned(), count.to_string(), warmup_count.to_string(), scenario.to_owned()], "methodology": "Monotonic wall-clock duration of read-only inspect; destination capabilities probed before timing; repeated scans compared for inventory consistency.", "cache_state": "uncontrolled", "source_label": "user-selected fixture (path redacted)", "cache_note": "Repeated scans may use warm filesystem caches.", "runs": runs});
    writeln!(
        output,
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|e| format!("Cannot serialize benchmark: {e}"))?
    )
    .map_err(|e| format!("Cannot write benchmark: {e}"))?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static TEST_PARENT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    #[test]
    fn rejects_elevated_generator_identity() {
        assert!(validate_generator_uid(0).is_err());
        assert!(validate_generator_uid(501).is_ok());
    }

    #[test]
    fn partial_failure_preserves_unrelated_data() {
        let parent = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("portstride-partial-unit-{}", std::process::id()));
        std::fs::create_dir(&parent).unwrap();
        std::fs::write(parent.join("sentinel"), b"untouched").unwrap();
        TEST_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|stage| {
                if stage == "file-1" {
                    Err("injected I/O failure".into())
                } else {
                    Ok(())
                }
            }))
        });
        let error = generate(&parent.join("fixture"), 3, 4, "flat", 1024).unwrap_err();
        TEST_HOOK.with(|hook| *hook.borrow_mut() = None);
        assert!(error.contains("injected I/O failure"));
        let report = generation_error(&error);
        assert_eq!(report["status"], "incomplete");
        assert_eq!(report["error"], error);
        assert_eq!(
            std::fs::read(parent.join("sentinel")).unwrap(),
            b"untouched"
        );
        assert_eq!(
            std::fs::metadata(parent.join("fixture/file-00000000"))
                .unwrap()
                .len(),
            4
        );
        assert!(!parent.join("fixture/file-00000001").exists());
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn substitution_by_private_directory_is_not_detected() {
        let parent = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "portstride-substitution-unit-{}",
            std::process::id()
        ));
        std::fs::create_dir(&parent).unwrap();
        std::fs::create_dir(parent.join("replacement")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            parent.join("replacement"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        std::fs::write(parent.join("replacement/sentinel"), b"untouched").unwrap();
        TEST_PARENT.with(|p| *p.borrow_mut() = Some(parent.clone()));
        TEST_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|stage| {
                if stage == "root-created" {
                    TEST_PARENT.with(|p| {
                        let binding = p.borrow();
                        let parent = binding.as_ref().unwrap();
                        std::fs::rename(parent.join("fixture"), parent.join("original")).unwrap();
                        std::fs::rename(parent.join("replacement"), parent.join("fixture"))
                            .unwrap();
                    });
                }
                Ok(())
            }))
        });
        // Zero files demonstrates acceptance without writing inside the replacement.
        assert!(generate(&parent.join("fixture"), 0, 0, "flat", 1024).is_ok());
        TEST_HOOK.with(|hook| *hook.borrow_mut() = None);
        assert_eq!(
            std::fs::read(parent.join("fixture/sentinel")).unwrap(),
            b"untouched"
        );
        assert_eq!(
            std::fs::read_dir(parent.join("fixture")).unwrap().count(),
            1
        );
        std::fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn failed_inspection_retains_prior_measurements() {
        let parent = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "portstride-bench-failure-unit-{}",
            std::process::id()
        ));
        std::fs::create_dir(&parent).unwrap();
        let source = parent.join("source");
        std::fs::create_dir(&source).unwrap();
        TEST_PARENT.with(|p| *p.borrow_mut() = Some(source.clone()));
        TEST_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|stage| {
                if stage == "scan-2" {
                    TEST_PARENT
                        .with(|p| std::fs::remove_dir(p.borrow().as_ref().unwrap()).unwrap());
                }
                Ok(())
            }))
        });
        let mut output = Vec::new();
        assert!(benchmark_with_writer(&source, &parent, 2, 1, "test", &mut output).is_err());
        TEST_HOOK.with(|hook| *hook.borrow_mut() = None);
        let report: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(report["status"], "incomplete");
        assert_eq!(report["warmups"].as_array().unwrap().len(), 1);
        assert_eq!(report["runs"].as_array().unwrap().len(), 1);
        assert!(report["failed_run"]["elapsed_ns"].is_number());
        std::fs::remove_dir(parent).unwrap();
    }
}
