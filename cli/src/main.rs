use std::{ffi::OsString, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
fn run() -> Result<ExitCode, String> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("inspect")) {
        return Err(
            "Usage: portstride inspect <source>... --destination <directory> [--json]".into(),
        );
    }
    let mut sources = Vec::<PathBuf>::new();
    let mut dest: Option<OsString> = None;
    let mut json = false;
    let mut show_paths = false;
    let mut positional_only = false;
    while let Some(arg) = args.next() {
        if !positional_only && arg == "--" {
            positional_only = true;
        } else if !positional_only && arg == "--json" {
            json = true;
        } else if !positional_only && arg == "--show-paths" {
            show_paths = true;
        } else if !positional_only && arg == "--destination" {
            if dest.is_some() {
                return Err("Destination was specified more than once.".into());
            }
            dest = Some(args.next().ok_or("Missing destination path.")?);
        } else if !positional_only && arg.to_string_lossy().starts_with('-') {
            return Err("Unknown option; use -- before source paths beginning with a dash.".into());
        } else {
            sources.push(arg.into());
        }
    }
    let dest = PathBuf::from(dest.ok_or("Missing --destination.")?);
    if sources.is_empty() {
        return Err("Select at least one source.".into());
    }
    let caps = portstride_platform_macos::probe(&dest);
    let display = if show_paths {
        portstride_engine::PathDisplay::RelativeNames
    } else {
        portstride_engine::PathDisplay::Redacted
    };
    let report = portstride_engine::inspect_with_paths(&sources, &dest, caps, display);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|e| format!("Cannot serialize report: {e}"))?
        );
    } else {
        println!(
            "Preflight: {} (scan {})",
            report.status,
            if report.inventory.complete {
                "complete"
            } else {
                "partial"
            }
        );
        println!("Transfer readiness: not assessed; no transfer is authorized.");
        println!(
            "{} files, {} directories, {} symbolic links, {} logical bytes",
            report.inventory.files,
            report.inventory.directories,
            report.inventory.symlinks,
            report.inventory.logical_bytes
        );
        println!(
            "Destination filesystem: {}",
            report
                .destination
                .filesystem
                .as_deref()
                .unwrap_or("unknown")
        );
        for issue in &report.issues {
            println!(
                "{} {} {:?}: {}",
                issue.severity, issue.code, issue.affected_paths, issue.evidence
            );
            for remedy in &issue.remedies {
                println!("  {}: {}", remedy.action, remedy.description);
            }
        }
    }
    Ok(match report.status {
        "complete" => ExitCode::SUCCESS,
        "blocked" => ExitCode::from(1),
        _ => ExitCode::from(3),
    })
}
