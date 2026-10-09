use crate::cli::{InputFormat, detect_format};
use anyhow::{Context, Result};
use flate2::read::MultiGzDecoder;
use oxrdf::{Graph, Triple};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// Open `path` for reading, transparently decompressing if it ends with `.gz`.
pub fn open_reader(path: &Path) -> Result<Box<dyn BufRead>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let is_gz = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase().ends_with(".gz"))
        .unwrap_or(false);

    let raw: Box<dyn Read> = if is_gz {
        Box::new(MultiGzDecoder::new(file))
    } else {
        Box::new(file)
    };
    Ok(Box::new(BufReader::new(raw)))
}

/// Parse `reader` and call `on_triple` for every statement. Quad formats are
/// flattened: the graph component is dropped so all graphs are merged.
/// Returns the prefix declarations seen in the source (Turtle and TriG only).
pub fn parse_triples<R: BufRead, F: FnMut(Triple)>(
    reader: R,
    format: InputFormat,
    mut on_triple: F,
) -> Result<Vec<(String, String)>> {
    let mut prefixes = Vec::new();
    match format {
        InputFormat::Nt => {
            for t in oxttl::NTriplesParser::new().for_reader(reader) {
                on_triple(t.context("N-Triples parse error")?);
            }
        }
        InputFormat::Ttl => {
            let mut parser = oxttl::TurtleParser::new().for_reader(reader);
            for t in parser.by_ref() {
                on_triple(t.context("Turtle parse error")?);
            }
            prefixes.extend(
                parser
                    .prefixes()
                    .map(|(k, v)| (k.to_string(), v.to_string())),
            );
        }
        InputFormat::Rdf => {
            for t in oxrdfxml::RdfXmlParser::new().for_reader(reader) {
                on_triple(t.context("RDF/XML parse error")?);
            }
        }
        InputFormat::Trig => {
            let mut parser = oxttl::TriGParser::new().for_reader(reader);
            for q in parser.by_ref() {
                let q = q.context("TriG parse error")?;
                on_triple(Triple::new(q.subject, q.predicate, q.object));
            }
            prefixes.extend(
                parser
                    .prefixes()
                    .map(|(k, v)| (k.to_string(), v.to_string())),
            );
        }
        InputFormat::Nq => {
            for q in oxttl::NQuadsParser::new().for_reader(reader) {
                let q = q.context("N-Quads parse error")?;
                on_triple(Triple::new(q.subject, q.predicate, q.object));
            }
        }
    }
    Ok(prefixes)
}

/// A file supplied by content rather than by path (e.g. uploaded in the viewer).
#[derive(Debug, Clone)]
pub struct MemoryFile {
    /// File name; used for format detection (including a trailing `.gz`).
    pub name: String,
    pub content: Vec<u8>,
}

/// Parse in-memory files into `graph`, like [`load_graph`] does for paths.
pub fn load_graph_from_memory(
    files: &[MemoryFile],
    format: Option<InputFormat>,
    graph: &mut Graph,
    prefixes: &mut Vec<(String, String)>,
) -> Result<()> {
    for file in files {
        let fmt = match format {
            Some(f) => f,
            None => detect_format(Path::new(&file.name))?,
        };
        let raw: Box<dyn Read + '_> = if file.name.to_ascii_lowercase().ends_with(".gz") {
            Box::new(MultiGzDecoder::new(file.content.as_slice()))
        } else {
            Box::new(file.content.as_slice())
        };
        let found = parse_triples(BufReader::new(raw), fmt, |t| {
            graph.insert(&t);
        })
        .with_context(|| format!("failed to parse {}", file.name))?;
        merge_prefixes(prefixes, found);
    }
    Ok(())
}

fn merge_prefixes(prefixes: &mut Vec<(String, String)>, found: Vec<(String, String)>) {
    for (name, iri) in found {
        if !prefixes.iter().any(|(n, _)| *n == name) {
            prefixes.push((name, iri));
        }
    }
}

/// Load one or more files into a single in-memory graph.
///
/// `format` applies to every file; when `None` it is detected per file.
/// Prefixes from all files are collected (first declaration of a name wins).
pub fn load_graph<P: AsRef<Path>>(
    paths: &[P],
    format: Option<InputFormat>,
    graph: &mut Graph,
    prefixes: &mut Vec<(String, String)>,
) -> Result<()> {
    for path in paths {
        let path = path.as_ref();
        let fmt = match format {
            Some(f) => f,
            None => detect_format(path)?,
        };
        let reader = open_reader(path)?;
        let found = parse_triples(reader, fmt, |t| {
            graph.insert(&t);
        })
        .with_context(|| format!("failed to parse {}", path.display()))?;
        merge_prefixes(prefixes, found);
    }
    Ok(())
}
