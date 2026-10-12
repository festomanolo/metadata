# Editing safety

## The pipeline

1. **Read** the file and show current values.
2. **Validate** field names (must be writable for this format) and values (dates must parse; OOXML dates need a timezone).
3. **Plan**: before → after diff, writability, warnings (signed file, whole-file rewrite, backend).
4. `--dry-run` stops here.
5. **Write to a private temporary file** in the destination directory. The source is never opened for writing.
6. **Re-extract** the temporary file and verify: format unchanged; every `set` reads back (dates compared as instants); every `remove` is gone; unrelated fields are compared and any difference is reported as *unexpected*.
7. **Backup** (only when replacing the original in place): `NAME.metadata-backup-<UTC timestamp>` beside the file; never overwritten.
8. **Atomic replace** by renaming the verified temporary file.
9. **Log** a JSON line (`operations.jsonl` in the config directory) linking source and result by SHA-256.

If anything fails, the temporary file is removed and the original is untouched.

## Defaults

* Output goes to `<name>.edited.<ext>` (or `<name>.cleaned.<ext>`) or `--output-dir`; existing files are never replaced without `--force`.
* `--in-place` is required to modify an original and needs `--yes` when not on a terminal.
* The TUI only ever writes new copies.
* ExifTool is run with `-o` to a separate file, so it never creates `_original` backups and never touches the source.

## Signed documents

DOCX/XLSX/PPTX with `_xmlsignatures/`, ODF with `documentsignatures.xml`, and PDFs with a signature dictionary are flagged. Any edit invalidates the signature; the CLI refuses unless `--accept-signature-loss` is given. The TUI refuses and points at the CLI.

## Cleaning profiles

| Profile | Native effect | Notes |
|---|---|---|
| `privacy` | JPEG: drops EXIF, XMP, IPTC, other APP segments, comments (keeps JFIF, ICC, Adobe color transform). PNG: drops text, EXIF, time chunks. PDF: removes Author/Creator and the XMP stream reference. Office/ODF: removes creator, last-modified-by, company, manager | ExifTool: `-all=` but keeps Orientation and ICC |
| `all` | everything the native writer knows how to remove | ExifTool: `-all=` (also loses orientation/ICC) |
| `gps` | not available natively | ExifTool only: `-gps:all=` |

## What deleting metadata does *not* do

Removing a field does not guarantee every trace is gone. Embedded thumbnails or previews, duplicate tags in other blocks, application-specific structures, revision history inside documents, and metadata in unsupported places may remain. Native PDF rewriting drops earlier incremental revisions, which can remove old values but also changes the file structure.

## Field values

Values are passed as single arguments (never through a shell). Control characters are rejected for ExifTool. XML text is escaped before insertion. Property parts are edited conservatively: if a needed namespace prefix is not declared the edit is refused rather than producing invalid XML.
