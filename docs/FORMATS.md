# Format capability matrix

`metadata capabilities` prints the live version of this table for your machine (it reflects which backends are installed).

*native* = built in. *exiftool\** = passes through the verify-after-write pipeline; tested only with a stub script, never against a real ExifTool.

| Format | Detection | Read | Write |
|---|---|---|---|
| JPEG | signature | native: EXIF (offset-aware dates, GPS + decimal position), IPTC, XMP, ICC, JFIF, comments, trailing-data warning; exiftool for maker notes | native: comment set/remove, metadata stripping (`clean` privacy/all). Field edits & GPS-only: exiftool\* |
| PNG | signature | native: IHDR, tEXt/iTXt, tIME, XMP, eXIf, ICC name | native: text chunks set/remove, strip |
| TIFF / RAW (NEF, CR2, ARW, DNG…) | signature | native: EXIF; exiftool for the rest | exiftool\* |
| WebP, GIF, BMP | signature | native: dimensions/version (+EXIF in WebP) | exiftool\* |
| HEIC/HEIF/AVIF | `ftyp` brand | native EXIF via container where possible; exiftool recommended | exiftool\* |
| PDF | `%PDF` | native: Info dictionary, XMP (uncompressed), page count, revision count, signature/JavaScript/launch/attachment indicators | native: Info fields (Title, Author, Subject, Keywords, Creator, Producer, CreationDate, ModDate); file is rewritten |
| DOCX / XLSX / PPTX | ZIP + parts | native: core/app/custom properties, tracked changes, comments, macros, signature, media/object counts | native: core (Title, Subject, Creator, Keywords, Description, LastModifiedBy, Category, Revision, Created, Modified) and app (Company, Manager, Application) |
| ODT / ODS / ODP | `mimetype` | native: meta.xml | native: Title, Subject, Description, InitialCreator, Creator, Generator, Keyword, CreationDate, Date |
| EPUB | `mimetype` | native: OPF `dc:` metadata | — |
| DOC / XLS / PPT (OLE2) | signature | detected only; exiftool required | exiftool\* |
| RTF, HTML, XML, CSV/TSV, JSON, Markdown, text, source code | content + extension | native: encoding, line endings, counts, HTML `<meta>`, RTF info, JSON shape | — |
| MP4 / MOV / MKV / WebM / AVI | signature | needs ffprobe and/or exiftool | exiftool\* |
| MP3 / WAV / FLAC / AAC / M4A / OGG | signature | needs ffprobe and/or exiftool | exiftool\* |
| ZIP / JAR | signature | native listing with limits (never extracts); `--deep` nested | — |
| TAR / TGZ / GZIP | signature | native listing with limits | — |
| 7z / RAR / BZ2 / XZ | signature | detected only (not listed) | — |

When a needed backend is missing the file report contains an `unavailable` note and `search` answers *unavailable* instead of *absent*.

## Timestamp meanings

| Source | What it records | Typical reliability |
|---|---|---|
| EXIF `DateTimeOriginal` | camera clock at capture, local time; timezone only if `OffsetTimeOriginal` exists | editable; clock may be wrong |
| EXIF `DateTime` | last modification by software/camera | not the capture time |
| Video `creation_time` | container timestamp written by the recorder/encoder (often UTC) | not proof of when the scene happened |
| Encoding/muxing time | when a tool produced the file | changes on every re-export |
| Office `Created`/`Modified` | clock of the saving machine, carried across templates and Save As | editable |
| `Revision`, `TotalTime` | application counters, not an editing duration | application-specific |
| ZIP/TAR entry mtime | copied from the source file by the archiver (DOS time has no timezone) | weak |
| Filesystem times | change on copy, download, extraction, sync | weak |
| Git commit dates | committer's clock, rewritable | not independent |
