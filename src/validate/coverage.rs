//! SHACL rule coverage: which resources of the data graph the shapes look at,
//! and which of their triples are checked by a rule.
//!
//! A resource is *targeted* when it is a focus node of an active shape, and
//! *reached* when a nested shape (`sh:node`, `sh:property` on a property
//! shape, `sh:qualifiedValueShape`, `sh:and`/`sh:or`/`sh:xone`/`sh:not`) is
//! applied to it as a value. A triple is covered when the path of a property
//! shape with at least one constraint walks over it, intermediate steps of
//! sequence paths included. Paths of property shapes without constraints do
//! not count, and neither does `sh:closed`: it only says the resource is
//! looked at. Subclasses follow SHACL: only `rdfs:subClassOf` triples in the
//! data graph count.
//!
//! Coverage is about what the rules look at, not whether the data passes,
//! so it does not depend on a validation report.

use super::path::{Path, Step};
use super::vocab::sh;
use super::{Constraint, Engine, Shape, object, objects};
use crate::report::TermDto;
use anyhow::{Result, bail};
use oxrdf::vocab::{rdf, rdfs};
use oxrdf::{Graph, NamedNode, NamedNodeRef, NamedOrBlankNode, NamedOrBlankNodeRef, Term, Triple};
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::LazyLock;
use std::time::Instant;

/// Constraint categories: key, label, components.
pub const CATEGORIES: &[(&str, &str, &str)] = &[
    (
        "cardinality",
        "Cardinality",
        "sh:minCount, sh:maxCount, sh:qualifiedMinCount, sh:qualifiedMaxCount",
    ),
    ("type", "Value type", "sh:class, sh:datatype, sh:nodeKind"),
    (
        "range",
        "Range",
        "sh:minInclusive, sh:maxInclusive, sh:minExclusive, sh:maxExclusive",
    ),
    (
        "text",
        "Text",
        "sh:pattern, sh:minLength, sh:maxLength, sh:languageIn, sh:uniqueLang",
    ),
    ("values", "Allowed values", "sh:in, sh:hasValue"),
    (
        "comparison",
        "Comparison",
        "sh:equals, sh:disjoint, sh:lessThan, sh:lessThanOrEquals",
    ),
    (
        "nested",
        "Nested shape",
        "sh:node, sh:property, sh:qualifiedValueShape",
    ),
    ("logic", "Logic", "sh:and, sh:or, sh:not, sh:xone"),
    (
        "sparql",
        "SPARQL",
        "sh:sparql and SPARQL-based constraint components",
    ),
];

const CARDINALITY: u16 = 1 << 0;
const TYPE: u16 = 1 << 1;
const RANGE: u16 = 1 << 2;
const TEXT: u16 = 1 << 3;
const VALUES: u16 = 1 << 4;
const COMPARISON: u16 = 1 << 5;
const NESTED: u16 = 1 << 6;
const LOGIC: u16 = 1 << 7;
const SPARQL: u16 = 1 << 8;

const HOW_PATH: u8 = 1;
const HOW_INVERSE: u8 = 2;
const HOW_SPARQL: u8 = 4;

/// Example resources kept per class, and example triples per property.
const CLASS_EXAMPLES: usize = 10;
const PROPERTY_EXAMPLES: usize = 5;

const SH_NS: &str = "http://www.w3.org/ns/shacl#";

/// Types of ontology and shapes resources, which are not data to cover.
const SCHEMA_TYPES: &[&str] = &[
    "http://www.w3.org/2000/01/rdf-schema#Class",
    "http://www.w3.org/2000/01/rdf-schema#Datatype",
    "http://www.w3.org/1999/02/22-rdf-syntax-ns#Property",
    "http://www.w3.org/2002/07/owl#Class",
    "http://www.w3.org/2002/07/owl#ObjectProperty",
    "http://www.w3.org/2002/07/owl#DatatypeProperty",
    "http://www.w3.org/2002/07/owl#AnnotationProperty",
    "http://www.w3.org/2002/07/owl#FunctionalProperty",
    "http://www.w3.org/2002/07/owl#InverseFunctionalProperty",
    "http://www.w3.org/2002/07/owl#TransitiveProperty",
    "http://www.w3.org/2002/07/owl#SymmetricProperty",
    "http://www.w3.org/2002/07/owl#Ontology",
    "http://www.w3.org/2002/07/owl#Restriction",
    "http://www.w3.org/2002/07/owl#AllDisjointClasses",
    "http://www.w3.org/ns/shacl#Shape",
    "http://www.w3.org/ns/shacl#NodeShape",
    "http://www.w3.org/ns/shacl#PropertyShape",
    "http://www.w3.org/ns/shacl#ValidationReport",
    "http://www.w3.org/ns/shacl#ValidationResult",
    "http://www.w3.org/ns/shacl#ConstraintComponent",
    "http://www.w3.org/ns/shacl#SPARQLConstraint",
    "http://www.w3.org/ns/shacl#PrefixDeclaration",
];

/// Predicates that only appear on ontology resources.
const SCHEMA_PREDICATES: &[&str] = &[
    "http://www.w3.org/2000/01/rdf-schema#subClassOf",
    "http://www.w3.org/2000/01/rdf-schema#subPropertyOf",
    "http://www.w3.org/2000/01/rdf-schema#domain",
    "http://www.w3.org/2000/01/rdf-schema#range",
    "http://www.w3.org/2002/07/owl#equivalentClass",
    "http://www.w3.org/2002/07/owl#equivalentProperty",
    "http://www.w3.org/2002/07/owl#inverseOf",
    "http://www.w3.org/2002/07/owl#disjointWith",
    "http://www.w3.org/2002/07/owl#onProperty",
    "http://www.w3.org/ns/shacl#declare",
];

/// Labels and documentation: counted, but flagged so they can be hidden.
const ANNOTATIONS: &[&str] = &[
    "http://www.w3.org/2000/01/rdf-schema#label",
    "http://www.w3.org/2000/01/rdf-schema#comment",
    "http://www.w3.org/2004/02/skos/core#prefLabel",
    "http://www.w3.org/2004/02/skos/core#altLabel",
    "http://www.w3.org/2004/02/skos/core#hiddenLabel",
    "http://www.w3.org/2004/02/skos/core#definition",
    "http://www.w3.org/2004/02/skos/core#note",
    "http://www.w3.org/2004/02/skos/core#scopeNote",
    "http://www.w3.org/2004/02/skos/core#editorialNote",
    "http://www.w3.org/2004/02/skos/core#example",
];
const ANNOTATION_NAMESPACES: &[&str] = &[
    "http://purl.org/dc/terms/",
    "http://purl.org/dc/elements/1.1/",
];

fn is_annotation(p: NamedNodeRef<'_>) -> bool {
    let p = p.as_str();
    ANNOTATIONS.contains(&p) || ANNOTATION_NAMESPACES.iter().any(|ns| p.starts_with(ns))
}

#[derive(Debug, Clone, Default)]
pub struct CoverageOptions {
    /// Classes and predicates to leave out.
    pub ignore: Vec<NamedNode>,
    /// Leave out labels, comments and other annotation properties.
    pub ignore_annotations: bool,
    /// Give up with an error once this has passed.
    pub deadline: Option<Instant>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryDto {
    pub key: &'static str,
    pub label: &'static str,
    pub components: &'static str,
}

/// Counts over distinct resources and triples.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Totals {
    pub resources: usize,
    pub targeted: usize,
    pub reached: usize,
    pub uncovered: usize,
    /// Resources that are focus nodes of a closed shape.
    pub closed: usize,
    pub triples: usize,
    pub covered: usize,
    /// Annotation triples among `triples`, and how many of them are covered.
    pub annotation_triples: usize,
    pub annotation_covered: usize,
}

/// Triples left out of the counts, by reason.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Ignored {
    pub rdf_type: usize,
    pub list_cells: usize,
    /// Ontology and shapes resources in the data graph.
    pub schema: usize,
    /// Triples that are also in the shapes graph.
    pub shapes_graph: usize,
    pub user: usize,
    pub annotations: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeCount {
    /// Shape key as in the shapes model (`<iri>` or `_:id`).
    pub shape: String,
    pub resources: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Via {
    pub predicate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Example {
    pub s: TermDto,
    pub o: TermDto,
    pub covered: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PropertyCoverage {
    pub iri: String,
    /// Resources of the class that have this property.
    pub resources: usize,
    pub triples: usize,
    pub covered: usize,
    /// Triples only walked by property shapes without constraints.
    pub declared: usize,
    /// Covered triples per category, in the order of [`CATEGORIES`].
    pub categories: Vec<usize>,
    /// `path`, `inverse` and/or `sparql`.
    pub how: Vec<&'static str>,
    pub shapes: Vec<String>,
    pub annotation: bool,
    pub examples: Vec<Example>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassCoverage {
    /// `<iri>` for a class, `untyped`, or `blank …` for untyped blank nodes.
    pub key: String,
    /// `class`, `untyped` or `blank`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iri: Option<String>,
    /// For untyped blank nodes: the property (and class) they are values of.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<Via>,
    /// Keys of the nearest superclasses that are rows too.
    pub parents: Vec<String>,
    pub resources: usize,
    pub targeted: usize,
    pub reached: usize,
    pub uncovered: usize,
    pub closed: usize,
    pub shapes: Vec<ShapeCount>,
    pub properties: Vec<PropertyCoverage>,
    /// Example resources, those not covered first.
    pub examples: Vec<TermDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnusedRule {
    pub shape: String,
    /// `no-focus` (targets match nothing), `no-values` (the path never has
    /// a value) or `not-evaluated` (SPARQL-based targets).
    pub kind: &'static str,
    /// For `no-values`: a property the focus nodes do have, with a similar name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// A class that would be covered if a subclass triple from the shapes graph
/// were in the data graph.
#[derive(Debug, Clone, Serialize)]
pub struct SubclassHint {
    pub class: String,
    pub superclass: String,
    pub shape: String,
    pub resources: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Coverage {
    pub categories: Vec<CategoryDto>,
    pub totals: Totals,
    pub ignored: Ignored,
    pub classes: Vec<ClassCoverage>,
    pub unused: Vec<UnusedRule>,
    pub hints: Vec<SubclassHint>,
    /// Resources whose triples the viewer shows (class and property examples).
    #[serde(skip)]
    pub example_nodes: Vec<Term>,
}

impl Coverage {
    /// Percentage of resources covered.
    pub fn resource_percent(&self) -> f64 {
        percent(
            self.totals.targeted + self.totals.reached,
            self.totals.resources,
        )
    }

    /// Percentage of triples covered.
    pub fn triple_percent(&self) -> f64 {
        percent(self.totals.covered, self.totals.triples)
    }
}

pub fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        100.0
    } else {
        part as f64 * 100.0 / whole as f64
    }
}

/// Compute the coverage of `data` by the shapes in `shapes`.
pub fn coverage(data: &Graph, shapes: &Graph, opts: &CoverageOptions) -> Result<Coverage> {
    let engine = Engine::new(data, shapes)?;
    let mut w = Walker::new(&engine, opts.deadline);
    let mut unused = Vec::new();

    for id in engine.shapes_with_targets() {
        let shape = engine.shape(&id)?;
        if shape.deactivated {
            continue;
        }
        let focus = engine.focus_nodes(&id);
        if focus.is_empty() {
            unused.push(UnusedRule {
                shape: key(&id),
                kind: "no-focus",
                suggestion: None,
            });
            continue;
        }
        let ix = w.shape_ix(&id);
        for f in focus {
            if matches!(f, Term::Literal(_)) {
                continue;
            }
            push_unique(w.targeted.entry(f.clone()).or_default(), ix);
            w.queue.push_back(Task(id.clone(), f, 0));
        }
        w.run()?;
    }
    let mut seen_target = HashSet::new();
    for t in shapes.triples_for_predicate(sh::TARGET) {
        let s: Term = t.subject.into_owned().into();
        if seen_target.insert(s.clone()) && !super::is_true(object(shapes, &s, sh::DEACTIVATED)) {
            unused.push(UnusedRule {
                shape: key(&s),
                kind: "not-evaluated",
                suggestion: None,
            });
        }
    }
    for (ps, u) in &w.prop_use {
        if u.walked > 0 && u.values == 0 {
            unused.push(UnusedRule {
                shape: key(ps),
                kind: "no-values",
                suggestion: suggest(&engine, ps, &u.sample),
            });
        }
    }
    unused.sort_by(|a, b| a.kind.cmp(b.kind).then_with(|| a.shape.cmp(&b.shape)));

    let mut cov = aggregate(&w, opts)?;
    cov.unused = unused;
    Ok(cov)
}

// ---------- walking the shapes ----------

/// Apply a shape (node or property shape) to a node. The categories are
/// added to whatever the shape covers: a property shape inside `sh:or`
/// checks its values as part of a logical constraint.
struct Task(Term, Term, u16);

#[derive(Default)]
struct Info {
    cats: u16,
    how: u8,
    shapes: Vec<u32>,
}

#[derive(Default)]
struct PropUse {
    walked: usize,
    values: usize,
    sample: Vec<Term>,
}

struct PropInfo {
    cats: u16,
    constrained: bool,
    /// Shapes applied to each value node.
    nested: Vec<Term>,
}

struct Walker<'e, 'a> {
    engine: &'e Engine<'a>,
    deadline: Option<Instant>,
    ticks: usize,
    queue: VecDeque<Task>,
    shape_keys: Vec<String>,
    shape_index: HashMap<Term, u32>,
    triples: HashMap<Triple, Info>,
    declared: HashSet<Triple>,
    targeted: HashMap<Term, Vec<u32>>,
    reached: HashMap<Term, Vec<u32>>,
    closed: HashSet<Term>,
    done: HashSet<(Term, Term)>,
    prop_info: HashMap<Term, Rc<PropInfo>>,
    node_cats: HashMap<Term, u16>,
    prop_use: HashMap<Term, PropUse>,
    sparql_preds: HashMap<String, Rc<Vec<NamedNode>>>,
}

fn push_unique(v: &mut Vec<u32>, x: u32) {
    if !v.contains(&x) {
        v.push(x);
    }
}

fn key(t: &Term) -> String {
    match t {
        Term::NamedNode(n) => format!("<{}>", n.as_str()),
        Term::BlankNode(b) => format!("_:{}", b.as_str()),
        other => other.to_string(),
    }
}

fn subject_of(t: &Term) -> Option<NamedOrBlankNode> {
    match t {
        Term::NamedNode(n) => Some(n.clone().into()),
        Term::BlankNode(b) => Some(b.clone().into()),
        _ => None,
    }
}

impl<'e, 'a> Walker<'e, 'a> {
    fn new(engine: &'e Engine<'a>, deadline: Option<Instant>) -> Self {
        Self {
            engine,
            deadline,
            ticks: 0,
            queue: VecDeque::new(),
            shape_keys: Vec::new(),
            shape_index: HashMap::new(),
            triples: HashMap::new(),
            declared: HashSet::new(),
            targeted: HashMap::new(),
            reached: HashMap::new(),
            closed: HashSet::new(),
            done: HashSet::new(),
            prop_info: HashMap::new(),
            node_cats: HashMap::new(),
            prop_use: HashMap::new(),
            sparql_preds: HashMap::new(),
        }
    }

    fn shape_ix(&mut self, id: &Term) -> u32 {
        if let Some(i) = self.shape_index.get(id) {
            return *i;
        }
        let i = self.shape_keys.len() as u32;
        self.shape_keys.push(key(id));
        self.shape_index.insert(id.clone(), i);
        i
    }

    /// Work through the queue. A queue instead of recursion keeps long
    /// chains of nested shapes (`ex:knows sh:node ex:PersonShape`) off the stack.
    fn run(&mut self) -> Result<()> {
        while let Some(Task(id, node, extra)) = self.queue.pop_front() {
            self.ticks += 1;
            if self.ticks.is_multiple_of(256) && self.deadline.is_some_and(|d| Instant::now() > d) {
                bail!("coverage took longer than this server allows");
            }
            if matches!(node, Term::Literal(_)) || !self.done.insert((id.clone(), node.clone())) {
                continue;
            }
            let shape = self.engine.shape(&id)?;
            if shape.deactivated {
                continue;
            }
            if shape.path.is_some() {
                self.walk_property(&id, &shape, &node, extra)?;
            } else {
                self.walk_node(&id, &shape, &node, extra);
            }
        }
        Ok(())
    }

    fn walk_node(&mut self, id: &Term, shape: &Shape, node: &Term, extra: u16) {
        for c in &shape.constraints {
            match c {
                Constraint::Property(ps) => {
                    self.queue.push_back(Task(ps.clone(), node.clone(), extra))
                }
                Constraint::Node(n) => self.queue.push_back(Task(n.clone(), node.clone(), extra)),
                Constraint::Not(n) => {
                    self.queue
                        .push_back(Task(n.clone(), node.clone(), extra | LOGIC))
                }
                Constraint::And(ms) | Constraint::Or(ms) | Constraint::Xone(ms) => {
                    for m in ms {
                        self.queue
                            .push_back(Task(m.clone(), node.clone(), extra | LOGIC));
                    }
                }
                Constraint::Closed(_) => {
                    self.closed.insert(node.clone());
                }
                Constraint::Sparql(q) => {
                    let (query, prefixes) = q.query();
                    self.sparql_node(id, node, query, prefixes);
                }
                Constraint::Component(comp, _) => {
                    for (query, prefixes) in comp.queries() {
                        self.sparql_node(id, node, query, prefixes);
                    }
                }
                _ => {}
            }
        }
    }

    fn walk_property(&mut self, id: &Term, shape: &Shape, focus: &Term, extra: u16) -> Result<()> {
        let Some(path) = &shape.path else {
            return Ok(());
        };
        let info = self.prop_info(id, shape)?;
        let ix = self.shape_ix(id);
        let mut steps: Vec<(Triple, bool)> = Vec::new();
        let values = path.eval_traced(self.engine.data, focus, &mut |s: Step<'_>| {
            if let Some(subject) = subject_of(s.subject) {
                steps.push((
                    Triple::new(subject, s.predicate.clone(), s.object.clone()),
                    s.inverse,
                ));
            }
        });
        let u = self.prop_use.entry(id.clone()).or_default();
        u.walked += 1;
        u.values += values.len();
        if u.sample.len() < 20 {
            u.sample.push(focus.clone());
        }
        for (t, inverse) in steps {
            if info.constrained {
                let e = self.triples.entry(t).or_default();
                e.cats |= info.cats | extra;
                e.how |= if inverse { HOW_INVERSE } else { HOW_PATH };
                push_unique(&mut e.shapes, ix);
            } else {
                self.declared.insert(t);
            }
        }
        for v in values {
            if matches!(v, Term::Literal(_)) {
                continue;
            }
            for n in &info.nested {
                let nix = self.shape_ix(n);
                push_unique(self.reached.entry(v.clone()).or_default(), nix);
                self.queue.push_back(Task(n.clone(), v.clone(), 0));
            }
        }
        Ok(())
    }

    /// Categories, and the shapes applied to the value nodes, of a property shape.
    fn prop_info(&mut self, id: &Term, shape: &Shape) -> Result<Rc<PropInfo>> {
        if let Some(i) = self.prop_info.get(id) {
            return Ok(i.clone());
        }
        let mut cats = 0;
        let mut nested = Vec::new();
        for c in &shape.constraints {
            cats |= match c {
                Constraint::Node(n) => {
                    nested.push(n.clone());
                    NESTED | self.node_cats(n)
                }
                Constraint::Not(n) => {
                    nested.push(n.clone());
                    LOGIC | self.node_cats(n)
                }
                Constraint::And(ms) | Constraint::Or(ms) | Constraint::Xone(ms) => {
                    let mut c = LOGIC;
                    for m in ms {
                        nested.push(m.clone());
                        c |= self.node_cats(m);
                    }
                    c
                }
                Constraint::Qualified { shape, .. } => {
                    nested.push(shape.clone());
                    CARDINALITY | NESTED | self.node_cats(shape)
                }
                Constraint::Property(ps) => {
                    nested.push(ps.clone());
                    NESTED
                }
                other => category(other),
            };
        }
        let info = Rc::new(PropInfo {
            cats,
            constrained: !shape.constraints.is_empty(),
            nested,
        });
        self.prop_info.insert(id.clone(), info.clone());
        Ok(info)
    }

    /// Categories of what a shape checks on the node it is applied to, used
    /// for the triple that leads to a value checked with `sh:node` and friends.
    fn node_cats(&mut self, id: &Term) -> u16 {
        if let Some(c) = self.node_cats.get(id) {
            return *c;
        }
        self.node_cats.insert(id.clone(), 0);
        let Ok(shape) = self.engine.shape(id) else {
            return 0;
        };
        if shape.deactivated || shape.path.is_some() {
            return 0;
        }
        let mut cats = 0;
        for c in &shape.constraints {
            cats |= match c {
                Constraint::Property(_) | Constraint::Closed(_) => 0,
                Constraint::Node(n) => NESTED | self.node_cats(n),
                Constraint::Not(n) => LOGIC | self.node_cats(n),
                Constraint::And(ms) | Constraint::Or(ms) | Constraint::Xone(ms) => {
                    let mut c = LOGIC;
                    for m in ms {
                        c |= self.node_cats(m);
                    }
                    c
                }
                other => category(other),
            };
        }
        self.node_cats.insert(id.clone(), cats);
        cats
    }

    /// A SPARQL constraint on a node shape: we cannot know what the query
    /// reads, so count the triples of every `$this <predicate>` pattern in it.
    fn sparql_node(&mut self, id: &Term, node: &Term, query: &str, prefixes: &[(String, String)]) {
        let preds = match self.sparql_preds.get(query) {
            Some(p) => p.clone(),
            None => {
                let p = Rc::new(this_predicates(query, prefixes));
                self.sparql_preds.insert(query.to_string(), p.clone());
                p
            }
        };
        let Some(s) = subject_of(node) else { return };
        let ix = self.shape_ix(id);
        for p in preds.iter() {
            let objects: Vec<Term> = self
                .engine
                .data
                .objects_for_subject_predicate(s.as_ref(), p)
                .map(|o| o.into_owned())
                .collect();
            for o in objects {
                let e = self
                    .triples
                    .entry(Triple::new(s.clone(), p.clone(), o))
                    .or_default();
                e.cats |= SPARQL;
                e.how |= HOW_SPARQL;
                push_unique(&mut e.shapes, ix);
            }
        }
    }
}

fn category(c: &Constraint) -> u16 {
    match c {
        Constraint::MinCount(_) | Constraint::MaxCount(_) => CARDINALITY,
        Constraint::Class(_) | Constraint::Datatype(_) | Constraint::NodeKind(_) => TYPE,
        Constraint::Range(..) => RANGE,
        Constraint::MinLength(_)
        | Constraint::MaxLength(_)
        | Constraint::Pattern(..)
        | Constraint::LanguageIn(_)
        | Constraint::UniqueLang => TEXT,
        Constraint::In(_) | Constraint::HasValue(_) => VALUES,
        Constraint::Pair(..) => COMPARISON,
        Constraint::Node(_) | Constraint::Property(_) => NESTED,
        Constraint::Qualified { .. } => CARDINALITY | NESTED,
        Constraint::Not(_) | Constraint::And(_) | Constraint::Or(_) | Constraint::Xone(_) => LOGIC,
        Constraint::Sparql(_) | Constraint::Component(..) => SPARQL,
        Constraint::Closed(_) => 0,
    }
}

static THIS_PRED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[$?]this\s+(<[^>\s]*>|[A-Za-z][\w.\-]*:[\w.\-]*|:[\w.\-]+)").unwrap()
});
static PREFIX_DECL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)PREFIX\s+([\w.\-]*):\s*<([^>]*)>").unwrap());

/// Predicates used in `$this <predicate> …` triple patterns of a query.
fn this_predicates(query: &str, prefixes: &[(String, String)]) -> Vec<NamedNode> {
    let mut ns: HashMap<String, String> = prefixes.iter().cloned().collect();
    for c in PREFIX_DECL.captures_iter(query) {
        ns.insert(c[1].to_string(), c[2].to_string());
    }
    let mut out = Vec::new();
    for c in THIS_PRED.captures_iter(query) {
        let t = &c[1];
        let iri = if let Some(iri) = t.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
            Some(iri.to_string())
        } else {
            t.split_once(':')
                .and_then(|(p, l)| ns.get(p).map(|base| format!("{base}{l}")))
        };
        if let Some(n) = iri.and_then(|i| NamedNode::new(i).ok())
            && !out.contains(&n)
        {
            out.push(n);
        }
    }
    out
}

/// For a property shape whose path never has a value: a property of the
/// focus nodes with a name close to the one in the path (a likely typo).
fn suggest(engine: &Engine<'_>, ps: &Term, sample: &[Term]) -> Option<String> {
    let path = object(engine.shapes, ps, sh::PATH)?;
    let Ok(Path::Predicate(p)) = Path::parse(engine.shapes, &path) else {
        return None;
    };
    let wanted = local_name(p.as_str());
    let mut best: Option<(usize, String)> = None;
    for n in sample {
        let Some(s) = subject_of(n) else { continue };
        for t in engine.data.triples_for_subject(s.as_ref()) {
            let q = t.predicate.as_str();
            if q == p.as_str() || t.predicate == rdf::TYPE {
                continue;
            }
            let d = levenshtein(wanted, local_name(q));
            let limit = (wanted.chars().count() / 3).clamp(1, 3);
            if d <= limit && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, q.to_string()));
            }
        }
    }
    best.map(|(_, q)| q)
}

fn local_name(iri: &str) -> &str {
    iri.rsplit(['#', '/', ':']).next().unwrap_or(iri)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

// ---------- aggregation ----------

#[derive(Default)]
struct PropAgg {
    resources: usize,
    triples: usize,
    covered: usize,
    declared: usize,
    cats: [usize; 9],
    how: u8,
    shapes: BTreeMap<u32, usize>,
    uncovered_examples: Vec<(Term, Term)>,
    covered_examples: Vec<(Term, Term)>,
}

#[derive(Default)]
struct ClassAgg {
    via: Option<Via>,
    resources: usize,
    targeted: usize,
    reached: usize,
    uncovered: usize,
    closed: usize,
    shapes: BTreeMap<u32, usize>,
    props: BTreeMap<String, PropAgg>,
    uncovered_examples: Vec<Term>,
    covered_examples: Vec<Term>,
}

/// Subjects that are ontology or shapes resources, plus the blank nodes
/// only they point to (OWL restrictions, lists).
fn schema_subjects(data: &Graph) -> HashSet<Term> {
    let mut out: HashSet<Term> = HashSet::new();
    for t in SCHEMA_TYPES {
        for s in data.subjects_for_predicate_object(rdf::TYPE, NamedNodeRef::new_unchecked(t)) {
            out.insert(s.into_owned().into());
        }
    }
    for p in SCHEMA_PREDICATES {
        for t in data.triples_for_predicate(NamedNodeRef::new_unchecked(p)) {
            out.insert(t.subject.into_owned().into());
        }
    }
    // Any subject using SHACL vocabulary is a shape or part of a report.
    for t in data.iter() {
        if t.predicate.as_str().starts_with(SH_NS) {
            out.insert(t.subject.into_owned().into());
        }
    }
    let mut todo: Vec<Term> = out.iter().cloned().collect();
    while let Some(n) = todo.pop() {
        let Some(s) = subject_of(&n) else { continue };
        for o in data.triples_for_subject(s.as_ref()).map(|t| t.object) {
            if let oxrdf::TermRef::BlankNode(b) = o {
                let t: Term = b.into_owned().into();
                if out.insert(t.clone()) {
                    todo.push(t);
                }
            }
        }
    }
    out
}

fn aggregate(w: &Walker<'_, '_>, opts: &CoverageOptions) -> Result<Coverage> {
    let data = w.engine.data;
    let shapes = w.engine.shapes;
    let schema = schema_subjects(data);
    let ignore: HashSet<&str> = opts.ignore.iter().map(NamedNode::as_str).collect();
    // rdf:type is the class itself, so it only counts when a path checks it.
    let type_checked = shapes
        .triples_for_predicate(sh::PATH)
        .filter_map(|t| Path::parse(shapes, &t.object.into_owned()).ok())
        .any(|p| p.mentions(rdf::TYPE));

    let mut ignored = Ignored::default();
    let mut totals = Totals::default();
    let mut classes: HashMap<String, ClassAgg> = HashMap::new();
    let mut subjects: Vec<NamedOrBlankNodeRef<'_>> = Vec::new();
    {
        let mut seen = HashSet::new();
        for t in data.iter() {
            if seen.insert(t.subject) {
                subjects.push(t.subject);
            }
        }
    }

    for (i, s) in subjects.into_iter().enumerate() {
        if i % 1024 == 0 && opts.deadline.is_some_and(|d| Instant::now() > d) {
            bail!("coverage took longer than this server allows");
        }
        let st: Term = s.into_owned().into();
        let n_triples = data.triples_for_subject(s).count();
        if schema.contains(&st) {
            ignored.schema += n_triples;
            continue;
        }
        let is_list_cell = matches!(s, NamedOrBlankNodeRef::BlankNode(_))
            && data.object_for_subject_predicate(s, rdf::FIRST).is_some();
        let mut types: Vec<String> = Vec::new();
        let mut any_type = false;
        for t in data.objects_for_subject_predicate(s, rdf::TYPE) {
            if let oxrdf::TermRef::NamedNode(c) = t {
                any_type = true;
                if !ignore.contains(c.as_str()) {
                    types.push(c.as_str().to_string());
                }
            }
        }
        if any_type && types.is_empty() {
            ignored.user += n_triples;
            continue;
        }
        // The triples that count.
        let mut counted = Vec::new();
        for t in data.triples_for_subject(s) {
            if shapes.contains(t) {
                ignored.shapes_graph += 1;
            } else if is_list_cell && (t.predicate == rdf::FIRST || t.predicate == rdf::REST) {
                ignored.list_cells += 1;
            } else if t.predicate == rdf::TYPE && !type_checked {
                ignored.rdf_type += 1;
            } else if ignore.contains(t.predicate.as_str()) {
                ignored.user += 1;
            } else if opts.ignore_annotations && is_annotation(t.predicate) {
                ignored.annotations += 1;
            } else {
                counted.push(t);
            }
        }
        if is_list_cell && counted.is_empty() {
            continue;
        }
        types.sort();
        let groups: Vec<(String, Option<Via>)> = if !types.is_empty() {
            types.iter().map(|c| (format!("<{c}>"), None)).collect()
        } else if let NamedOrBlankNodeRef::BlankNode(_) = s {
            match incoming(data, &st, &schema) {
                Some((p, class)) => {
                    let k = match &class {
                        Some(c) => format!("blank <{p}> <{c}>"),
                        None => format!("blank <{p}>"),
                    };
                    vec![(
                        k,
                        Some(Via {
                            predicate: p,
                            class,
                        }),
                    )]
                }
                None => vec![("blank".to_string(), None)],
            }
        } else {
            vec![("untyped".to_string(), None)]
        };

        let targeted = w.targeted.get(&st);
        let reached = w.reached.get(&st);
        let closed = w.closed.contains(&st);
        totals.resources += 1;
        if targeted.is_some() {
            totals.targeted += 1;
        } else if reached.is_some() {
            totals.reached += 1;
        } else {
            totals.uncovered += 1;
        }
        totals.closed += usize::from(closed);
        let mut shape_ixs: Vec<u32> = Vec::new();
        for ix in targeted.into_iter().chain(reached).flatten() {
            push_unique(&mut shape_ixs, *ix);
        }

        let mut infos = Vec::with_capacity(counted.len());
        for t in &counted {
            let owned = t.into_owned();
            let info = w.triples.get(&owned);
            let declared = info.is_none() && w.declared.contains(&owned);
            totals.triples += 1;
            totals.covered += usize::from(info.is_some());
            if is_annotation(t.predicate) {
                totals.annotation_triples += 1;
                totals.annotation_covered += usize::from(info.is_some());
            }
            infos.push((owned, info, declared));
        }

        for (gk, via) in groups {
            let agg = classes.entry(gk).or_default();
            if agg.via.is_none() {
                agg.via = via;
            }
            agg.resources += 1;
            let covered_resource = targeted.is_some() || reached.is_some();
            if targeted.is_some() {
                agg.targeted += 1;
            } else if reached.is_some() {
                agg.reached += 1;
            } else {
                agg.uncovered += 1;
            }
            agg.closed += usize::from(closed);
            for ix in &shape_ixs {
                *agg.shapes.entry(*ix).or_default() += 1;
            }
            if covered_resource {
                if agg.covered_examples.len() < CLASS_EXAMPLES {
                    agg.covered_examples.push(st.clone());
                }
            } else if agg.uncovered_examples.len() < CLASS_EXAMPLES {
                agg.uncovered_examples.push(st.clone());
            }
            let mut seen_preds: HashSet<&str> = HashSet::new();
            for (t, info, declared) in &infos {
                let p = t.predicate.as_str();
                let pa = agg.props.entry(p.to_string()).or_default();
                if seen_preds.insert(p) {
                    pa.resources += 1;
                }
                pa.triples += 1;
                pa.declared += usize::from(*declared);
                match info {
                    Some(info) => {
                        pa.covered += 1;
                        pa.how |= info.how;
                        for (i, c) in pa.cats.iter_mut().enumerate() {
                            if info.cats & (1 << i) != 0 {
                                *c += 1;
                            }
                        }
                        for ix in &info.shapes {
                            *pa.shapes.entry(*ix).or_default() += 1;
                        }
                        if pa.covered_examples.len() < PROPERTY_EXAMPLES {
                            pa.covered_examples.push((st.clone(), t.object.clone()));
                        }
                    }
                    None => {
                        if pa.uncovered_examples.len() < PROPERTY_EXAMPLES {
                            pa.uncovered_examples.push((st.clone(), t.object.clone()));
                        }
                    }
                }
            }
        }
    }

    // Superclasses, from the data and the shapes graph (display only).
    let row_keys: HashSet<String> = classes.keys().cloned().collect();
    let supers = |c: &str| -> Vec<String> {
        let n = Term::NamedNode(NamedNode::new_unchecked(c));
        let mut out: Vec<String> = Vec::new();
        for g in [data, shapes] {
            for s in objects(g, &n, rdfs::SUB_CLASS_OF) {
                if let Term::NamedNode(s) = s
                    && !out.iter().any(|o| o == s.as_str())
                {
                    out.push(s.as_str().to_string());
                }
            }
        }
        out
    };
    let nearest_parents = |c: &str| -> Vec<String> {
        let mut found = Vec::new();
        let mut seen: HashSet<String> = HashSet::from([c.to_string()]);
        let mut level = supers(c);
        while !level.is_empty() && found.is_empty() {
            let mut next = Vec::new();
            for s in level {
                if !seen.insert(s.clone()) {
                    continue;
                }
                let k = format!("<{s}>");
                if row_keys.contains(&k) {
                    found.push(k);
                } else {
                    next.extend(supers(&s));
                }
            }
            level = next;
        }
        found
    };

    let mut example_nodes: Vec<Term> = Vec::new();
    let mut example_seen: HashSet<Term> = HashSet::new();
    let mut add_example = |t: &Term, out: &mut Vec<Term>| {
        if example_seen.insert(t.clone()) {
            out.push(t.clone());
        }
    };
    let dto = |t: &Term| TermDto::from_term(t.as_ref());
    let mut hints = Vec::new();
    let mut rows: Vec<ClassCoverage> = Vec::new();
    for (k, agg) in classes {
        let iri = k
            .strip_prefix('<')
            .and_then(|k| k.strip_suffix('>'))
            .map(str::to_string);
        let kind = if iri.is_some() {
            "class"
        } else if k == "untyped" {
            "untyped"
        } else {
            "blank"
        };
        if let Some(c) = &iri
            && agg.uncovered > 0
            && let Some(h) = subclass_hint(w.engine, c, agg.uncovered)
        {
            hints.push(h);
        }
        let mut examples = Vec::new();
        for t in agg
            .uncovered_examples
            .iter()
            .chain(&agg.covered_examples)
            .take(CLASS_EXAMPLES)
        {
            add_example(t, &mut example_nodes);
            examples.push(dto(t));
        }
        let mut shapes_list: Vec<ShapeCount> = agg
            .shapes
            .iter()
            .map(|(ix, n)| ShapeCount {
                shape: w.shape_keys[*ix as usize].clone(),
                resources: *n,
            })
            .collect();
        shapes_list.sort_by(|a, b| {
            b.resources
                .cmp(&a.resources)
                .then_with(|| a.shape.cmp(&b.shape))
        });
        let mut properties: Vec<PropertyCoverage> = agg
            .props
            .into_iter()
            .map(|(p, pa)| {
                let mut how = Vec::new();
                for (bit, name) in [
                    (HOW_PATH, "path"),
                    (HOW_INVERSE, "inverse"),
                    (HOW_SPARQL, "sparql"),
                ] {
                    if pa.how & bit != 0 {
                        how.push(name);
                    }
                }
                let mut shapes: Vec<(u32, usize)> = pa.shapes.into_iter().collect();
                shapes.sort_by_key(|s| std::cmp::Reverse(s.1));
                let mut examples = Vec::new();
                for (s, o) in &pa.uncovered_examples {
                    examples.push(Example {
                        s: dto(s),
                        o: dto(o),
                        covered: false,
                    });
                }
                for (s, o) in pa
                    .covered_examples
                    .iter()
                    .take(PROPERTY_EXAMPLES.saturating_sub(examples.len()))
                {
                    examples.push(Example {
                        s: dto(s),
                        o: dto(o),
                        covered: true,
                    });
                }
                for (s, _) in pa.uncovered_examples.iter().take(2) {
                    add_example(s, &mut example_nodes);
                }
                PropertyCoverage {
                    annotation: is_annotation(NamedNodeRef::new_unchecked(&p)),
                    iri: p,
                    resources: pa.resources,
                    triples: pa.triples,
                    covered: pa.covered,
                    declared: pa.declared,
                    categories: pa.cats.to_vec(),
                    how,
                    shapes: shapes
                        .into_iter()
                        .map(|(ix, _)| w.shape_keys[ix as usize].clone())
                        .collect(),
                    examples,
                }
            })
            .collect();
        properties.sort_by(|a, b| {
            (b.triples - b.covered)
                .cmp(&(a.triples - a.covered))
                .then_with(|| b.triples.cmp(&a.triples))
                .then_with(|| a.iri.cmp(&b.iri))
        });
        rows.push(ClassCoverage {
            parents: iri.as_deref().map(&nearest_parents).unwrap_or_default(),
            key: k,
            kind,
            iri,
            via: agg.via,
            resources: agg.resources,
            targeted: agg.targeted,
            reached: agg.reached,
            uncovered: agg.uncovered,
            closed: agg.closed,
            shapes: shapes_list,
            properties,
            examples,
        });
    }
    let kind_rank = |k: &str| match k {
        "class" => 0,
        "blank" => 1,
        _ => 2,
    };
    rows.sort_by(|a, b| {
        kind_rank(a.kind)
            .cmp(&kind_rank(b.kind))
            .then_with(|| b.resources.cmp(&a.resources))
            .then_with(|| a.key.cmp(&b.key))
    });
    hints.sort_by(|a: &SubclassHint, b| {
        b.resources
            .cmp(&a.resources)
            .then_with(|| a.class.cmp(&b.class))
    });

    Ok(Coverage {
        categories: CATEGORIES
            .iter()
            .map(|(key, label, components)| CategoryDto {
                key,
                label,
                components,
            })
            .collect(),
        totals,
        ignored,
        classes: rows,
        unused: Vec::new(),
        hints,
        example_nodes,
    })
}

/// The property (and the class of its subject) an untyped blank node is a value of.
fn incoming(data: &Graph, node: &Term, schema: &HashSet<Term>) -> Option<(String, Option<String>)> {
    let mut best: Option<(String, Option<String>)> = None;
    for t in data.triples_for_object(node) {
        let s: Term = t.subject.into_owned().into();
        if schema.contains(&s) || t.predicate == rdf::REST || t.predicate == rdf::FIRST {
            continue;
        }
        let class = data
            .objects_for_subject_predicate(t.subject, rdf::TYPE)
            .filter_map(|c| match c {
                oxrdf::TermRef::NamedNode(c) => Some(c.as_str().to_string()),
                _ => None,
            })
            .min();
        let cand = (t.predicate.as_str().to_string(), class);
        if best.as_ref().is_none_or(|b| cand < *b) {
            best = Some(cand);
        }
    }
    best
}

/// A shape that targets a superclass of `class` through a subclass triple
/// that is in the shapes graph but not in the data graph.
fn subclass_hint(engine: &Engine<'_>, class: &str, resources: usize) -> Option<SubclassHint> {
    let c = Term::NamedNode(NamedNode::new_unchecked(class));
    let in_data = engine.superclasses_of(&c);
    let mut seen: HashSet<Term> = HashSet::new();
    let mut todo = vec![c.clone()];
    while let Some(n) = todo.pop() {
        for g in [engine.shapes, engine.data] {
            for s in objects(g, &n, rdfs::SUB_CLASS_OF) {
                if seen.insert(s.clone()) {
                    todo.push(s);
                }
            }
        }
    }
    let mut candidates: Vec<&Term> = seen.iter().filter(|s| !in_data.contains(*s)).collect();
    candidates.sort_by_key(|t| t.to_string());
    for sup in candidates {
        let mut targeting: Vec<Term> = engine
            .shapes
            .subjects_for_predicate_object(sh::TARGET_CLASS, sup)
            .map(|s| s.into_owned().into())
            .collect();
        if objects(engine.shapes, sup, rdf::TYPE)
            .iter()
            .any(|t| *t == sh::NODE_SHAPE.into() || *t == sh::PROPERTY_SHAPE.into())
        {
            targeting.push(sup.clone());
        }
        targeting.sort_by_key(|t| t.to_string());
        for t in targeting {
            if super::is_true(object(engine.shapes, &t, sh::DEACTIVATED)) {
                continue;
            }
            let Term::NamedNode(sup_iri) = sup else {
                continue;
            };
            return Some(SubclassHint {
                class: class.to_string(),
                superclass: sup_iri.as_str().to_string(),
                shape: key(&t),
                resources,
            });
        }
    }
    None
}
