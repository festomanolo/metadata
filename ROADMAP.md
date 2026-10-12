# ROADMAP

Live status: `STATE.md`.

| Milestone | Status |
|---|---|
| M1 Foundation | done |
| M2 Extraction | done natively; ExifTool/FFprobe adapters unverified against real tools |
| M3 TUI | done (Cases/Reports tabs defer to the CLI) |
| M4 Editing | done for native formats; ExifTool path unverified |
| M5 Forensics | done (timeline, rules, manifests) |
| M6 Cases & reports | done (SQLite, HTML/CSV/JSON) |
| M7 Hardening | benchmarks, docs, audit done; fuzzing and release packaging open |

## Next

* Verify and test the ExifTool and FFprobe paths in CI against real binaries.
* Source/documentation/screenshot consistency checks for assessment.
* 7z/RAR listing, Apache Tika adapter, OLE2 properties natively.
* TUI: interactive Cases and Reports tabs, theme/config persistence.
* Fuzz targets for native parsers; release packaging per platform.
