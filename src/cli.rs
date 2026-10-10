use anyhow::{Result, anyhow, bail};
use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

/// Top-level CLI entry point. Supports the default report invocation
/// (`shacl-report REPORT …`) and a `serve` subcommand for the web viewer.
#[derive(Debug, Parser)]
#[command(
    version,
    about,
    long_about = None,
    after_help = "Questions, bug reports and feature requests: https://github.com/Matdata-eu/shacl-report/issues"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub report: Args,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the local web viewer. Files can be preloaded via flags or
    /// selected interactively in the browser.
    Serve(ServeArgs),
}

/// Summarise a SHACL validation report and optionally render it as HTML or
/// open it in the local web viewer.
#[derive(Debug, Clone, ClapArgs)]
pub struct Args {
    /// SHACL validation report (an RDF file containing an sh:ValidationReport).
    /// Leave it out and give --data and --shapes to validate with the
    /// built-in SHACL engine instead.
    pub report: Option<PathBuf>,

    /// Data graph that was validated. Used to show the triples of each focus
    /// node next to its results. Can be repeated.
    #[arg(long = "data", value_name = "FILE")]
    pub data: Vec<PathBuf>,

    /// Shapes graph that was used. Used to show shape names, the violated
    /// constraint and shape messages. Can be repeated.
    #[arg(long = "shapes", value_name = "FILE")]
    pub shapes: Vec<PathBuf>,

    /// Override the input format of the report (auto-detected from extension by default).
    #[arg(long = "format", value_enum)]
    pub format: Option<InputFormat>,

    /// Write the validation report to FILE as Turtle. Without a REPORT,
    /// shacl-report validates --data against --shapes itself; this saves
    /// that report for other tools.
    #[arg(long = "save-report", value_name = "FILE")]
    pub save_report: Option<PathBuf>,

    /// Write a self-contained HTML report to FILE (works offline, no server needed).
    #[arg(long = "html", value_name = "FILE")]
    pub html: Option<PathBuf>,

    /// Suppress the summary on stderr.
    #[arg(long)]
    pub quiet: bool,

    /// Exit with code 1 if any result at or above `--fail-on` severity is found.
    #[arg(long)]
    pub ci: bool,

    /// Lowest severity that makes `--ci` fail.
    #[arg(long = "fail-on", value_enum, default_value_t = FailOn::Violation)]
    pub fail_on: FailOn,

    /// SPARQL endpoint holding the validated data; preset in the viewer's
    /// query editor for running SPARQL constraints and generated queries.
    #[arg(long = "endpoint", value_name = "URL")]
    pub endpoint: Option<String>,

    /// Open the report in the local web viewer.
    #[arg(long)]
    pub view: bool,

    /// Do not auto-open the system browser when starting the viewer.
    #[arg(long = "no-open")]
    pub no_open: bool,

    /// Bind address for the web viewer.
    #[arg(long, default_value = "127.0.0.1:0")]
    pub bind: String,
}

/// Arguments accepted by `shacl-report serve`.
#[derive(Debug, ClapArgs)]
pub struct ServeArgs {
    /// SHACL validation report to preload.
    #[arg(long = "report")]
    pub report: Option<PathBuf>,
    /// Data graph to preload (file or http(s) URL). Can be repeated.
    #[arg(long = "data", value_name = "FILE")]
    pub data: Vec<PathBuf>,
    /// Shapes graph to preload (file or http(s) URL); enough on its own to
    /// explore the shapes. Can be repeated.
    #[arg(long = "shapes", value_name = "FILE")]
    pub shapes: Vec<PathBuf>,
    /// Override the input format of the report.
    #[arg(long = "format", value_enum)]
    pub format: Option<InputFormat>,
    /// SPARQL endpoint holding the validated data; preset in the viewer's
    /// query editor for running SPARQL constraints and generated queries.
    #[arg(long = "endpoint", value_name = "URL", env = "SHACL_REPORT_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Bind address for the web viewer.
    #[arg(long, default_value = "127.0.0.1:0", env = "SHACL_REPORT_BIND")]
    pub bind: String,
    /// Do not auto-open the system browser.
    #[arg(long = "no-open")]
    pub no_open: bool,

    #[command(flatten)]
    pub public: PublicArgs,
}

/// Settings for running the viewer as a public, multi-user website.
#[derive(Debug, Clone, ClapArgs)]
#[command(next_help_heading = "Public server")]
pub struct PublicArgs {
    /// Serve many users safely: every visitor gets their own session, the
    /// server never reads its own files or fetches URLs for visitors, the
    /// SPARQL relay only reaches --allow-endpoint, and the limits below apply.
    #[arg(long, env = "SHACL_REPORT_PUBLIC")]
    pub public: bool,

    /// Most visitors that can have files loaded at the same time. Others are
    /// asked to try again later until a session ends or expires.
    #[arg(long, default_value_t = 10, env = "SHACL_REPORT_MAX_SESSIONS")]
    pub max_sessions: usize,

    /// Most sessions one IP address can hold.
    #[arg(long, default_value_t = 2, env = "SHACL_REPORT_MAX_SESSIONS_PER_IP")]
    pub max_sessions_per_ip: usize,

    /// Minutes without any request after which a session and its files are dropped.
    #[arg(long, default_value_t = 30, env = "SHACL_REPORT_SESSION_IDLE_MINUTES")]
    pub session_idle_minutes: u64,

    /// Most uploads parsed or validated at the same time; the rest wait in
    /// line for up to --queue-seconds. Each job uses one CPU core and, at the
    /// default limits, up to about 1 GB of memory while it runs.
    #[arg(long, default_value_t = 2, env = "SHACL_REPORT_MAX_JOBS")]
    pub max_jobs: usize,

    /// Longest an upload waits for a free job slot before the visitor is
    /// told the server is busy.
    #[arg(long, default_value_t = 30, env = "SHACL_REPORT_QUEUE_SECONDS")]
    pub queue_seconds: u64,

    /// Longest one validation may run.
    #[arg(long, default_value_t = 120, env = "SHACL_REPORT_JOB_SECONDS")]
    pub job_seconds: u64,

    /// Largest upload in MB, and the largest any single file may grow to
    /// once decompressed.
    #[arg(long, default_value_t = 20, env = "SHACL_REPORT_MAX_UPLOAD_MB")]
    pub max_upload_mb: u64,

    /// Most triples one upload may hold across report, shapes and data.
    #[arg(long, default_value_t = 100_000, env = "SHACL_REPORT_MAX_TRIPLES")]
    pub max_triples: usize,

    /// SPARQL endpoint the relay may forward to. Can be repeated, or given
    /// comma-separated in the environment variable. --endpoint is always allowed.
    #[arg(
        long = "allow-endpoint",
        value_name = "URL",
        env = "SHACL_REPORT_ALLOW_ENDPOINTS",
        value_delimiter = ','
    )]
    pub allow_endpoint: Vec<String>,

    /// Take the visitor's IP address from the last X-Forwarded-For entry.
    /// Only set this behind a reverse proxy that sets that header.
    #[arg(long, env = "SHACL_REPORT_TRUST_PROXY")]
    pub trust_proxy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InputFormat {
    /// N-Triples (.nt)
    Nt,
    /// Turtle (.ttl)
    Ttl,
    /// RDF/XML (.rdf, .owl, .xml)
    #[value(alias = "xml")]
    Rdf,
    /// TriG (.trig) — all graphs are merged
    Trig,
    /// N-Quads (.nq) — all graphs are merged
    Nq,
}

/// Severity threshold for `--ci`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum FailOn {
    /// Fail on sh:Violation only.
    Violation,
    /// Fail on sh:Warning or worse.
    Warning,
    /// Fail on any result, including sh:Info.
    Info,
}

/// Detect input format from a path. Strips a trailing `.gz` first.
pub fn detect_format(path: &Path) -> Result<InputFormat> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("path has no filename: {}", path.display()))?
        .to_ascii_lowercase();

    let stem = name.strip_suffix(".gz").unwrap_or(&name);
    let ext = stem.rsplit_once('.').map(|(_, e)| e).unwrap_or("");

    match ext {
        "nt" | "ntriples" => Ok(InputFormat::Nt),
        "ttl" | "turtle" | "shacl" => Ok(InputFormat::Ttl),
        "rdf" | "owl" | "xml" => Ok(InputFormat::Rdf),
        "trig" => Ok(InputFormat::Trig),
        "nq" | "nquads" => Ok(InputFormat::Nq),
        "" => bail!(
            "could not detect RDF format for {} (no extension)",
            path.display()
        ),
        other => bail!("unknown RDF extension '.{}' for {}", other, path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_basic_extensions() {
        assert_eq!(detect_format(Path::new("a.ttl")).unwrap(), InputFormat::Ttl);
        assert_eq!(detect_format(Path::new("a.nt")).unwrap(), InputFormat::Nt);
        assert_eq!(detect_format(Path::new("a.rdf")).unwrap(), InputFormat::Rdf);
        assert_eq!(
            detect_format(Path::new("a.trig")).unwrap(),
            InputFormat::Trig
        );
        assert_eq!(detect_format(Path::new("a.nq")).unwrap(), InputFormat::Nq);
    }

    #[test]
    fn detects_gzipped_extensions() {
        assert_eq!(
            detect_format(Path::new("a.ttl.gz")).unwrap(),
            InputFormat::Ttl
        );
    }

    #[test]
    fn rejects_unknown_extension() {
        assert!(detect_format(Path::new("a.foo")).is_err());
        assert!(detect_format(Path::new("noext")).is_err());
    }
}
