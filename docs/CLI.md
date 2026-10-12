# CLI reference

Generated from `metadata --help` output of this build; regenerate when commands change.

## Conventions

* **stdout** carries results (human text, JSON or CSV). **stderr** carries progress, notes and errors. JSON/CSV output never contains decorative text.
* **Exit codes:** `0` success · `1` failure · `2` usage error · `3` finished but some files failed or were skipped (batch commands). `verify-manifest` returns `1` when verification fails; `assess` returns `1` if an input file changed while it ran.
* **Formats:** `-f human|json|csv`. JSON documents carry `schema_version` (currently `1.0.0`); breaking changes bump the major version and are listed in `CHANGELOG.md`.
* **Field keys** are `Group:Name` (for example `OOXML-Core:Creator`). `search --field` and `edit --set` also accept a bare name when unambiguous.
* **Output files** are never overwritten without `--force`. Reports are written to a temporary file and renamed into place.
* **Directories** need `-r/--recursive`. `.git` is skipped unless `--include-vcs`. Symlinks are never followed.
* Configuration: `metadata config --init` creates a commented template (`METADATA_CONFIG` overrides the path). Flags override the file.
* Environment: `NO_COLOR`, `METADATA_LOG` (tracing filter), `METADATA_EXIFTOOL` / `METADATA_FFPROBE` (explicit tool paths), `METADATA_DB` (case database).

## `metadata`

```text
Inspect, search, compare and fingerprint metadata across documents, images, media, archives and source projects.
Machine-readable output (json/csv) goes to stdout; diagnostics go to stderr.

Exit codes: 0 success · 1 failure · 2 usage error · 3 completed with per-file errors

Usage: metadata [OPTIONS] <COMMAND>

Commands:
  inspect          Human-readable inspection of files, folders and archives
  extract          Extract metadata as structured data (default: JSON)
  search           Find files by metadata field and/or value
  compare          Compare metadata of two files or two directories
  batch            Process a folder and produce a per-file success/failure report (default: JSON)
  hash             Compute file hashes, optionally writing an integrity manifest
  verify-manifest  Verify files against a manifest created by `hash --manifest`
  timeline         Build a chronological timeline from embedded, archive, filesystem and git sources (kept separate)
  assess           Evidence-based review of a project against declared dates; produces findings, not verdicts
  edit             Edit metadata fields safely (writes a new file unless --in-place; always re-reads and verifies)
  clean            Remove metadata by profile (privacy | all | gps); writes a new file unless --in-place
  case             Persistent assessment cases: submissions, reviewer decisions, notes and reports
  tui              Open the interactive full-screen interface
  capabilities     Show format support and which backend handles each format
  doctor           Diagnose backends, configuration and environment
  config           Show effective configuration (see --path, --init)
  help             Print this message or the help of the given subcommand(s)

Options:
  -v, --verbose...
          Increase diagnostics on stderr (-v info, -vv debug)

      --no-color
          Disable colored output

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Examples:
  metadata inspect photo.jpg
  metadata extract ./submissions -r -f json -o out.json
  metadata search ./photos --field GPSLatitude -r
  metadata compare a.docx b.docx
  metadata hash ./submissions -r --manifest manifest.json
  metadata verify-manifest manifest.json --root ./submissions
  metadata doctor
```

## `metadata inspect`

```text
Human-readable inspection of files, folders and archives

Usage: metadata inspect [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  

Options:
  -r, --recursive          Descend into directories
  -v, --verbose...         Increase diagnostics on stderr (-v info, -vv debug)
  -f, --format <FORMAT>    Output format [default: from config, else per-command] [possible values: human, json, csv]
      --no-color           Disable colored output
  -o, --output <OUTPUT>    Write output to FILE instead of stdout (refuses to overwrite without --force)
      --force              Allow --output to replace an existing file
      --cache              Reuse cached extraction results keyed by file hash and settings (stored in the config dir)
      --all                Include raw/unknown fields and list every archive entry
      --deep               Inspect nested archives (bounded by archive limits)
      --no-hash            Skip SHA-256 computation
      --no-external        Do not use exiftool/ffprobe even if installed
      --redact             Mask GPS and personal identifiers (author, serial numbers, ...)
  -j, --jobs <JOBS>        Parallel workers
      --timeout <TIMEOUT>  Per external tool timeout (seconds)
      --exclude <DIR>      Directory name to skip while walking (repeatable)
      --include-vcs        Include `.git` internals when walking (excluded by default)
      --filter <TEXT>      Only show fields whose group, name or value contains TEXT
  -h, --help               Print help
```

## `metadata extract`

```text
Extract metadata as structured data (default: JSON)

Usage: metadata extract [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  

Options:
  -r, --recursive          Descend into directories
  -v, --verbose...         Increase diagnostics on stderr (-v info, -vv debug)
  -f, --format <FORMAT>    Output format [default: from config, else per-command] [possible values: human, json, csv]
      --no-color           Disable colored output
  -o, --output <OUTPUT>    Write output to FILE instead of stdout (refuses to overwrite without --force)
      --force              Allow --output to replace an existing file
      --cache              Reuse cached extraction results keyed by file hash and settings (stored in the config dir)
      --all                Include raw/unknown fields and list every archive entry
      --deep               Inspect nested archives (bounded by archive limits)
      --no-hash            Skip SHA-256 computation
      --no-external        Do not use exiftool/ffprobe even if installed
      --redact             Mask GPS and personal identifiers (author, serial numbers, ...)
  -j, --jobs <JOBS>        Parallel workers
      --timeout <TIMEOUT>  Per external tool timeout (seconds)
      --exclude <DIR>      Directory name to skip while walking (repeatable)
      --include-vcs        Include `.git` internals when walking (excluded by default)
      --filter <TEXT>      Only show fields whose group, name or value contains TEXT
  -h, --help               Print help
```

## `metadata search`

```text
Find files by metadata field and/or value

Usage: metadata search [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  

