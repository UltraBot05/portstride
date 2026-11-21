# M0 scanner baselines

These are read-only inspection timings on a local macOS aarch64 APFS source and destination, using rustc 1.99.1 and an optimized release build. The machine reported 12 logical CPUs. The raw runs, generator reports, environment and integrity digests are in [m0-scanner-baselines.json](m0-scanner-baselines.json). Paths are redacted. This is one machine and one filesystem, not a filesystem coverage matrix or a copy benchmark.

Each scenario used 1,000 generated 128-byte files, one warm-up and five measured scans. The edge fixture also contained two 128-byte files, three directories, and two symlinks. Nested and wide each had four additional directories. The mixed fixture began as a separate edge fixture. A 4,096-byte file was then manually created in `input-a` with mode 0600 and byte values 0 through 255 repeated 16 times. It therefore contained 1,002 regular files of 128 bytes and one of 4,096 bytes, four directories including the root, and two symlinks. The scanner counts the fixture root as a directory.

| Scenario | Entries | Median ms | Min–max ms |
| --- | ---: | ---: | ---: |
| Flat | 1,001 | 3.271 | 3.105–3.823 |
| Nested | 1,005 | 3.579 | 3.273–3.799 |
| Wide | 1,005 | 3.363 | 2.842–4.370 |
| Edge | 1,008 | 3.278 | 3.017–3.612 |
| Mixed | 1,009 | 2.897 | 2.684–3.145 |

All warm-up and measured inspections were complete and their inventory counts matched within each scenario. Before and after each benchmark, an independent SHA-256 digest covered sorted relative paths, entry types, permission bits, regular-file contents and symlink targets, including the fixture root. The empty destination was checked in the same way. The digests and counts matched. This check does not cover all metadata or prove an atomic snapshot. The harness does not follow symlinks. Edge conflict names are in separate directories, so this run does not establish collision detection coverage.

To repeat, build with `cargo build --release -p portstride --bin portstride-harness`. Create a new private, trusted parent and an empty destination outside the fixture. For each of `flat`, `nested`, `wide` and `edge`, run `target/release/portstride-harness generate <new-fixture> 1000 128 <scenario>`, then `target/release/portstride-harness benchmark <fixture> <destination> 5 1 <scenario>`. For mixed, generate a separate edge fixture, exclusively create `input-a/manual-4096` with mode 0600 and the byte sequence described above, then benchmark it with scenario label `mixed`. Inspect and hash the fixture and destination before and after. The generator assumes trusted ancestors and ACLs and no concurrent modification. Fixtures are retained for inspection; the harness does not clean them up.

Timings include read-only inspection but exclude the destination capability probe. Cache state was uncontrolled, and repeated scans may benefit from warm caches. The independent content reads before measurement also warm caches. These numbers do not measure transfer throughput, metadata preservation, cold-cache performance, or removable volumes. The generator and mixed fixture setup wrote disposable files to host APFS; no removable volume was modified and no transfer was performed.
