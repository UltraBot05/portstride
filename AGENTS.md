# Portstride engineering policy

## Project identity

Portstride is a native, macOS-first adaptive file-transfer application. It uses a shared Rust engine and a native SwiftUI/AppKit macOS frontend; a native WinUI 3 Windows frontend follows. Portstride is the final product name and supersedes provisional names in the specifications.

The documents in `adaptive-transfer-spec/` are the authoritative design baseline. Read the relevant documents before work, beginning with the agent handoff when implementing. Do not invent unsupported requirements or treat proposed interfaces, targets, or unimplemented features as established facts. Record unresolved decisions rather than silently making them. Correctness and source-data preservation come before performance.

## Engineering rules

- Use safe Rust by default. Prefer established standard-library and platform APIs. Keep modules small and cohesive, handle errors explicitly, and document invariants.
- Avoid speculative abstractions, unnecessary dependencies, premature async runtimes, and unproven performance claims.
- Separate portable engine logic from platform-specific filesystem adapters. Probe capabilities at runtime, represent unknowns explicitly, and use conservative fallbacks. Never silently assume a capability exists or reject a mounted filesystem solely because it is outside the initial tested matrix.
- Maintain structured, versioned diagnostics and machine-readable contracts. Redact sensitive paths in exported diagnostics by default. Do not interpolate untrusted paths into shell commands.
- Follow the roadmap and its release gates. File-level concurrency within one job precedes job-level scheduling. Benchmark strategy choices against controlled, equivalent baselines with integrity checks.

## Transfer safety

- Preserve source data during copies. For cross-volume moves, delete sources only after the required verification and destination commit, explicit move policy, and applicable identity checks. The exact move verification policy remains an open decision. Obtain approval before implementing deletion or move support.
- Never silently overwrite user files, skip items, or discard metadata. Require an explicit conflict or loss policy and report failures and limitations.
- Treat cancellation, unexpected disconnects, metadata preservation, and failures as correctness requirements. Do not present incomplete files as completed; validate recovery state before resuming.
- Never run destructive operations against real devices during tests. Use disposable fixtures and mocked filesystem capabilities where appropriate. Never automatically format, partition, erase, or convert a drive as part of a transfer.

## Testing and quality

When Ponytail and test-driven-development guidance differ, apply this project policy: tests are mandatory for substantive implemented behavior and bug fixes. Purely cosmetic or documentation-only changes may not require new tests. Never skip verification of changed behavior.

Run rustfmt, Clippy, and relevant tests before reporting completion when applicable to the changes. Report commands and results accurately, and distinguish successful verification from checks that were not run. Review changes for security, correctness, simplicity, and regressions. Benchmark future optimizations against controlled baselines; report verification and comparable metadata semantics with performance results.

## Skill selection

Use only skills relevant to the current task. If an optional skill is unavailable, do not fabricate its guidance or install dependencies without authorization.

- Rust engineering: `rust-best-practices` and `ponytail`.
- Tests: `test-driven-development`. Debugging: `systematic-debugging`. Verification: `verification-before-completion`. Reviews: `requesting-code-review`.
- Documentation and user-facing prose: `unslop`.
- Native UI: `macos-design-guidelines`, `mac-arsed-mac-app`, `macos-app-architecture`, `swiftui-patterns`, `swiftui-design-tokens`, and relevant SwiftUI interaction skills.
- Native build and debugging: `build-run-debug`. Visual ideation: `frontend-design`, with native macOS behavior and accessibility taking precedence.

## Agent autonomy and permissions

Inspect and plan briefly, then execute the authorized milestone. Do not repeatedly request permission for routine local changes. Explain important Rust concepts concisely to the project owner. Avoid unnecessary boilerplate and placeholder implementations. Do not expand beyond the current milestone without authorization.

Ordinary local source edits, builds, formatting, linting, and non-destructive tests are allowed within the project. Obtain explicit permission before drive formatting or partitioning, deleting personal files, privileged installations, automatic hooks, global OS modifications, remote pushes, or public releases. Obtain approval before implementing system-level integrations. Do not modify the original specifications unless instructed.

## Completion report

For every substantive milestone, report:

- Implemented features and changed files.
- Architecture decisions.
- Commands and tests executed, with results, and relevant checks not run.
- Safety and correctness limitations and unresolved issues.
- Recommended next milestone.
