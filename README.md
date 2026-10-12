# metadata

A fast, local, read-first command-line tool and terminal UI for **extracting, comparing, editing and reviewing file metadata**, with a transparent assessment mode that helps a lecturer examine whether a final-year project's digital evidence is consistent with the timeline a student describes.

It never decides that anyone cheated. It reports *observations*, explains what each one could also mean, and keeps the original evidence untouched.

```text
$ metadata inspect report.docx
report.docx
  Format      Word document (OOXML) · application/vnd.openxmlformats-… · extension matches
  Size        14.2 KiB (14540 bytes)
  SHA-256     9f2c…
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

## Highlights

* **Content-based format detection** (signatures, not extensions) with extension-mismatch reporting.
* **Native, always-available extraction** for images (EXIF with timezone offsets, IPTC, XMP, ICC, PNG chunks), Office/OpenDocument/EPUB, PDF (Info dictionary, XMP, incremental-update, signature, JavaScript and attachment indicators), ZIP/TAR/GZIP archives and text/source files. Optional **ExifTool** and **FFprobe** adapters widen coverage (RAW, HEIC, video, audio, legacy Office) when installed.
* **Honest time handling:** a timestamp without a timezone is never treated as UTC; it is shown as such and compared with a tolerance.
* **Safe editing:** plan → diff preview → write a *new* file → re-read and verify → atomic replace. Originals are untouched unless you pass `--in-place`, and then a backup is kept. Signed documents require explicit acknowledgement.
* **Integrity manifests** (`hash --manifest`, `verify-manifest`) with a versioned schema.
* **Assessment engine** with stable finding IDs, `INFO` / `REVIEW` / `HIGH PRIORITY` levels, benign alternatives, and separate confidence for the observation and for any interpretation.
* **Cases:** SQLite-backed submissions, reviewer dispositions (dismissal requires a reason), notes, and printable HTML/CSV/JSON reports.
* **Full-screen TUI** (`metadata tui`) with a searchable file list, grouped metadata inspector, provenance panel, compare, timeline and edit-to-copy.
* **Bounded parallelism, cancellation (Ctrl+C), resource limits** for hostile input; opt-in extraction cache keyed by content hash.

## Install

Requires a recent stable Rust toolchain (tested with 1.99; minimum supported 1.82).

```sh
git clone <this repository> && cd metadata
cargo build --release
# the binary is target/release/metadata
cp target/release/metadata /usr/local/bin/      # or anywhere on your PATH
metadata doctor                                  # shows which optional backends were found
```

### Optional backends

| Backend | Adds | macOS | Debian/Ubuntu | Windows |
|---|---|---|---|---|
| ExifTool | RAW, HEIC, legacy Office, broad tag read; tag writing for many formats | `brew install exiftool` | `apt install libimage-exiftool-perl` | `winget install OliverBetz.ExifTool` |
| FFprobe | audio/video containers and streams | `brew install ffmpeg` | `apt install ffmpeg` | `winget install Gyan.FFmpeg` |
| git | repository history for `timeline` / `assess` | Xcode CLT | `apt install git` | `winget install Git.Git` |

Without them everything native still works; formats that need a missing backend are reported as *unavailable*, never silently skipped. Set `METADATA_EXIFTOOL` / `METADATA_FFPROBE` to point at specific binaries.

Apache Tika and 7-Zip adapters are **not implemented**.

## Quick start

```sh
metadata inspect photo.jpg                     # human-readable
metadata extract ./submissions -r -f json -o all.json
metadata search ./photos -r --field GPSLatitude
metadata compare original.docx revised.docx
metadata timeline ./project -r                 # embedded / filesystem / archive / git, kept separate
metadata hash ./submissions -r --manifest manifest.json
metadata verify-manifest manifest.json --root ./submissions
metadata capabilities                          # what is readable / writable here
metadata tui ./submissions                     # interactive
```

### Editing

```sh
metadata edit report.docx --dry-run --set "Creator=Student Name"   # preview only
metadata edit report.docx --set "Creator=Student Name"             # writes report.edited.docx, verified
metadata edit report.pdf --set Title="Final" --remove Author
metadata edit photo.png --set "Description=Field trip"             # PNG text chunk
metadata edit a.docx --copy-from template.docx                     # non-date fields only
metadata clean photo.jpg --profile privacy --output-dir cleaned/   # strip EXIF/GPS/XMP/IPTC/comments
metadata edit report.docx --in-place --yes --set Title=X           # replaces the original; backup kept
```

What is writable is deliberately small and verified; see [docs/FORMATS.md](docs/FORMATS.md) and [docs/EDITING-SAFETY.md](docs/EDITING-SAFETY.md).

### Academic assessment

```sh
metadata assess ./student-project --case stud-001 \
  --start 2024-01-10 --deadline 2024-06-01 \
  --milestone "Proposal=2024-02-01:proposal/" \
  --html review.html

# or keep a case over several submissions
metadata case new stud-001 --title "Alice" --start 2024-01-10 --deadline 2024-06-01
metadata case add stud-001 ./submission-1 --label "milestone 1"
metadata case add stud-001 ./submission-2 --label "final"       # compared with the previous one
metadata case finding stud-001 TS-BEFORE-START-1a2b3c4d --status dismissed --reason "course template"
metadata case report stud-001 --html report.html --csv findings.csv
```

Read [docs/ASSESSMENT.md](docs/ASSESSMENT.md) before relying on any finding.

## Privacy and safety

* Everything runs locally. No telemetry, no uploads.
* Files are treated as untrusted: nothing is executed, archives are only *listed* (path traversal, symlinks, nested depth, entry counts and compression ratios are limited), parsers are panic-isolated, external tools run without a shell, with timeouts and output caps.
* GPS and personal identifiers (author names, serial numbers…) are masked by `--redact`, hidden in the TUI until you press `r`, and masked by default in assessment reports.
* Deleting a field does not prove every trace is gone; see the editing guide.

## Known limitations

* Native writing covers DOCX/XLSX/PPTX, ODT/ODS/ODP, PDF Info fields, PNG text chunks and JPEG comments/stripping only. Individual EXIF/IPTC/XMP tag edits and everything for video/audio/RAW/HEIC require ExifTool; that path builds safe arguments and verifies the result but is **not covered by automated tests against a real ExifTool**.
* Video/audio/RAW/HEIC/legacy-Office metadata and 7z/RAR/BZ2/XZ listing are unavailable without the matching tool.
* Assessment does not yet compare source code, documentation and screenshots for consistency (listed as *not performed* in every report).
* The TUI's Cases and Reports tabs are placeholders pointing at the CLI; use `metadata case …`.
* PDF edits rewrite the whole file (earlier incremental revisions are not preserved).
* Clipboard copy in the TUI uses the OSC 52 terminal sequence; not every terminal supports it.

## Troubleshooting

| Symptom | Fix |
|---|---|
| "needs an interactive terminal" | `metadata tui` requires a TTY; use `inspect`/`extract` in scripts |
| Fields missing for video/RAW/HEIC | run `metadata doctor`; install ExifTool/FFprobe |
| `edit` says a field is not writable | `metadata capabilities` and `docs/FORMATS.md` list what is; others need ExifTool |
| "already exists (use --force)" | outputs are never overwritten silently |
| git history "unavailable" | git missing, or the repository belongs to another user ("dubious ownership") |
| Colors look wrong | `NO_COLOR=1` or press `T` in the TUI to switch theme; `COLORTERM=truecolor` enables the full palette |

## Development

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
cargo bench -p metadata-engine --bench scan     # sequential vs parallel scan, measured
```

Tests use synthetic fixtures built by `crates/metadata-testkit`; no real student data is stored. See [ARCHITECTURE.md](ARCHITECTURE.md) for the crate layout and [STATE.md](STATE.md) for the live feature ledger.

### Measured scan performance

Synthetic mix of 2000 DOCX/JPEG/PDF/ZIP/TXT files, 8 logical CPUs, release build, extraction + SHA-256, no external tools (two runs; absolute numbers vary by machine and are not a promise):

| jobs | run 1 | run 2 |
|---|---|---|
| 1 | 939 ms | 621 ms |
| 2 | 433 ms | 382 ms |
| 4 | 223 ms | 220 ms |
| 8 | 173 ms | 187 ms |

## License

MIT.
