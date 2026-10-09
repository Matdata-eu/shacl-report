// shacl-report: SPARQL support for the shapes explorer. Builds queries that
// look for violations of SHACL Core constraints, prepares SPARQL-based
// constraints for running outside a validator, and wraps the embedded MatGUI
// editor with an endpoint bar.
(function () {
  "use strict";

  const SH = "http://www.w3.org/ns/shacl#";
  const RDF_TYPE = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
  const RDFS_SUBCLASS = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
  const ENDPOINT_KEY = "shacl-report-endpoint";
  const PROXY_KEY = "shacl-report-proxy";
  const LIMIT = 1000;

  const local = (iri) => (iri && iri.startsWith(SH) ? iri.slice(SH.length) : null);
  const strLit = (s) => `"${String(s).replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n").replace(/\r/g, "\\r")}"`;

  // ---------- focus nodes from targets ----------

  /** SPARQL pattern binding ?this to the focus nodes of `targets`, or null. */
  function targetPattern(targets) {
    const parts = [];
    for (const t of targets || []) {
      switch (t.kind) {
        case "class":
        case "implicitClass":
          parts.push(`?this <${RDF_TYPE}>/<${RDFS_SUBCLASS}>* ${t.value} .`);
          break;
        case "node":
          parts.push(`VALUES ?this { ${t.value} }`);
          break;
        case "subjectsOf":
          parts.push(`?this ${t.value} ?_object .`);
          break;
        case "objectsOf":
          parts.push(`?_subject ${t.value} ?this .`);
          break;
        case "sparql":
          parts.push(`{ ${stripPrologue(t.value).replace(/\$this\b/g, "?this")} }`);
          break;
      }
    }
    if (!parts.length) return null;
    if (parts.length === 1) return parts[0];
    return parts.map((p) => `{ ${p} }`).join("\n    UNION\n    ");
  }

  /** Prefix declarations needed by SPARQL-based targets. */
  function targetPrefixes(targets) {
    const out = [];
    for (const t of targets || []) for (const p of t.prefixes || []) out.push(p);
    return out;
  }

  function stripPrologue(q) {
    return q.replace(/^\s*((PREFIX\s+[\w.-]*:\s*<[^>]*>|BASE\s+<[^>]*>)\s*)*/i, "").trim();
  }

  // ---------- SHACL Core constraints as violation queries ----------

  const NODE_KIND = {
    IRI: "!isIRI(?value)",
    Literal: "!isLiteral(?value)",
    BlankNode: "!isBlank(?value)",
    BlankNodeOrIRI: "isLiteral(?value)",
    BlankNodeOrLiteral: "isIRI(?value)",
    IRIOrLiteral: "isBlank(?value)",
  };

  function first(shape, name) {
    const c = shape.constraints.find((x) => x.param === SH + name);
    return c && c.values.length ? c.values[0] : null;
  }

  /**
   * One violation query per SHACL Core constraint of `shape` that can be
   * expressed in plain SPARQL. `focus` is the target pattern (may be null).
   * Returns [{ param, label, select, where, groupHaving }] where the
   * select clause projects ?this and ?value.
   */
  function coreChecks(shape, focus, model) {
    const out = [];
    const path = shape.path || null;
    const valuePattern = path ? `?this ${path} ?value .` : "BIND (?this AS ?value)";
    const f = focus ? `${focus}\n    ` : "";
    const add = (param, label, where, extra) => out.push({ param, label, where, ...(extra || {}) });
    const simple = (param, label, filter) => add(param, label, `${f}${valuePattern}\n    FILTER (${filter})`);

    for (const c of shape.constraints) {
      const name = local(c.param);
      const v = c.values[0];
      if (!name || !v) continue;
      const s = v.sparql;
      switch (name) {
        case "minCount":
          if (!path || !focus) break;
          add(c.param, `minCount ${s}`, `${focus}\n    OPTIONAL { ?this ${path} ?v . }`, {
            select: "?this (COUNT(DISTINCT ?v) AS ?count)",
            tail: `GROUP BY ?this\nHAVING (COUNT(DISTINCT ?v) < ${s})`,
          });
          break;
        case "maxCount":
          if (!path) break;
          add(c.param, `maxCount ${s}`, `${f}?this ${path} ?v .`, {
            select: "?this (COUNT(DISTINCT ?v) AS ?count)",
            tail: `GROUP BY ?this\nHAVING (COUNT(DISTINCT ?v) > ${s})`,
          });
          break;
        case "datatype":
          if (s) simple(c.param, `datatype ${v.text}`, `!isLiteral(?value) || datatype(?value) != ${s}`);
          break;
        case "class":
          if (s) add(c.param, `class ${v.text}`, `${f}${valuePattern}\n    FILTER NOT EXISTS { ?value <${RDF_TYPE}>/<${RDFS_SUBCLASS}>* ${s} . }`);
          break;
        case "nodeKind": {
          const k = NODE_KIND[local(v.text.replace(/^<|>$/g, "")) || ""];
          if (k) simple(c.param, `nodeKind ${v.text}`, k);
          break;
        }
        case "pattern": {
          const flags = first(shape, "flags");
          const fl = flags ? `, ${flags.sparql}` : "";
          simple(c.param, `pattern ${s}`, `isBlank(?value) || !regex(str(?value), ${s}${fl})`);
          break;
        }
        case "in":
          if (c.values.every((x) => x.sparql)) simple(c.param, "in (…)", `?value NOT IN (${c.values.map((x) => x.sparql).join(", ")})`);
          break;
        case "languageIn":
          add(c.param, "languageIn (…)", `${f}${valuePattern}\n    FILTER NOT EXISTS { VALUES ?lang { ${c.values.map((x) => x.sparql).join(" ")} } FILTER (langMatches(lang(?value), ?lang)) }`);
          break;
        case "hasValue":
          if (!focus || !s) break;
          add(c.param, `hasValue ${v.text}`, `${focus}\n    FILTER NOT EXISTS { ${path ? `?this ${path} ${s}` : `FILTER (?this = ${s})`} }`, { select: "?this" });
          break;
        case "minInclusive": if (s) simple(c.param, `minInclusive ${s}`, `!(?value >= ${s})`); break;
        case "maxInclusive": if (s) simple(c.param, `maxInclusive ${s}`, `!(?value <= ${s})`); break;
        case "minExclusive": if (s) simple(c.param, `minExclusive ${s}`, `!(?value > ${s})`); break;
        case "maxExclusive": if (s) simple(c.param, `maxExclusive ${s}`, `!(?value < ${s})`); break;
        case "minLength": if (s) simple(c.param, `minLength ${s}`, `isBlank(?value) || STRLEN(str(?value)) < ${s}`); break;
        case "maxLength": if (s) simple(c.param, `maxLength ${s}`, `isBlank(?value) || STRLEN(str(?value)) > ${s}`); break;
        case "uniqueLang":
          if (path && s === "true") add(c.param, "uniqueLang", `${f}?this ${path} ?value, ?other .\n    FILTER (?value != ?other && lang(?value) != "" && lang(?value) = lang(?other))`);
          break;
        case "disjoint":
          if (s) add(c.param, `disjoint ${v.text}`, `${f}${valuePattern}\n    ?this ${s} ?value .`);
          break;
        case "equals":
          if (s) add(c.param, `equals ${v.text}`, `${f}{ ${valuePattern} FILTER NOT EXISTS { ?this ${s} ?value . } }\n    UNION\n    { ?this ${s} ?value . FILTER NOT EXISTS { ${valuePattern.replace(/\.$/, "")} } }`);
          break;
        case "lessThan":
          if (s) add(c.param, `lessThan ${v.text}`, `${f}${valuePattern}\n    ?this ${s} ?other .\n    FILTER (!(?value < ?other))`);
          break;
        case "lessThanOrEquals":
          if (s) add(c.param, `lessThanOrEquals ${v.text}`, `${f}${valuePattern}\n    ?this ${s} ?other .\n    FILTER (!(?value <= ?other))`);
          break;
        case "closed": {
          if (s !== "true" || path) break;
          const allowed = shape.properties
            .map((i) => model.shapes[i].path_iri)
            .filter(Boolean)
            .map((p) => `<${p}>`);
          const ignored = shape.constraints.find((x) => x.param === SH + "ignoredProperties");
          if (ignored) for (const x of ignored.values) if (x.sparql) allowed.push(x.sparql);
          add(c.param, "closed", `${f}?this ?predicate ?value .${allowed.length ? `\n    FILTER (?predicate NOT IN (${allowed.join(", ")}))` : ""}`, { select: "?this ?predicate ?value" });
          break;
        }
      }
    }
    return out;
  }

  function checkQuery(check) {
    const select = check.select || "?this ?value";
    return `SELECT DISTINCT ${select}\nWHERE {\n    ${check.where}\n}${check.tail ? "\n" + check.tail : ""}\nLIMIT ${LIMIT}`;
  }

  /** One query listing every Core violation of a shape and its property shapes. */
  function shapeQuery(shape, model, focusOf, textOf) {
    textOf = textOf || ((x) => x);
    const parts = [];
    const collect = (s) => {
      for (const check of coreChecks(s, focusOf(s), model)) {
        const label = textOf(s.path ? `${s.path}: ${check.label}` : check.label);
        let sub;
        if (check.tail) {
          sub = `SELECT ?this (COUNT(DISTINCT ?v) AS ?value) WHERE {\n      ${check.where.replace(/\n/g, "\n  ")}\n    }\n    ${check.tail.replace(/\n/g, "\n    ")}`;
        } else if (check.select === "?this") {
          sub = `SELECT ?this WHERE {\n      ${check.where.replace(/\n/g, "\n  ")}\n    }`;
        } else {
          sub = `SELECT ?this ?value WHERE {\n      ${check.where.replace(/\n/g, "\n  ")}\n    }`;
        }
        parts.push(`{\n    {\n    ${sub}\n    }\n    BIND (${strLit(label)} AS ?rule)\n  }`);
      }
    };
    collect(shape);
    for (const i of shape.properties) collect(model.shapes[i]);
    if (!parts.length) return null;
    return `SELECT ?rule ?this ?value\nWHERE {\n  ${parts.join("\n  UNION\n  ")}\n}\nORDER BY ?rule ?this\nLIMIT ${LIMIT}`;
  }

  // ---------- SPARQL-based constraints ----------

  /**
   * Make a SPARQL constraint (or custom component validator) runnable on an
   * endpoint: substitute $PATH, $currentShape and parameters, and optionally
   * bind $this to the shape's focus nodes.
   */
  function prepareConstraint(query, opts) {
    let q = dedent(query);
    if (opts.path) q = q.replace(/\$PATH\b/g, opts.path);
    if (opts.shapeIri) q = q.replace(/\$currentShape\b/g, `<${opts.shapeIri}>`);
    for (const [name, value] of Object.entries(opts.params || {})) {
      if (value) q = q.replace(new RegExp(`[$?]${name}\\b`, "g"), value);
    }
    if (opts.focus) {
      const m = /\bWHERE\s*\{/i.exec(q);
      if (m) {
        const at = m.index + m[0].length;
        q = `${q.slice(0, at)}\n    # Focus nodes of the shape (added by shacl-report)\n    ${opts.focus.replace(/\n/g, "\n    ")}\n${q.slice(at)}`;
      }
    }
    return q;
  }

  /** Remove the indentation shared by all lines after the first. */
  function dedent(text) {
    const lines = text.replace(/^\s*\n/, "").replace(/\s+$/, "").split("\n");
    const indents = lines.filter((l) => l.trim()).map((l) => l.match(/^[ \t]*/)[0].length);
    const cut = indents.length ? Math.min(...indents) : 0;
    return lines.map((l) => l.slice(Math.min(cut, l.match(/^[ \t]*/)[0].length))).join("\n");
  }

  // ---------- prefixes ----------

  const PN = /^[A-Za-z][\w-]*$|^$/;
  const LOCAL = /^[A-Za-z_][\w-]*$/;

  /**
   * Shorten `<iri>` tokens outside string literals with the known prefixes
   * and prepend PREFIX declarations for the ones used and for `declared`
   * prefixes whose name appears in the query.
   */
  function withPrefixes(query, prefixes, declared) {
    const byLength = prefixes.filter(([n, b]) => b && PN.test(n)).slice().sort((a, b) => b[1].length - a[1].length);
    const used = new Map();
    const shortenAll = (text) =>
      text.replace(/<([A-Za-z][\w+.-]*:[^\s<>"{}|^`\\]*)>/g, (m, iri) => {
        for (const [name, base] of byLength) {
          if (iri.startsWith(base) && LOCAL.test(iri.slice(base.length))) {
            used.set(name, base);
            return `${name}:${iri.slice(base.length)}`;
          }
        }
        return m;
      });
    // Split on string literals so their content stays untouched.
    let body = "";
    // IRIs come first so a "#" inside an IRI is not read as a comment.
    const re = /(<[A-Za-z][\w+.-]*:[^\s<>"{}|^`\\]*>|"""[\s\S]*?"""|'''[\s\S]*?'''|"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|#[^\n]*)/g;
    let last = 0;
    for (let m; (m = re.exec(query)); ) {
      body += shortenAll(query.slice(last, m.index)) + (m[0][0] === "<" ? shortenAll(m[0]) : m[0]);
      last = m.index + m[0].length;
    }
    body += shortenAll(query.slice(last));

    const head = [];
    const seen = new Set();
    const already = new Set([...body.matchAll(/PREFIX\s+([\w.-]*):/gi)].map((m) => m[1]));
    const declare = (name, ns) => {
      if (seen.has(name) || already.has(name)) return;
      seen.add(name);
      head.push(`PREFIX ${name}: <${ns}>`);
    };
    for (const [name, ns] of declared || []) declare(name, ns);
    for (const [name, base] of [...used.entries()].sort()) declare(name, base);
    // Prefixed names in the original text that nothing declares yet.
    for (const [name, base] of prefixes) {
      if (!seen.has(name) && !already.has(name) && new RegExp(`(^|[\\s(/^|,;{])${name.replace(/[.-]/g, "\\$&")}:[A-Za-z_]`).test(body)) declare(name, base);
    }
    return (head.length ? head.join("\n") + "\n\n" : "") + body;
  }

  // ---------- editor ----------

  function storage(key, value) {
    try {
      if (value === undefined) return localStorage.getItem(key);
      if (value === null) localStorage.removeItem(key);
      else localStorage.setItem(key, value);
    } catch (_) {
      return null;
    }
    return null;
  }

  /**
   * The SPARQL tab: an endpoint bar plus an embedded MatGUI. In server mode
   * requests can go through the local /api/sparql relay, which avoids CORS
   * problems with endpoints that do not allow browser access.
   */
  class Editor {
    constructor(root, opts) {
      this.root = root;
      this.server = !!opts.server;
      this.onNeedEndpoint = opts.onNeedEndpoint || (() => {});
      this.toast = opts.toast || (() => {});
      this.theme = opts.theme || (() => "light");
      this.shorten = opts.shorten || ((iri) => `<${iri}>`);
      this.yasgui = null;
      this.endpointInput = root.querySelector("#endpoint");
      this.proxyInput = root.querySelector("#endpoint-proxy");
      this.status = root.querySelector("#endpoint-status");
      this.host = root.querySelector("#yasgui");
      root.querySelector(".proxy-toggle").classList.toggle("hidden", !this.server);
      const saved = storage(ENDPOINT_KEY);
      this.endpointInput.value = opts.preset || saved || "";
      const proxy = storage(PROXY_KEY);
      this.proxyInput.checked = this.server && proxy !== "off";
      root.querySelector("#endpoint-form").addEventListener("submit", (e) => {
        e.preventDefault();
        this.applyEndpoint(true);
      });
      this.proxyInput.addEventListener("change", () => {
        storage(PROXY_KEY, this.proxyInput.checked ? "on" : "off");
        this.applyEndpoint(false);
      });
      this.renderStatus();
    }

    get endpoint() {
      return this.endpointInput.value.trim();
    }

    /** The URL MatGUI should send requests to. */
    requestUrl() {
      const ep = this.endpoint;
      if (!ep) return "";
      if (this.server && this.proxyInput.checked) return `${location.origin}/api/sparql?endpoint=${encodeURIComponent(ep)}`;
      return ep;
    }

    renderStatus() {
      const ep = this.endpoint;
      if (!ep) {
        this.status.textContent = "Enter the SPARQL endpoint of the triple store that holds your data.";
        this.status.className = "endpoint-status warn";
      } else if (!/^https?:\/\//i.test(ep)) {
        this.status.textContent = "The endpoint must be an http(s) URL.";
        this.status.className = "endpoint-status warn";
      } else {
        const via = this.server && this.proxyInput.checked ? "through the shacl-report server (no CORS needed)" : "directly from your browser (the endpoint must allow CORS)";
        this.status.textContent = `Queries go to ${ep} ${via}.`;
        this.status.className = "endpoint-status";
      }
    }

    applyEndpoint(announce) {
      storage(ENDPOINT_KEY, this.endpoint || null);
      this.renderStatus();
      if (this.yasgui) {
        const url = this.requestUrl();
        this.yasgui.config.requestConfig = { ...this.yasgui.config.requestConfig, endpoint: url };
        for (const id of Object.keys(this.yasgui._tabs || {})) {
          const tab = this.yasgui.getTab(id);
          if (tab && tab.setEndpoint) tab.setEndpoint(url);
        }
      }
      if (announce && this.endpoint) this.toast("Endpoint saved");
      // Run the query that was waiting for an endpoint.
      if (this.pending && this.endpoint && /^https?:\/\//i.test(this.endpoint)) {
        const { tab, query, plugin, onEmpty } = this.pending;
        this.pending = null;
        this.run(tab, query, plugin, onEmpty);
      }
    }

    ensure() {
      if (this.yasgui) return this.yasgui;
      const Y = window.Yasgui && (window.Yasgui.default || window.Yasgui);
      if (typeof Y !== "function") {
        this.host.innerHTML = '<div class="callout">The SPARQL editor could not be loaded.</div>';
        return null;
      }
      this.host.innerHTML = "";
      this.yasgui = new Y(this.host, {
        requestConfig: { endpoint: this.requestUrl(), method: "POST" },
        theme: this.theme(),
        showThemeToggle: false,
        showSnippetsBar: false,
        endpointDescribe: { enabled: false },
        copyEndpointOnNewTab: true,
        persistenceId: null,
        populateFromUrl: false,
        autoAddOnInit: true,
        endpointCatalogueOptions: { getData: () => [] },
        yasqe: { resizeable: true, editorHeight: "260px" },
        yasr: { persistenceId: null },
      });
      return this.yasgui;
    }

    /** Follow the viewer's light or dark theme. */
    setTheme(theme) {
      if (this.yasgui && this.yasgui.themeManager) this.yasgui.themeManager.setTheme(theme);
    }

    /**
     * Describe an IRI the way MatGUI does on Ctrl+click: the triples where it
     * is the subject, or (asObject, Ctrl+Shift+click) where it is the object,
     * shown in MatGUI's Turtle view. Ctrl+click on IRIs there explores further.
     */
    describe(iri, asObject) {
      const query = asObject
        ? `CONSTRUCT { ?s ?p <${iri}> } WHERE { ?s ?p <${iri}> } LIMIT 1000`
        : `CONSTRUCT { <${iri}> ?p ?o } WHERE { <${iri}> ?p ?o }`;
      // N-Triples keeps every IRI in angle brackets, which is what MatGUI's
      // Turtle view needs to follow it with another Ctrl+click.
      const label = this.shorten(iri);
      this.open(`${asObject ? "Used by" : "Describe"} ${label}`, query, "response", {
        acceptHeaderGraph: "application/n-triples,text/turtle;q=0.9,*/*;q=0.5",
      }, () => this.toast(asObject ? `Nothing in the endpoint points to ${label}` : `The endpoint has no triples about ${label}`));
    }

    /** Open `query` in a new editor tab and run it when an endpoint is set. */
    open(name, query, plugin, request, onEmpty) {
      const y = this.ensure();
      if (!y) return;
      const tab = y.addTab(true, {
        name: name.length > 40 ? name.slice(0, 39) + "…" : name,
        yasqe: { value: query },
        requestConfig: { ...y.config.requestConfig, ...(request || {}), endpoint: this.requestUrl() },
      });
      if (!this.endpoint) {
        this.pending = { tab, query, plugin, onEmpty };
        this.onNeedEndpoint();
        this.endpointInput.focus();
        return;
      }
      this.run(tab, query, plugin, onEmpty);
    }

    run(tab, query, plugin, onEmpty) {
      const y = this.yasgui;
      // The editor is created asynchronously; wait for it before querying.
      const attempt = (tries) => {
        const t = typeof tab.getId === "function" ? y.getTab(tab.getId()) : tab;
        if (t && t.getYasqe && t.getYasqe()) {
          t.getYasqe().setValue(query);
          const done = t.query();
          if (plugin) {
            Promise.resolve(done)
              .then(() => {
                const yasr = t.getYasr && t.getYasr();
                if (yasr && yasr.plugins && yasr.plugins[plugin]) yasr.selectPlugin(plugin);
                if (onEmpty && yasr && yasr.results && !yasr.results.getError()) {
                  const text = String(yasr.results.getOriginalResponseAsString() || "").trim();
                  if (!text || (text.startsWith("{") && /"content"\s*:\s*""/.test(text))) onEmpty();
                }
              })
              .catch(() => {});
          } else if (done && done.catch) {
            done.catch(() => {});
          }
        } else if (tries > 0) {
          setTimeout(() => attempt(tries - 1), 50);
        }
      };
      attempt(40);
    }
  }

  window.ShaclSparql = { dedent, targetPattern, targetPrefixes, coreChecks, checkQuery, shapeQuery, prepareConstraint, withPrefixes, Editor, LIMIT };
})();
