//! Runs the built-in validator against the W3C SHACL test suite
//! (core and SPARQL tests, vendored in tests/w3c from w3c/data-shapes).
//!
//! A test passes when `sh:conforms` and the multiset of results match. Each
//! result is compared on focus node, path, value, severity, constraint
//! component and source shape; blank nodes compare equal to each other.

use oxrdf::vocab::rdf;
use oxrdf::{Graph, NamedNode, NamedNodeRef, NamedOrBlankNodeRef, Term, TermRef};
use shacl_report::validate::{ShaclPath, validate};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const MF: &str = "http://www.w3.org/2001/sw/DataAccess/tests/test-manifest#";
const SHT: &str = "http://www.w3.org/ns/shacl-test#";
const SH: &str = "http://www.w3.org/ns/shacl#";

fn iri(base: &str, local: &str) -> NamedNode {
    NamedNode::new_unchecked(format!("{base}{local}"))
}

fn load(path: &Path) -> (Graph, String) {
    let base = format!("file://{}", path.canonicalize().unwrap().display());
    let mut g = Graph::new();
    let text = std::fs::read(path).unwrap();
    for t in oxttl::TurtleParser::new()
        .with_base_iri(&base)
        .unwrap()
        .for_slice(&text)
    {
        g.insert(&t.unwrap_or_else(|e| panic!("{}: {e}", path.display())));
    }
    (g, base)
}

fn obj(g: &Graph, s: &Term, p: NamedNodeRef<'_>) -> Option<Term> {
    let s: NamedOrBlankNodeRef<'_> = match s {
        Term::NamedNode(n) => n.as_ref().into(),
        Term::BlankNode(b) => b.as_ref().into(),
        _ => return None,
    };
    g.object_for_subject_predicate(s, p)
        .map(TermRef::into_owned)
}

fn objs(g: &Graph, s: &Term, p: NamedNodeRef<'_>) -> Vec<Term> {
    let s: NamedOrBlankNodeRef<'_> = match s {
        Term::NamedNode(n) => n.as_ref().into(),
        Term::BlankNode(b) => b.as_ref().into(),
        _ => return Vec::new(),
    };
    g.objects_for_subject_predicate(s, p)
        .map(TermRef::into_owned)
        .collect()
}

fn list(g: &Graph, head: &Term) -> Vec<Term> {
    let mut out = Vec::new();
    let mut node = head.clone();
    while node != Term::from(rdf::NIL) {
        out.push(obj(g, &node, rdf::FIRST).unwrap());
        node = obj(g, &node, rdf::REST).unwrap();
    }
    out
}

/// All test files reachable from a manifest through `mf:include`.
fn test_files(manifest: &Path, out: &mut Vec<PathBuf>) {
    let (g, base) = load(manifest);
    let includes = objs(&g, &iri(&base, "").into(), iri(MF, "include").as_ref());
    if includes.is_empty() {
        out.push(manifest.to_path_buf());
    }
    for inc in includes {
        let Term::NamedNode(n) = inc else { continue };
        let file = PathBuf::from(n.as_str().strip_prefix("file://").unwrap());
        test_files(&file, out);
    }
}

fn show(t: Option<Term>) -> String {
    match t {
        Some(Term::BlankNode(_)) => "_".into(),
        Some(t) => t.to_string(),
        None => "-".into(),
    }
}

/// The comparable form of each result in a report.
fn results(g: &Graph, report: &Term) -> (bool, Vec<String>) {
    let sh = |l: &str| iri(SH, l);
    let conforms = obj(g, report, sh("conforms").as_ref())
        .is_some_and(|t| matches!(t, Term::Literal(l) if l.value() == "true"));
    let mut out = Vec::new();
    for r in objs(g, report, sh("result").as_ref()) {
        let path = obj(g, &r, sh("resultPath").as_ref())
            .map(|p| {
                ShaclPath::parse(g, &p)
                    .map(|p| p.to_sparql())
                    .unwrap_or_else(|e| format!("bad path: {e}"))
            })
            .unwrap_or_else(|| "-".into());
        out.push(format!(
            "focus={} path={} value={} severity={} component={} shape={}",
            show(obj(g, &r, sh("focusNode").as_ref())),
            path,
            show(obj(g, &r, sh("value").as_ref())),
            show(obj(g, &r, sh("resultSeverity").as_ref())),
            show(obj(g, &r, sh("sourceConstraintComponent").as_ref())),
            show(obj(g, &r, sh("sourceShape").as_ref())),
        ));
    }
    out.sort();
    (conforms, out)
}

fn run_suite(manifest: &str) -> (usize, Vec<String>) {
    let mut files = Vec::new();
    test_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(manifest),
        &mut files,
    );
    let mut passed = 0;
    let mut failures = Vec::new();
    for file in files {
        let (g, base) = load(&file);
        let entries = obj(&g, &iri(&base, "").into(), iri(MF, "entries").as_ref())
            .map(|l| list(&g, &l))
            .unwrap_or_default();
        for entry in entries {
            let name = entry.to_string();
            let action = obj(&g, &entry, iri(MF, "action").as_ref()).unwrap();
            let graph_of = |p: &str| {
                let Some(Term::NamedNode(n)) = obj(&g, &action, iri(SHT, p).as_ref()) else {
                    panic!("{name}: no {p}");
                };
                if n.as_str() == base {
                    g.clone()
                } else {
                    load(Path::new(n.as_str().strip_prefix("file://").unwrap())).0
                }
            };
            let data = graph_of("dataGraph");
            let shapes = graph_of("shapesGraph");
            let expected = obj(&g, &entry, iri(MF, "result").as_ref()).unwrap();
            let outcome = validate(&data, &shapes);
            if expected == Term::from(iri(SHT, "Failure")) {
                match outcome {
                    Err(_) => passed += 1,
                    Ok(_) => failures.push(format!("{name}: expected a failure")),
                }
                continue;
            }
            let v = match outcome {
                Ok(v) => v,
                Err(e) => {
                    failures.push(format!("{name}: error {e:#}"));
                    continue;
                }
            };
            let report_node: Term = v
                .report
                .subjects_for_predicate_object(rdf::TYPE, iri(SH, "ValidationReport").as_ref())
                .next()
                .unwrap()
                .into_owned()
                .into();
            let want = results(&g, &expected);
            let got = results(&v.report, &report_node);
            if want == got {
                passed += 1;
            } else {
                let want_set: BTreeSet<_> = want.1.iter().collect();
                let got_set: BTreeSet<_> = got.1.iter().collect();
                let missing: Vec<_> = want.1.iter().filter(|r| !got_set.contains(r)).collect();
                let extra: Vec<_> = got.1.iter().filter(|r| !want_set.contains(r)).collect();
                failures.push(format!(
                    "{name}: conforms want={} got={}; {} vs {} results\n  missing: {missing:#?}\n  extra: {extra:#?}",
                    want.0,
                    got.0,
                    want.1.len(),
                    got.1.len()
                ));
            }
        }
    }
    (passed, failures)
}

fn check(manifest: &str) {
    let (passed, failures) = run_suite(manifest);
    for f in &failures {
        eprintln!("FAIL {f}");
    }
    eprintln!("{manifest}: {passed} passed, {} failed", failures.len());
    assert!(failures.is_empty(), "{} W3C tests failed", failures.len());
}

#[test]
fn w3c_core() {
    check("tests/w3c/core/manifest.ttl");
}

#[test]
fn w3c_sparql() {
    check("tests/w3c/sparql/manifest.ttl");
}
