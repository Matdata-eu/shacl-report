use clap::Parser;
use shacl_report::cli::{Args, Cli, Command, FailOn, ServeArgs};
use shacl_report::report::{Report, ReportInputs, load_report};
use shacl_report::web::{self, Preload, export};
use std::collections::HashMap;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Some(Command::Serve(s)) = cli.command {
        return run_serve(s);
    }

    let args = cli.report;
    let Some(report_path) = args.report.clone() else {
        eprintln!(
            "error: the following required arguments were not provided:\n  \
             <REPORT>\n\nUsage: shacl-report <REPORT> [--data FILE] [--shapes FILE]\n\n\
             For more information, try '--help'."
        );
        return ExitCode::from(2);
    };
    let inputs = ReportInputs {
        report: report_path,
        data: args.data.clone(),
        shapes: args.shapes.clone(),
        format: args.format,
    };
    let report = match load_report(&inputs) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("shacl-report: error: {err:#}");
            return ExitCode::from(2);
        }
    };
    if !args.quiet {
        print_summary(&report);
    }
    if let Some(path) = &args.html {
        if let Err(err) = export::write_html(&report, path) {
            eprintln!("shacl-report: error: {err:#}");
            return ExitCode::from(2);
        }
        if !args.quiet {
            eprintln!("html report written to {}", path.display());
        }
    }
    let failed = args.ci && report.fails_at(threshold(args.fail_on));
    if (args.view || args.no_open)
        && let Err(err) = view(&args, report)
    {
        eprintln!("shacl-report: error: {err:#}");
        return ExitCode::from(2);
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn view(args: &Args, report: Report) -> anyhow::Result<()> {
    web::run_blocking(&args.bind, !args.no_open, Preload::Loaded(report))
}

fn threshold(f: FailOn) -> u8 {
    match f {
        FailOn::Violation => 0,
        FailOn::Warning => 1,
        FailOn::Info => 2,
    }
}

fn run_serve(s: ServeArgs) -> ExitCode {
    let preload = match s.report {
        Some(report) => Preload::Files(ReportInputs {
            report,
            data: s.data,
            shapes: s.shapes,
            format: s.format,
        }),
        None => Preload::None,
    };
    if let Err(err) = web::run_blocking(&s.bind, !s.no_open, preload) {
        eprintln!("shacl-report: error: {err:#}");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

fn print_summary(r: &Report) {
    let s = &r.stats;
    let conforms = match r.conforms {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    };
    eprintln!(
        "conforms={conforms}  results={}  violations={}  warnings={}  info={}{}",
        s.total,
        s.violations,
        s.warnings,
        s.infos,
        if s.other > 0 {
            format!("  other={}", s.other)
        } else {
            String::new()
        }
    );
    eprintln!("focus-nodes={}  shapes={}", s.focus_nodes, s.shapes);

    let mut by_shape: HashMap<&str, usize> = HashMap::new();
    for res in &r.results {
        *by_shape.entry(res.shape_label.as_str()).or_default() += 1;
    }
    let mut top: Vec<(&str, usize)> = by_shape.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    for (label, n) in top.iter().take(10) {
        eprintln!("  {n:>6}  {}", shorten(label, &r.prefixes));
    }
    if top.len() > 10 {
        eprintln!("  … and {} more shapes", top.len() - 10);
    }
}

/// Replace `<iri>` tokens with `prefix:local` where a prefix matches.
fn shorten(text: &str, prefixes: &[(String, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let iri = &rest[start + 1..start + end];
        let short = prefixes
            .iter()
            .filter(|(_, base)| iri.starts_with(base.as_str()))
            .max_by_key(|(_, base)| base.len())
            .map(|(name, base)| format!("{name}:{}", &iri[base.len()..]));
        out.push_str(&short.unwrap_or_else(|| format!("<{iri}>")));
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}
