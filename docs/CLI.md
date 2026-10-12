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

Options:
      --field <FIELD>      Field name (e.g. Author, GPSLatitude) or Group:Name. Common names expand to equivalents across formats
  -v, --verbose...         Increase diagnostics on stderr (-v info, -vv debug)
      --no-color           Disable colored output
      --value <VALUE>      Substring to match in values (case-insensitive)
  -r, --recursive          Descend into directories
  -f, --format <FORMAT>    Output format [default: from config, else per-command] [possible values: human, json, csv]
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

## `metadata compare`

```text
Compare metadata of two files or two directories

Usage: metadata compare [OPTIONS] <LEFT> <RIGHT>

Arguments:
  <LEFT>   
  <RIGHT>  

Options:
      --show-identical     Also list identical fields
  -v, --verbose...         Increase diagnostics on stderr (-v info, -vv debug)
      --no-color           Disable colored output
  -r, --recursive          Descend into directories
  -f, --format <FORMAT>    Output format [default: from config, else per-command] [possible values: human, json, csv]
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

## `metadata batch`

```text
Process a folder and produce a per-file success/failure report (default: JSON)

Usage: metadata batch [OPTIONS] <PATHS>...

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

## `metadata hash`

```text
Compute file hashes, optionally writing an integrity manifest

Usage: metadata hash [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  

Options:
      --algorithm <ALGORITHM>  [default: sha256] [possible values: sha256, sha512]
  -v, --verbose...             Increase diagnostics on stderr (-v info, -vv debug)
      --manifest <MANIFEST>    Write a versioned manifest JSON to this path
      --no-color               Disable colored output
  -r, --recursive              Descend into directories
  -f, --format <FORMAT>        Output format [default: from config, else per-command] [possible values: human, json, csv]
  -o, --output <OUTPUT>        Write output to FILE instead of stdout (refuses to overwrite without --force)
      --force                  Allow --output to replace an existing file
      --cache                  Reuse cached extraction results keyed by file hash and settings (stored in the config dir)
      --all                    Include raw/unknown fields and list every archive entry
      --deep                   Inspect nested archives (bounded by archive limits)
      --no-hash                Skip SHA-256 computation
      --no-external            Do not use exiftool/ffprobe even if installed
      --redact                 Mask GPS and personal identifiers (author, serial numbers, ...)
  -j, --jobs <JOBS>            Parallel workers
      --timeout <TIMEOUT>      Per external tool timeout (seconds)
      --exclude <DIR>          Directory name to skip while walking (repeatable)
      --include-vcs            Include `.git` internals when walking (excluded by default)
      --filter <TEXT>          Only show fields whose group, name or value contains TEXT
  -h, --help                   Print help
```

## `metadata verify-manifest`

```text
Verify files against a manifest created by `hash --manifest`

Usage: metadata verify-manifest [OPTIONS] <MANIFEST>

Arguments:
  <MANIFEST>  

Options:
      --root <ROOT>      Directory the manifest paths are relative to [default: manifest's recorded root]
  -v, --verbose...       Increase diagnostics on stderr (-v info, -vv debug)
      --check-extra      Also report files under --root that are not in the manifest
      --no-color         Disable colored output
  -f, --format <FORMAT>  [possible values: human, json, csv]
  -h, --help             Print help
```

## `metadata timeline`

```text
Build a chronological timeline from embedded, archive, filesystem and git sources (kept separate)

Usage: metadata timeline [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  

Options:
      --source <SOURCE>      Show only these sources (repeatable): declared, embedded, archive, filesystem, git, intake
  -v, --verbose...           Increase diagnostics on stderr (-v info, -vv debug)
      --no-color             Disable colored output
      --no-git               Do not read git history even if a repository is present
      --manifest <MANIFEST>  Add the intake time recorded in a manifest created by `hash --manifest`
      --claims <CLAIMS>      Claims file (TOML/JSON) whose dates are shown as `declared` events
  -r, --recursive            Descend into directories
  -f, --format <FORMAT>      Output format [default: from config, else per-command] [possible values: human, json, csv]
  -o, --output <OUTPUT>      Write output to FILE instead of stdout (refuses to overwrite without --force)
      --force                Allow --output to replace an existing file
      --cache                Reuse cached extraction results keyed by file hash and settings (stored in the config dir)
      --all                  Include raw/unknown fields and list every archive entry
      --deep                 Inspect nested archives (bounded by archive limits)
      --no-hash              Skip SHA-256 computation
      --no-external          Do not use exiftool/ffprobe even if installed
      --redact               Mask GPS and personal identifiers (author, serial numbers, ...)
  -j, --jobs <JOBS>          Parallel workers
      --timeout <TIMEOUT>    Per external tool timeout (seconds)
      --exclude <DIR>        Directory name to skip while walking (repeatable)
      --include-vcs          Include `.git` internals when walking (excluded by default)
      --filter <TEXT>        Only show fields whose group, name or value contains TEXT
  -h, --help                 Print help
```

## `metadata assess`

```text
Evidence-based review of a project against declared dates; produces findings, not verdicts

Usage: metadata assess [OPTIONS] <PATH>

Arguments:
  <PATH>  Project folder (read-only)

Options:
      --case <CASE>                  Case identifier recorded in the report [default: unnamed]
  -v, --verbose...                   Increase diagnostics on stderr (-v info, -vv debug)
      --no-color                     Disable colored output
      --student <STUDENT>            
      --start <START>                Claimed start date (YYYY-MM-DD or RFC 3339)
      --completion <COMPLETION>      Claimed completion date
      --deadline <DEADLINE>          Submission deadline
      --milestone <SPEC>             Claimed milestone `Name=DATE` or `Name=DATE:path-substring` (repeatable)
      --claims <CLAIMS>              Claims/thresholds file (TOML or JSON); flags override it
      --previous <PREVIOUS>          Previous submission folder to compare against
      --no-git                       Do not read git history
      --include-personal             Keep author names and other personal identifiers in evidence values (masked by default)
      --html <FILE>                  Also write a printable HTML report to FILE
      --min-severity <MIN_SEVERITY>  Hide findings below this level [default: info] [possible values: info, review, high]
  -r, --recursive                    Descend into directories
  -f, --format <FORMAT>              Output format [default: from config, else per-command] [possible values: human, json, csv]
  -o, --output <OUTPUT>              Write output to FILE instead of stdout (refuses to overwrite without --force)
      --force                        Allow --output to replace an existing file
      --cache                        Reuse cached extraction results keyed by file hash and settings (stored in the config dir)
      --all                          Include raw/unknown fields and list every archive entry
      --deep                         Inspect nested archives (bounded by archive limits)
      --no-hash                      Skip SHA-256 computation
      --no-external                  Do not use exiftool/ffprobe even if installed
      --redact                       Mask GPS and personal identifiers (author, serial numbers, ...)
  -j, --jobs <JOBS>                  Parallel workers
      --timeout <TIMEOUT>            Per external tool timeout (seconds)
      --exclude <DIR>                Directory name to skip while walking (repeatable)
      --include-vcs                  Include `.git` internals when walking (excluded by default)
      --filter <TEXT>                Only show fields whose group, name or value contains TEXT
  -h, --help                         Print help
```

## `metadata edit`

```text
Edit metadata fields safely (writes a new file unless --in-place; always re-reads and verifies)

Usage: metadata edit [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  

Options:
      --set <KEY=VALUE>        Set a field: `Group:Name=value` or `Name=value` (repeatable)
  -v, --verbose...             Increase diagnostics on stderr (-v info, -vv debug)
      --no-color               Disable colored output
      --remove <KEY>           Remove a field (repeatable)
      --copy-from <FILE>       Copy every natively writable, non-date field from this file
      --interactive            Choose fields interactively (reads commands from stdin)
      --dry-run                Show the plan and exit without writing anything
      --in-place               Modify the original file (a backup is kept unless --no-backup)
      --no-backup              Skip the backup copy when using --in-place
      --output-dir <DIR>       Directory for outputs when editing several files
  -y, --yes                    Confirm in-place or multi-file writes without prompting
      --accept-signature-loss  Proceed even though the file looks digitally signed (the signature will be invalidated)
  -r, --recursive              Descend into directories
  -f, --format <FORMAT>        Output format [default: from config, else per-command] [possible values: human, json, csv]
  -o, --output <OUTPUT>        Write output to FILE instead of stdout (refuses to overwrite without --force)
      --force                  Allow --output to replace an existing file
      --cache                  Reuse cached extraction results keyed by file hash and settings (stored in the config dir)
      --all                    Include raw/unknown fields and list every archive entry
      --deep                   Inspect nested archives (bounded by archive limits)
      --no-hash                Skip SHA-256 computation
      --no-external            Do not use exiftool/ffprobe even if installed
      --redact                 Mask GPS and personal identifiers (author, serial numbers, ...)
  -j, --jobs <JOBS>            Parallel workers
      --timeout <TIMEOUT>      Per external tool timeout (seconds)
      --exclude <DIR>          Directory name to skip while walking (repeatable)
      --include-vcs            Include `.git` internals when walking (excluded by default)
      --filter <TEXT>          Only show fields whose group, name or value contains TEXT
  -h, --help                   Print help
```

## `metadata clean`

```text
Remove metadata by profile (privacy | all | gps); writes a new file unless --in-place

Usage: metadata clean [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  

Options:
      --profile <PROFILE>      [default: privacy] [possible values: privacy, all, gps]
  -v, --verbose...             Increase diagnostics on stderr (-v info, -vv debug)
      --dry-run                Show the plan and exit without writing anything
      --no-color               Disable colored output
      --in-place               Modify the original file (a backup is kept unless --no-backup)
      --no-backup              Skip the backup copy when using --in-place
      --output-dir <DIR>       Directory for outputs when editing several files
  -y, --yes                    Confirm in-place or multi-file writes without prompting
      --accept-signature-loss  Proceed even though the file looks digitally signed (the signature will be invalidated)
  -r, --recursive              Descend into directories
  -f, --format <FORMAT>        Output format [default: from config, else per-command] [possible values: human, json, csv]
  -o, --output <OUTPUT>        Write output to FILE instead of stdout (refuses to overwrite without --force)
      --force                  Allow --output to replace an existing file
      --cache                  Reuse cached extraction results keyed by file hash and settings (stored in the config dir)
      --all                    Include raw/unknown fields and list every archive entry
      --deep                   Inspect nested archives (bounded by archive limits)
      --no-hash                Skip SHA-256 computation
      --no-external            Do not use exiftool/ffprobe even if installed
      --redact                 Mask GPS and personal identifiers (author, serial numbers, ...)
  -j, --jobs <JOBS>            Parallel workers
      --timeout <TIMEOUT>      Per external tool timeout (seconds)
      --exclude <DIR>          Directory name to skip while walking (repeatable)
      --include-vcs            Include `.git` internals when walking (excluded by default)
      --filter <TEXT>          Only show fields whose group, name or value contains TEXT
  -h, --help                   Print help
```

## `metadata case`

```text
Persistent assessment cases: submissions, reviewer decisions, notes and reports

Usage: metadata case [OPTIONS] <COMMAND>

Commands:
  new      Create a case with declared dates
  list     List or search cases
  show     Show a case: claims, submissions and finding dispositions
  add      Ingest a submission folder: assess it against the case claims and the previous submission
  finding  Set a finding's disposition (open | reviewed | dismissed; dismissal needs --reason)
  note     Add a free-text reviewer note
  report   Export a report for a submission (default: latest)
  diff     Compare two submissions of a case file by file
  help     Print this message or the help of the given subcommand(s)

Options:
      --db <DB>     Case database [default: <config dir>/cases.sqlite] [env: METADATA_DB=]
  -v, --verbose...  Increase diagnostics on stderr (-v info, -vv debug)
      --no-color    Disable colored output
  -h, --help        Print help
```

## `metadata tui`

```text
Open the interactive full-screen interface

Usage: metadata tui [OPTIONS] [PATH]

Arguments:
  [PATH]  Directory or file to open [default: current directory]

Options:
      --no-scan      Do not start scanning on launch
  -v, --verbose...   Increase diagnostics on stderr (-v info, -vv debug)
      --no-color     Disable colored output
      --no-external  Do not use exiftool/ffprobe even if installed
  -h, --help         Print help
```

## `metadata capabilities`

```text
Show format support and which backend handles each format

Usage: metadata capabilities [OPTIONS]

Options:
  -f, --format <FORMAT>  [possible values: human, json, csv]
  -v, --verbose...       Increase diagnostics on stderr (-v info, -vv debug)
      --no-color         Disable colored output
  -h, --help             Print help
```

## `metadata doctor`

```text
Diagnose backends, configuration and environment

Usage: metadata doctor [OPTIONS]

Options:
  -f, --format <FORMAT>  [possible values: human, json, csv]
  -v, --verbose...       Increase diagnostics on stderr (-v info, -vv debug)
      --no-color         Disable colored output
  -h, --help             Print help
```

## `metadata config`

```text
Show effective configuration (see --path, --init)

Usage: metadata config [OPTIONS]

Options:
      --path        Print the config file location only
  -v, --verbose...  Increase diagnostics on stderr (-v info, -vv debug)
      --init        Create a commented template config file if none exists
      --no-color    Disable colored output
  -h, --help        Print help
```

### `metadata case new`

```text
Create a case with declared dates

Usage: metadata case new [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --db <DB>                  Case database [default: <config dir>/cases.sqlite] [env: METADATA_DB=]
      --title <TITLE>            [default: ""]
  -v, --verbose...               Increase diagnostics on stderr (-v info, -vv debug)
      --no-color                 Disable colored output
      --student <STUDENT>        
      --start <START>            
      --completion <COMPLETION>  
      --deadline <DEADLINE>      
      --milestone <SPEC>         
      --claims <CLAIMS>          
  -h, --help                     Print help
```

### `metadata case list`

```text
List or search cases

Usage: metadata case list [OPTIONS]

Options:
      --db <DB>          Case database [default: <config dir>/cases.sqlite] [env: METADATA_DB=]
      --search <SEARCH>  
  -v, --verbose...       Increase diagnostics on stderr (-v info, -vv debug)
  -f, --format <FORMAT>  [possible values: human, json, csv]
      --no-color         Disable colored output
  -h, --help             Print help
```

### `metadata case show`

```text
Show a case: claims, submissions and finding dispositions

Usage: metadata case show [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --db <DB>          Case database [default: <config dir>/cases.sqlite] [env: METADATA_DB=]
  -f, --format <FORMAT>  [possible values: human, json, csv]
  -v, --verbose...       Increase diagnostics on stderr (-v info, -vv debug)
      --no-color         Disable colored output
  -h, --help             Print help
```

### `metadata case add`

```text
Ingest a submission folder: assess it against the case claims and the previous submission

Usage: metadata case add [OPTIONS] <ID> <PATH>

Arguments:
  <ID>    
  <PATH>  

Options:
      --db <DB>            Case database [default: <config dir>/cases.sqlite] [env: METADATA_DB=]
      --label <LABEL>      [default: ""]
  -v, --verbose...         Increase diagnostics on stderr (-v info, -vv debug)
      --no-color           Disable colored output
      --no-git             
      --include-personal   
  -r, --recursive          Descend into directories
  -f, --format <FORMAT>    Output format [default: from config, else per-command] [possible values: human, json, csv]
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

### `metadata case finding`

```text
Set a finding's disposition (open | reviewed | dismissed; dismissal needs --reason)

Usage: metadata case finding [OPTIONS] --status <STATUS> <ID> <FINDING_ID>

Arguments:
  <ID>          
  <FINDING_ID>  

Options:
      --db <DB>          Case database [default: <config dir>/cases.sqlite] [env: METADATA_DB=]
      --status <STATUS>  [possible values: open, reviewed, dismissed]
  -v, --verbose...       Increase diagnostics on stderr (-v info, -vv debug)
      --no-color         Disable colored output
      --note <NOTE>      
      --reason <REASON>  
  -h, --help             Print help
```

### `metadata case note`

```text
Add a free-text reviewer note

Usage: metadata case note [OPTIONS] <ID> <TEXT>

Arguments:
  <ID>    
  <TEXT>  

Options:
      --db <DB>     Case database [default: <config dir>/cases.sqlite] [env: METADATA_DB=]
  -v, --verbose...  Increase diagnostics on stderr (-v info, -vv debug)
      --no-color    Disable colored output
  -h, --help        Print help
```

### `metadata case report`

```text
Export a report for a submission (default: latest)

Usage: metadata case report [OPTIONS] <ID>

Arguments:
  <ID>  

Options:
      --db <DB>                  Case database [default: <config dir>/cases.sqlite] [env: METADATA_DB=]
      --submission <SUBMISSION>  
  -v, --verbose...               Increase diagnostics on stderr (-v info, -vv debug)
      --html <FILE>              
      --no-color                 Disable colored output
      --csv <FILE>               
      --timeline                 Include the per-file timeline in the HTML report
      --force                    
  -f, --format <FORMAT>          [possible values: human, json, csv]
  -h, --help                     Print help
```

### `metadata case diff`

```text
Compare two submissions of a case file by file

Usage: metadata case diff [OPTIONS] <ID> <A> <B>

Arguments:
  <ID>  
  <A>   
  <B>   

Options:
      --db <DB>          Case database [default: <config dir>/cases.sqlite] [env: METADATA_DB=]
  -f, --format <FORMAT>  [possible values: human, json, csv]
  -v, --verbose...       Increase diagnostics on stderr (-v info, -vv debug)
      --no-color         Disable colored output
  -h, --help             Print help
```

