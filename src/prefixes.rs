//! Prefixes used to shorten IRIs: the ones the input files declare, SHACL
//! `sh:declare` declarations, and well-known prefixes from prefix.cc.

use oxrdf::{Graph, NamedNodeRef, TermRef};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Prefixes declared the SHACL way, anywhere in `g`:
/// `[ sh:prefix "ex" ; sh:namespace "http://example.org/"^^xsd:anyURI ]`.
pub fn declared(g: &Graph) -> Vec<(String, String)> {
    let prefix = NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#prefix");
    let namespace = NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#namespace");
    let mut out = Vec::new();
    for t in g.triples_for_predicate(prefix) {
        let TermRef::Literal(p) = t.object else {
            continue;
        };
        let ns = match g.object_for_subject_predicate(t.subject, namespace) {
            Some(TermRef::Literal(l)) => l.value().to_string(),
            Some(TermRef::NamedNode(n)) => n.as_str().to_string(),
            _ => continue,
        };
        out.push((p.value().to_string(), ns));
    }
    out.sort();
    out
}

/// Add `found` to `prefixes`, skipping names that are already taken.
pub fn merge(prefixes: &mut Vec<(String, String)>, found: Vec<(String, String)>) {
    for (name, ns) in found {
        if !prefixes.iter().any(|(n, _)| *n == name) {
            prefixes.push((name, ns));
        }
    }
}

/// prefix.cc's popular prefixes (fetched at build time), as namespace → prefix.
/// Where several prefixes share a namespace, the most popular one wins.
fn well_known() -> &'static HashMap<String, String> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| {
        let json = include_str!(concat!(env!("OUT_DIR"), "/prefixes.json"));
        let mut map = HashMap::new();
        for (prefix, ns) in ordered_pairs(json) {
            map.entry(ns).or_insert(prefix);
        }
        map
    })
}

/// The `"prefix": "namespace"` pairs of a JSON object, in file order
/// (prefix.cc lists the most popular first).
fn ordered_pairs(json: &str) -> Vec<(String, String)> {
    struct Pairs(Vec<(String, String)>);
    impl<'de> serde::Deserialize<'de> for Pairs {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> serde::de::Visitor<'de> for V {
                type Value = Pairs;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("an object of prefix to namespace strings")
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut a: A,
                ) -> Result<Pairs, A::Error> {
                    let mut out = Vec::new();
                    while let Some(pair) = a.next_entry::<String, String>()? {
                        out.push(pair);
                    }
                    Ok(Pairs(out))
                }
            }
            d.deserialize_map(V)
        }
    }
    serde_json::from_str::<Pairs>(json).map_or_else(|_| Vec::new(), |p| p.0)
}

/// The namespace part of an IRI: everything up to its last `#` or `/`.
fn namespace_of(iri: &str) -> Option<&str> {
    let end = iri.rfind(['#', '/'])? + 1;
    (end < iri.len()).then(|| &iri[..end])
}

/// Add a well-known prefix for every namespace used in `graphs` that no
/// declared prefix covers yet. Only namespaces that occur are added, so the
/// viewer and generated queries do not carry thousands of unused prefixes.
pub fn add_well_known(prefixes: &mut Vec<(String, String)>, graphs: &[&Graph]) {
    let known = well_known();
    let mut used = std::collections::BTreeSet::new();
    for g in graphs {
        for t in g.iter() {
            let s = match t.subject {
                oxrdf::NamedOrBlankNodeRef::NamedNode(n) => Some(n.as_str()),
                _ => None,
            };
            let o = match t.object {
                TermRef::NamedNode(n) => Some(n.as_str()),
                TermRef::Literal(l) => Some(l.datatype().as_str()),
                _ => None,
            };
            for iri in [s, Some(t.predicate.as_str()), o].into_iter().flatten() {
                if let Some(ns) = namespace_of(iri) {
                    used.insert(ns);
                }
            }
        }
    }
    for ns in used {
        if prefixes.iter().any(|(_, n)| n == ns) {
            continue;
        }
        if let Some(p) = known.get(ns)
            && !prefixes.iter().any(|(n, _)| n == p)
        {
            prefixes.push((p.clone(), ns.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_shacl_declarations() {
        let ttl = r#"
            @prefix sh: <http://www.w3.org/ns/shacl#> .
            @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
            <http://example.org/> sh:declare
                [ sh:prefix "era" ; sh:namespace "http://data.europa.eu/949/"^^xsd:anyURI ],
                [ sh:prefix "dim" ; sh:namespace <http://example.org/dim#> ] .
        "#;
        let mut g = Graph::new();
        for t in oxttl::TurtleParser::new().for_slice(ttl.as_bytes()) {
            g.insert(&t.unwrap());
        }
        assert_eq!(
            declared(&g),
            vec![
                ("dim".into(), "http://example.org/dim#".into()),
                ("era".into(), "http://data.europa.eu/949/".into()),
            ]
        );
    }

    #[test]
    fn adds_only_used_well_known_namespaces() {
        let ttl = r#"<http://example.org/a> <http://xmlns.com/foaf/0.1/name> "A" ."#;
        let mut g = Graph::new();
        for t in oxttl::TurtleParser::new().for_slice(ttl.as_bytes()) {
            g.insert(&t.unwrap());
        }
        let mut p = vec![("ex".to_string(), "http://example.org/".to_string())];
        add_well_known(&mut p, &[&g]);
        assert!(p.contains(&("foaf".into(), "http://xmlns.com/foaf/0.1/".into())));
        assert!(!p.iter().any(|(n, _)| n == "dbo"));
        // A declared prefix for the namespace wins over prefix.cc.
        assert_eq!(
            p.iter().filter(|(_, n)| n == "http://example.org/").count(),
            1
        );
    }
}
