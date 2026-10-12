# Architecture

```
            ┌──────────────┐   ┌─────────────┐
 files ───► │ scan (walk)  │──►│ detect      │  magic bytes → format id, extension check
            └──────────────┘   └─────┬───────┘
                                     ▼
                         ┌───────────────────────┐
                         │ extract (per file)    │  native parsers (always) + exiftool/ffprobe (optional)
                         └─────┬─────────────────┘  → Fields {raw, normalized, time, backend, provenance}
                               ▼
        FileReport (versioned JSON schema) ──► forensics: compare · timeline · manifest · assess
                               │                                │
                               ▼                                ▼
                 edit (plan→temp→verify→replace)     storage (cases, cache)  →  report (HTML/CSV)
                               ▼
                         CLI renderers  /  TUI
```

## Crates

| Crate | Responsibility |
|---|---|
| `metadata-core` | schema (`Field`, `FileReport`, `SCHEMA_VERSION`), timezone-honest `TimeValue`, hashing, errors |
| `metadata-engine` | detection, native extractors (`native/*`), external backends (`backends.rs`: discovery, `run_capped`), scanning with bounded rayon pool and cancellation, search semantics, capability matrix |
| `metadata-forensics` | manifests, compare, timeline, git history reader, project summary, assessment rules/findings |
| `metadata-edit` | edit planning, native writers (office/PDF/PNG/JPEG), ExifTool argument builder, verification, backup, atomic replace, operation log |
| `metadata-storage` | SQLite schema v1 (cases, submissions, finding dispositions, notes, metadata cache) |
| `metadata-report` | escaped, self-contained HTML and CSV rendering |
| `metadata-tui` | ratatui interface: `App` (pure state machine), `ui` (render), `theme` |
| `metadata-cli` | binary `metadata`: clap arguments, config, renderers, command handlers |
| `metadata-testkit` | dev-only synthetic fixture builders |

## Principles

* **Provenance everywhere.** Each `Field` records raw value, backend and whether it was stored in the file, inferred by a parser, read from the filesystem or computed.
* **Adapters, not rewrites.** Native parsers cover what is reliable and cheap; specialist tools are probed at runtime (`Backends::discover`) and merged with de-duplication. Adding an engine means producing `Field`s and `Warning`s inside an `Extraction`.
* **Untrusted input.** `Limits` bound memory and work; archives are only listed; each native parser runs under `catch_unwind`; subprocesses use argument vectors, timeouts and output caps; git is run read-only with hardened flags.
* **Machine/human separation.** stdout is data, stderr is commentary; TUI code never prints to stdout outside the alternate screen.
* **No fake capability.** Write support is listed per format/backend pair only when implemented and tested; everything else is reported as unavailable.

## Schema compatibility

JSON output carries `schema_version` (`1.0.0`). Additive fields are minor changes; renames or removals bump the major version and are recorded in `CHANGELOG.md`. Manifests use `metadata.manifest/1`, assessments `metadata.assessment/1`, and the database `PRAGMA user_version` (currently 1; newer databases are refused).

## Extending

* New native format: add a module under `metadata-engine/src/native/`, dispatch in `native/mod.rs`, add a signature to `detect.rs`, a fixture builder to `metadata-testkit`, and rows to `capabilities.rs` and `docs/FORMATS.md`.
* New writer: implement in `metadata-edit/src/native/`, register in `keys`, `resolve_key`, `write`, and add a round-trip test; only then add it to the capability matrix.
* New assessment rule: add a `rule_*` function in `assess.rs` producing a `Draft` with all required fields, add a scenario test, document it in `docs/ASSESSMENT.md`.
