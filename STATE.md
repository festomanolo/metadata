# STATE.md — living project ledger (READ THIS FIRST, UPDATE AFTER EVERY CHANGE)

Purpose: lets any session resume without scanning the codebase. Keep it terse and accurate.
Rule: after adding/changing/removing a feature, file, public API, or dependency, update the relevant section here **in the same turn**.

- Toolchain: `export PATH=$HOME/.cargo/bin:$PATH` (cargo/rustc 1.99; MSRV set to 1.82). Not on default PATH; PATH does not persist between Bash calls.
- Backends on dev machine: exiftool NO, ffprobe NO, 7z NO, tika NO → tests use native paths only (`use_external:false`); adapters tested via JSON-parse unit tests.
- First build is slow (~3 min); run long cargo commands with a 600000 ms timeout.
- Check: `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace` (all green; ~80 tests).
- Last updated: 2026-10-12 — M1–M5 done incl. TUI; cases (SQLite), HTML reports, docs, benches remain.

## Feature status
Legend: DONE (works + tested) · PARTIAL · PLANNED

| Area | Feature | Status | Notes |
|---|---|---|---|
| Foundation | Workspace, CLI (clap), exit codes 0/1/2/3 | DONE | |
| Foundation | Content-based format detection | DONE | `detect.rs`; 7z/rar/bz2/xz detected only |
| Foundation | SHA-256 / SHA-512 streaming | DONE | `core/hash.rs` |
| Foundation | Backend discovery, `doctor`, `capabilities` | DONE | exiftool, ffprobe; Tika/7z PLANNED |
| Foundation | Config file (TOML) + `config [--path|--init]` | DONE | `cli/config.rs`; keys: default_format, jobs, timeout_secs, use_external, hash, redact, limits.* |
| Extraction | Universal file info (size, mime, ext-mismatch, fs times, perms, sha256) | DONE | |
| Extraction | Images native: EXIF(+offset-aware dates, GPS decimal), IPTC, XMP, ICC, JFIF, PNG text/tIME, GIF/BMP/WebP dims, JPEG trailing-data warning | DONE | `native/image.rs` |
| Extraction | OOXML/ODF/EPUB native props, comments, tracked changes, macros/signature flags | DONE | `native/ooxml.rs` |
| Extraction | PDF: Info dict, XMP, revisions, sig/JS/launch/embedded indicators | DONE | `native/pdf.rs` (lopdf) |
| Extraction | Archives: zip/tar/tgz/gzip listing, traversal/symlink/ratio/entry limits, nested (--deep) | DONE | never extracts to disk |
| Extraction | Text/source/HTML/XML/CSV/JSON/RTF | DONE | `native/text.rs` |
| Extraction | ExifTool adapter (read) | PARTIAL | code + JSON-parse tests; not run against real exiftool (not installed) |
| Extraction | FFprobe adapter (read) | PARTIAL | same as above |
| Extraction | Legacy OLE2 doc/xls/ppt, 7z/rar listing, video/audio w/o tools | PLANNED | reported as `unavailable`, not faked |
| Extraction | Source-project inspection (manifests, git history) | PLANNED | for directories; needs git subprocess |
| Commands | inspect, extract, batch, search, compare (file+dir), hash, verify-manifest, capabilities, doctor, config | DONE | integration-tested |
| Commands | timeline (sources separate; --source, --no-git, --manifest intake, --claims) | DONE | `cli/forensic.rs` |
| Commands | assess (claims flags/file, --previous, git, redaction default, integrity re-hash, --min-severity) | DONE | JSON+human; HTML PLANNED (M6) |
| Commands | edit (--set/--remove/--copy-from/--interactive/--dry-run/--in-place/--output-dir/--yes/--accept-signature-loss), clean (--profile privacy/all/gps) | DONE | `cli/edit_cmd.rs`; new copy by default `<stem>.edited.<ext>`, in-place needs --yes + backup `.metadata-backup-<ts>`, ops log `<config dir>/operations.jsonl` |
| Editing | Native writers: docx/xlsx/pptx core+app props, odt/ods/odp meta, PDF Info (file rewritten), PNG text chunks, JPEG comment + strip | DONE | round-trip verified in `metadata-edit/tests/edit.rs` |
| Editing | ExifTool write backend | PARTIAL | argv built safely+unit-tested; NOT run against real exiftool; listed as `exiftool*` in capabilities |
| Editing | JPEG/TIFF/etc. individual EXIF/IPTC/XMP tag edit, GPS-only removal | PLANNED | needs exiftool |
| Commands | assess, cases, reports | PLANNED | M5/M6 |
| Commands | tui [PATH] [--no-scan] [--no-external] | DONE | needs a TTY (errors otherwise); pty smoke-tested |
| Forensics | Evidence manifest (versioned, entries digest, verify, traversal-safe) | DONE | `forensics/manifest.rs` |
| Forensics | Field-level compare | DONE | `forensics/compare.rs` |
| Forensics | Timeline, git history reader, project summary, assessment rules | DONE | `forensics/{timeline,git,project,claims,assess}.rs`; rules: TS-FUTURE, TS-BEFORE-START, TS-AFTER, TS-MEDIA-AFTER, TS-MOD-BEFORE-CREATE, MS-LATE-CREATION, UNI-FS, UNI-EMBEDDED, FMT-EXT-MISMATCH, DUP-CONTENT, MISS-METADATA, INC-PRESENT/ABSENT, GIT-{BEFORE-START,LATE-START,AFTER-DEADLINE,FUTURE,NONMONOTONIC,AUTHOR-COMMIT-SKEW}, GAP-HISTORY, SUB-DIFF, DUP-PREVIOUS, SUB-CREATED-CHANGED, SUB-MODIFIED-BACKWARDS |
| Forensics | Code/docs/screenshots/structure consistency check | PLANNED | listed in `checks_not_implemented` of every assessment |
| Output | JSON envelope (schema 1.0.0), CSV (formula-injection guarded), human, redaction | DONE | |
| Perf | Bounded rayon pool, cancellation (Ctrl+C), progress bar (TTY only) | DONE | benchmarks PLANNED; caching/index PLANNED |
| Security | No shell, timeout+output cap for subprocesses, panic isolation per parser, symlinks not followed | DONE | |
| Reports | HTML/CSV/JSON assessment reports | PLANNED | |
| Storage | SQLite cases/cache | PLANNED | |
| Docs | README, CLI ref, format matrix, safety/methodology guides | PLANNED | PLAN/ARCHITECTURE/ROADMAP exist (short) |

## Crate / file map
- `Cargo.toml` workspace (members: core, engine, forensics, cli, testkit). `.gitignore`, `CLAUDE.md` (points here).
- `crates/metadata-core` — types only, no I/O policy
  - `schema.rs`: `SCHEMA_VERSION`, `Field{group,name,raw,normalized,time,backend,provenance,writable,sensitive}` (`Field::new/.with_time/.with_normalized/.key()`), `FileReport` (`.find("Group:Name"|"Name")`), `FormatInfo`, `Category`, `ExtensionMatch`, `FsInfo`, `Warning`, `BackendUsed`, `is_sensitive_name()`
  - `time.rs`: `TimeValue{iso,offset_known,utc}` (+`.sort_key()`), `parse_exif/parse_pdf/parse_iso/parse_any`. Naive times are never treated as UTC.
  - `hash.rs`: `Algorithm{Sha256,Sha512}`, `digest_file`, `sha256_file`, `sha256_bytes`; `error.rs`: `MetaError`, `Result`
- `crates/metadata-engine`
  - `lib.rs`: `Engine::new(Options)`, `Engine::inspect(path)->FileReport` (never fails; errors in report), `Options{all,deep,hash,use_external,timeout_secs,limits,cancel}`, `Limits`, `Extraction{fields,warnings,unavailable}`
  - `detect.rs` (SPECS table, `detect_file`, zip sub-classification) · `fsinfo.rs` · `explain.rs` (`explain(group,name)` field reliability text)
  - `scan.rs`: `collect_files(paths,&ScanOptions)->Collected{files,skipped}`, `Engine::inspect_many(files,jobs,progress)`
  - `search.rs`: `aliases()`, `search(report,&Query)->SearchResult{presence: Found|Absent|Unsupported|Unavailable|Error}`
  - `backends.rs`: `Backends::discover()/status()/extract_external()`, `run_capped()` (timeout, output cap, no shell), `parse_exiftool`, `parse_ffprobe`
  - `capabilities.rs`: `matrix(&Backends)` (write column intentionally empty until M4)
  - `native/{mod,image,ooxml,pdf,archive,text,xmp}.rs`: native extractors; `mod.rs` dispatches + `catch_unwind`
- `crates/metadata-forensics`: `claims.rs` (Claims, Milestone, Thresholds, parse_claim day-ranges), `git.rs` (read_history, hardened git subprocess), `timeline.rs` (Source, Role, Event, build), `project.rs`, `assess.rs` (assess, verify_unchanged, redact, Finding), `manifest.rs` (`create/load/verify`, `Manifest`, `VerifyReport`), `compare.rs` (`compare_reports`, `compare_sets`, `DiffStatus`)
- `crates/metadata-cli` (binary `metadata`): `args.rs` (clap), `config.rs`, `render.rs` (json_envelope, csv_fields, human_report, redact, Style), `main.rs` (all command handlers `cmd_*`, `scan()`, `emit()`, `write_output()`); tests in `tests/cli.rs` (assert_cmd)
- `crates/metadata-edit`: `lib.rs` (`plan`, `execute`, `Plan`, `Change`, `EditRequest/Op/Profile`, `ApplyOpts{backup,replace_original,allow_signature_loss,force,log_path}`, `EditOutcome`, verification + unexpected_changes), `exiftool.rs` (build_args/run), `native/{office,pdf,png,jpeg,zipedit,xmlprops}.rs` (`keys`, `resolve_key`, `write`, `has_signature`). Pipeline: temp file in dest dir → re-extract/verify → backup → atomic persist → log.
- `crates/metadata-tui`: `App` (sections, focus, scan thread via mpsc, filters, edit/compare state), `ui::draw`, `theme::Theme`, `run()`. Keys documented in the in-app help (`?`). Edits from TUI never in-place, never accept signature loss.
- `crates/metadata-testkit` (dev-only): `jpeg(&ExifSpec,..)`, `png`, `docx(&DocxSpec)`, `odt`, `pdf(..,incremental)`, `zip_with`, `tar_gz`. Use these for all new tests.
- Tests: `crates/metadata-engine/tests/extract.rs` (19 fixture tests incl. malformed, bombs, cancel, symlinks)

## Conventions
- Machine output → stdout; diagnostics → stderr. Never mix.
- Each `Field` keeps raw value + backend + provenance; times via `TimeValue` (offset_known honesty).
- Sensitive = GPS/author-like names (`is_sensitive_name`); `--redact` masks; reports must redact by default.
- No fake features: unimplemented = PLANNED here, absent from CLI help.
- New external parsing must be panic-safe, bounded (Limits), and never execute content.

## Known issues / decisions log
- quick-xml 0.41 emits entities (`&amp;`) as `Event::GeneralRef`; readers must handle it (fixed in ooxml.rs/xmp.rs via `xmp::entity_char`). Custom-props reader in ooxml.rs still ignores entities (minor TODO).
- quick-xml `unescape_value` deprecated; silenced with `#![allow(deprecated)]` in native/mod.rs.
- `Backends::extract_external` with exiftool dedupes against native fields by (group,name,raw); exiftool group names (EXIF, XMP, PDF…) were chosen to align.
- Search alias table + "format can hold" table live in `search.rs`; extend when adding fields.
- Archive entry timestamps (ZIP DOS time) have no timezone → naive `TimeValue`.

## Next steps (in order)
1. M6: HTML/CSV report for `assess` (metadata-report crate), SQLite cases (metadata-storage).
3. M7: benches, README/CLI ref/format matrix/safety+methodology docs, audit.
