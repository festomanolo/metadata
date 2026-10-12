# STATE.md — living project ledger (READ THIS FIRST, UPDATE AFTER EVERY CHANGE)

Purpose: lets any session resume without scanning the codebase. Keep it terse and accurate.
Rule: after adding/changing/removing a feature, file, public API, or dependency, update the relevant section here **in the same turn**.

- Toolchain: `export PATH=$HOME/.cargo/bin:$PATH` (cargo/rustc 1.99; MSRV set to 1.82). Not on default PATH; PATH does not persist between Bash calls.
- Backends on dev machine: exiftool NO, ffprobe NO, 7z NO, tika NO → tests use native paths only (`use_external:false`); adapters tested via JSON-parse unit tests.
- First build is slow (~3 min); run long cargo commands with a 600000 ms timeout.
- Check: `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace` (all green as of last update; 53 tests).
- Last updated: 2026-10-12 — M1 foundation + most of M2 + manifest/compare done.

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
| Commands | timeline | PLANNED | M5 |
| Commands | edit, clean | PLANNED | M4 (no write support is claimed anywhere until verified) |
| Commands | assess, cases, reports | PLANNED | M5/M6 |
| Commands | tui | PLANNED | M3 |
| Forensics | Evidence manifest (versioned, entries digest, verify, traversal-safe) | DONE | `forensics/manifest.rs` |
| Forensics | Field-level compare | DONE | `forensics/compare.rs` |
| Forensics | Timeline, assessment rules/findings | PLANNED | |
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
- `crates/metadata-forensics`: `manifest.rs` (`create/load/verify`, `Manifest`, `VerifyReport`), `compare.rs` (`compare_reports`, `compare_sets`, `DiffStatus`)
- `crates/metadata-cli` (binary `metadata`): `args.rs` (clap), `config.rs`, `render.rs` (json_envelope, csv_fields, human_report, redact, Style), `main.rs` (all command handlers `cmd_*`, `scan()`, `emit()`, `write_output()`); tests in `tests/cli.rs` (assert_cmd)
- `crates/metadata-testkit` (dev-only): `jpeg(&ExifSpec,..)`, `png`, `docx(&DocxSpec)`, `odt`, `pdf(..,incremental)`, `zip_with`, `tar_gz`. Use these for all new tests.
- Tests: `crates/metadata-engine/tests/extract.rs` (19 fixture tests incl. malformed, bombs, cancel, symlinks)

## Conventions
- Machine output → stdout; diagnostics → stderr. Never mix.
- Each `Field` keeps raw value + backend + provenance; times via `TimeValue` (offset_known honesty).
- Sensitive = GPS/author-like names (`is_sensitive_name`); `--redact` masks; reports must redact by default.
- No fake features: unimplemented = PLANNED here, absent from CLI help.
- New external parsing must be panic-safe, bounded (Limits), and never execute content.

## Known issues / decisions log
- quick-xml `unescape_value` deprecated; silenced with `#![allow(deprecated)]` in native/mod.rs.
- `Backends::extract_external` with exiftool dedupes against native fields by (group,name,raw); exiftool group names (EXIF, XMP, PDF…) were chosen to align.
- Search alias table + "format can hold" table live in `search.rs`; extend when adding fields.
- Archive entry timestamps (ZIP DOS time) have no timezone → naive `TimeValue`.

## Next steps (in order)
1. M5: `timeline` command + forensics `timeline.rs` (sources kept separate: embedded / filesystem / archive / git / intake), `assess` engine (rules → findings with id, severity INFO/REVIEW/HIGH PRIORITY, alternatives, observation vs interpretation confidence), assessment config (claimed start/milestones/deadline).
2. M4: edit/clean (exiftool-backed writes + native-safe pipeline: dry-run, diff, backup, temp+verify+atomic replace, signature warning).
3. M3: TUI (ratatui+crossterm).
4. M6: SQLite cases, HTML/CSV/JSON reports. 5. M7: benches, docs (README, CLI ref, format matrix), audit.
