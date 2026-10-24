use oxrdf::Graph;
use shacl_report::validate::coverage::{ClassCoverage, Coverage, CoverageOptions, coverage};
use std::path::PathBuf;
use std::process::Command;

const PREFIXES: &str = "
@prefix sh:   <http://www.w3.org/ns/shacl#> .
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl:  <http://www.w3.org/2002/07/owl#> .
@prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .
@prefix ex:   <http://example.org/> .
";
const EX: &str = "http://example.org/";

fn graph(ttl: &str) -> Graph {
    let mut g = Graph::new();
    let src = format!("{PREFIXES}{ttl}");
    for t in oxttl::TurtleParser::new().for_slice(src.as_bytes()) {
        g.insert(&t.unwrap());
    }
    g
}

fn run(data: &str, shapes: &str) -> Coverage {
    run_with(data, shapes, &CoverageOptions::default())
}

fn run_with(data: &str, shapes: &str, opts: &CoverageOptions) -> Coverage {
    coverage(&graph(data), &graph(shapes), opts).unwrap()
}

fn class<'c>(c: &'c Coverage, key: &str) -> &'c ClassCoverage {
    c.classes
        .iter()
        .find(|r| r.key == key || r.iri.as_deref() == Some(&format!("{EX}{key}")))
        .unwrap_or_else(|| panic!("no row {key}: {:?}", keys(c)))
}

fn keys(c: &Coverage) -> Vec<&str> {
    c.classes.iter().map(|r| r.key.as_str()).collect()
}

/// (triples, covered, declared) of a property of a class row.
fn prop(c: &Coverage, key: &str, p: &str) -> (usize, usize, usize) {
    let row = class(c, key);
    let p = row
        .properties
        .iter()
        .find(|x| x.iri == format!("{EX}{p}"))
        .unwrap_or_else(|| panic!("no property {p} on {key}"));
    (p.triples, p.covered, p.declared)
}

#[test]
fn subclass_in_data_is_targeted_subclass_in_shapes_is_a_hint() {
    let c = run(
        "ex:Dog rdfs:subClassOf ex:Animal .
         ex:rex a ex:Dog ; ex:name \"Rex\" .
         ex:tom a ex:Cat ; ex:name \"Tom\" .",
        "ex:Cat rdfs:subClassOf ex:Animal .
         ex:AnimalShape a sh:NodeShape ; sh:targetClass ex:Animal ;
             sh:property [ sh:path ex:name ; sh:minCount 1 ] .",
    );
    assert_eq!(class(&c, "Dog").targeted, 1);
    assert_eq!(prop(&c, "Dog", "name"), (1, 1, 0));
    // Strict SHACL: the subclass triple in the shapes graph does not count.
    assert_eq!(class(&c, "Cat").uncovered, 1);
    assert_eq!(c.hints.len(), 1);
    assert_eq!(c.hints[0].class, format!("{EX}Cat"));
    assert_eq!(c.hints[0].superclass, format!("{EX}Animal"));
    // The subclass triple in the data is schema, not data.
    assert_eq!(c.ignored.schema, 1);
}

#[test]
fn multi_typed_resource_counts_in_every_class() {
    let c = run(
        "ex:a a ex:Book , ex:Work ; ex:title \"T\" .",
        "ex:S a sh:NodeShape ; sh:targetClass ex:Book ;
             sh:property [ sh:path ex:title ; sh:minCount 1 ] .",
    );
    assert_eq!(class(&c, "Book").targeted, 1);
    assert_eq!(class(&c, "Work").targeted, 1);
    assert_eq!(prop(&c, "Work", "title"), (1, 1, 0));
    // The totals count the resource once.
    assert_eq!(c.totals.resources, 1);
    assert_eq!(c.totals.triples, 1);
}

#[test]
fn untyped_blank_nodes_are_grouped_by_incoming_predicate_and_class() {
    let c = run(
        "ex:p a ex:Person ; ex:address [ ex:city \"Gent\" ] .
         ex:q a ex:Person ; ex:address [ ex:city \"Brussel\" ] .
         ex:loose ex:city \"Leuven\" .",
        "ex:S a sh:NodeShape ; sh:targetClass ex:Person .",
    );
    let key = format!("blank <{EX}address> <{EX}Person>");
    let row = class(&c, &key);
    assert_eq!(row.kind, "blank");
    assert_eq!(row.resources, 2);
    assert_eq!(row.uncovered, 2);
    assert_eq!(class(&c, "untyped").resources, 1);
}

#[test]
fn inverse_and_sequence_paths_cover_the_triples_they_walk() {
    let c = run(
        "ex:d a ex:Dept ; ex:site ex:Gent .
         ex:e a ex:Emp ; ex:memberOf ex:d ; ex:heads ex:d .",
        "ex:E a sh:NodeShape ; sh:targetClass ex:Emp ;
             sh:property [ sh:path ( ex:memberOf ex:site ) ; sh:minCount 1 ] .
         ex:D a sh:NodeShape ; sh:targetClass ex:Dept ;
             sh:property [ sh:path [ sh:inversePath ex:heads ] ; sh:maxCount 1 ] .",
    );
    assert_eq!(prop(&c, "Emp", "memberOf"), (1, 1, 0));
    assert_eq!(prop(&c, "Emp", "heads"), (1, 1, 0));
    // The intermediate step of the sequence lands on the department.
    assert_eq!(prop(&c, "Dept", "site"), (1, 1, 0));
}

#[test]
fn declared_only_property_shapes_do_not_cover() {
    let c = run(
        "ex:a a ex:T ; ex:p 1 ; ex:q 2 .",
        "ex:S a sh:NodeShape ; sh:targetClass ex:T ;
             sh:property [ sh:path ex:p ; sh:name \"p\" ] ;
             sh:property [ sh:path ex:q ; sh:datatype xsd:integer ] .",
    );
    assert_eq!(prop(&c, "T", "p"), (1, 0, 1));
    assert_eq!(prop(&c, "T", "q"), (1, 1, 0));
}

#[test]
fn closed_shapes_cover_the_resource_not_its_triples() {
    let c = run(
        "ex:a a ex:T ; ex:p 1 .",
        "ex:S a sh:NodeShape ; sh:targetClass ex:T ; sh:closed true ;
             sh:ignoredProperties ( rdf:type ) .",
    );
    let row = class(&c, "T");
    assert_eq!((row.targeted, row.closed), (1, 1));
    assert_eq!(prop(&c, "T", "p"), (1, 0, 0));
    // rdf:type in sh:ignoredProperties is not a path: rdf:type stays ignored.
    assert_eq!(c.ignored.rdf_type, 1);
}

#[test]
fn rdf_type_counts_when_a_path_mentions_it() {
    let c = run(
        "ex:a a ex:T .",
        "ex:S a sh:NodeShape ; sh:targetClass ex:T ;
             sh:property [ sh:path rdf:type ; sh:minCount 1 ] .",
    );
    assert_eq!(c.ignored.rdf_type, 0);
    assert_eq!((c.totals.triples, c.totals.covered), (1, 1));
}

#[test]
fn sparql_constraints_cover_predicates_of_this() {
    let c = run(
        "ex:a a ex:T ; ex:salary 10 ; ex:other 1 .",
        "ex:S a sh:NodeShape ; sh:targetClass ex:T ;
             sh:sparql [ sh:select \"\"\"
                 PREFIX ex: <http://example.org/>
                 SELECT $this WHERE { $this ex:salary ?s . FILTER (?s < 0) }\"\"\" ] .",
    );
    assert_eq!(prop(&c, "T", "salary"), (1, 1, 0));
    assert_eq!(prop(&c, "T", "other"), (1, 0, 0));
    let row = class(&c, "T");
    let salary = row
        .properties
        .iter()
        .find(|p| p.iri.ends_with("salary"))
        .unwrap();
    assert!(salary.how.contains(&"sparql"), "{:?}", salary.how);
}

#[test]
fn recursive_shapes_terminate_and_reach_nested_resources() {
    let c = run(
        "ex:a a ex:Emp ; ex:reportsTo ex:b .
         ex:b ex:reportsTo ex:c .
         ex:c ex:reportsTo ex:a .",
        "ex:E a sh:NodeShape ; sh:targetClass ex:Emp ;
             sh:property [ sh:path ex:reportsTo ; sh:node ex:E ] .",
    );
    let row = class(&c, "untyped");
    assert_eq!((row.resources, row.reached, row.uncovered), (2, 2, 0));
    assert_eq!((c.totals.triples, c.totals.covered), (3, 3));
}

#[test]
fn list_cells_and_schema_resources_are_left_out() {
    let c = run(
        "ex:Onto a owl:Ontology .
         ex:Book a owl:Class ; rdfs:label \"Book\" .
         ex:b a ex:Book ; ex:authors ( ex:x ex:y ) .",
        "ex:S a sh:NodeShape ; sh:targetClass ex:Book .",
    );
    assert_eq!(c.ignored.list_cells, 4);
    assert!(c.ignored.schema >= 3, "{:?}", c.ignored);
    assert_eq!(keys(&c), vec![format!("<{EX}Book>")]);
    assert_eq!(prop(&c, "Book", "authors"), (1, 0, 0));
}

#[test]
fn user_ignores_and_annotations_are_left_out() {
    let data = "ex:a a ex:T ; ex:p 1 ; rdfs:label \"a\" .
                ex:x a ex:Skip ; ex:p 2 .";
    let shapes = "ex:S a sh:NodeShape ; sh:targetClass ex:T .";
    let opts = CoverageOptions {
        ignore: vec![
            oxrdf::NamedNode::new(format!("{EX}Skip")).unwrap(),
            oxrdf::NamedNode::new(format!("{EX}p")).unwrap(),
        ],
        ignore_annotations: true,
        deadline: None,
    };
    let c = run_with(data, shapes, &opts);
    assert_eq!(keys(&c), vec![format!("<{EX}T>")]);
    assert_eq!(c.totals.triples, 0);
    assert_eq!(c.ignored.annotations, 1);
    assert!(c.ignored.user >= 2, "{:?}", c.ignored);
}

#[test]
fn unused_rules_report_typos_and_shapes_without_focus() {
    let c = run(
        "ex:a a ex:T ; ex:address \"x\" .",
        "ex:S a sh:NodeShape ; sh:targetClass ex:T ;
             sh:property [ sh:path ex:adress ; sh:minCount 1 ] .
         ex:None a sh:NodeShape ; sh:targetClass ex:Nothing .",
    );
    let no_focus = c.unused.iter().find(|u| u.kind == "no-focus").unwrap();
    assert_eq!(no_focus.shape, format!("<{EX}None>"));
    let typo = c.unused.iter().find(|u| u.kind == "no-values").unwrap();
    assert_eq!(typo.suggestion.as_deref(), Some(&*format!("{EX}address")));
}

#[test]
fn constraint_categories_are_counted_per_triple() {
    let c = run(
        "ex:a a ex:T ; ex:p \"x\" .",
        "ex:S a sh:NodeShape ; sh:targetClass ex:T ;
             sh:property [ sh:path ex:p ; sh:minCount 1 ; sh:pattern \"x\" ] .",
    );
    let row = class(&c, "T");
    let p = &row.properties[0];
    let names: Vec<_> = c
        .categories
        .iter()
        .zip(&p.categories)
        .filter(|(_, n)| **n > 0)
        .map(|(cat, _)| cat.key)
        .collect();
    assert_eq!(names, vec!["cardinality", "text"]);
}

fn example(set: &str, file: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "examples",
        "coverage",
        set,
        file,
    ]
    .iter()
    .collect()
}

fn webshop(data: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_shacl-report"))
        .arg("--data")
        .arg(example("webshop", data))
        .arg("--shapes")
        .arg(example("webshop", "shapes.ttl"))
        .args(["--coverage-min", "100", "--coverage-min-resources", "100"])
        .output()
        .unwrap()
}

#[test]
fn coverage_min_fails_the_run_below_the_threshold() {
    let ok = webshop("data.ttl");
    assert_eq!(
        ok.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );

    let fail = webshop("data-new-field.ttl");
    let err = String::from_utf8_lossy(&fail.stderr);
    assert_eq!(fail.status.code(), Some(1), "{err}");
    assert!(err.contains("shop:giftWrap"), "{err}");
    assert!(err.contains("below the minimum of 100%"), "{err}");
}

#[test]
fn demo_sets_load() {
    for set in ["people", "paths", "library", "webshop"] {
        let out = Command::new(env!("CARGO_BIN_EXE_shacl-report"))
            .arg("--data")
            .arg(example(set, "data.ttl"))
            .arg("--shapes")
            .arg(example(set, "shapes.ttl"))
            .arg("--coverage")
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("coverage: resources="), "{set}: {err}");
    }
}
