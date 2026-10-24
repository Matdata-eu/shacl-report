//! The demo data sets offered on the viewer's start screen (assets/demos).

use shacl_report::report::{Report, ReportInputs, load_report};
use shacl_report::web::assets;
use std::path::PathBuf;

fn demo(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("assets");
    p.push("demos");
    p.push(name);
    p
}

fn load(report: Option<&str>, shapes: &str, data: &str) -> Report {
    load_report(&ReportInputs {
        report: report.map(demo),
        data: vec![demo(data)],
        shapes: vec![demo(shapes)],
        format: None,
        prefixes: Vec::new(),
    })
    .unwrap()
}

#[test]
fn every_demo_file_is_served_and_listed_in_the_viewer() {
    let app = std::str::from_utf8(assets::lookup("/assets/app/app.js").unwrap().bytes).unwrap();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/demos");
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        let asset = assets::lookup(&format!("/assets/demos/{name}"))
            .unwrap_or_else(|| panic!("{name} is not embedded in the binary"));
        assert!(asset.mime.starts_with("text/turtle"));
        assert!(
            app.contains(&format!("\"{name}\"")),
            "{name} is not used by any demo"
        );
    }
}

#[test]
fn library_demo_breaks_shacl_core_rules() {
    let r = load(None, "library-shapes.ttl", "library-data.ttl");
    assert_eq!(r.conforms, Some(false));
    assert_eq!(r.stats.total, 29);
    assert!(r.stats.violations > 0 && r.stats.warnings > 0 && r.stats.infos > 0);
    assert!(r.results.iter().all(|x| x.constraint.is_some()));
}

#[test]
fn railway_demo_uses_sparql_constraints_and_components() {
    let r = load(None, "railway-shapes.ttl", "railway-data.ttl");
    assert_eq!(r.conforms, Some(false));
    assert_eq!(r.stats.total, 27);
    let component = |c: &str| {
        r.results
            .iter()
            .any(|x| x.component.as_deref().is_some_and(|v| v.ends_with(c)))
    };
    assert!(component("#SPARQLConstraintComponent"));
    assert!(component("UniqueConstraintComponent"));
    assert!(component("#XoneConstraintComponent"));
    assert!(component("#QualifiedMinCountConstraintComponent"));
    assert!(component("#NodeConstraintComponent"));
}

#[test]
fn pyshacl_demo_matches_the_built_in_validation() {
    let ours = load(None, "library-shapes.ttl", "library-data.ttl");
    let theirs = load(
        Some("library-report-pyshacl.ttl"),
        "library-shapes.ttl",
        "library-data.ttl",
    );
    assert_eq!(theirs.stats.total, ours.stats.total);
    assert_eq!(theirs.stats.violations, ours.stats.violations);
    // Every anonymous property shape in pyshacl's report is matched back to the shapes.
    assert!(theirs.results.iter().all(|x| x.constraint.is_some()));
}
