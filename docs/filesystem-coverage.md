# Filesystem coverage

The [machine-readable matrix](filesystem-coverage.json) records the current preflight test evidence. Synthetic capability tests are not filesystem integration tests.

| Filesystem | Disposable directory | Disposable volume | Simulated behavior |
|---|---|---|---|
| APFS | Current host; case variant unspecified | Not tested | Case and capability logic |
| NTFS | Not tested | Not tested | Read-only capability |
| exFAT | Not tested | Not tested | Generic capabilities |
| FAT32 | Not tested | Not tested | Per-file size bound |
| Other mounted | Not tested | Not tested | Unknown capability fallback |

For future integration testing, obtain explicit authorization for a disposable mounted volume, confirm its identity and that it contains no personal data, then run read-only inspection against separately prepared disposable fixtures. Record filesystem, mount flags, OS and exact results. This project does not create, mount, format, unmount or detach volumes automatically.
