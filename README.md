# shacl-report

A small, fast CLI that turns a [SHACL](https://www.w3.org/TR/shacl/) validation
report into something people can actually read, and lets you explore the shapes
behind it and run them as SPARQL against your triple store. It works with the report of
**any** validator (pyshacl, Apache Jena, TopBraid, rudof, RDF4J, …), because it
reads the standard `sh:ValidationReport` vocabulary rather than running
validation itself.

Validators produce reports as raw RDF. On a real data set that is hundreds of
anonymous `sh:ValidationResult` nodes pointing at blank-node property shapes.
`shacl-report` flattens them into a table you can group, filter and search,
names every shape, shows the constraint that was broken, and puts the focus
node's own triples next to each result.

![Viewer with a result opened](screenshot-webapp.png)

![Shapes explorer](screenshot-shapes.png)

![A shape's rules running as SPARQL in the embedded MatGUI editor](screenshot-sparql.png)

<details><summary>Dark theme</summary>

![Viewer in dark theme](screenshot-dark.png)

</details>

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
- **Plain-language problems.** Every result is explained in words, e.g.
  *"212" is above the maximum of 150* or *Missing a value for ex:name*, and
  grouped into problem types such as *Missing value* or *Out of range*.
- **Web viewer** (`--view` / `serve`):
  - an overview with conformance status, severity counts and the most affected
    shapes and most common problems (all clickable as filters);
  - results as cards grouped by shape, resource or problem type, or as a
    sortable table;
  - filters for severity, problem type and shape, plus full-text search;
  - a details panel with the rule, the resource's data and keyboard
    navigation (`j`/`k`, `/` to search, `Esc` to close);
  - drag-and-drop loading of the report, shapes and data files;
  - light and dark themes, and a layout that works on a phone;
  - CSV export of the shown results and a standalone HTML download.
- **Shapes explorer** (`--shapes`, with or without a report):
  - every node and property shape with its targets, described in plain
    language (*Every instance of ex:Person*, *Every resource with a value for
    ex:email*), and how many results it produced;
  - every SHACL Core constraint as a readable rule: cardinality (`sh:minCount`,
    `sh:maxCount`), value type (`sh:datatype`, `sh:class`, `sh:nodeKind`),
    ranges and lengths, `sh:pattern`, lists (`sh:in`, `sh:languageIn`),
    `sh:hasValue`, property pairs (`sh:equals`, `sh:disjoint`, `sh:lessThan`),
    nested shapes (`sh:node`), qualified value shapes
    (`sh:qualifiedValueShape` with its min/max counts), logical constraints
    (`sh:and`, `sh:or`, `sh:not`, `sh:xone`) and closed shapes;
  - SPARQL constraints (`sh:sparql`) and custom SPARQL-based constraint
    components (`sh:ConstraintComponent` validators), with their queries;
  - links between shapes and results in both directions.
- **SPARQL editor** ([MatGUI](https://github.com/Matdata-eu/MatGUI), bundled):
  point it at the SPARQL endpoint that holds your data and run, with one click,
  - a generated query that finds the violations of a single rule or of a whole
    shape, scoped to the shape's focus nodes;
  - a SPARQL constraint or custom component validator, with `$PATH`,
    `$currentShape` and parameters substituted, `sh:prefixes` declared and
    `$this` bound to the shape's targets;
  - the focus nodes of a shape.

- **Describe any IRI**, as in MatGUI: Ctrl+click (Cmd+click on a Mac) an IRI
  anywhere in the report, the details panel or the shapes explorer to load its
  triples from the endpoint into MatGUI's Turtle view; Ctrl+Shift+click loads
  the triples that point to it. Ctrl+click IRIs in that view to keep exploring.

  In the local viewer, queries can be relayed through `shacl-report` so
  endpoints that do not send CORS headers work too.
- **Files or URLs**: the report, shapes and data can be local files or
  `http(s)` URLs (content negotiation picks an RDF format).
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

# Explore only the shapes, and run them against your triple store
shacl-report serve --shapes https://example.org/shapes.ttl \
    --endpoint https://example.org/sparql

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
# Empty viewer — drop the files into the browser
shacl-report serve

# Preload a report with its shapes and data, and the SPARQL endpoint of the data
shacl-report serve --report report.ttl --shapes shapes.ttl --data data.ttl \
    --endpoint http://localhost:3030/ds/sparql

# Bind to a fixed port and skip auto-opening the browser
shacl-report serve --report report.ttl --bind 127.0.0.1:8080 --no-open
```

The viewer runs entirely offline — all assets (Tabulator, MatGUI) are bundled
inside the binary. Only the SPARQL editor talks to the endpoint you give it.

#### Shapes and SPARQL

With shapes loaded, the viewer gets a **Shapes** and a **SPARQL** tab. Pick a
shape to see what it applies to and every rule it checks; the ▶ buttons open a
query in the SPARQL tab and run it on your endpoint:

| Button | Query |
| --- | --- |
| ▶ next to a rule | Focus nodes or values that break that one rule. |
| *Find all violations* | Every SHACL Core rule of the shape and its property shapes in one `UNION`, with a `?rule` column. |
| *Run in SPARQL editor* on a SPARQL constraint | The constraint's own query, ready to run outside a validator. |
| *List focus nodes* | The resources the shape's targets select. |

Generated queries cover the constraints that plain SPARQL can express
(cardinality, datatype, class, node kind, ranges, lengths, pattern, `sh:in`,
`sh:languageIn`, `sh:hasValue`, `sh:uniqueLang`, property pairs and closed
shapes). Nested and logical constraints are described but left to your
validator. Shapes used only through `sh:node` get their focus nodes from the
shape that refers to them. The endpoint is remembered in the browser; in the
local viewer the *Relay through shacl-report* option sends queries via the
`/api/sparql` relay, which also forwards an `Authorization` header. In a
standalone HTML report the browser talks to the endpoint directly, so the
endpoint must allow CORS.

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
| `--shapes <FILE>` | Shapes graph that was used; shows shape names and constraints, and enables the shapes explorer. Without a report, `--shapes` alone summarises the shapes graph. Repeatable. |
| `--format <FMT>` | Force the report's input format (`nt`, `ttl`, `rdf`/`xml`, `trig`, `nq`). |
| `--html <FILE>` | Write a self-contained HTML report. |
| `--ci` | Exit with code 1 when results at or above `--fail-on` exist. |
| `--fail-on <SEV>` | `violation` (default), `warning` or `info`. |
| `--endpoint <URL>` | SPARQL endpoint holding the data, preset in the viewer's SPARQL tab. |
| `--quiet` | Suppress the summary on stderr. |
| `--view` | Open the report in the local web viewer. |
| `--no-open` | Do not auto-open the system browser (implies `--view`). |
| `--bind <ADDR>` | Bind address for the viewer (default: `127.0.0.1:0`). |

### Options — `serve` subcommand

| Flag | Description |
| --- | --- |
| `--report <FILE>` | Report to preload. |
| `--data <FILE>` | Data graph to preload. Repeatable. |
| `--shapes <FILE>` | Shapes graph to preload; enough on its own to explore shapes. Repeatable. |
| `--endpoint <URL>` | SPARQL endpoint holding the data, preset in the SPARQL tab. |
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
5. The shapes graph is turned into an explorable model: every node and
   property shape (named, blank, or nested in `sh:node`, `sh:or`,
   `sh:qualifiedValueShape`, …), its targets, constraint parameters, SPARQL
   constraints and custom components. Results link to it by the resolved shape.
6. The model is served as JSON to the viewer, or inlined into the HTML export.

## Limitations

- `shacl-report` does not validate; it reads reports. The generated SPARQL is a
  way to explore the data, not a conforming SHACL engine: nested and logical
  constraints are not translated, and comparisons follow your endpoint's
  SPARQL semantics.
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
`shapes.ttl` / `shapes-advanced.ttl` and `data.ttl`.

## License

Apache-2.0. Bundles [Tabulator](https://tabulator.info) (MIT) and
[MatGUI](https://github.com/Matdata-eu/MatGUI) (MIT, see
`assets/vendor/matgui/NOTICE.md`).
