use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "metadata",
    version,
    about = "Metadata extraction, comparison and integrity tool",
    long_about = "Inspect, search, compare and fingerprint metadata across documents, images, media, archives and source projects.\n\
Machine-readable output (json/csv) goes to stdout; diagnostics go to stderr.\n\n\
Exit codes: 0 success · 1 failure · 2 usage error · 3 completed with per-file errors",
    after_help = "Examples:\n  metadata inspect photo.jpg\n  metadata extract ./submissions -r -f json -o out.json\n  metadata search ./photos --field GPSLatitude -r\n  metadata compare a.docx b.docx\n  metadata hash ./submissions -r --manifest manifest.json\n  metadata verify-manifest manifest.json --root ./submissions\n  metadata doctor"
)]
pub struct Cli {
    /// Increase diagnostics on stderr (-v info, -vv debug)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum, PartialEq, Eq, Debug)]
pub enum Fmt {
    Human,
    Json,
    Csv,
}

impl Fmt {
    pub fn parse(s: &str) -> Option<Fmt> {
        match s {
            "human" => Some(Fmt::Human),
            "json" => Some(Fmt::Json),
            "csv" => Some(Fmt::Csv),
            _ => None,
        }
    }
}

#[derive(Args, Clone)]
pub struct Common {
    /// Descend into directories
    #[arg(short, long)]
    pub recursive: bool,
    /// Output format [default: from config, else per-command]
    #[arg(short, long, value_enum)]
    pub format: Option<Fmt>,
    /// Write output to FILE instead of stdout (refuses to overwrite without --force)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// Allow --output to replace an existing file
    #[arg(long)]
    pub force: bool,
    /// Reuse cached extraction results keyed by file hash and settings (stored in the config dir)
    #[arg(long)]
    pub cache: bool,
    /// Include raw/unknown fields and list every archive entry
    #[arg(long)]
    pub all: bool,
    /// Inspect nested archives (bounded by archive limits)
    #[arg(long)]
    pub deep: bool,
    /// Skip SHA-256 computation
    #[arg(long)]
    pub no_hash: bool,
    /// Do not use exiftool/ffprobe even if installed
    #[arg(long)]
    pub no_external: bool,
    /// Mask GPS and personal identifiers (author, serial numbers, ...)
    #[arg(long)]
    pub redact: bool,
    /// Parallel workers
    #[arg(short, long)]
    pub jobs: Option<usize>,
    /// Per external tool timeout (seconds)
    #[arg(long)]
    pub timeout: Option<u64>,
    /// Directory name to skip while walking (repeatable)
    #[arg(long, value_name = "DIR")]
    pub exclude: Vec<String>,
    /// Include `.git` internals when walking (excluded by default)
    #[arg(long)]
    pub include_vcs: bool,
    /// Only show fields whose group, name or value contains TEXT
    #[arg(long, value_name = "TEXT")]
    pub filter: Option<String>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Human-readable inspection of files, folders and archives
    Inspect {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[command(flatten)]
        c: Common,
    },
    /// Extract metadata as structured data (default: JSON)
    Extract {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[command(flatten)]
        c: Common,
    },
    /// Find files by metadata field and/or value
    Search {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Field name (e.g. Author, GPSLatitude) or Group:Name. Common names expand to equivalents across formats.
        #[arg(long)]
        field: Option<String>,
        /// Substring to match in values (case-insensitive)
        #[arg(long)]
        value: Option<String>,
        #[command(flatten)]
        c: Common,
    },
    /// Compare metadata of two files or two directories
    Compare {
        left: PathBuf,
        right: PathBuf,
        /// Also list identical fields
        #[arg(long)]
        show_identical: bool,
        #[command(flatten)]
        c: Common,
    },
    /// Process a folder and produce a per-file success/failure report (default: JSON)
    Batch {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[command(flatten)]
        c: Common,
    },
    /// Compute file hashes, optionally writing an integrity manifest
    Hash {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long, default_value = "sha256", value_parser = ["sha256", "sha512"])]
        algorithm: String,
        /// Write a versioned manifest JSON to this path
        #[arg(long)]
        manifest: Option<PathBuf>,
        #[command(flatten)]
        c: Common,
    },
    /// Verify files against a manifest created by `hash --manifest`
    VerifyManifest {
        manifest: PathBuf,
        /// Directory the manifest paths are relative to [default: manifest's recorded root]
        #[arg(long)]
        root: Option<PathBuf>,
        /// Also report files under --root that are not in the manifest
        #[arg(long)]
        check_extra: bool,
        #[arg(short, long, value_enum)]
        format: Option<Fmt>,
    },
    /// Build a chronological timeline from embedded, archive, filesystem and git sources (kept separate)
    Timeline {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Show only these sources (repeatable): declared, embedded, archive, filesystem, git, intake
        #[arg(long, value_name = "SOURCE")]
        source: Vec<String>,
        /// Do not read git history even if a repository is present
        #[arg(long)]
        no_git: bool,
        /// Add the intake time recorded in a manifest created by `hash --manifest`
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Claims file (TOML/JSON) whose dates are shown as `declared` events
        #[arg(long)]
        claims: Option<PathBuf>,
        #[command(flatten)]
        c: Common,
    },
    /// Evidence-based review of a project against declared dates; produces findings, not verdicts
    Assess {
        /// Project folder (read-only)
        path: PathBuf,
        /// Case identifier recorded in the report
        #[arg(long, default_value = "unnamed")]
        case: String,
        #[arg(long)]
        student: Option<String>,
        /// Claimed start date (YYYY-MM-DD or RFC 3339)
        #[arg(long)]
        start: Option<String>,
        /// Claimed completion date
        #[arg(long)]
        completion: Option<String>,
        /// Submission deadline
        #[arg(long)]
        deadline: Option<String>,
        /// Claimed milestone `Name=DATE` or `Name=DATE:path-substring` (repeatable)
        #[arg(long, value_name = "SPEC")]
        milestone: Vec<String>,
        /// Claims/thresholds file (TOML or JSON); flags override it
        #[arg(long)]
        claims: Option<PathBuf>,
        /// Previous submission folder to compare against
        #[arg(long)]
        previous: Option<PathBuf>,
        /// Do not read git history
        #[arg(long)]
        no_git: bool,
        /// Keep author names and other personal identifiers in evidence values (masked by default)
        #[arg(long)]
        include_personal: bool,
        /// Also write a printable HTML report to FILE
        #[arg(long, value_name = "FILE")]
        html: Option<PathBuf>,
        /// Hide findings below this level
        #[arg(long, value_parser = ["info", "review", "high"], default_value = "info")]
        min_severity: String,
        #[command(flatten)]
        c: Common,
    },
    /// Edit metadata fields safely (writes a new file unless --in-place; always re-reads and verifies)
    Edit {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Set a field: `Group:Name=value` or `Name=value` (repeatable)
        #[arg(long, value_name = "KEY=VALUE")]
        set: Vec<String>,
        /// Remove a field (repeatable)
        #[arg(long, value_name = "KEY")]
        remove: Vec<String>,
        /// Copy every natively writable, non-date field from this file
        #[arg(long, value_name = "FILE")]
        copy_from: Option<PathBuf>,
        /// Choose fields interactively (reads commands from stdin)
        #[arg(long)]
        interactive: bool,
        #[command(flatten)]
        w: WriteOpts,
        #[command(flatten)]
        c: Common,
    },
    /// Remove metadata by profile (privacy | all | gps); writes a new file unless --in-place
    Clean {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long, value_parser = ["privacy", "all", "gps"], default_value = "privacy")]
        profile: String,
        #[command(flatten)]
        w: WriteOpts,
        #[command(flatten)]
        c: Common,
    },
    /// Persistent assessment cases: submissions, reviewer decisions, notes and reports
    Case {
        /// Case database [default: <config dir>/cases.sqlite]
        #[arg(long, global = true, env = "METADATA_DB")]
        db: Option<PathBuf>,
        #[command(subcommand)]
        cmd: CaseCmd,
    },
    /// Open the interactive full-screen interface
    Tui {
        /// Directory or file to open [default: current directory]
        path: Option<PathBuf>,
        /// Do not start scanning on launch
        #[arg(long)]
        no_scan: bool,
        /// Do not use exiftool/ffprobe even if installed
        #[arg(long)]
        no_external: bool,
    },
    /// Show format support and which backend handles each format
    Capabilities {
        #[arg(short, long, value_enum)]
        format: Option<Fmt>,
    },
    /// Diagnose backends, configuration and environment
    Doctor {
        #[arg(short, long, value_enum)]
        format: Option<Fmt>,
    },
    /// Show effective configuration (see --path, --init)
    Config {
        /// Print the config file location only
        #[arg(long)]
        path: bool,
        /// Create a commented template config file if none exists
        #[arg(long)]
        init: bool,
    },
}

/// Options shared by commands that write files.
#[derive(Args, Clone)]
pub struct WriteOpts {
    /// Show the plan and exit without writing anything
    #[arg(long)]
    pub dry_run: bool,
    /// Modify the original file (a backup is kept unless --no-backup)
    #[arg(long)]
    pub in_place: bool,
    /// Skip the backup copy when using --in-place
    #[arg(long)]
    pub no_backup: bool,
    /// Directory for outputs when editing several files
    #[arg(long, value_name = "DIR")]
    pub output_dir: Option<PathBuf>,
    /// Confirm in-place or multi-file writes without prompting
    #[arg(short, long)]
    pub yes: bool,
    /// Proceed even though the file looks digitally signed (the signature will be invalidated)
    #[arg(long)]
    pub accept_signature_loss: bool,
}

#[derive(Subcommand)]
pub enum CaseCmd {
    /// Create a case with declared dates
    New {
        id: String,
        #[arg(long, default_value = "")]
        title: String,
        #[arg(long)]
        student: Option<String>,
        #[arg(long)]
        start: Option<String>,
        #[arg(long)]
        completion: Option<String>,
        #[arg(long)]
        deadline: Option<String>,
        #[arg(long, value_name = "SPEC")]
        milestone: Vec<String>,
        #[arg(long)]
        claims: Option<PathBuf>,
    },
    /// List or search cases
    List {
        #[arg(long)]
        search: Option<String>,
        #[arg(short, long, value_enum)]
        format: Option<Fmt>,
    },
    /// Show a case: claims, submissions and finding dispositions
    Show {
        id: String,
        #[arg(short, long, value_enum)]
        format: Option<Fmt>,
    },
    /// Ingest a submission folder: assess it against the case claims and the previous submission
    Add {
        id: String,
        path: PathBuf,
        #[arg(long, default_value = "")]
        label: String,
        #[arg(long)]
        no_git: bool,
        #[arg(long)]
        include_personal: bool,
        #[command(flatten)]
        c: Common,
    },
    /// Set a finding's disposition (open | reviewed | dismissed; dismissal needs --reason)
    Finding {
        id: String,
        finding_id: String,
        #[arg(long, value_parser = ["open", "reviewed", "dismissed"])]
        status: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Add a free-text reviewer note
    Note { id: String, text: String },
    /// Export a report for a submission (default: latest)
    Report {
        id: String,
        #[arg(long)]
        submission: Option<i64>,
        #[arg(long, value_name = "FILE")]
        html: Option<PathBuf>,
        #[arg(long, value_name = "FILE")]
        csv: Option<PathBuf>,
        /// Include the per-file timeline in the HTML report
        #[arg(long)]
        timeline: bool,
        #[arg(long)]
        force: bool,
        #[arg(short, long, value_enum)]
        format: Option<Fmt>,
    },
    /// Compare two submissions of a case file by file
    Diff {
        id: String,
        a: i64,
        b: i64,
        #[arg(short, long, value_enum)]
        format: Option<Fmt>,
    },
}
