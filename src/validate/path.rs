//! SHACL property paths: parsing from the shapes graph, evaluation against the
//! data graph and rendering as a SPARQL property path.

use super::vocab::sh;
use super::{as_subject, list, object};
use anyhow::{Result, bail};
use oxrdf::vocab::rdf;
use oxrdf::{Graph, NamedNode, Term, TermRef};
use std::collections::HashSet;

/// One data triple a path walks over, as `subject predicate object`.
pub struct Step<'t> {
    pub subject: &'t Term,
    pub predicate: &'t NamedNode,
    pub object: &'t Term,
    /// Walked from object to subject (an inverse path).
    pub inverse: bool,
}

struct Ctx<'g, 't> {
    g: &'g Graph,
    trace: &'t mut dyn FnMut(Step<'_>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Path {
    Predicate(NamedNode),
    Inverse(Box<Path>),
    Sequence(Vec<Path>),
    Alternative(Vec<Path>),
    ZeroOrMore(Box<Path>),
    OneOrMore(Box<Path>),
    ZeroOrOne(Box<Path>),
}

impl Path {
    /// Parse the path that starts at `node` in the shapes graph.
    pub fn parse(g: &Graph, node: &Term) -> Result<Path> {
        Self::parse_depth(g, node, 0)
    }

    fn parse_depth(g: &Graph, node: &Term, depth: usize) -> Result<Path> {
        if depth > 64 {
            bail!("property path is nested too deeply");
        }
        let next = |t: &Term| Self::parse_depth(g, t, depth + 1);
        match node {
            Term::NamedNode(n) if n.as_ref() != rdf::NIL => return Ok(Path::Predicate(n.clone())),
            Term::BlankNode(_) | Term::NamedNode(_) => {}
            _ => bail!("{node} is not a valid property path"),
        }
        if object(g, node, rdf::FIRST).is_some() {
            let items = list(g, node);
            if items.len() < 2 {
                bail!("a sequence path needs at least two members");
            }
            return Ok(Path::Sequence(
                items.iter().map(next).collect::<Result<_>>()?,
            ));
        }
        if let Some(p) = object(g, node, sh::INVERSE_PATH) {
            return Ok(Path::Inverse(Box::new(next(&p)?)));
        }
        if let Some(p) = object(g, node, sh::ALTERNATIVE_PATH) {
            let items = list(g, &p);
            if items.len() < 2 {
                bail!("an alternative path needs at least two members");
            }
            return Ok(Path::Alternative(
                items.iter().map(next).collect::<Result<_>>()?,
            ));
        }
        if let Some(p) = object(g, node, sh::ZERO_OR_MORE_PATH) {
            return Ok(Path::ZeroOrMore(Box::new(next(&p)?)));
        }
        if let Some(p) = object(g, node, sh::ONE_OR_MORE_PATH) {
            return Ok(Path::OneOrMore(Box::new(next(&p)?)));
        }
        if let Some(p) = object(g, node, sh::ZERO_OR_ONE_PATH) {
            return Ok(Path::ZeroOrOne(Box::new(next(&p)?)));
        }
        bail!("{node} is not a valid property path")
    }

    /// The simple predicate of this path, if it is one.
    pub fn predicate(&self) -> Option<&NamedNode> {
        match self {
            Path::Predicate(p) => Some(p),
            _ => None,
        }
    }

    /// True when `predicate` occurs anywhere in the path.
    pub fn mentions(&self, predicate: oxrdf::NamedNodeRef<'_>) -> bool {
        match self {
            Path::Predicate(p) => p.as_ref() == predicate,
            Path::Inverse(p) | Path::ZeroOrMore(p) | Path::OneOrMore(p) | Path::ZeroOrOne(p) => {
                p.mentions(predicate)
            }
            Path::Sequence(items) | Path::Alternative(items) => {
                items.iter().any(|p| p.mentions(predicate))
            }
        }
    }

    /// The value nodes reachable from `node`, without duplicates, in order.
    pub fn eval(&self, g: &Graph, node: &Term) -> Vec<Term> {
        self.eval_traced(g, node, &mut |_| {})
    }

    /// [`Path::eval`], reporting every data triple the path walks over,
    /// intermediate steps of sequences and closures included.
    pub fn eval_traced(
        &self,
        g: &Graph,
        node: &Term,
        trace: &mut dyn FnMut(Step<'_>),
    ) -> Vec<Term> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut ctx = Ctx { g, trace };
        self.step(&mut ctx, node, false, &mut |t| {
            if seen.insert(t.clone()) {
                out.push(t);
            }
        });
        out
    }

    fn step(&self, ctx: &mut Ctx<'_, '_>, node: &Term, inverse: bool, emit: &mut dyn FnMut(Term)) {
        let g = ctx.g;
        match self {
            Path::Predicate(p) => {
                if inverse {
                    for s in g.subjects_for_predicate_object(p, node) {
                        let s: Term = s.into_owned().into();
                        (ctx.trace)(Step {
                            subject: &s,
                            predicate: p,
                            object: node,
                            inverse,
                        });
                        emit(s);
                    }
                } else if let Some(s) = as_subject(node) {
                    for o in g.objects_for_subject_predicate(s, p) {
                        let o = o.into_owned();
                        (ctx.trace)(Step {
                            subject: node,
                            predicate: p,
                            object: &o,
                            inverse,
                        });
                        emit(o);
                    }
                }
            }
            Path::Inverse(p) => p.step(ctx, node, !inverse, emit),
            Path::Sequence(items) => {
                let mut current = vec![node.clone()];
                let ordered: Box<dyn Iterator<Item = &Path>> = if inverse {
                    Box::new(items.iter().rev())
                } else {
                    Box::new(items.iter())
                };
                for p in ordered {
                    let mut next = Vec::new();
                    let mut seen = HashSet::new();
                    for n in &current {
                        p.step(ctx, n, inverse, &mut |t| {
                            if seen.insert(t.clone()) {
                                next.push(t);
                            }
                        });
                    }
                    current = next;
                }
                current.into_iter().for_each(emit);
            }
            Path::Alternative(items) => {
                for p in items {
                    p.step(ctx, node, inverse, emit);
                }
            }
            Path::ZeroOrMore(p) => Self::closure(p, ctx, node, inverse, true, emit),
            Path::OneOrMore(p) => Self::closure(p, ctx, node, inverse, false, emit),
            Path::ZeroOrOne(p) => {
                emit(node.clone());
                p.step(ctx, node, inverse, emit);
            }
        }
    }

    fn closure(
        p: &Path,
        ctx: &mut Ctx<'_, '_>,
        node: &Term,
        inverse: bool,
        reflexive: bool,
        emit: &mut dyn FnMut(Term),
    ) {
        let mut seen: HashSet<Term> = HashSet::new();
        let mut queue = Vec::new();
        if reflexive {
            seen.insert(node.clone());
            emit(node.clone());
        }
        p.step(ctx, node, inverse, &mut |t| queue.push(t));
        while let Some(t) = queue.pop() {
            if !seen.insert(t.clone()) {
                continue;
            }
            emit(t.clone());
            p.step(ctx, &t, inverse, &mut |n| queue.push(n));
        }
    }

    /// The path in SPARQL property path syntax, used for `$PATH`.
    pub fn to_sparql(&self) -> String {
        let join = |items: &[Path], sep: &str| {
            items
                .iter()
                .map(Path::to_sparql)
                .collect::<Vec<_>>()
                .join(sep)
        };
        match self {
            Path::Predicate(p) => format!("<{}>", p.as_str()),
            Path::Inverse(p) => format!("^({})", p.to_sparql()),
            Path::Sequence(items) => format!("({})", join(items, "/")),
            Path::Alternative(items) => format!("({})", join(items, "|")),
            Path::ZeroOrMore(p) => format!("({})*", p.to_sparql()),
            Path::OneOrMore(p) => format!("({})+", p.to_sparql()),
            Path::ZeroOrOne(p) => format!("({})?", p.to_sparql()),
        }
    }
}

/// Copy the RDF structure of a path from `from` into `to` (blank nodes keep
/// their identity), so a report can carry complex result paths.
pub fn copy_path(from: &Graph, node: &Term, to: &mut Graph) {
    let mut todo = vec![node.clone()];
    let mut seen = HashSet::new();
    while let Some(n) = todo.pop() {
        if !matches!(n, Term::BlankNode(_)) || !seen.insert(n.clone()) {
            continue;
        }
        let Some(s) = as_subject(&n) else { continue };
        for t in from.triples_for_subject(s) {
            if let TermRef::BlankNode(_) = t.object {
                todo.push(t.object.into_owned());
            }
            to.insert(t);
        }
    }
}
