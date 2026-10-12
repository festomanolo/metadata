# ARCHITECTURE
Pipeline: scan (walk) → detect (magic) → extract (backend chain) → normalize → analyze (forensics) → render (CLI/TUI/report).
- Backends implement `Extractor` trait (name, version, capabilities(), extract(path, opts)). Engine merges outputs, records provenance and warnings.
- Subprocess backends: no shell, arg vector, timeout, output cap.
- Parallelism: rayon bounded pool; cancellation via shared `AtomicBool`.
- Output contract: JSON schema versioned (`schema_version`), see metadata-core.
