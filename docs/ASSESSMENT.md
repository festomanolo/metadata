# Assessment methodology

The assessment is a **review aid**, not a decision. It never states or implies guilt and produces no probability of misconduct.

## Inputs

* **Evidence**: the submitted folder (analysed read-only; hashed before and re-hashed after to show it was not changed), optional git history, optionally a previous submission.
* **Claims** (supplied by the assessor, shown as `declared` and never treated as evidence): student ID, start, completion, deadline, milestones (optionally restricted to paths containing a text).
* **Thresholds** (defaults; override in a claims file): timezone tolerance 24 h, uniform-timestamp minimum 5 files, history gap 21 days.

Claims files may be TOML or JSON:

```toml
student = "A123"
start = "2024-01-10"
deadline = "2024-06-01"
[[milestones]]
name = "Proposal"
date = "2024-02-01"
path_contains = "proposal/"
[thresholds]
tolerance_hours = 24
gap_days = 21
```

Date-only claims cover the whole day. When either side of a comparison lacks a timezone, the tolerance is applied, and the observation confidence is lowered to *medium*.

## Finding anatomy

Each finding has: a **stable ID** (`RULE-<8 hex>`, derived from the rule and the relative-path evidence, so it is the same on any machine and across submissions), **severity** (`INFO`, `REVIEW`, `HIGH PRIORITY`), **review priority** 1–5, affected files, exact evidence (field, value, source), the **comparison rule**, why it was flagged, **benign alternatives**, **observation confidence** (is the data really there?) separate from **interpretation confidence** (what it means), and a suggested follow-up.

## Rules

| Rule | Level | Fires when |
|---|---|---|
| `TS-FUTURE` | HIGH PRIORITY | an embedded timestamp is later than the analysis time (+ tolerance) |
| `GIT-FUTURE` | HIGH PRIORITY | commits dated in the future |
| `TS-BEFORE-START` | REVIEW | embedded creation time earlier than the claimed start |
| `TS-AFTER` / `TS-MEDIA-AFTER` | REVIEW | embedded or media-container time after the deadline / claimed completion |
| `TS-MOD-BEFORE-CREATE` | REVIEW | modified earlier than created inside one metadata group |
| `MS-LATE-CREATION` | REVIEW | file created after a milestone it should belong to |
| `UNI-EMBEDDED` | REVIEW | ≥N different files share one embedded creation time |
| `UNI-FS` | INFO | ≥N different files share one filesystem modification time |
| `FMT-EXT-MISMATCH` | REVIEW | content type differs from the extension |
| `DUP-CONTENT`, `DUP-PREVIOUS` | INFO | identical content in several paths / relocated since the previous submission |
| `MISS-METADATA` | INFO | formats that normally carry dates have none |
| `INC-PRESENT`, `INC-ABSENT` | INFO | evidence (or lack) of incremental saves: revisions, tracked changes, PDF updates, commits |
| `GIT-BEFORE-START`, `GIT-LATE-START`, `GIT-AFTER-DEADLINE`, `GIT-NONMONOTONIC`, `GIT-AUTHOR-COMMIT-SKEW` | REVIEW/INFO | history vs. claims, order anomalies, rebase signs |
| `GAP-HISTORY` | INFO | gaps longer than the threshold between dated artefacts |
| `SUB-DIFF` | INFO | added/removed/changed/identical versus the previous submission |
| `SUB-CREATED-CHANGED`, `SUB-MODIFIED-BACKWARDS` | REVIEW | same file, new content, creation time changed / modification time went backwards |

**Not performed:** consistency between source code, documentation, screenshots and project structure. Every report lists this under limitations.

## Interpreting findings fairly

* Missing metadata is common (exports, messaging apps, privacy settings) and is only ever `INFO`.
* Timestamps change through copying, downloading, exporting, synchronisation, extraction and editing; they can also be edited deliberately.
* Filesystem times are weak evidence; git dates are written by the committer's clock; the manifest's `recorded_at` is the intake machine's clock. None is independently verified.
* A hash shows two files are byte-identical, or that a file matches a recorded copy. It does not show who made it or when.
* Weigh findings together with the student's explanation, the work itself and other evidence, and give the student an opportunity to respond.

## Case workflow

`case add` stores the assessment and the extracted metadata of each submission (append-only). Reviewer decisions (`open`, `reviewed`, `dismissed` with a mandatory reason) and free-text notes are stored separately and keyed by finding ID, so a dismissed finding remains dismissed when it recurs in a later submission. Reports (`case report`) combine the stored assessment with current dispositions and notes. HTML reports are self-contained (no scripts or external resources) and print cleanly; use your browser's *Save as PDF*.
