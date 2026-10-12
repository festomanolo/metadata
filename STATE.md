# STATE.md — living project ledger (READ THIS FIRST, UPDATE AFTER EVERY CHANGE)

Purpose: lets any session resume without scanning the codebase. Keep it terse and accurate.
Rule: after adding/changing/removing a feature, file, public API, or dependency, update the relevant section here **in the same turn**.

- Toolchain: `export PATH=$HOME/.cargo/bin:$PATH` (cargo/rustc 1.99). Not on default PATH.
- Backends installed on dev machine: exiftool NO, ffprobe NO, 7z NO, tika NO (native fallbacks required; tests must not depend on them).
- Last updated: 2026-10-12 (project start)

## Feature status
Legend: DONE (works + tested) · PARTIAL · PLANNED · —

| Area | Feature | Status | Notes |
|---|---|---|---|
| Foundation | Workspace + CLI skeleton | PLANNED | |
| Foundation | Format detection (magic bytes) | PLANNED | |
| Foundation | SHA-256 hashing | PLANNED | |
| Foundation | Backend discovery / `doctor` / `capabilities` | PLANNED | |
| Extraction | Universal file info | PLANNED | |
| Extraction | Native: images (EXIF) | PLANNED | |
| Extraction | Native: OOXML/ODF docs (zip+xml) | PLANNED | |
| Extraction | Native: PDF info dict | PLANNED | |
| Extraction | Native: archives (zip/tar/gz) w/ limits | PLANNED | |
| Extraction | ExifTool adapter | PLANNED | |
| Extraction | FFprobe adapter | PLANNED | |
| Commands | inspect / extract / search / compare / timeline | PLANNED | |
| Commands | hash / verify-manifest / batch | PLANNED | |
| Commands | edit / clean | PLANNED | |
| Commands | assess / cases / reports | PLANNED | |
| TUI | Dashboard + inspector | PLANNED | |
| Config | config file | PLANNED | |

## Crate / file map
(filled in as files are created: `path` — purpose — key public items)

## Conventions
- Machine output → stdout; diagnostics → stderr. Never mix.
- Metadata schema is versioned (`SCHEMA_VERSION` in metadata-core); each field keeps raw value, normalized value, backend, provenance.
- No fake features: unimplemented = clearly labeled PLANNED here and errors out "not implemented" in CLI only if hidden from help.

## Known issues / decisions log
- (none yet)

## Next steps
1. Workspace + core types + CLI skeleton.
