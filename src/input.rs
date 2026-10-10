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

/// Caps on what one load may cost. The public web server sets them so that
/// a single upload cannot exhaust memory or CPU; everything else is unlimited.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoadLimits {
    /// Most bytes read per file after decompression (guards against gzip bombs).
    pub max_bytes: Option<u64>,
    /// Most triples across all graphs of one load.
    pub max_triples: Option<usize>,
    /// Validation stops with an error after this moment.
    pub deadline: Option<std::time::Instant>,
}

/// A reader that fails, instead of silently truncating, once `left` runs out.
struct Capped<R> {
    inner: R,
    left: u64,
    name: String,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.left = self.left.checked_sub(n as u64).ok_or_else(|| {
            std::io::Error::other(format!("{} is larger than this server accepts", self.name))
        })?;
        Ok(n)
    }
}

/// Parse in-memory files into `graph`, like [`load_graph`] does for paths.
pub fn load_graph_from_memory(
    files: &[MemoryFile],
    format: Option<InputFormat>,
    graph: &mut Graph,
    prefixes: &mut Vec<(String, String)>,
) -> Result<()> {
    load_graph_from_memory_limited(
        files,
        format,
        graph,
        prefixes,
        &LoadLimits::default(),
        &mut 0,
    )
}

/// [`load_graph_from_memory`] under `limits`; `triples` counts the triples
/// read so far across calls, for [`LoadLimits::max_triples`].
pub fn load_graph_from_memory_limited(
    files: &[MemoryFile],
    format: Option<InputFormat>,
    graph: &mut Graph,
    prefixes: &mut Vec<(String, String)>,
    limits: &LoadLimits,
    triples: &mut usize,
) -> Result<()> {
    for file in files {
        let fmt = match format {
            Some(f) => f,
            None => detect_format(Path::new(&file.name))?,
        };
        let mut raw: Box<dyn Read + '_> = if file.name.to_ascii_lowercase().ends_with(".gz") {
            Box::new(MultiGzDecoder::new(file.content.as_slice()))
        } else {
            Box::new(file.content.as_slice())
        };
        if let Some(max) = limits.max_bytes {
            raw = Box::new(Capped {
                inner: raw,
                left: max,
                name: file.name.clone(),
            });
        }
        let mut too_many = false;
        let found = parse_triples(BufReader::new(raw), fmt, |t| {
            if limits.max_triples.is_some_and(|max| *triples >= max) {
                too_many = true;
                return;
            }
            *triples += 1;
            graph.insert(&t);
        })
        .with_context(|| format!("failed to parse {}", file.name))?;
        if too_many {
            anyhow::bail!(
                "the files hold more than {} triples, the most this server accepts",
                limits.max_triples.unwrap_or_default()
            );
        }
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
        if let Some(url) = path.to_str().filter(|s| crate::http::is_url(s)) {
            let file = crate::http::fetch_rdf(url)?;
            load_graph_from_memory(std::slice::from_ref(&file), format, graph, prefixes)?;
            continue;
        }
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
