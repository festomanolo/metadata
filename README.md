# metadata

[![Rust](https://img.shields.io/badge/rust-1.82%2B-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Zero Telemetry](https://img.shields.io/badge/telemetry-none-brightgreen.svg)](#privacy-and-safety)
[![Local First](https://img.shields.io/badge/execution-100%25%20local-success.svg)](#privacy-and-safety)

A fast, local, read-first command-line tool and interactive terminal UI for **extracting, comparing, editing, and reviewing file metadata**, featuring a transparent assessment mode designed to examine whether digital evidence in submissions aligns with an expected project timeline.

> **Forensic Philosophy**: It never accuses or decides that anyone cheated. It reports strictly factual *observations*, provides objective benign alternative interpretations, and leaves original evidence completely untouched.

```text
$ metadata inspect report.docx
report.docx
  Format      Word document (OOXML) · application/vnd.openxmlformats-… · extension matches
  Size        14.2 KiB (14540 bytes)
  SHA-256     9f2c38b4a098…
  Backends    native 0.1.0

  Filesystem  (changes on copy/download/extract; weak evidence alone)
    Modified    2026-10-12T09:14:03+00:00

  OOXML-Core (6)
    Creator          Ann
    LastModifiedBy   Bob
    Revision         7
    Created          2024-01-01T09:00:00Z
    Modified         2024-02-01T09:00:00Z
```

---

## Table of Contents

- [Highlights](#highlights)
- [Architecture & Crate Ecosystem](#architecture--crate-ecosystem)
- [Installation](#installation)
  - [Prerequisites](#prerequisites)
  - [Optional External Backends](#optional-external-backends)
- [Quick Start](#quick-start)
  - [Inspection & Batch Extraction](#inspection--batch-extraction)
  - [Safe Metadata Editing](#safe-metadata-editing)
  - [Timeline & Integrity Manifests](#timeline--integrity-manifests)
  - [Academic & Forensic Assessment](#academic--forensic-assessment)
  - [Interactive Terminal UI (TUI)](#interactive-terminal-ui-tui)
- [Supported Formats Matrix](#supported-formats-matrix)
- [Editing Safety & Pipeline](#editing-safety--pipeline)
- [Privacy, Limits & Security](#privacy-limits--security)
- [Known Limitations](#known-limitations)
- [Performance Benchmarks](#performance-benchmarks)
- [Troubleshooting](#troubleshooting)
- [License](#license)

---

## Highlights

* **Signature-Based Format Detection**: Identifies true file types by byte signatures (magic numbers) rather than extensions, flagging extension mismatches and spoofed files.
* **Native, Always-Available Extraction**: Built-in zero-dependency parsers for images (EXIF with timezone offsets, IPTC, XMP, ICC, PNG chunks), Microsoft Office (DOCX/XLSX/PPTX), OpenDocument (ODT/ODS/ODP), EPUB, PDF (Info dictionary, uncompressed XMP, incremental revisions, signature and attachment markers), archives (ZIP, TAR, GZIP listing), and source code.
* **Honest Timestamp Semantics**: A timestamp missing timezone information is never assumed to be UTC. It is preserved as naive local time and compared using configurable tolerances.
* **Non-Destructive Safe Editing**: Plan-first verification: validation → diff preview → private temporary write → re-extract & verify → atomic rename. Originals are never touched without `--in-place`, which creates timestamped backups. Signed documents require explicit override.
* **Integrity Manifests**: Generate and verify versioned SHA-256 manifests (`hash --manifest`, `verify-manifest`) to detect tampered, added, or deleted files across submissions.
* **Evidence Assessment Engine**: Stable rule IDs (`TS-BEFORE-START`, `MOD-BEFORE-CREATION`, `GIT-HISTORY-GAP`), confidence scoring (`HIGH`, `MEDIUM`, `LOW`), priority levels (`HIGH PRIORITY`, `REVIEW`, `INFO`), and explicit benign alternatives.
* **Case Management & Persistent Audits**: SQLite-backed case store (`metadata case`) supporting multi-stage student submissions, structured reviewer notes, dismissal audit trails (reasons required), and exportable reports (HTML, CSV, JSON).
* **Interactive Terminal UI**: Full-screen Ratatui-powered TUI (`metadata tui`) with keyboard navigation, fuzzy filtering, grouped inspectors, diffing, visual timeline, and OSC 52 clipboard export.
* **Resilient Engine**: Panic-isolated parsers, bounded concurrency, cancellation support (`Ctrl+C`), zip-bomb mitigations, and content-hash caching.

---

## Architecture & Crate Ecosystem

The project is structured as a modular Rust workspace of focused crates:

```text
metadata/
├── crates/
│   ├── metadata-core       # Domain schemas, TimeValue, hashing, and error types
│   ├── metadata-engine     # Format detection, native parsers, and external tool runners
│   ├── metadata-forensics  # Assessment rules, timeline synthesis, and manifest verification
│   ├── metadata-edit       # Safe editing pipeline, pre-flight diffs, and verification
│   ├── metadata-storage    # SQLite persistence for case tracking, audit logs, and cache
│   ├── metadata-report     # Sanitized HTML, CSV, and JSON report generator
│   ├── metadata-tui        # Ratatui terminal UI with tabs, search, and inspector
│   ├── metadata-cli        # Unified command-line interface entry point
│   └── metadata-testkit    # Synthetic fixture generators for deterministic testing
├── docs/                   # Detailed specifications and guides
│   ├── ASSESSMENT.md       # Assessment philosophy, rule catalog, and evidence interpretation
│   ├── CLI.md              # Full CLI reference guide
│   ├── EDITING-SAFETY.md   # Safe editing workflow and verification guarantees
│   └── FORMATS.md          # Format capability matrix and backend coverage
└── tests/                  # Integration tests across all modules
```

---

## Installation

### Prerequisites

Requires a stable Rust toolchain (Rust 1.82+):

```sh
# Clone the repository
git clone https://github.com/festomanolo/metadata.git
cd metadata

# Build release binary
cargo build --release

# Install binary to your PATH
cp target/release/metadata /usr/local/bin/

# Verify your installation and check detected backends
metadata doctor
```

### Optional External Backends

Core features work out of the box with zero external dependencies. Installing optional tools extends coverage for specialized formats:

| Backend | Extends Coverage | macOS | Debian / Ubuntu | Windows |
|---|---|---|---|---|
| **ExifTool** | RAW images, HEIC/AVIF, legacy OLE2 Office (DOC/XLS/PPT), broader tag editing | `brew install exiftool` | `apt install libimage-exiftool-perl` | `winget install OliverBetz.ExifTool` |
| **FFprobe** | Video containers (MP4, MKV, MOV, AVI) and audio streams (MP3, FLAC, WAV, AAC) | `brew install ffmpeg` | `apt install ffmpeg` | `winget install Gyan.FFmpeg` |
| **git** | Repository commit history analysis for `timeline` and `assess` | Xcode CLT | `apt install git` | `winget install Git.Git` |

Set custom paths via environment variables if binaries are in non-standard locations:
```sh
export METADATA_EXIFTOOL=/path/to/exiftool
export METADATA_FFPROBE=/path/to/ffprobe
```

---

## Quick Start

### Inspection & Batch Extraction

```sh
# Inspect a single file with formatted output
metadata inspect photo.jpg

# Recursive extraction across a directory to JSON
metadata extract ./submissions -r --format json -o metadata_dump.json

# Search files matching a specific field or value
metadata search ./photos -r --field GPSLatitude
metadata search ./submissions -r --field Creator --value "Student"

# Compare metadata between two files side-by-side
metadata compare original.docx revised.docx
```

### Safe Metadata Editing

All edits write to `<name>.edited.<ext>` by default unless `--in-place` is explicitly specified.

```sh
# Dry run: preview planned edits and diff without writing
metadata edit report.docx --dry-run --set "Creator=Author Name"

# Edit document property (produces report.edited.docx, fully verified)
metadata edit report.docx --set "Creator=Author Name"

# Edit PDF Info dictionary
metadata edit report.pdf --set Title="Final Submission" --remove Author

# Set PNG text chunk
metadata edit diagram.png --set "Description=Architecture Overview"

# Copy non-date metadata from a template
metadata edit report.docx --copy-from template.docx

# Sanitize and strip privacy-sensitive metadata (EXIF/GPS/XMP)
metadata clean photo.jpg --profile privacy --output-dir ./sanitized/

# In-place modification (replaces original, automatically creates timestamped backup)
metadata edit report.docx --in-place --yes --set Title="Published Version"
```

### Timeline & Integrity Manifests

```sh
# Generate chronological event timeline (separating filesystem, embedded, archive, and git events)
metadata timeline ./project_dir -r

# Generate a verifiable SHA-256 integrity manifest
metadata hash ./submissions -r --manifest manifest.json

# Verify a directory against an existing manifest
metadata verify-manifest manifest.json --root ./submissions
```

### Academic & Forensic Assessment

Examine whether file evidence aligns with declared project milestones:

```sh
# Run a single-shot assessment against defined dates and milestones
metadata assess ./student-submission \
  --case stud-2024-001 \
  --start 2024-01-10 \
  --deadline 2024-06-01 \
  --milestone "Proposal=2024-02-01:proposal/" \
  --html review_report.html

# Multi-submission case workflow with SQLite persistence:
metadata case new stud-001 --title "Alice Thesis" --start 2024-01-10 --deadline 2024-06-01
metadata case add stud-001 ./milestone-1 --label "Preliminary Submission"
metadata case add stud-001 ./final-draft --label "Final Submission"

# Record reviewer review disposition (dismissals require a justification reason)
metadata case finding stud-001 TS-BEFORE-START-8f2a1b --status dismissed --reason "Department template file"

# Generate comprehensive audit report
metadata case report stud-001 --html audit.html --csv findings.csv
```

### Interactive Terminal UI (TUI)

Launch the full-screen terminal interface:

```sh
metadata tui ./submissions
```

#### TUI Keyboard Shortcuts

| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | Cycle through panels |
| `1` – `5` | Switch tabs: Files (1), Inspector (2), Compare (3), Timeline (4), Cases (5) |
| `j` / `k` or `↑` / `↓` | Move selection up / down |
| `/` | Focus search / filter input |
| `r` | Toggle reveal masked sensitive values (GPS, PII, serials) |
| `c` | Copy selected field to clipboard via OSC 52 sequence |
| `e` | Open edit modal for selected file |
| `T` | Switch visual theme (Default, Nord, High Contrast, Amber) |
| `?` | Toggle help overlay |
| `q` / `Esc` | Close dialog or exit TUI |

---

## Supported Formats Matrix

| Category | Format | Detection | Native Read | Native Write | External Backend Support |
|---|---|---|---|---|---|
| **Images** | JPEG | Magic Bytes | EXIF, IPTC, XMP, ICC, JFIF, Comments | Comments, Strip (`clean`) | ExifTool (all tags, GPS, maker notes) |
| | PNG | Magic Bytes | IHDR, tEXt/iTXt, tIME, XMP, eXIf, ICC | Text chunks, Strip | ExifTool |
| | TIFF / RAW | Magic Bytes | EXIF | — | ExifTool (CR2, NEF, ARW, DNG, etc.) |
| | WebP, GIF, BMP | Magic Bytes | Header dimensions, WebP EXIF | — | ExifTool |
| | HEIC / AVIF | `ftyp` brand | Container EXIF | — | ExifTool |
| **Documents** | DOCX, XLSX, PPTX | ZIP structure | Core, App, Custom props, Tracked changes | Core & App properties | ExifTool |
| | ODT, ODS, ODP | `mimetype` | meta.xml properties, Generator | Standard metadata fields | — |
| | PDF | `%PDF` header | Info dict, uncompressed XMP, Revisions | Info dictionary fields | ExifTool |
| | EPUB | `mimetype` | OPF `dc:` metadata | — | — |
| | DOC, XLS, PPT (OLE2) | Magic Bytes | Detected & flagged | — | ExifTool |
| **Audio / Video**| MP4, MOV, MKV, AVI | Magic Bytes | Container detection | — | FFprobe & ExifTool |
| | MP3, FLAC, WAV, AAC| Magic Bytes | Container detection | — | FFprobe & ExifTool |
| **Archives** | ZIP / JAR | Magic Bytes | Bounded entry listing, mtime | — | — |
| | TAR / TGZ / GZIP | Magic Bytes | Bounded entry listing | — | — |

*Detailed format specifications are documented in [docs/FORMATS.md](docs/FORMATS.md).*

---

## Editing Safety & Pipeline

The editing engine enforces rigorous validation before modifying any file:

1. **Pre-flight Inspection**: Reads and validates target fields and value types against format specifications.
2. **Diff Plan**: Generates an in-memory diff of proposed modifications.
3. **Isolated Temporary File**: Writes the edited output to an isolated temporary file in the target directory. The source file is never opened in write mode.
4. **Re-Extraction & Verification**: Extracts metadata back from the temporary file to verify that every requested field matches, deleted fields are removed, and unrelated fields remain unchanged.
5. **Automated Backup**: When using `--in-place`, creates a backup named `<file>.metadata-backup-<timestamp>`.
6. **Atomic Rename**: Uses OS-level atomic replace to swap the verified temporary file into position.
7. **Audit Logging**: Appends a verifiable record to `operations.jsonl` linking source and destination hashes.

*Read [docs/EDITING-SAFETY.md](docs/EDITING-SAFETY.md) for full safety guarantees.*

---

## Privacy, Limits & Security

- **100% Offline & Private**: Operates entirely on the local machine without network requests or telemetry.
- **Untrusted Input Hardening**: Archives are traversed without extraction; recursion limits, entry caps, and decompression ratios protect against zip bombs.
- **Subprocess Isolation**: External helpers (ExifTool, FFprobe, git) are invoked with argument vectors (never shells), execution timeouts, and bounded stdout/stderr buffers.
- **PII Redaction**: Author identities, serial numbers, and geographic coordinates are automatically masked when `--redact` is specified or when viewing reports.

---

## Known Limitations

- Native writing supports standard fields in DOCX/XLSX/PPTX, ODT/ODS/ODP, PDF Info dictionaries, PNG text chunks, and JPEG comments. Arbitrary raw EXIF/IPTC/XMP writes on binary formats require ExifTool.
- PDF editing rewrites the file structure, omitting prior incremental revision blocks.
- Media stream analysis (audio/video bitrates, codecs) requires FFprobe.
- The TUI Cases tab provides viewing capabilities; advanced case management operations are performed via the CLI (`metadata case`).

---

## Performance Benchmarks

Measured on a synthetic benchmark suite consisting of 2,000 files (mixed DOCX, JPEG, PDF, ZIP, and TXT) with SHA-256 computation and native extraction enabled:

| Concurrency (Jobs) | Run 1 Duration | Run 2 Duration |
|---|---|---|
| **1 (Sequential)** | 939 ms | 621 ms |
| **2** | 433 ms | 382 ms |
| **4** | 223 ms | 220 ms |
| **8 (Parallel)** | **173 ms** | **187 ms** |

*Run benchmarks locally with `cargo bench -p metadata-engine --bench scan`.*

---

## Troubleshooting

| Issue | Cause | Solution |
|---|---|---|
| `needs an interactive terminal` | `metadata tui` requires an interactive TTY | Run in a real terminal or use `inspect`/`extract` for scripts |
| `Fields missing for video or RAW` | Optional backend missing | Run `metadata doctor` and install FFprobe / ExifTool |
| `Field is not writable` | Field is read-only or unsupported natively | Run `metadata capabilities` or check `docs/FORMATS.md` |
| `File already exists (use --force)` | Destination file already exists | Pass `--force` or specify a distinct `--output-dir` |
| `git history unavailable` | Git is missing or repository ownership check failed | Ensure `git` is installed and the directory is safe |
| Colors inverted or missing | Terminal color profile mismatch | Run with `COLORTERM=truecolor` or press `T` in the TUI |

---

## Development & Testing

```sh
# Format check
cargo fmt --all -- --check

# Lints and static analysis
cargo clippy --workspace --all-targets -- -D warnings

# Execute all workspace tests
cargo test --workspace

# Run extraction benchmarks
cargo bench -p metadata-engine --bench scan
```

---

## License

This project is licensed under the [MIT License](LICENSE).
