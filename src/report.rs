//! Parse a SHACL validation report (any validator) into a flat, serialisable
//! model, optionally enriched with the shapes graph and the data graph.
//!
//! Text fields that mention RDF terms (paths, shape labels, constraints) use
//! `<iri>` tokens so the front end can shorten them with the known prefixes.

use crate::cli::InputFormat;
use crate::input::load_graph;
use anyhow::{Result, bail};
use oxrdf::vocab::{rdf, rdfs};
use oxrdf::{Graph, NamedNodeRef, NamedOrBlankNodeRef, TermRef};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

const SH: &str = "http://www.w3.org/ns/shacl#";

mod sh {
    use oxrdf::NamedNodeRef;
    macro_rules! sh_term {
        ($name:ident, $local:literal) => {
            pub const $name: NamedNodeRef<'_> =
                NamedNodeRef::new_unchecked(concat!("http://www.w3.org/ns/shacl#", $local));
        };
    }
    sh_term!(VALIDATION_REPORT, "ValidationReport");
    sh_term!(VALIDATION_RESULT, "ValidationResult");
    sh_term!(CONFORMS, "conforms");
    sh_term!(RESULT, "result");
    sh_term!(FOCUS_NODE, "focusNode");
    sh_term!(RESULT_PATH, "resultPath");
    sh_term!(VALUE, "value");
    sh_term!(RESULT_SEVERITY, "resultSeverity");
    sh_term!(SOURCE_CONSTRAINT_COMPONENT, "sourceConstraintComponent");
    sh_term!(SOURCE_SHAPE, "sourceShape");
    sh_term!(RESULT_MESSAGE, "resultMessage");
    sh_term!(DETAIL, "detail");
    sh_term!(MESSAGE, "message");
    sh_term!(NAME, "name");
    sh_term!(PATH, "path");
    sh_term!(PROPERTY, "property");
    sh_term!(TARGET_CLASS, "targetClass");
    sh_term!(TARGET_NODE, "targetNode");
    sh_term!(INVERSE_PATH, "inversePath");
    sh_term!(ALTERNATIVE_PATH, "alternativePath");
    sh_term!(ZERO_OR_MORE_PATH, "zeroOrMorePath");
    sh_term!(ONE_OR_MORE_PATH, "oneOrMorePath");
    sh_term!(ZERO_OR_ONE_PATH, "zeroOrOnePath");
    sh_term!(FLAGS, "flags");
}

/// Maximum number of data-graph triples kept per focus node.
pub const MAX_FOCUS_TRIPLES: usize = 200;

/// Paths of the files a report was built from.
#[derive(Debug, Clone, Default)]
pub struct ReportInputs {
    pub report: PathBuf,
    pub data: Vec<PathBuf>,
    pub shapes: Vec<PathBuf>,
    pub format: Option<InputFormat>,
}

/// One RDF term, in the same shape the rdf-compare viewer uses.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TermDto {
    /// `iri`, `bnode` or `lit`.
    pub t: &'static str,
    pub v: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lng: Option<String>,
}

impl TermDto {
    fn from_term(t: TermRef<'_>) -> Self {
        match t {
            TermRef::NamedNode(n) => Self::iri(n.as_str()),
            TermRef::BlankNode(b) => TermDto {
                t: "bnode",
                v: b.as_str().to_string(),
                dt: None,
                lng: None,
            },
            TermRef::Literal(l) => {
                let lng = l.language().map(str::to_string);
                let dt = if lng.is_some() {
                    None
                } else {
                    Some(l.datatype().as_str().to_string())
                };
                TermDto {
                    t: "lit",
                    v: l.value().to_string(),
                    dt,
                    lng,
                }
            }
            #[allow(unreachable_patterns)]
            _ => TermDto {
                t: "lit",
                v: t.to_string(),
                dt: None,
                lng: None,
            },
        }
    }

    fn iri(v: &str) -> Self {
        TermDto {
            t: "iri",
            v: v.to_string(),
            dt: None,
            lng: None,
        }
    }

    /// Stable key used to look up focus-node context.
    pub fn key(&self) -> String {
        match self.t {
            "iri" => format!("<{}>", self.v),
            "bnode" => format!("_:{}", self.v),
            _ => format!("\"{}\"", self.v),
        }
    }
}

/// A predicate/object pair of a focus node in the data graph.
#[derive(Debug, Clone, Serialize)]
pub struct ContextTriple {
    pub p: String,
    pub o: TermDto,
}

/// One validation result, flattened for display.
#[derive(Debug, Clone, Serialize)]
pub struct ResultRow {
    pub id: usize,
    /// `Violation`, `Warning`, `Info`, or the IRI of a custom severity.
    pub severity: String,
    pub focus: TermDto,
    /// Result path in SPARQL property-path syntax with `<iri>` tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// IRI of the result path when it is a plain predicate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_iri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<TermDto>,
    pub messages: Vec<String>,
    /// IRI of the source constraint component.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<TermDto>,
    /// Human label for the source shape, with `<iri>` tokens.
    pub shape_label: String,
    /// The violated constraint, e.g. `<…#minCount> 1`, when the shapes graph
    /// is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraint: Option<String>,
    /// Number of nested `sh:detail` results.
    pub details: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    pub total: usize,
    pub violations: usize,
    pub warnings: usize,
    pub infos: usize,
    pub other: usize,
    pub focus_nodes: usize,
    pub shapes: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Sources {
    pub report: String,
    pub data: Vec<String>,
    pub shapes: Vec<String>,
}

/// The full model served to the viewer and embedded in HTML exports.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    /// Value of `sh:conforms`, if the report states it.
    pub conforms: Option<bool>,
    pub sources: Sources,
    pub prefixes: Vec<(String, String)>,
    pub stats: Stats,
    pub results: Vec<ResultRow>,
    /// Data-graph triples per focus node key (only when a data graph was given).
    pub focus: BTreeMap<String, Vec<ContextTriple>>,
    pub has_data: bool,
    pub has_shapes: bool,
}

impl Report {
    /// True when any result is at or above `threshold`
    /// (0 = Violation only, 1 = Warning, 2 = Info and custom severities).
    pub fn fails_at(&self, threshold: u8) -> bool {
        let s = &self.stats;
        s.violations > 0
            || (threshold >= 1 && s.warnings > 0)
            || (threshold >= 2 && (s.infos > 0 || s.other > 0))
    }
}

/// Load the report (plus optional data and shapes) and build the model.
pub fn load_report(inputs: &ReportInputs) -> Result<Report> {
    let mut prefixes = Vec::new();
    let mut report_graph = Graph::new();
    load_graph(
        std::slice::from_ref(&inputs.report),
        inputs.format,
        &mut report_graph,
        &mut prefixes,
    )?;
    let mut shapes_graph = Graph::new();
    load_graph(&inputs.shapes, None, &mut shapes_graph, &mut prefixes)?;
    let mut data_graph = Graph::new();
    load_graph(&inputs.data, None, &mut data_graph, &mut prefixes)?;

    let mut report = build_report(
        &report_graph,
        &shapes_graph,
        &data_graph,
        !inputs.data.is_empty(),
    )?;
    report.has_shapes = !inputs.shapes.is_empty();
    report.sources = Sources {
        report: inputs.report.display().to_string(),
        data: inputs
            .data
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        shapes: inputs
            .shapes
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
    };
    if !prefixes.iter().any(|(n, _)| n == "sh") {
        prefixes.push(("sh".to_string(), SH.to_string()));
    }
    report.prefixes = prefixes;
    Ok(report)
}

/// Build the model from already-parsed graphs. `shapes` and `data` may be empty.
pub fn build_report(
    report: &Graph,
    shapes: &Graph,
    data: &Graph,
    has_data: bool,
) -> Result<Report> {
    let lookup = Lookup { report, shapes };

    let report_nodes: Vec<NamedOrBlankNodeRef<'_>> = report
        .subjects_for_predicate_object(rdf::TYPE, sh::VALIDATION_REPORT)
        .collect();

    let conforms = report_nodes.iter().find_map(|n| {
        report
            .object_for_subject_predicate(*n, sh::CONFORMS)
            .and_then(|t| match t {
                TermRef::Literal(l) => Some(l.value() == "true" || l.value() == "1"),
                _ => None,
            })
    });

    // Top-level results hang off the report via sh:result. Fall back to every
    // typed sh:ValidationResult that is not nested in an sh:detail.
    let mut result_nodes: Vec<NamedOrBlankNodeRef<'_>> = Vec::new();
    let mut seen = HashSet::new();
    for n in &report_nodes {
        for r in report.objects_for_subject_predicate(*n, sh::RESULT) {
            if let Some(r) = as_subject(r)
                && seen.insert(r)
            {
                result_nodes.push(r);
            }
        }
    }
    if result_nodes.is_empty() {
        for r in report.subjects_for_predicate_object(rdf::TYPE, sh::VALIDATION_RESULT) {
            let nested = report
                .subjects_for_predicate_object(sh::DETAIL, r)
                .next()
                .is_some();
            if !nested && seen.insert(r) {
                result_nodes.push(r);
            }
        }
    }
    if report_nodes.is_empty() && result_nodes.is_empty() {
        bail!("no sh:ValidationReport or sh:ValidationResult found in the report file");
    }

    let mut results = Vec::with_capacity(result_nodes.len());
    for r in result_nodes {
        let Some(focus) = report.object_for_subject_predicate(r, sh::FOCUS_NODE) else {
            continue;
        };
        let focus_term = TermDto::from_term(focus);
        let path_term = report.object_for_subject_predicate(r, sh::RESULT_PATH);
        let path = path_term.map(|p| render_path(report, p, 0));
        let path_iri = match path_term {
            Some(TermRef::NamedNode(n)) => Some(n.as_str().to_string()),
            _ => None,
        };
        let severity = match report.object_for_subject_predicate(r, sh::RESULT_SEVERITY) {
            Some(TermRef::NamedNode(n)) => match n.as_str().strip_prefix(SH) {
                Some(local @ ("Violation" | "Warning" | "Info")) => local.to_string(),
                _ => n.as_str().to_string(),
            },
            _ => "Violation".to_string(),
        };
        let component =
            match report.object_for_subject_predicate(r, sh::SOURCE_CONSTRAINT_COMPONENT) {
                Some(TermRef::NamedNode(n)) => Some(n.as_str().to_string()),
                _ => None,
            };
        let mut messages: Vec<String> = report
            .objects_for_subject_predicate(r, sh::RESULT_MESSAGE)
            .filter_map(literal_value)
            .collect();
        messages.sort();
        messages.dedup();

        let source_shape = report.object_for_subject_predicate(r, sh::SOURCE_SHAPE);
        let resolved = source_shape
            .and_then(as_subject)
            .and_then(|s| lookup.resolve_shape(s, path_term, component.as_deref(), focus, data));
        if messages.is_empty()
            && let Some(s) = resolved
        {
            messages.extend(lookup.objects(s, sh::MESSAGE).filter_map(literal_value));
        }
        let shape_label = match resolved {
            Some(s) => lookup.shape_label(s),
            None => match source_shape {
                Some(TermRef::NamedNode(n)) => format!("<{}>", n.as_str()),
                _ => match &path {
                    Some(p) => format!("(property shape) › {p}"),
                    None => "(anonymous shape)".to_string(),
                },
            },
        };
        let constraint = match (resolved, component.as_deref()) {
            (Some(s), Some(c)) => lookup.constraint(s, c),
            _ => None,
        };
        let details = report.objects_for_subject_predicate(r, sh::DETAIL).count();

        results.push(ResultRow {
            id: 0,
            severity,
            focus: focus_term,
            path,
            path_iri,
            value: report
                .object_for_subject_predicate(r, sh::VALUE)
                .map(TermDto::from_term),
            messages,
            component,
            shape: source_shape.map(TermDto::from_term),
            shape_label,
            constraint,
            details,
        });
    }

    results.sort_by(|a, b| {
        severity_rank(&a.severity)
            .cmp(&severity_rank(&b.severity))
            .then_with(|| a.shape_label.cmp(&b.shape_label))
            .then_with(|| a.focus.v.cmp(&b.focus.v))
            .then_with(|| a.path.cmp(&b.path))
    });
    for (i, r) in results.iter_mut().enumerate() {
        r.id = i;
    }

    let mut stats = Stats {
        total: results.len(),
        ..Stats::default()
    };
    let mut focus_keys = HashSet::new();
    let mut shape_labels = HashSet::new();
    for r in &results {
        match r.severity.as_str() {
            "Violation" => stats.violations += 1,
            "Warning" => stats.warnings += 1,
            "Info" => stats.infos += 1,
            _ => stats.other += 1,
        }
        focus_keys.insert(r.focus.key());
        shape_labels.insert(r.shape_label.as_str());
    }
    stats.focus_nodes = focus_keys.len();
    stats.shapes = shape_labels.len();

    let mut focus = BTreeMap::new();
    if has_data {
        for r in &results {
            // Blank-node labels are file-local, so they cannot be matched
            // against a separately parsed data graph.
            if r.focus.t != "iri" || focus.contains_key(&r.focus.key()) {
                continue;
            }
            let node = NamedNodeRef::new_unchecked(&r.focus.v);
            let mut triples: Vec<ContextTriple> = data
                .triples_for_subject(node)
                .take(MAX_FOCUS_TRIPLES)
                .map(|t| ContextTriple {
                    p: t.predicate.as_str().to_string(),
                    o: TermDto::from_term(t.object),
                })
                .collect();
            triples.sort_by(|a, b| a.p.cmp(&b.p).then_with(|| a.o.v.cmp(&b.o.v)));
            focus.insert(r.focus.key(), triples);
        }
    }

    Ok(Report {
        conforms,
        sources: Sources::default(),
        prefixes: Vec::new(),
        stats,
        results,
        focus,
        has_data,
        has_shapes: !shapes.is_empty(),
    })
}

fn severity_rank(s: &str) -> u8 {
    match s {
        "Violation" => 0,
        "Warning" => 1,
        "Info" => 2,
        _ => 3,
    }
}

fn as_subject(t: TermRef<'_>) -> Option<NamedOrBlankNodeRef<'_>> {
    match t {
        TermRef::NamedNode(n) => Some(n.into()),
        TermRef::BlankNode(b) => Some(b.into()),
        _ => None,
    }
}

fn literal_value(t: TermRef<'_>) -> Option<String> {
    match t {
        TermRef::Literal(l) => Some(l.value().to_string()),
        _ => None,
    }
}

/// Read an RDF list into its members. Stops on malformed or cyclic lists.
fn read_list<'a>(g: &'a Graph, head: TermRef<'a>) -> Option<Vec<TermRef<'a>>> {
    let mut out = Vec::new();
    let mut cur = head;
    for _ in 0..10_000 {
        if cur == TermRef::NamedNode(rdf::NIL) {
            return Some(out);
        }
        let node = as_subject(cur)?;
        out.push(g.object_for_subject_predicate(node, rdf::FIRST)?);
        cur = g.object_for_subject_predicate(node, rdf::REST)?;
    }
    None
}

/// Render a SHACL property path as SPARQL property-path syntax.
pub fn render_path(g: &Graph, p: TermRef<'_>, depth: usize) -> String {
    render_path_inner(g, p, depth).0
}

/// Returns the rendered path and whether it needs parentheses when used as
/// the operand of another path operator.
fn render_path_inner(g: &Graph, p: TermRef<'_>, depth: usize) -> (String, bool) {
    if depth > 32 {
        return ("…".to_string(), false);
    }
    let node = match p {
        TermRef::NamedNode(n) => return (format!("<{}>", n.as_str()), false),
        TermRef::BlankNode(b) => NamedOrBlankNodeRef::from(b),
        _ => return (p.to_string(), false),
    };
    let operand = |t: TermRef<'_>| {
        let (s, complex) = render_path_inner(g, t, depth + 1);
        if complex { format!("({s})") } else { s }
    };
    let unary = |pred: NamedNodeRef<'_>| g.object_for_subject_predicate(node, pred);
    if let Some(inner) = unary(sh::INVERSE_PATH) {
        return (format!("^{}", operand(inner)), false);
    }
    if let Some(inner) = unary(sh::ZERO_OR_MORE_PATH) {
        return (format!("{}*", operand(inner)), false);
    }
    if let Some(inner) = unary(sh::ONE_OR_MORE_PATH) {
        return (format!("{}+", operand(inner)), false);
    }
    if let Some(inner) = unary(sh::ZERO_OR_ONE_PATH) {
        return (format!("{}?", operand(inner)), false);
    }
    if let Some(list) = unary(sh::ALTERNATIVE_PATH).and_then(|l| read_list(g, l)) {
        let parts: Vec<String> = list.into_iter().map(operand).collect();
        return (format!("({})", parts.join(" | ")), false);
    }
    if let Some(list) = read_list(g, p) {
        let parts: Vec<String> = list.into_iter().map(operand).collect();
        let complex = parts.len() > 1;
        return (parts.join(" / "), complex);
    }
    (node.to_string(), false)
}

/// Render a constraint parameter value with `<iri>` tokens.
fn render_value(g: &Graph, t: TermRef<'_>) -> String {
    match t {
        TermRef::NamedNode(n) => format!("<{}>", n.as_str()),
        TermRef::Literal(l) => {
            let numeric = l
                .datatype()
                .as_str()
                .starts_with("http://www.w3.org/2001/XMLSchema#")
                && matches!(
                    &l.datatype().as_str()[33..],
                    "integer" | "decimal" | "double" | "float" | "int" | "long" | "boolean"
                );
            if numeric {
                l.value().to_string()
            } else {
                format!("\"{}\"", l.value())
            }
        }
        TermRef::BlankNode(_) => match read_list(g, t) {
            Some(items) => {
                let parts: Vec<String> = items.into_iter().map(|i| render_value(g, i)).collect();
                format!("({})", parts.join(" "))
            }
            None => "[…]".to_string(),
        },
        #[allow(unreachable_patterns)]
        _ => t.to_string(),
    }
}

/// Reads shape information from the shapes graph, falling back to the report
/// graph (some validators copy the shapes into the report).
struct Lookup<'a> {
    report: &'a Graph,
    shapes: &'a Graph,
}

impl<'a> Lookup<'a> {
    fn objects(
        &self,
        s: NamedOrBlankNodeRef<'a>,
        p: NamedNodeRef<'a>,
    ) -> impl Iterator<Item = TermRef<'a>> + 'a {
        let primary: Vec<TermRef<'a>> = self.shapes.objects_for_subject_predicate(s, p).collect();
        let fallback: Vec<TermRef<'a>> = if primary.is_empty() {
            self.report.objects_for_subject_predicate(s, p).collect()
        } else {
            Vec::new()
        };
        primary.into_iter().chain(fallback)
    }

    fn object(&self, s: NamedOrBlankNodeRef<'a>, p: NamedNodeRef<'a>) -> Option<TermRef<'a>> {
        self.objects(s, p).next()
    }

    fn graph_of(&self, s: NamedOrBlankNodeRef<'_>) -> &'a Graph {
        if self.shapes.triples_for_subject(s).next().is_some() {
            self.shapes
        } else {
            self.report
        }
    }

    fn is_known(&self, s: NamedOrBlankNodeRef<'_>) -> bool {
        self.shapes.triples_for_subject(s).next().is_some()
            || self
                .report
                .object_for_subject_predicate(s, sh::PATH)
                .is_some()
            || self
                .report
                .object_for_subject_predicate(s, sh::PROPERTY)
                .is_some()
    }

    /// Find the shape a result came from. Named shapes resolve directly.
    /// Blank-node property shapes from a separately parsed shapes file are
    /// matched on path and constraint parameter, using the focus node's
    /// targets to break ties.
    fn resolve_shape(
        &self,
        source: NamedOrBlankNodeRef<'a>,
        result_path: Option<TermRef<'_>>,
        component: Option<&str>,
        focus: TermRef<'_>,
        data: &Graph,
    ) -> Option<NamedOrBlankNodeRef<'a>> {
        let in_shapes = self.shapes.triples_for_subject(source).next().is_some();
        if in_shapes || matches!(source, NamedOrBlankNodeRef::NamedNode(_)) {
            return Some(source);
        }
        let matched = result_path.and_then(|rp| {
            let wanted = render_path(self.report, rp, 0);
            let param = component.and_then(component_parameter);
            let mut candidates: Vec<NamedOrBlankNodeRef<'a>> = self
                .shapes
                .triples_for_predicate(sh::PATH)
                .filter(|t| render_path(self.shapes, t.object, 0) == wanted)
                .map(|t| t.subject)
                .filter(|s| match &param {
                    Some(p) => self
                        .shapes
                        .object_for_subject_predicate(*s, NamedNodeRef::new_unchecked(p))
                        .is_some(),
                    None => true,
                })
                .collect();
            if candidates.len() > 1 {
                let types: Vec<TermRef<'_>> = match as_subject(focus) {
                    Some(f) => data.objects_for_subject_predicate(f, rdf::TYPE).collect(),
                    None => Vec::new(),
                };
                candidates.retain(|c| {
                    self.shapes
                        .subjects_for_predicate_object(sh::PROPERTY, *c)
                        .any(|parent| {
                            self.shapes
                                .objects_for_subject_predicate(parent, sh::TARGET_CLASS)
                                .any(|tc| types.contains(&tc))
                                || self
                                    .shapes
                                    .objects_for_subject_predicate(parent, sh::TARGET_NODE)
                                    .any(|tn| tn == focus)
                        })
                });
            }
            (candidates.len() == 1).then(|| candidates[0])
        });
        matched.or_else(|| self.is_known(source).then_some(source))
    }

    fn name_of(&self, s: NamedOrBlankNodeRef<'a>) -> Option<String> {
        self.object(s, sh::NAME)
            .or_else(|| self.object(s, rdfs::LABEL))
            .and_then(literal_value)
    }

    fn shape_label(&self, s: NamedOrBlankNodeRef<'a>) -> String {
        if let Some(name) = self.name_of(s) {
            return name;
        }
        let own = match s {
            NamedOrBlankNodeRef::NamedNode(n) => Some(format!("<{}>", n.as_str())),
            _ => None,
        };
        let path = self
            .object(s, sh::PATH)
            .map(|p| render_path(self.graph_of(s), p, 0));
        if let Some(path) = path {
            let g = self.graph_of(s);
            let parent =
                g.subjects_for_predicate_object(sh::PROPERTY, s)
                    .next()
                    .map(|p| match self.name_of(p) {
                        Some(n) => n,
                        None => match p {
                            NamedOrBlankNodeRef::NamedNode(n) => format!("<{}>", n.as_str()),
                            _ => "(anonymous shape)".to_string(),
                        },
                    });
            return match (parent, own) {
                (Some(parent), _) => format!("{parent} › {path}"),
                (None, Some(own)) => own,
                (None, None) => format!("(property shape) › {path}"),
            };
        }
        own.unwrap_or_else(|| "(anonymous shape)".to_string())
    }

    fn constraint(&self, s: NamedOrBlankNodeRef<'a>, component: &str) -> Option<String> {
        let param = component_parameter(component)?;
        let pred = NamedNodeRef::new_unchecked(&param);
        let g = self.graph_of(s);
        let values: Vec<String> = self.objects(s, pred).map(|v| render_value(g, v)).collect();
        if values.is_empty() {
            return None;
        }
        let mut out = format!("<{param}> {}", values.join(", "));
        if param.ends_with("#pattern")
            && let Some(flags) = self.object(s, sh::FLAGS).and_then(literal_value)
        {
            out.push_str(&format!(" (flags \"{flags}\")"));
        }
        Some(out)
    }
}

/// Map a core constraint component IRI to its main parameter IRI, e.g.
/// `sh:MinCountConstraintComponent` → `sh:minCount`.
pub fn component_parameter(component: &str) -> Option<String> {
    let local = component
        .strip_prefix(SH)?
        .strip_suffix("ConstraintComponent")?;
    let mut chars = local.chars();
    let first = chars.next()?.to_ascii_lowercase();
    Some(format!("{SH}{first}{}", chars.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_component_to_parameter() {
        assert_eq!(
            component_parameter("http://www.w3.org/ns/shacl#MinCountConstraintComponent").unwrap(),
            "http://www.w3.org/ns/shacl#minCount"
        );
        assert_eq!(
            component_parameter("http://www.w3.org/ns/shacl#QualifiedMaxCountConstraintComponent")
                .unwrap(),
            "http://www.w3.org/ns/shacl#qualifiedMaxCount"
        );
        assert!(component_parameter("http://example.org/Custom").is_none());
    }
}
