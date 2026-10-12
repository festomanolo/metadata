# PLAN — `metadata`

Rust workspace CLI/TUI for metadata extraction, editing, forensic timeline and academic-assessment evidence review.
Living status lives in `STATE.md`; this file is the stable plan.

## Architecture (see ARCHITECTURE.md)
Crates: `metadata-core` (types, schema, errors, hashing, time) · `metadata-engine` (detection, backends, native parsers, scanning) ·
`metadata-forensics` (manifest, timeline, compare, assessment rules) · `metadata-edit` (safe edit pipeline) ·
`metadata-report` (JSON/CSV/HTML) · `metadata-storage` (SQLite cases/cache) · `metadata-tui` · `metadata-cli` (binary `metadata`).
Crates are added only when they hold real code.

## Key decisions
- Native Rust parsers are the always-available baseline (EXIF, zip/OOXML/ODF, PDF info, archives, PNG/etc.); ExifTool/FFprobe are optional adapters, detected at runtime, giving broader coverage and writing.
- Write support claimed only per (format, backend) pair verified by round-trip tests; native writing limited to what is implemented.
- Every field: raw + normalized value, backend, provenance, timezone-aware time model (never assume UTC).
- Assessment = findings with severity INFO/REVIEW/HIGH PRIORITY, alternatives, observation vs interpretation confidence. No verdicts.

## Milestones (acceptance in ROADMAP.md)
M1 Foundation · M2 Extraction · M3 TUI · M4 Editing · M5 Forensics · M6 Cases/Reports · M7 Hardening.

## Risks
- No ExifTool/FFprobe on dev machine → tests use native paths; adapters tested via fake-backend scripts.
- Scope is huge → ship runnable milestones, mark unfinished items PLANNED.
- Archive/untrusted input → strict limits, no extraction to disk by default.

## Status
See ROADMAP.md and STATE.md. Plan acceptance criteria are tracked as tests in each crate.
