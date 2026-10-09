/* Turtle viewer: describe IRIs against the SPARQL endpoint and keep exploring.
 *
 * Ported from MatGUI's Response view (packages/yasr/src/plugins/response),
 * which is built into MatGUI rather than shipped as a plugin. It uses the
 * same CodeMirror 6 editor and Turtle grammar, bundled in
 * assets/vendor/codemirror. On top of MatGUI it also follows prefixed names
 * (ex:thing) and the `a` keyword, expanding them with the prefixes the
 * document declares, then the report's prefixes.
 */
(function () {
  "use strict";

  const RDF_TYPE = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
  const ACCEPT = "text/turtle,application/n-triples;q=0.9,*/*;q=0.1";

  // ---------- helpers ported from MatGUI's uriUtils.ts ----------

  // Angle-bracket IRIs as they appear in Turtle / N-Triples, e.g. `<http://example.org/foo>`.
  const ANGLE_IRI_RE = /<([^<>\s"{}|\\^`]+)>/g;
  // Bare URIs as they appear in comments or other formats, e.g. `http://example.org/foo`.
  const BARE_URI_RE = /(?:https?|urn|ftp|mailto):[^\s<>"'`{}|\\^[\]]+/g;
  // Turtle/SPARQL prefix declarations such as `@prefix ex: <...> .` and `PREFIX ex: <...>`.
  const PREFIX_DECL_RE = /^\s*(?:@prefix|PREFIX)\s+([\w.-]*):\s*<([^>]*)>\s*\.?\s*$/i;

  /** Drop punctuation that ends a Turtle statement but is not part of the URI. */
  function sanitizeUri(uri) {
    return uri.replace(/[.,;)\]]+$/, "").trim();
  }

  /** The URI at character `offset` of `text`; angle-bracket IRIs win over bare URIs. */
  function extractUriAtOffset(text, offset) {
    if (!text || offset < 0 || offset >= text.length) return undefined;
    for (const re of [ANGLE_IRI_RE, BARE_URI_RE]) {
      re.lastIndex = 0;
      let m;
      while ((m = re.exec(text)) !== null) {
        if (offset >= m.index && offset <= m.index + m[0].length) {
          const uri = sanitizeUri(re === ANGLE_IRI_RE ? m[1] : m[0]);
          return uri || undefined;
        }
      }
    }
    return undefined;
  }

  function buildSubjectOfQuery(uri) {
    return `CONSTRUCT { <${uri}> ?p ?o } WHERE { <${uri}> ?p ?o }`;
  }

  function buildObjectOfQuery(uri) {
    return `CONSTRUCT { ?s ?p <${uri}> } WHERE { ?s ?p <${uri}> } LIMIT 1000`;
  }

  /** Remove prefix declarations whose label is already declared (case-insensitive). */
  function stripDuplicatePrefixDeclarations(text, existingText) {
    if (!text) return text;
    const seen = new Set();
    const note = (source) => {
      for (const line of source.split("\n")) {
        const m = line.match(PREFIX_DECL_RE);
        if (m) seen.add(m[1].toLowerCase());
      }
    };
    note(existingText || "");
    const out = [];
    for (const line of text.split("\n")) {
      const m = line.match(PREFIX_DECL_RE);
      if (m) {
        const label = m[1].toLowerCase();
        if (seen.has(label)) continue;
        seen.add(label);
      }
      out.push(line);
    }
    return out.join("\n");
  }

  // ---------- prefixed names ----------

  /** Prefix declarations in a Turtle document, by label. */
  function declaredPrefixes(text) {
    const map = new Map();
    for (const line of text.split("\n")) {
      const m = line.match(PREFIX_DECL_RE);
      if (m) map.set(m[1], m[2]);
    }
    return map;
  }

  /** Expand `prefix:local` with the document's prefixes, then the fallback list. */
  function expandPrefixedName(pname, docPrefixes, fallback) {
    const i = pname.indexOf(":");
    const prefix = pname.slice(0, i);
    // Turtle escapes reserved characters in local names with a backslash.
    const local = pname.slice(i + 1).replace(/\\([^\w])/g, "$1");
    if (docPrefixes.has(prefix)) return { iri: docPrefixes.get(prefix) + local };
    const hit = (fallback || []).find(([name]) => name === prefix);
    if (hit) return { iri: hit[1] + local };
    return { error: `The prefix ${prefix}: is not declared` };
  }

  /**
   * What sits at `pos`: a full IRI, a prefixed name, the `a` keyword or a URI
   * in a comment. Returns { iri } or { error }, or null for anything else.
   */
  function iriAt(cm, state, pos, fallback) {
    const tree = cm.syntaxTree(state);
    const text = state.doc.toString();
    for (const side of [1, -1]) {
      for (let node = tree.resolveInner(pos, side); node; node = node.parent) {
        const value = text.slice(node.from, node.to);
        if (node.name === "Iriref") return { iri: value.slice(1, -1) };
        if (node.name === "Pname_ln" || node.name === "Pname_ns") {
          return expandPrefixedName(value, declaredPrefixes(text), fallback);
        }
        if (node.name === "Verb" && value === "a") return { iri: RDF_TYPE };
        if (node.name === "PrefixID" || node.name === "Literal" || node.name === "BlankNode") return null;
      }
    }
    const uri = extractUriAtOffset(text, pos);
    return uri ? { iri: uri } : null;
  }

  /** Mark the IRIs and prefixed names in view so they can be styled as links. */
  function linkMarks(cm) {
    const mark = cm.Decoration.mark({ class: "cm-iri" });
    const build = (view) => {
      const builder = new cm.RangeSetBuilder();
      const tree = cm.syntaxTree(view.state);
      for (const { from, to } of view.visibleRanges) {
        tree.iterate({
          from,
          to,
          enter: (node) => {
            if (node.name === "PrefixID") return false;
            if (node.name === "Iriref" || node.name === "Pname_ln" || node.name === "Pname_ns" || (node.name === "Verb" && view.state.doc.sliceString(node.from, node.to) === "a")) {
              builder.add(node.from, node.to, mark);
              return false;
            }
            return undefined;
          },
        });
      }
      return builder.finish();
    };
    return cm.ViewPlugin.fromClass(
      class {
        constructor(view) {
          this.decorations = build(view);
        }
        update(u) {
          if (u.docChanged || u.viewportChanged || cm.syntaxTree(u.startState) !== cm.syntaxTree(u.state)) this.decorations = build(u.view);
        }
      },
      { decorations: (v) => v.decorations },
    );
  }

  // ---------- the viewer ----------

  class TurtleView {
    /**
     * opts.endpoint(): the endpoint as typed; opts.requestUrl(): where to send
     * requests; opts.whenEndpoint(fn): ask for an endpoint, then call fn;
     * opts.prefixes(): [[name, base]] from the report; opts.shorten(iri).
     */
    constructor(root, opts) {
      this.root = root;
      this.opts = opts;
      this.host = root.querySelector("#explore-body");
      this.sub = root.querySelector("#explore-endpoint");
      this.cm = null;
      this.busy = 0;
      this.generation = 0;
      root.querySelector("#explore-clear").addEventListener("click", () => this.clear());
      root.querySelector("#explore-copy").addEventListener("click", () => this.copy());
      root.querySelector("#explore-download").addEventListener("click", () => this.download());
      root.querySelector("#close-explore").addEventListener("click", () => this.close());
    }

    get isOpen() {
      return this.root.classList.contains("open");
    }

    open() {
      this.root.classList.add("open");
      this.root.setAttribute("aria-hidden", "false");
      this.sub.textContent = this.opts.endpoint() || "";
      this.ensure();
    }

    close() {
      this.root.classList.remove("open");
      this.root.setAttribute("aria-hidden", "true");
    }

    ensure() {
      if (this.cm) return this.cm;
      const cm = window.ShaclCodeMirror;
      if (!cm) {
        this.host.innerHTML = '<div class="callout">The Turtle viewer could not be loaded.</div>';
        return null;
      }
      this.lib = cm;
      this.cm = new cm.EditorView({
        state: cm.EditorState.create({
          doc: "",
          extensions: [
            cm.EditorState.readOnly.of(true),
            cm.EditorView.editable.of(false),
            cm.lineNumbers(),
            cm.foldGutter(),
            cm.highlightSpecialChars(),
            cm.bracketMatching(),
            cm.EditorView.lineWrapping,
            // Stable `tok-*` classes so styles.css controls the colors in both themes.
            cm.syntaxHighlighting(cm.classHighlighter),
            cm.turtle(),
            linkMarks(cm),
          ],
        }),
        parent: this.host,
      });
      // Capture, so a Shift+click does not extend the selection first.
      this.cm.dom.addEventListener("mousedown", (e) => this.onMouseDown(e), true);
      return this.cm;
    }

    get text() {
      return this.cm ? this.cm.state.doc.toString() : "";
    }

    setText(value) {
      if (!this.cm) return;
      this.cm.dispatch({ changes: { from: 0, to: this.cm.state.doc.length, insert: value } });
    }

    clear() {
      this.generation++;
      this.setText("");
      this.setBusy(0);
    }

    copy() {
      const text = this.text;
      if (!text) return;
      navigator.clipboard.writeText(text).then(
        () => this.opts.toast("Turtle copied"),
        () => this.opts.toast("Copying is not allowed here"),
      );
    }

    download() {
      const text = this.text;
      if (!text) return;
      const url = URL.createObjectURL(new Blob([text], { type: "text/turtle;charset=utf-8" }));
      const a = document.createElement("a");
      a.href = url;
      a.download = "explore.ttl";
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    }

    /**
     * Ctrl/Cmd+click on an IRI describes it (triples where it is the subject);
     * Ctrl/Cmd+Shift+click shows the triples where it is the object.
     */
    onMouseDown(event) {
      if (!(event.ctrlKey || event.metaKey) || event.button !== 0) return;
      const pos = this.cm.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos === null) return;
      const hit = iriAt(this.lib, this.cm.state, pos, this.opts.prefixes());
      if (!hit) return;
      event.preventDefault();
      event.stopPropagation();
      if (hit.error) {
        this.opts.toast(hit.error);
        return;
      }
      this.describe(hit.iri, event.shiftKey, false);
    }

    /**
     * Append the triples about `iri` to the view. `fresh` starts a new
     * exploration, as a Ctrl+click in the report does.
     */
    describe(iri, asObject, fresh) {
      this.open();
      if (!this.ensure()) return;
      if (fresh) this.clear();
      if (!this.opts.endpoint()) {
        this.opts.whenEndpoint(() => this.describe(iri, asObject, fresh));
        return;
      }
      this.sub.textContent = this.opts.endpoint();
      const role = asObject ? "object" : "subject";
      const head = `# Triples where <${iri}> is ${role}`;
      const generation = this.generation;
      this.setBusy(1);
      this.fetch(asObject ? buildObjectOfQuery(iri) : buildSubjectOfQuery(iri))
        .then((content) => {
          if (generation !== this.generation) return;
          if (content.trim()) this.append(`${head}\n${content.trim().replace(/\n{3,}/g, "\n\n")}\n`);
          else {
            this.append(`${head}: no data\n`);
            const label = this.opts.shorten(iri);
            this.opts.toast(asObject ? `Nothing in the endpoint points to ${label}` : `The endpoint has no triples about ${label}`);
          }
        })
        .catch((err) => {
          if (generation !== this.generation) return;
          this.append(`${head} failed: ${err.message}\n`);
        })
        .finally(() => {
          if (generation === this.generation) this.setBusy(-1);
        });
    }

    async fetch(query) {
      const res = await fetch(this.opts.requestUrl(), {
        method: "POST",
        headers: { Accept: ACCEPT, "Content-Type": "application/x-www-form-urlencoded;charset=UTF-8" },
        body: new URLSearchParams({ query }).toString(),
      });
      const body = await res.text();
      if (!res.ok) {
        const detail = body.trim().split("\n")[0].slice(0, 200);
        throw new Error(`HTTP ${res.status}${detail ? ` ${detail}` : ""}`);
      }
      return body;
    }

    /** Append a section and scroll to it, like MatGUI's appendToView. */
    append(section) {
      const existing = this.text;
      const text = (existing ? "\n" : "") + stripDuplicatePrefixDeclarations(section, existing);
      const at = existing.length;
      this.cm.dispatch({ changes: { from: at, insert: text } });
      const line = this.cm.state.doc.lineAt(Math.min(at + (existing ? 1 : 0), this.cm.state.doc.length));
      this.cm.dispatch({ effects: this.lib.EditorView.scrollIntoView(line.from, { y: "start", yMargin: 8 }) });
    }

    setBusy(delta) {
      this.busy = delta === 0 ? 0 : Math.max(0, this.busy + delta);
      this.root.classList.toggle("busy", this.busy > 0);
    }
  }

  window.ShaclTurtle = {
    TurtleView,
    extractUriAtOffset,
    buildSubjectOfQuery,
    buildObjectOfQuery,
    stripDuplicatePrefixDeclarations,
    declaredPrefixes,
    expandPrefixedName,
  };
})();
