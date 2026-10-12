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
        match s { "human" => Some(Fmt::Human), "json" => Some(Fmt::Json), "csv" => Some(Fmt::Csv), _ => None }
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
