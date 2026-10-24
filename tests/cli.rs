use shacl_report::report::{ReportInputs, load_report};
use shacl_report::web::export::render_html;
use std::path::PathBuf;
use std::process::Command;

const EX: &str = "http://example.org/ns#";
const SH: &str = "http://www.w3.org/ns/shacl#";

fn fixtures(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push(name);
    p
}

fn inputs(report: &str, data: bool, shapes: bool) -> ReportInputs {
    ReportInputs {
        report: Some(fixtures(report)),
        data: if data {
            vec![fixtures("data.ttl")]
        } else {
            vec![]
        },
        shapes: if shapes {
            vec![fixtures("shapes.ttl")]
        } else {
            vec![]
        },
        format: None,
        prefixes: Vec::new(),
        coverage: Default::default(),
    }
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_shacl-report"))
}

#[test]
fn parses_pyshacl_turtle_report() {
    let r = load_report(&inputs("report-pyshacl.ttl", false, false)).unwrap();
    assert_eq!(r.conforms, Some(false));
    assert_eq!(r.stats.total, 11);
    assert_eq!(r.stats.violations, 8);
    assert_eq!(r.stats.warnings, 2);
    assert_eq!(r.stats.infos, 1);
    assert_eq!(r.stats.focus_nodes, 3);
    // Violations sort before warnings and info.
    assert_eq!(r.results[0].severity, "Violation");
    assert_eq!(r.results.last().unwrap().severity, "Info");
    assert!(r.focus.is_empty());
}

#[test]
fn ntriples_report_gives_same_counts() {
    let ttl = load_report(&inputs("report-pyshacl.ttl", false, false)).unwrap();
    let nt = load_report(&inputs("report-pyshacl.nt", false, false)).unwrap();
    assert_eq!(ttl.stats.total, nt.stats.total);
    assert_eq!(ttl.stats.violations, nt.stats.violations);
}

#[test]
fn renders_complex_paths() {
    let r = load_report(&inputs("report-pyshacl.ttl", false, false)).unwrap();
    let paths: Vec<&str> = r.results.iter().filter_map(|x| x.path.as_deref()).collect();
    assert!(paths.contains(&format!("^<{EX}worksFor>").as_str()));
    assert!(paths.contains(&format!("<{EX}address> / <{EX}country>").as_str()));
}

#[test]
fn shapes_graph_resolves_anonymous_property_shapes() {
    let r = load_report(&inputs("report-pyshacl.nt", true, true)).unwrap();
    assert!(r.has_shapes);
    let age_max = r
        .results
        .iter()
        .find(|x| x.component.as_deref() == Some(&format!("{SH}MaxInclusiveConstraintComponent")))
        .unwrap();
    assert_eq!(age_max.shape_label, format!("Person › <{EX}age>"));
    assert_eq!(
        age_max.constraint.as_deref(),
        Some(format!("<{SH}maxInclusive> 150").as_str())
    );
    let in_list = r
        .results
        .iter()
        .find(|x| x.component.as_deref() == Some(&format!("{SH}InConstraintComponent")))
        .unwrap();
    assert_eq!(
        in_list.constraint.as_deref(),
        Some(format!("<{SH}in> (\"BE\" \"NL\" \"LU\")").as_str())
    );
    // Named property shapes use their sh:name.
    assert!(r.results.iter().any(|x| x.shape_label == "employer"));
    // Every result was matched to a shape.
    assert!(r.results.iter().all(|x| !x.shape_label.starts_with('(')));
}

#[test]
fn data_graph_provides_focus_context() {
    let r = load_report(&inputs("report-pyshacl.ttl", true, false)).unwrap();
    assert!(r.has_data);
    let bob = r.focus.get(&format!("<{EX}bob>")).unwrap();
    assert!(
        bob.iter()
            .any(|t| t.p == format!("{EX}email") && t.o.v == "bob.example.org")
    );
}

#[test]
fn conforming_report_has_no_results() {
    let r = load_report(&inputs("report-conforms.ttl", false, false)).unwrap();
    assert_eq!(r.conforms, Some(true));
    assert_eq!(r.stats.total, 0);
}

#[test]
fn rejects_files_without_a_report() {
    let err = load_report(&inputs("data.ttl", false, false)).unwrap_err();
    assert!(format!("{err:#}").contains("no sh:ValidationReport"));
}

#[test]
fn html_export_is_self_contained() {
    let r = load_report(&inputs("report-pyshacl.ttl", true, true)).unwrap();
    let html = render_html(&r).unwrap();
    assert!(html.contains("window.SHACL_REPORT = {"));
    assert!(!html.contains("src=\"/assets/"));
    assert!(!html.contains("href=\"/assets/"));
    assert!(html.contains("Tabulator"));
}

#[test]
fn ci_exit_codes() {
    let status = bin()
        .arg(fixtures("report-pyshacl.ttl"))
        .args(["--ci", "--quiet"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));

    let status = bin()
        .arg(fixtures("report-conforms.ttl"))
        .args(["--ci", "--quiet", "--fail-on", "info"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));

    let status = bin()
        .arg(fixtures("report-pyshacl.ttl"))
        .args(["--quiet"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));

    let status = bin()
        .arg(fixtures("missing.ttl"))
        .args(["--quiet"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}

#[test]
fn writes_html_file() {
    let out = std::env::temp_dir().join("shacl-report-test.html");
    let _ = std::fs::remove_file(&out);
    let status = bin()
        .arg(fixtures("report-pyshacl.ttl"))
        .arg("--html")
        .arg(&out)
        .arg("--quiet")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    let body = std::fs::read_to_string(&out).unwrap();
    assert!(body.contains("SHACL_REPORT"));
}

#[test]
fn loads_uploaded_files_from_memory() {
    use shacl_report::input::MemoryFile;
    use shacl_report::report::load_report_from_memory;
    let file = |name: &str| MemoryFile {
        name: name.to_string(),
        content: std::fs::read(fixtures(name)).unwrap(),
    };
    let r = load_report_from_memory(
        Some(&file("report-pyshacl.ttl")),
        &[file("shapes.ttl")],
        &[file("data.ttl")],
    )
    .unwrap();
    assert_eq!(r.stats.total, 11);
    assert_eq!(r.sources.report, "report-pyshacl.ttl");
    assert!(r.has_data && r.has_shapes);
    let in_list = r
        .results
        .iter()
        .find(|x| x.component.as_deref() == Some(&format!("{SH}InConstraintComponent")))
        .unwrap();
    assert_eq!(in_list.params, vec!["\"BE\"", "\"NL\"", "\"LU\""]);
}

fn advanced(report: bool) -> shacl_report::report::Report {
    load_report(&ReportInputs {
        report: report.then(|| fixtures("report-advanced.ttl")),
        // Shapes plus data without a report would run the validator.
        data: if report {
            vec![fixtures("data.ttl")]
        } else {
            vec![]
        },
        shapes: vec![fixtures("shapes-advanced.ttl")],
        format: None,
        prefixes: Vec::new(),
        coverage: Default::default(),
    })
    .unwrap()
}

#[test]
fn explores_shapes_without_a_report() {
    let r = advanced(false);
    assert!(!r.has_report && r.has_shapes && r.results.is_empty());
    let m = r.shapes.as_ref().unwrap();
    let by_iri = |local: &str| {
        m.shapes
            .iter()
            .find(|s| s.iri.as_deref() == Some(&format!("{EX}{local}")))
            .unwrap()
    };
    let person = by_iri("PersonShape");
    assert_eq!(person.label, "Person");
    assert_eq!(person.targets[0].kind, "class");
    assert!(
        person
            .constraints
            .iter()
            .any(|c| c.param == format!("{SH}not"))
    );
    // Property shapes are ordered by sh:order.
    let names: Vec<_> = person
        .properties
        .iter()
        .map(|p| m.shapes[*p].name.clone().unwrap())
        .collect();
    assert_eq!(names, ["name", "age", "employer"]);
    let employer = &m.shapes[person.properties[2]];
    let qvs = employer
        .constraints
        .iter()
        .find(|c| c.param == format!("{SH}qualifiedValueShape"))
        .unwrap();
    assert!(qvs.values[0].shape.is_some());
    // SPARQL constraint with its sh:prefixes declarations.
    assert_eq!(person.sparql.len(), 1);
    assert_eq!(person.sparql[0].kind, "select");
    assert_eq!(
        person.sparql[0].prefixes,
        vec![("ex".to_string(), EX.to_string())]
    );
    assert_eq!(by_iri("EmailShape").targets[0].kind, "subjectsOf");
    let org = by_iri("OrganisationShape");
    let ignored = org
        .constraints
        .iter()
        .find(|c| c.param.ends_with("#ignoredProperties"))
        .unwrap();
    assert!(ignored.list);
    // Custom SPARQL-based constraint component and its parameter.
    assert_eq!(m.components.len(), 1);
    assert_eq!(m.components[0].validators[0].role, "propertyValidator");
    let email = &m.shapes[by_iri("EmailShape").properties[0]];
    let custom = email
        .constraints
        .iter()
        .find(|c| c.param == format!("{EX}noWhitespace"))
        .unwrap();
    assert_eq!(custom.component, Some(0));
    // The address shape is only reachable through sh:node, yet named, so it is a root.
    assert!(m.roots.contains(&by_iri("AddressShape").id));
}

#[test]
fn links_results_to_shapes() {
    let r = advanced(true);
    assert_eq!(r.stats.total, 12);
    let m = r.shapes.as_ref().unwrap();
    for res in &r.results {
        let key = res
            .shape_key
            .as_deref()
            .unwrap_or_else(|| panic!("{} unresolved", res.shape_label));
        assert!(m.shapes.iter().any(|s| s.key == key), "{key}");
    }
}

#[test]
fn shapes_only_cli_summary() {
    let out = bin()
        .arg("--shapes")
        .arg(fixtures("shapes-advanced.ttl"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("sparql-constraints=1"), "{err}");
}

#[test]
fn html_export_embeds_shapes_and_sparql_editor() {
    let html = render_html(&advanced(true)).unwrap();
    assert!(html.contains("\"shapes\":{"));
    assert!(html.contains("ShaclShapes"));
    assert!(html.contains("Yasgui"));
    assert!(!html.contains("<script src="));
}

/// The comparable part of each result: focus, path, value, component, severity.
fn result_keys(r: &shacl_report::report::Report) -> Vec<String> {
    let mut keys: Vec<String> = r
        .results
        .iter()
        .map(|x| {
            let term = |t: &shacl_report::report::TermDto| {
                if t.t == "bnode" {
                    "_".to_string()
                } else {
                    t.v.clone()
                }
            };
            format!(
                "{} {} {} {} {}",
                term(&x.focus),
                x.path.clone().unwrap_or_default(),
                x.value.as_ref().map(term).unwrap_or_default(),
                x.component.clone().unwrap_or_default(),
                x.severity
            )
        })
        .collect();
    keys.sort();
    keys
}

#[test]
fn built_in_validator_matches_pyshacl() {
    let mut i = inputs("report-pyshacl.ttl", true, true);
    let expected = load_report(&i).unwrap();
    i.report = None;
    let ours = load_report(&i).unwrap();
    assert!(ours.has_report);
    assert!(ours.validation.is_some());
    assert_eq!(ours.conforms, Some(false));
    assert_eq!(result_keys(&ours), result_keys(&expected));
    // Shapes and data still enrich the generated results.
    assert!(ours.results.iter().all(|r| r.shape_key.is_some()));
    assert!(!ours.focus.is_empty());
}

#[test]
fn validates_from_the_cli_and_saves_the_report() {
    let saved = std::env::temp_dir().join("shacl-report-generated.ttl");
    let out = bin()
        .args(["--ci", "--save-report"])
        .arg(&saved)
        .arg("--data")
        .arg(fixtures("data.ttl"))
        .arg("--shapes")
        .arg(fixtures("shapes-advanced.ttl"))
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("validated with shacl-report"), "{err}");
    assert!(err.contains("conforms=false  results=12"), "{err}");

    // The saved report is a regular report: it loads back with the same results.
    let reloaded = load_report(&ReportInputs {
        report: Some(saved.clone()),
        data: vec![fixtures("data.ttl")],
        shapes: vec![fixtures("shapes-advanced.ttl")],
        format: None,
        prefixes: Vec::new(),
        coverage: Default::default(),
    })
    .unwrap();
    let generated = load_report(&ReportInputs {
        report: None,
        data: vec![fixtures("data.ttl")],
        shapes: vec![fixtures("shapes-advanced.ttl")],
        format: None,
        prefixes: Vec::new(),
        coverage: Default::default(),
    })
    .unwrap();
    assert_eq!(result_keys(&reloaded), result_keys(&generated));
    std::fs::remove_file(saved).ok();

    // --save-report only applies to a report shacl-report made itself.
    let status = bin()
        .arg(fixtures("report-pyshacl.ttl"))
        .args(["--quiet", "--save-report"])
        .arg(std::env::temp_dir().join("unused.ttl"))
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}

#[test]
fn validates_uploaded_files() {
    let file = |name: &str| shacl_report::input::MemoryFile {
        name: name.to_string(),
        content: std::fs::read(fixtures(name)).unwrap(),
    };
    let r = shacl_report::report::load_report_from_memory(
        None,
        &[file("shapes-advanced.ttl")],
        &[file("data.ttl")],
    )
    .unwrap();
    assert!(r.validation.is_some());
    assert_eq!(r.stats.total, 12);
    assert!(
        r.report_turtle
            .as_deref()
            .unwrap()
            .contains("sh:ValidationReport")
    );
}

#[test]
fn opens_a_report_given_as_data_as_the_report() {
    use shacl_report::report::{ReportInputs, load_report};
    let r = load_report(&ReportInputs {
        report: None,
        data: vec![fixtures("report-pyshacl.ttl")],
        shapes: vec![fixtures("shapes.ttl")],
        format: None,
        prefixes: Vec::new(),
        coverage: Default::default(),
    })
    .unwrap();
    assert_eq!(r.stats.total, 11);
    assert!(r.validation.is_none());
    assert!(r.sources.report.ends_with("report-pyshacl.ttl"));
    assert!(r.sources.data.is_empty());
}

#[test]
fn adds_prefixes_from_shacl_declarations() {
    let mut i = inputs("report-pyshacl.ttl", true, true);
    i.prefixes = vec![fixtures("prefixes.ttl")];
    let r = load_report(&i).unwrap();
    let has = |name: &str, ns: &str| r.prefixes.iter().any(|(n, s)| n == name && s == ns);
    assert!(has("era", "http://data.europa.eu/949/"));
    // Unused well-known prefixes stay out.
    assert!(!r.prefixes.iter().any(|(n, _)| n == "dbo"));
}
