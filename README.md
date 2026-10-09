# shacl-report

A small, fast CLI that turns a [SHACL](https://www.w3.org/TR/shacl/) validation
report into something people can actually read. It works with the report of
**any** validator (pyshacl, Apache Jena, TopBraid, rudof, RDF4J, …), because it
reads the standard `sh:ValidationReport` vocabulary rather than running
validation itself.

Validators produce reports as raw RDF. On a real data set that is hundreds of
anonymous `sh:ValidationResult` nodes pointing at blank-node property shapes.
`shacl-report` flattens them into a table you can group, filter and search,
names every shape, shows the constraint that was broken, and puts the focus
node's own triples next to each result.

Screenshot:
![image](screenshot-webapp.png)

## Features

- **Validator-agnostic.** Reads any RDF file containing an
  `sh:ValidationReport` (or bare `sh:ValidationResult` nodes).
- **Readable shapes.** Property shapes are shown as `Parent shape › path`,
  using `sh:name` / `rdfs:label` when present. Anonymous property shapes from a
  separate shapes file are matched back by path and constraint (pass `--shapes`).
- **The broken constraint.** With `--shapes`, each result shows the parameter it
  violated, e.g. `sh:maxInclusive 150` or `sh:in ("BE" "NL" "LU")`.
- **Focus-node context.** With `--data`, the details panel lists the focus
  node's triples and highlights the offending value (or says the value is
  missing).
- **Full SHACL property paths**, rendered as SPARQL paths: `^ex:worksFor`,
  `ex:address / ex:country`, `(ex:a | ex:b)*`.
- **Web viewer** (`--view` / `serve`): group by shape, focus node, constraint,
  path or severity; toggle severities; facet by shape and constraint; full-text
  search; CSV download of the visible rows.
- **Self-contained HTML report** (`--html report.html`): the same viewer with
  the data inlined, no server needed. Ideal as a CI artifact.
- **CI mode** (`--ci`) exits non-zero when results at or above `--fail-on`
  severity exist.
- **Multiple input formats** auto-detected from the extension: Turtle (`.ttl`),
  N-Triples (`.nt`), RDF/XML (`.rdf`, `.owl`, `.xml`), TriG (`.trig`),
  N-Quads (`.nq`); transparent gzip for `.gz`.

## Install

From source (requires a stable Rust toolchain):

```sh
cargo install --path .
```

Or, once published:

```sh
cargo install shacl-report
```

## Usage

First validate with the tool you already use, e.g.:

```sh
pyshacl -s shapes.ttl -f turtle data.ttl > report.ttl
# or: shacl validate --shapes shapes.ttl --data data.ttl > report.ttl   (Apache Jena)
```

Then:

```sh
# Summary on stderr
shacl-report report.ttl

# Explore it in the browser, with shape and data context
shacl-report report.ttl --shapes shapes.ttl --data data.ttl --view

# Write a standalone HTML report and fail the build on violations
shacl-report report.ttl --shapes shapes.ttl --data data.ttl \
    --html shacl-report.html --ci
```

Summary output:

```
conforms=false  results=11  violations=8  warnings=2  info=1
focus-nodes=3  shapes=7
       4  Person › ex:age
       2  Person › ex:name
       1  ex:OrganisationShape › ex:address / ex:country
       …
```

### Web viewer

```sh
# Empty viewer — load files interactively in the browser
shacl-report serve

# Preload a report with its shapes and data
shacl-report serve --report report.ttl --shapes shapes.ttl --data data.ttl

# Bind to a fixed port and skip auto-opening the browser
shacl-report serve --report report.ttl --bind 127.0.0.1:8080 --no-open
```

The viewer runs entirely offline — all assets (Tabulator) are bundled inside
the binary.

### In CI

```yaml
- run: pyshacl -s shapes.ttl -f turtle data.ttl > report.ttl || true
- run: shacl-report report.ttl --shapes shapes.ttl --data data.ttl --html shacl-report.html --ci
- uses: actions/upload-artifact@v4
  if: always()
  with:
    name: shacl-report
    path: shacl-report.html
```

### Options — report (default)

| Flag | Description |
| --- | --- |
| `--data <FILE>` | Data graph that was validated; shows focus-node triples. Repeatable. |
| `--shapes <FILE>` | Shapes graph that was used; shows shape names and constraints. Repeatable. |
| `--format <FMT>` | Force the report's input format (`nt`, `ttl`, `rdf`/`xml`, `trig`, `nq`). |
| `--html <FILE>` | Write a self-contained HTML report. |
| `--ci` | Exit with code 1 when results at or above `--fail-on` exist. |
| `--fail-on <SEV>` | `violation` (default), `warning` or `info`. |
| `--quiet` | Suppress the summary on stderr. |
| `--view` | Open the report in the local web viewer. |
| `--no-open` | Do not auto-open the system browser (implies `--view`). |
| `--bind <ADDR>` | Bind address for the viewer (default: `127.0.0.1:0`). |

### Options — `serve` subcommand

| Flag | Description |
| --- | --- |
| `--report <FILE>` | Report to preload. |
| `--data <FILE>` | Data graph to preload (requires `--report`). Repeatable. |
| `--shapes <FILE>` | Shapes graph to preload (requires `--report`). Repeatable. |
| `--format <FMT>` | Force the report's input format. |
| `--bind <ADDR>` | Bind address (default: `127.0.0.1:0`). |
| `--no-open` | Do not auto-open the system browser. |

### Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success (or, without `--ci`, completed even if results exist). |
| `1` | `--ci` was set and results at or above `--fail-on` were found. |
| `2` | Error (I/O, parse, no report found, etc.). |

## How it works

1. The report, shapes and data files are each parsed into an in-memory graph
   (quad formats are flattened).
2. Results are collected from `sh:result` on every `sh:ValidationReport`
   (falling back to un-nested `sh:ValidationResult` nodes) and flattened:
   severity, focus node, path, value, messages, source shape and component.
3. Each source shape is resolved against the shapes graph. Named shapes resolve
   directly. Blank-node property shapes cannot, because blank-node labels are
   local to a file, so they are matched on their rendered `sh:path` and the
   constraint parameter of the result's component (`sh:MinCountConstraintComponent`
   → `sh:minCount`). Ties are broken by the parent shape's `sh:targetClass` /
   `sh:targetNode` against the focus node in the data graph.
4. For every IRI focus node, up to 200 of its triples are taken from the data
   graph. Blank-node focus nodes have no context for the same reason as above.
5. The model is served as JSON to the viewer, or inlined into the HTML export.

## Limitations

- `shacl-report` does not validate; it reads reports.
- Nested `sh:detail` results are counted but not yet shown individually.
- Blank-node property shapes that share a path and constraint with another
  shape can only be told apart when `--data` is given.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

Test fixtures in `tests/fixtures/` were produced with pyshacl from
`shapes.ttl` and `data.ttl`.

## License

Apache-2.0.
