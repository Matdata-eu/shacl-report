//! Build an explorable model of a SHACL shapes graph: node and property
//! shapes, their targets, every constraint parameter (SHACL Core and custom
//! SPARQL-based components), nested shapes and SPARQL constraints.
//!
//! As in the report model, display text uses `<iri>` tokens so the viewer can
//! shorten IRIs with the known prefixes.

use crate::report::{SH, as_subject, literal_value, read_list, render_path, render_value, sh};
use oxrdf::vocab::{rdf, rdfs};
use oxrdf::{Graph, NamedNodeRef, NamedOrBlankNodeRef, TermRef};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Shape-structure predicates that are not constraint parameters.
const STRUCTURAL: &[&str] = &[
    "path",
    "property",
    "name",
    "description",
    "message",
    "severity",
    "deactivated",
    "targetClass",
    "targetNode",
    "targetSubjectsOf",
    "targetObjectsOf",
    "target",
    "sparql",
    "order",
    "group",
    "defaultValue",
    "prefixes",
    "declare",
    "rule",
];

/// Parameters whose values are themselves shapes.
const SHAPE_PARAMS: &[&str] = &["node", "qualifiedValueShape", "not", "and", "or", "xone"];

/// Parameters whose value is an RDF list that should be shown item by item.
const LIST_PARAMS: &[&str] = &["in", "languageIn", "ignoredProperties", "and", "or", "xone"];

#[derive(Debug, Clone, Serialize)]
pub struct ValueDto {
    /// Display text with `<iri>` tokens.
    pub text: String,
    /// The value as a SPARQL term (empty when it cannot be expressed, e.g. a blank node).
    pub sparql: String,
    /// Index of the shape this value refers to, for shape-valued parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConstraintDto {
    /// Full IRI of the parameter, e.g. `…shacl#minCount`.
    pub param: String,
    pub values: Vec<ValueDto>,
    /// The parameter's value was an RDF list (e.g. `sh:in`).
    pub list: bool,
    /// Set for parameters of a custom (SPARQL-based) constraint component.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetDto {
    /// `class`, `node`, `subjectsOf`, `objectsOf`, `implicitClass` or `sparql`.
    pub kind: &'static str,
    /// IRI token for the first five kinds; the SELECT query for `sparql`.
    pub value: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prefixes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SparqlDto {
    /// `select` or `ask`.
    pub kind: &'static str,
    pub query: String,
    pub messages: Vec<String>,
    /// Prefix declarations from `sh:prefixes` / `sh:declare`.
    pub prefixes: Vec<(String, String)>,
    pub deactivated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeDto {
    pub id: usize,
    /// `<iri>` or `_:label`; matches `ResultRow::shape_key`.
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iri: Option<String>,
    /// `node` or `property`.
    pub kind: &'static str,
    /// Display label with `<iri>` tokens.
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_iri: Option<String>,
    pub targets: Vec<TargetDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    pub deactivated: bool,
    pub messages: Vec<String>,
    pub constraints: Vec<ConstraintDto>,
    pub properties: Vec<usize>,
    /// Shapes that point at this one (via `sh:property` or a shape parameter).
    pub parents: Vec<usize>,
    pub sparql: Vec<SparqlDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidatorDto {
    /// `validator`, `nodeValidator` or `propertyValidator`.
    pub role: String,
    pub kind: &'static str,
    pub query: String,
    pub prefixes: Vec<(String, String)>,
    pub messages: Vec<String>,
}

/// A custom constraint component declared with `sh:ConstraintComponent`.
#[derive(Debug, Clone, Serialize)]
pub struct ComponentDto {
    pub id: usize,
    pub iri: String,
    pub label: String,
    /// Parameter path IRIs.
    pub params: Vec<String>,
    pub validators: Vec<ValidatorDto>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ShapesModel {
    pub shapes: Vec<ShapeDto>,
    /// Shapes to list at the top level: named shapes and anything no other
    /// shape points at.
    pub roots: Vec<usize>,
    pub components: Vec<ComponentDto>,
}

pub(crate) fn key_of(n: NamedOrBlankNodeRef<'_>) -> String {
    match n {
        NamedOrBlankNodeRef::NamedNode(n) => format!("<{}>", n.as_str()),
        NamedOrBlankNodeRef::BlankNode(b) => format!("_:{}", b.as_str()),
    }
}

/// Render a term as SPARQL syntax.
fn sparql_term(t: TermRef<'_>) -> String {
    match t {
        TermRef::NamedNode(n) => format!("<{}>", n.as_str()),
        TermRef::Literal(l) => {
            let dt = l.datatype().as_str();
            let plain = matches!(
                dt.strip_prefix("http://www.w3.org/2001/XMLSchema#"),
                Some("integer" | "decimal" | "double" | "boolean")
            );
            if plain && l.language().is_none() {
                l.value().to_string()
            } else {
                l.to_string()
            }
        }
        _ => String::new(),
    }
}

fn local(iri: &str) -> Option<&str> {
    iri.strip_prefix(SH)
}

struct Builder<'a> {
    g: &'a Graph,
    index: HashMap<NamedOrBlankNodeRef<'a>, usize>,
    nodes: Vec<NamedOrBlankNodeRef<'a>>,
    /// Custom component parameter predicate → component index.
    custom_params: HashMap<String, usize>,
}

impl<'a> Builder<'a> {
    fn intern(&mut self, n: NamedOrBlankNodeRef<'a>) -> usize {
        if let Some(i) = self.index.get(&n) {
            return *i;
        }
        let i = self.nodes.len();
        self.nodes.push(n);
        self.index.insert(n, i);
        i
    }

    fn literals(&self, s: NamedOrBlankNodeRef<'a>, p: NamedNodeRef<'_>) -> Vec<String> {
        let mut v: Vec<String> = self
            .g
            .objects_for_subject_predicate(s, p)
            .filter_map(literal_value)
            .collect();
        v.sort();
        v.dedup();
        v
    }

    fn sh(&self, s: NamedOrBlankNodeRef<'a>, local: &str) -> Option<TermRef<'a>> {
        let p = format!("{SH}{local}");
        self.g
            .object_for_subject_predicate(s, NamedNodeRef::new_unchecked(&p))
    }

    fn sh_all(&self, s: NamedOrBlankNodeRef<'a>, local: &str) -> Vec<TermRef<'a>> {
        let p = format!("{SH}{local}");
        self.g
            .objects_for_subject_predicate(s, NamedNodeRef::new_unchecked(&p))
            .collect()
    }

    fn is_true(&self, s: NamedOrBlankNodeRef<'a>, local: &str) -> bool {
        matches!(self.sh(s, local), Some(TermRef::Literal(l)) if l.value() == "true" || l.value() == "1")
    }

    /// Collect `sh:prefixes` → `sh:declare` declarations reachable from `s`.
    fn prefixes(&self, s: NamedOrBlankNodeRef<'a>) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut todo: Vec<NamedOrBlankNodeRef<'a>> = self
            .sh_all(s, "prefixes")
            .into_iter()
            .filter_map(as_subject)
            .collect();
        while let Some(n) = todo.pop() {
            if !seen.insert(n) {
                continue;
            }
            for d in self.sh_all(n, "declare").into_iter().filter_map(as_subject) {
                let prefix = self.sh(d, "prefix").and_then(literal_value);
                let ns = self.sh(d, "namespace").and_then(|t| match t {
                    TermRef::Literal(l) => Some(l.value().to_string()),
                    TermRef::NamedNode(n) => Some(n.as_str().to_string()),
                    _ => None,
                });
                if let (Some(p), Some(ns)) = (prefix, ns)
                    && !out.iter().any(|(q, _)| *q == p)
                {
                    out.push((p, ns));
                }
            }
            // owl:imports-style chaining of prefix declarations.
            todo.extend(
                self.sh_all(n, "prefixes")
                    .into_iter()
                    .filter_map(as_subject),
            );
        }
        out
    }

    fn sparql_of(&self, s: NamedOrBlankNodeRef<'a>) -> Vec<SparqlDto> {
        self.sh_all(s, "sparql")
            .into_iter()
            .filter_map(as_subject)
            .filter_map(|c| {
                let (kind, query) = match (self.sh(c, "select"), self.sh(c, "ask")) {
                    (Some(q), _) => ("select", literal_value(q)?),
                    (None, Some(q)) => ("ask", literal_value(q)?),
                    _ => return None,
                };
                Some(SparqlDto {
                    kind,
                    query,
                    messages: self.literals(c, sh::MESSAGE),
                    prefixes: self.prefixes(c),
                    deactivated: self.is_true(c, "deactivated"),
                })
            })
            .collect()
    }

    fn value(&mut self, t: TermRef<'a>, is_shape: bool) -> ValueDto {
        let shape = if is_shape {
            as_subject(t).map(|n| self.intern(n))
        } else {
            None
        };
        ValueDto {
            text: render_value(self.g, t),
            sparql: sparql_term(t),
            shape,
        }
    }
}

/// Build the explorable model of a shapes graph.
pub fn build_shapes(g: &Graph) -> ShapesModel {
    let mut b = Builder {
        g,
        index: HashMap::new(),
        nodes: Vec::new(),
        custom_params: HashMap::new(),
    };

    // Custom SPARQL-based constraint components and their parameters.
    let sh_cc = NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#ConstraintComponent");
    let mut components = Vec::new();
    for c in g.subjects_for_predicate_object(rdf::TYPE, sh_cc) {
        let NamedOrBlankNodeRef::NamedNode(iri) = c else {
            continue;
        };
        let id = components.len();
        let params: Vec<String> = b
            .sh_all(c, "parameter")
            .into_iter()
            .filter_map(as_subject)
            .filter_map(|p| match b.sh(p, "path") {
                Some(TermRef::NamedNode(n)) => Some(n.as_str().to_string()),
                _ => None,
            })
            .collect();
        for p in &params {
            b.custom_params.insert(p.clone(), id);
        }
        let mut validators = Vec::new();
        for role in ["validator", "nodeValidator", "propertyValidator"] {
            for v in b.sh_all(c, role).into_iter().filter_map(as_subject) {
                let (kind, query) = match (b.sh(v, "select"), b.sh(v, "ask")) {
                    (Some(q), _) => ("select", literal_value(q)),
                    (None, Some(q)) => ("ask", literal_value(q)),
                    _ => continue,
                };
                let Some(query) = query else { continue };
                validators.push(ValidatorDto {
                    role: role.to_string(),
                    kind,
                    query,
                    prefixes: b.prefixes(v),
                    messages: b.literals(v, sh::MESSAGE),
                });
            }
        }
        let label = b
            .literals(c, rdfs::LABEL)
            .into_iter()
            .next()
            .unwrap_or_else(|| format!("<{}>", iri.as_str()));
        components.push(ComponentDto {
            id,
            iri: iri.as_str().to_string(),
            label,
            params,
            validators,
        });
    }

    // Seed with every node that is evidently a shape.
    let mut seeds: Vec<NamedOrBlankNodeRef<'_>> = Vec::new();
    for ty in ["NodeShape", "PropertyShape"] {
        let t = format!("{SH}{ty}");
        seeds.extend(g.subjects_for_predicate_object(rdf::TYPE, NamedNodeRef::new_unchecked(&t)));
    }
    for p in [
        "targetClass",
        "targetNode",
        "targetSubjectsOf",
        "targetObjectsOf",
        "target",
        "path",
        "property",
    ] {
        let pred = format!("{SH}{p}");
        seeds.extend(
            g.triples_for_predicate(NamedNodeRef::new_unchecked(&pred))
                .map(|t| t.subject),
        );
    }
    // Parameter declarations of custom components also carry sh:path.
    let sh_parameter = format!("{SH}parameter");
    let declarations: HashSet<_> = g
        .triples_for_predicate(NamedNodeRef::new_unchecked(&sh_parameter))
        .filter_map(|t| as_subject(t.object))
        .collect();
    seeds.retain(|n| !declarations.contains(n));
    seeds.sort_by_key(|n| key_of(*n));
    seeds.dedup();
    for s in seeds {
        b.intern(s);
    }

    // Build shapes; interning nested shapes may grow `nodes` while we go.
    let mut shapes: Vec<ShapeDto> = Vec::new();
    let mut i = 0;
    while i < b.nodes.len() {
        let n = b.nodes[i];
        let path_term = b.sh(n, "path");
        let path = path_term.map(|p| render_path(g, p, 0));
        let path_iri = match path_term {
            Some(TermRef::NamedNode(p)) => Some(p.as_str().to_string()),
            _ => None,
        };

        let mut targets = Vec::new();
        for (kind, local_name) in [
            ("class", "targetClass"),
            ("node", "targetNode"),
            ("subjectsOf", "targetSubjectsOf"),
            ("objectsOf", "targetObjectsOf"),
        ] {
            for t in b.sh_all(n, local_name) {
                targets.push(TargetDto {
                    kind,
                    value: render_value(g, t),
                    prefixes: Vec::new(),
                });
            }
        }
        let is_class = g.contains(oxrdf::TripleRef::new(n, rdf::TYPE, rdfs::CLASS))
            || g.contains(oxrdf::TripleRef::new(
                n,
                rdf::TYPE,
                NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#Class"),
            ));
        if is_class && let NamedOrBlankNodeRef::NamedNode(c) = n {
            targets.push(TargetDto {
                kind: "implicitClass",
                value: format!("<{}>", c.as_str()),
                prefixes: Vec::new(),
            });
        }
        for t in b.sh_all(n, "target").into_iter().filter_map(as_subject) {
            if let Some(q) = b.sh(t, "select").and_then(literal_value) {
                targets.push(TargetDto {
                    kind: "sparql",
                    value: q,
                    prefixes: b.prefixes(t),
                });
            }
        }

        // Constraint parameters, grouped by predicate.
        let mut by_param: BTreeMap<String, Vec<TermRef<'_>>> = BTreeMap::new();
        for t in g.triples_for_subject(n) {
            let p = t.predicate.as_str();
            let is_core = local(p).is_some_and(|l| !STRUCTURAL.contains(&l));
            if is_core || b.custom_params.contains_key(p) {
                by_param.entry(p.to_string()).or_default().push(t.object);
            }
        }
        let mut constraints = Vec::new();
        for (param, objs) in by_param {
            let l = local(&param).unwrap_or("");
            let is_shape = SHAPE_PARAMS.contains(&l);
            let is_list = LIST_PARAMS.contains(&l);
            let mut values = Vec::new();
            let mut list = false;
            for o in objs {
                match (is_list, read_list(g, o)) {
                    (true, Some(items)) => {
                        list = true;
                        for it in items {
                            values.push(b.value(it, is_shape));
                        }
                    }
                    _ => values.push(b.value(o, is_shape)),
                }
            }
            constraints.push(ConstraintDto {
                component: b.custom_params.get(&param).copied(),
                param,
                values,
                list,
            });
        }

        let properties: Vec<usize> = b
            .sh_all(n, "property")
            .into_iter()
            .filter_map(as_subject)
            .map(|p| b.intern(p))
            .collect();

        let severity = match b.sh(n, "severity") {
            Some(TermRef::NamedNode(s)) => Some(match local(s.as_str()) {
                Some(l @ ("Violation" | "Warning" | "Info")) => l.to_string(),
                _ => s.as_str().to_string(),
            }),
            _ => None,
        };
        let name = b
            .literals(n, sh::NAME)
            .into_iter()
            .next()
            .or_else(|| b.literals(n, rdfs::LABEL).into_iter().next());
        let order = b
            .sh(n, "order")
            .and_then(literal_value)
            .and_then(|v| v.parse().ok());

        shapes.push(ShapeDto {
            id: i,
            key: key_of(n),
            iri: match n {
                NamedOrBlankNodeRef::NamedNode(x) => Some(x.as_str().to_string()),
                _ => None,
            },
            kind: if path_term.is_some() {
                "property"
            } else {
                "node"
            },
            label: String::new(),
            name,
            description: b
                .literals(
                    n,
                    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#description"),
                )
                .into_iter()
                .next()
                .or_else(|| b.literals(n, rdfs::COMMENT).into_iter().next()),
            path,
            path_iri,
            targets,
            severity,
            deactivated: b.is_true(n, "deactivated"),
            messages: b.literals(n, sh::MESSAGE),
            constraints,
            properties,
            parents: Vec::new(),
            sparql: b.sparql_of(n),
            order,
        });
        i += 1;
    }

    // Parent links (sh:property and shape-valued parameters).
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); shapes.len()];
    for s in &shapes {
        let refs = s.properties.iter().copied().chain(
            s.constraints
                .iter()
                .flat_map(|c| c.values.iter().filter_map(|v| v.shape)),
        );
        for r in refs {
            if !parents[r].contains(&s.id) {
                parents[r].push(s.id);
            }
        }
    }
    for (s, p) in shapes.iter_mut().zip(parents) {
        s.parents = p;
    }

    // Labels: own name, else IRI, else "Parent › path".
    let labels: Vec<String> = shapes.iter().map(own_label).collect();
    for s in shapes.iter_mut() {
        let label = match (&s.name, &s.iri, &s.path, s.parents.first()) {
            (Some(n), _, _, _) => n.clone(),
            (None, Some(iri), _, _) => format!("<{iri}>"),
            (None, None, Some(path), Some(p)) => format!("{} › {path}", labels[*p]),
            (None, None, Some(path), None) => format!("(property shape) › {path}"),
            _ => "(anonymous shape)".to_string(),
        };
        s.label = label;
    }

    // Order property shapes by sh:order, then path.
    let order_of: Vec<(Option<f64>, String)> = shapes
        .iter()
        .map(|s| (s.order, s.path.clone().unwrap_or_default()))
        .collect();
    for s in shapes.iter_mut() {
        s.properties.sort_by(|a, b| {
            let (oa, pa) = &order_of[*a];
            let (ob, pb) = &order_of[*b];
            oa.unwrap_or(f64::MAX)
                .partial_cmp(&ob.unwrap_or(f64::MAX))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| pa.cmp(pb))
        });
    }

    let mut roots: Vec<usize> = shapes
        .iter()
        .filter(|s| s.parents.is_empty() || (s.iri.is_some() && s.kind == "node"))
        .map(|s| s.id)
        .collect();
    roots.sort_by(|a, b| {
        shapes[*a]
            .label
            .to_lowercase()
            .cmp(&shapes[*b].label.to_lowercase())
    });

    ShapesModel {
        shapes,
        roots,
        components,
    }
}

fn own_label(s: &ShapeDto) -> String {
    match (&s.name, &s.iri) {
        (Some(n), _) => n.clone(),
        (None, Some(iri)) => format!("<{iri}>"),
        _ => "(anonymous shape)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(ttl: &str) -> Graph {
        let mut g = Graph::new();
        for t in oxttl::TurtleParser::new().for_reader(ttl.as_bytes()) {
            g.insert(&t.unwrap());
        }
        g
    }

    #[test]
    fn models_property_shapes_and_lists() {
        let g = parse(
            r#"@prefix sh: <http://www.w3.org/ns/shacl#> .
               @prefix ex: <http://example.org/> .
               ex:S a sh:NodeShape ; sh:targetClass ex:C ;
                 sh:property [ sh:path ex:p ; sh:minCount 1 ; sh:in ( "a" "b" ) ] ;
                 sh:or ( [ sh:class ex:A ] [ sh:class ex:B ] ) ."#,
        );
        let m = build_shapes(&g);
        let s = m
            .shapes
            .iter()
            .find(|s| s.iri.as_deref() == Some("http://example.org/S"))
            .unwrap();
        assert_eq!(s.targets[0].kind, "class");
        assert_eq!(s.properties.len(), 1);
        let p = &m.shapes[s.properties[0]];
        assert_eq!(p.kind, "property");
        assert_eq!(p.label, "<http://example.org/S> › <http://example.org/p>");
        let in_c = p
            .constraints
            .iter()
            .find(|c| c.param.ends_with("#in"))
            .unwrap();
        assert!(in_c.list);
        assert_eq!(in_c.values.len(), 2);
        assert_eq!(in_c.values[0].sparql, "\"a\"");
        let or_c = s
            .constraints
            .iter()
            .find(|c| c.param.ends_with("#or"))
            .unwrap();
        assert!(or_c.values.iter().all(|v| v.shape.is_some()));
        assert_eq!(m.roots, vec![s.id]);
    }
}
