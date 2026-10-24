use clap::Parser;
use shacl_report::cli::{Args, Cli, Command, FailOn, ServeArgs};
use shacl_report::report::{CoverageSettings, Report, ReportInputs, load_report};
use shacl_report::validate::coverage::{Coverage, percent};
use shacl_report::web::{self, Preload, export};
use std::collections::HashMap;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Some(Command::Serve(s)) = cli.command {
        return run_serve(s);
    }

    let args = cli.report;
    if args.report.is_none() && args.shapes.is_empty() {
        eprintln!(
            "error: the following required arguments were not provided:\n  \
             <REPORT>\n\nUsage: shacl-report <REPORT> [--data FILE] [--shapes FILE]\n       \
             shacl-report --data FILE --shapes FILE   (validate with the built-in engine)\n       \
             shacl-report --shapes FILE --view   (explore shapes without a report)\n\n\
             For more information, try '--help'."
        );
        return ExitCode::from(2);
    }
    let inputs = ReportInputs {
        report: args.report.clone(),
        data: args.data.clone(),
        shapes: args.shapes.clone(),
        format: args.format,
        prefixes: args.prefixes.clone(),
        coverage: CoverageSettings {
            ignore: args.coverage_ignore.clone(),
            ignore_annotations: args.coverage_ignore_annotations,
        },
    };
    let mut report = match load_report(&inputs) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("shacl-report: error: {err:#}");
            return ExitCode::from(2);
        }
    };
    report.sparql_endpoint = args.endpoint.clone();
    if !args.quiet {
        print_summary(&report);
    }
    if let Some(path) = &args.save_report {
        let Some(turtle) = &report.report_turtle else {
            eprintln!(
                "shacl-report: error: --save-report needs --data and --shapes without a REPORT \
                 (a report you pass in is already saved)"
            );
            return ExitCode::from(2);
        };
        if let Err(err) = std::fs::write(path, turtle) {
            eprintln!(
                "shacl-report: error: cannot write {}: {err}",
                path.display()
            );
            return ExitCode::from(2);
        }
        if !args.quiet {
            eprintln!("validation report written to {}", path.display());
        }
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
    let wants_coverage = args.coverage
        || args.coverage_report.is_some()
        || args.coverage_min.is_some()
        || args.coverage_min_resources.is_some();
    let mut coverage_failed = false;
    if wants_coverage {
        let Some(cov) = &report.coverage else {
            eprintln!(
                "shacl-report: error: coverage needs --data and --shapes{}",
                report
                    .coverage_error
                    .as_ref()
                    .map(|e| format!(" ({e})"))
                    .unwrap_or_default()
            );
            return ExitCode::from(2);
        };
        let below: Vec<String> = [
            (args.coverage_min, cov.triple_percent(), "property values"),
            (
                args.coverage_min_resources,
                cov.resource_percent(),
                "resources",
            ),
        ]
        .into_iter()
        .filter_map(|(min, got, what)| {
            let min = min.filter(|min| got < *min)?;
            Some(format!(
                "coverage of {what} is {got:.1}%, below the minimum of {min}%"
            ))
        })
        .collect();
        coverage_failed = !below.is_empty();
        // A failed minimum shows where the gaps are, so CI logs explain it.
        if (args.coverage || coverage_failed) && !args.quiet {
            print_coverage(cov, &report);
        }
        if let Some(path) = &args.coverage_report {
            let csv = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("csv"));
            let body = if csv {
                coverage_csv(cov)
            } else {
                serde_json::to_string_pretty(cov).unwrap_or_default()
            };
            if let Err(err) = std::fs::write(path, body) {
                eprintln!(
                    "shacl-report: error: cannot write {}: {err}",
                    path.display()
                );
                return ExitCode::from(2);
            }
            if !args.quiet {
                eprintln!("coverage report written to {}", path.display());
            }
        }
        for line in below {
            eprintln!("{line}");
        }
    }
    let failed = (args.ci && report.fails_at(threshold(args.fail_on))) || coverage_failed;
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
    web::run_blocking(
        &args.bind,
        !args.no_open,
        Preload::Loaded(Box::new(report)),
        args.endpoint.clone(),
        None,
    )
}

fn threshold(f: FailOn) -> u8 {
    match f {
        FailOn::Violation => 0,
        FailOn::Warning => 1,
        FailOn::Info => 2,
    }
}

fn run_serve(s: ServeArgs) -> ExitCode {
    let preload = if s.report.is_some() || !s.shapes.is_empty() {
        Preload::Files(ReportInputs {
            report: s.report,
            data: s.data,
            shapes: s.shapes,
            format: s.format,
            prefixes: s.prefixes,
            ..Default::default()
        })
    } else {
        Preload::None
    };
    let public = s
        .public
        .public
        .then(|| web::public::PublicConfig::from_args(&s.public, s.endpoint.as_deref()));
    if let Err(err) = web::run_blocking(&s.bind, !s.no_open, preload, s.endpoint, public) {
        eprintln!("shacl-report: error: {err:#}");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

/// Coverage per class and property as CSV, one row per class and one per
/// class › property.
fn coverage_csv(c: &Coverage) -> String {
    let field = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let mut out =
        String::from("class,property,resources,targeted,reached,uncovered,triples,covered,percent");
    for cat in &c.categories {
        out.push(',');
        out.push_str(cat.key);
    }
    out.push('\n');
    for row in &c.classes {
        let triples: usize = row.properties.iter().map(|p| p.triples).sum();
        let covered: usize = row.properties.iter().map(|p| p.covered).sum();
        out.push_str(&format!(
            "{},,{},{},{},{},{triples},{covered},{:.1}",
            field(&row.key),
            row.resources,
            row.targeted,
            row.reached,
            row.uncovered,
            percent(covered, triples)
        ));
        for i in 0..c.categories.len() {
            let n: usize = row.properties.iter().map(|p| p.categories[i]).sum();
            out.push_str(&format!(",{n}"));
        }
        out.push('\n');
        for p in &row.properties {
            out.push_str(&format!(
                "{},{},{},,,,{},{},{:.1}",
                field(&row.key),
                field(&p.iri),
                p.resources,
                p.triples,
                p.covered,
                percent(p.covered, p.triples)
            ));
            for n in &p.categories {
                out.push_str(&format!(",{n}"));
            }
            out.push('\n');
        }
    }
    out
}

fn class_name(
    row: &shacl_report::validate::coverage::ClassCoverage,
    prefixes: &[(String, String)],
) -> String {
    match (&row.iri, &row.via) {
        (Some(iri), _) => shorten(&format!("<{iri}>"), prefixes),
        (None, Some(v)) => match &v.class {
            Some(c) => shorten(
                &format!("(blank nodes) <{c}> › <{}>", v.predicate),
                prefixes,
            ),
            None => shorten(&format!("(blank nodes) › <{}>", v.predicate), prefixes),
        },
        (None, None) if row.kind == "untyped" => "(no type)".to_string(),
        _ => "(blank nodes)".to_string(),
    }
}

fn print_coverage(c: &Coverage, report: &Report) {
    let prefixes = &report.prefixes;
    let shape_name = |key: &str| {
        let label = report
            .shapes
            .as_ref()
            .and_then(|m| m.shapes.iter().find(|s| s.key == key))
            .map(|s| s.label.clone())
            .unwrap_or_else(|| key.to_string());
        shorten(&label, prefixes)
    };
    let mut classes: Vec<_> = c.classes.iter().collect();
    classes.sort_by_key(|r| {
        std::cmp::Reverse(
            r.properties
                .iter()
                .map(|p| p.triples - p.covered)
                .sum::<usize>()
                + r.uncovered,
        )
    });
    eprintln!("coverage by class (resources covered · values checked):");
    for row in classes.iter().take(15) {
        let triples: usize = row.properties.iter().map(|p| p.triples).sum();
        let covered: usize = row.properties.iter().map(|p| p.covered).sum();
        eprintln!(
            "  {:>5.1}% of {:>6}  · {:>5.1}% of {:>7}  {}",
            percent(row.targeted + row.reached, row.resources),
            row.resources,
            percent(covered, triples),
            triples,
            class_name(row, prefixes)
        );
    }
    if classes.len() > 15 {
        eprintln!("  … and {} more", classes.len() - 15);
    }
    let mut props: Vec<(String, usize, usize)> = Vec::new();
    for row in &c.classes {
        for p in &row.properties {
            if p.covered < p.triples {
                props.push((
                    format!(
                        "{} › {}",
                        class_name(row, prefixes),
                        shorten(&format!("<{}>", p.iri), prefixes)
                    ),
                    p.triples - p.covered,
                    p.triples,
                ));
            }
        }
    }
    props.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if !props.is_empty() {
        eprintln!("largest gaps (values no rule checks):");
        for (name, gap, total) in props.iter().take(10) {
            eprintln!("  {gap:>7} of {total:>7}  {name}");
        }
    }
    for h in &c.hints {
        eprintln!(
            "hint: {} {} resources would be checked by {} if \"{} rdfs:subClassOf {}\" were in the data graph",
            h.resources,
            shorten(&format!("<{}>", h.class), prefixes),
            shape_name(&h.shape),
            shorten(&format!("<{}>", h.class), prefixes),
            shorten(&format!("<{}>", h.superclass), prefixes),
        );
    }
    for u in &c.unused {
        let what = match u.kind {
            "no-focus" => "matches no focus node",
            "no-values" => "path never has a value",
            _ => "has SPARQL-based targets, which are not evaluated",
        };
        let hint = u
            .suggestion
            .as_ref()
            .map(|s| format!(" (did you mean {}?)", shorten(&format!("<{s}>"), prefixes)))
            .unwrap_or_default();
        eprintln!("unused rule: {} {what}{hint}", shape_name(&u.shape));
    }
}

fn print_summary(r: &Report) {
    if let Some(c) = &r.coverage {
        let t = &c.totals;
        eprintln!(
            "coverage: resources={:.1}% ({} of {}: targeted={} reached={})  values={:.1}% ({} of {})  unused-rules={}",
            c.resource_percent(),
            t.targeted + t.reached,
            t.resources,
            t.targeted,
            t.reached,
            c.triple_percent(),
            t.covered,
            t.triples,
            c.unused.len()
        );
    }
    if let Some(m) = &r.shapes {
        let props = m.shapes.iter().filter(|s| s.kind == "property").count();
        let sparql: usize = m.shapes.iter().map(|s| s.sparql.len()).sum();
        eprintln!(
            "shapes-graph: node-shapes={}  property-shapes={props}  sparql-constraints={sparql}  custom-components={}",
            m.shapes.len() - props,
            m.components.len()
        );
    }
    if !r.has_report {
        return;
    }
    if let Some(v) = &r.validation {
        eprintln!("validated with {} in {} ms", v.engine, v.millis);
    }
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
