// shacl-report: the shapes explorer. Lists the shapes of the shapes graph,
// describes every constraint in plain language, links shapes to the results
// they produced, and opens SPARQL for any constraint in the query editor.
(function () {
  "use strict";

  const SH = "http://www.w3.org/ns/shacl#";
  const LIST_PAGE = 60;
  const S = () => window.ShaclSparql;

  const local = (iri) => (iri && iri.startsWith(SH) ? iri.slice(SH.length) : null);

  const ICON = {
    play: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5.5v13l10-6.5z" /></svg>',
    copy: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 00-2-2H6a2 2 0 00-2 2v8a2 2 0 002 2h2" /></svg>',
    node: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="4" width="16" height="16" rx="4" /></svg>',
    prop: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h10m0 0l-4-4m4 4l-4 4" /><circle cx="19" cy="12" r="2" /></svg>',
    code: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 7l-5 5 5 5M15 7l5 5-5 5" /></svg>',
    back: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 6l-6 6 6 6" /></svg>',
    list: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01" /></svg>',
  };

  /**
   * @param {object} ctx helpers and callbacks from app.js:
   *   els, esc, tokensToHtml, tokensToText, iriHtml, iriText, plural, sevClass,
   *   showResults(keys, label), runQuery(name, query), copy(text), prefixes()
   */
  function create(ctx) {
    const { esc, tokensToHtml, tokensToText, iriHtml, plural } = ctx;
    const st = {
      model: null,
      counts: new Map(),
      selected: null,
      query: "",
      shown: LIST_PAGE,
      history: [],
      hasReport: false,
      queries: new Map(),
    };

    // ---------- model helpers ----------

    const shape = (i) => st.model.shapes[i];
    const isInline = (s) => !s.iri && !s.path && !s.properties.length && !s.sparql.length && !s.targets.length;

    /** Result counts per shape, including the results of its property shapes. */
    function countResults(rows) {
      const own = new Map();
      for (const d of rows) {
        if (!d.shape_key) continue;
        let e = own.get(d.shape_key);
        if (!e) own.set(d.shape_key, (e = { total: 0, Violation: 0, Warning: 0, Info: 0, Other: 0 }));
        e.total += 1;
        e[d.sev] += 1;
      }
      const counts = new Map();
      for (const s of st.model.shapes) {
        const keys = [s.key, ...s.properties.map((i) => shape(i).key)];
        const e = { total: 0, Violation: 0, Warning: 0, Info: 0, Other: 0, keys };
        for (const k of keys) {
          const o = own.get(k);
          if (o) for (const f of ["total", "Violation", "Warning", "Info", "Other"]) e[f] += o[f];
        }
        counts.set(s.id, e);
      }
      return counts;
    }

    /** Targets that decide the focus nodes of `s`: its own, or its parent's. */
    function effectiveTargets(s, seen) {
      if (s.targets.length) return { targets: s.targets, from: s };
      seen = seen || new Set();
      seen.add(s.id);
      for (const p of s.parents) {
        if (seen.has(p)) continue;
        const ps = shape(p);
        if (!ps.properties.includes(s.id)) continue;
        const r = effectiveTargets(ps, seen);
        if (r) return r;
      }
      return null;
    }

    let focusVar = 0;
    /**
     * SPARQL pattern binding ?this to the focus nodes of `s`: from its own or
     * its parent's targets, or, for a shape used through sh:node or
     * sh:qualifiedValueShape, the values of the referring property shape.
     */
    function focusOf(s, depth) {
      depth = depth || 0;
      if (s.targets.length) return S().targetPattern(s.targets);
      if (depth > 4) return null;
      const via = [];
      for (const p of s.parents) {
        const ps = shape(p);
        // Property shapes share the focus nodes of their node shape.
        if (ps.properties.includes(s.id)) {
          const f = focusOf(ps, depth + 1);
          if (f) return f;
          continue;
        }
        if (!ps.path) continue;
        const refers = ps.constraints.some((c) => ["node", "qualifiedValueShape"].includes(local(c.param)) && c.values.some((v) => v.shape === s.id));
        if (!refers) continue;
        const outer = focusOf(ps, depth + 1);
        if (!outer) continue;
        const v = `?_focus${++focusVar}`;
        via.push(`{ SELECT DISTINCT ?this WHERE { ${outer.replace(/\?this\b/g, v).replace(/\n\s*/g, " ")} ${v} ${ps.path} ?this . } }`);
      }
      if (!via.length) return null;
      return via.length === 1 ? via[0] : via.map((x) => `{ ${x} }`).join("\n    UNION\n    ");
    }

    // ---------- plain-language rules ----------

    const term = (v) => `<span class="term${v.text.startsWith('"') ? " lit" : ""}">${tokensToHtml(v.text, false)}</span>`;
    const terms = (vals) => vals.map(term).join(", ");

    function shapeRef(i) {
      const s = shape(i);
      if (isInline(s)) {
        const rules = describeConstraints(s, true).map((r) => r.html);
        return `<span class="inline-shape">${rules.length ? rules.join(" and ") : "anything"}</span>`;
      }
      return `<button class="shape-link" data-shape="${i}">${esc(tokensToText(s.label))}</button>`;
    }

    // "conform to <shape>" for named shapes, "be (rule)" for inline ones.
    function shapeVerb(vals) {
      return vals.every((v) => v.shape === undefined || v.shape === null || isInline(shape(v.shape))) ? "be" : "conform to";
    }

    function rangeText(min, minEx, max, maxEx) {
      const lo = min || minEx;
      const hi = max || maxEx;
      const val = (v) => `<span class="term lit">${esc(v.sparql || tokensToText(v.text))}</span>`;
      if (lo && hi) return `Between ${val(lo)} and ${val(hi)}${min && max ? " (inclusive)" : minEx && maxEx ? " (exclusive)" : ""}`;
      if (min) return `At least ${val(min)}`;
      if (minEx) return `Greater than ${val(minEx)}`;
      if (max) return `At most ${val(max)}`;
      return `Less than ${val(maxEx)}`;
    }

    /**
     * Plain-language rules of a shape, merging related parameters
     * (min/max count, ranges, lengths, qualified counts).
     * Returns [{ params: [iri], html, kind }].
     */
    function describeConstraints(s, inline) {
      const by = new Map(s.constraints.map((c) => [local(c.param) || c.param, c]));
      const v1 = (n) => (by.get(n) ? by.get(n).values[0] : null);
      const used = new Set();
      const out = [];
      const add = (names, html, kind) => {
        names.forEach((n) => used.add(n));
        out.push({ params: names.filter((n) => by.has(n)).map((n) => by.get(n).param), html, kind: kind || "core" });
      };

      // Cardinality.
      const minC = v1("minCount");
      const maxC = v1("maxCount");
      if (minC || maxC) {
        const lo = minC ? Number(minC.sparql) : 0;
        const hi = maxC ? Number(maxC.sparql) : null;
        let t;
        if (hi === 0) t = "Must not have any value";
        else if (lo === hi) t = lo === 1 ? "Exactly one value (required)" : `Exactly ${lo} values`;
        else if (lo && hi !== null) t = `Between ${lo} and ${hi} values${lo >= 1 ? " (required)" : ""}`;
        else if (lo) t = lo === 1 ? "Required: at least one value" : `At least ${lo} values`;
        else t = hi === 1 ? "At most one value" : `At most ${hi} values`;
        add(["minCount", "maxCount"], t, "card");
      }

      // Value type.
      const are = inline ? "" : "Values are ";
      if (v1("datatype")) add(["datatype"], `${are}${inline ? "a literal" : "literals"} of type ${term(v1("datatype"))}`);
      if (v1("class")) add(["class"], `${are}${inline ? "an instance" : "instances"} of ${terms(by.get("class").values)}`);
      if (v1("nodeKind")) {
        const kinds = { IRI: "IRIs", Literal: "literals", BlankNode: "blank nodes", BlankNodeOrIRI: "IRIs or blank nodes", BlankNodeOrLiteral: "blank nodes or literals", IRIOrLiteral: "IRIs or literals" };
        const k = kinds[local(v1("nodeKind").text.replace(/^<|>$/g, "")) || ""];
        add(["nodeKind"], `${are}${k || term(v1("nodeKind"))}`);
      }

      // Ranges and lengths.
      const [mi, me, xi, xe] = ["minInclusive", "minExclusive", "maxInclusive", "maxExclusive"].map(v1);
      if (mi || me || xi || xe) add(["minInclusive", "minExclusive", "maxInclusive", "maxExclusive"], rangeText(mi, me, xi, xe), "range");
      const minL = v1("minLength");
      const maxL = v1("maxLength");
      if (minL || maxL) {
        const n = (v) => esc(v.sparql);
        add(["minLength", "maxLength"], minL && maxL ? `Between ${n(minL)} and ${n(maxL)} characters long` : minL ? `At least ${n(minL)} characters long` : `At most ${n(maxL)} characters long`, "range");
      }
      if (v1("pattern")) {
        const fl = v1("flags");
        add(["pattern", "flags"], `Matches the pattern <code class="regex">/${esc(v1("pattern").text.replace(/^"|"$/g, ""))}/${fl ? esc(fl.text.replace(/^"|"$/g, "")) : ""}</code>`);
      }

      // Lists and values.
      if (by.has("in")) add(["in"], `${inline ? "one of" : "One of:"} ${terms(by.get("in").values)}`, "list");
      if (by.has("languageIn")) add(["languageIn"], `Language is one of: ${terms(by.get("languageIn").values)}`, "list");
      if (v1("uniqueLang") && v1("uniqueLang").sparql === "true") add(["uniqueLang"], "At most one value per language");
      if (by.has("hasValue")) add(["hasValue"], `${inline ? "has the value" : "Includes the value"} ${terms(by.get("hasValue").values)}`);

      // Property pairs.
      if (v1("equals")) add(["equals"], `Same values as ${term(v1("equals"))}`, "pair");
      if (v1("disjoint")) add(["disjoint"], `No values in common with ${term(v1("disjoint"))}`, "pair");
      if (v1("lessThan")) add(["lessThan"], `Less than every value of ${term(v1("lessThan"))}`, "pair");
      if (v1("lessThanOrEquals")) add(["lessThanOrEquals"], `At most every value of ${term(v1("lessThanOrEquals"))}`, "pair");

      // Shape-based constraints.
      const shapes = (n) => (by.get(n) ? by.get(n).values.filter((v) => v.shape !== undefined && v.shape !== null).map((v) => shapeRef(v.shape)) : []);
      if (by.has("node")) add(["node"], `${s.path ? "Each value must" : "Must"} ${shapeVerb(by.get("node").values)} ${shapes("node").join(" and ")}`, "shape");
      if (by.has("qualifiedValueShape")) {
        const lo = v1("qualifiedMinCount");
        const hi = v1("qualifiedMaxCount");
        const n = lo && hi ? (lo.sparql === hi.sparql ? `Exactly ${esc(lo.sparql)}` : `Between ${esc(lo.sparql)} and ${esc(hi.sparql)}`) : lo ? `At least ${esc(lo.sparql)}` : hi ? `At most ${esc(hi.sparql)}` : "Some";
        const disj = v1("qualifiedValueShapesDisjoint") && v1("qualifiedValueShapesDisjoint").sparql === "true" ? ", not counting values that match a sibling qualified shape" : "";
        const one = (lo ? lo.sparql === "1" : true) && (hi ? hi.sparql === "1" : true);
        add(["qualifiedValueShape", "qualifiedMinCount", "qualifiedMaxCount", "qualifiedValueShapesDisjoint"], `${n} value${one ? "" : "s"} must ${shapeVerb(by.get("qualifiedValueShape").values)} ${shapes("qualifiedValueShape").join(", ")}${disj}`, "shape");
      }
      const subj = s.path ? "Each value must" : "Must";
      if (by.has("not")) add(["not"], `${subj} not ${shapeVerb(by.get("not").values)} ${shapes("not").join(" or ")}`, "logic");
      if (by.has("and")) add(["and"], `${subj} ${shapeVerb(by.get("and").values)} all of: ${shapes("and").join(" <b>and</b> ")}`, "logic");
      if (by.has("or")) add(["or"], `${subj} ${shapeVerb(by.get("or").values)} at least one of: ${shapes("or").join(" <b>or</b> ")}`, "logic");
      if (by.has("xone")) add(["xone"], `${subj} ${shapeVerb(by.get("xone").values)} exactly one of: ${shapes("xone").join(", ")}`, "logic");

      // Closed shapes.
      if (v1("closed") && v1("closed").sparql === "true") {
        const ign = by.get("ignoredProperties");
        add(["closed", "ignoredProperties"], `Closed: only the properties listed here are allowed${ign ? `, plus ${terms(ign.values)}` : ""}`);
      } else if (by.has("ignoredProperties")) {
        add(["ignoredProperties"], `Ignored properties: ${terms(by.get("ignoredProperties").values)}`);
      }

      // Custom components and anything else.
      const custom = new Map();
      for (const c of s.constraints) {
        const n = local(c.param) || c.param;
        if (used.has(n)) continue;
        if (c.component !== undefined && c.component !== null) {
          const list = custom.get(c.component) || [];
          list.push(c);
          custom.set(c.component, list);
          continue;
        }
        add([n], `${tokensToHtml(`<${c.param}>`, false)} ${terms(c.values)}`, "other");
      }
      for (const [ci, cs] of custom) {
        const comp = st.model.components[ci];
        const params = cs.map((c) => `${tokensToHtml(`<${c.param}>`, false)} = ${terms(c.values)}`).join(", ");
        out.push({ params: cs.map((c) => c.param), html: `Custom rule <b>${tokensToHtml(comp.label, false)}</b>: ${params}`, kind: "custom", component: ci });
      }
      return out;
    }

    function targetText(t) {
      const v = `<span class="term">${tokensToHtml(t.value, true)}</span>`;
      switch (t.kind) {
        case "class": return `Every instance of ${v} (including subclasses)`;
        case "implicitClass": return `Every instance of ${v}, because the shape is also a class`;
        case "node": return `The resource ${v}`;
        case "subjectsOf": return `Every resource with a value for ${v}`;
        case "objectsOf": return `Every resource that is a value of ${v}`;
        case "sparql": return "The resources selected by a SPARQL query";
        default: return esc(t.kind);
      }
    }

    // ---------- queries ----------

    let queryId = 0;
    function registerQuery(name, query) {
      const id = `q${++queryId}`;
      st.queries.set(id, { name, query });
      return id;
    }

    function queryButtons(name, query, label) {
      const id = registerQuery(name, query);
      return `<span class="q-actions"><button class="btn small" data-run="${id}" title="Open in the SPARQL editor and run it">${ICON.play}<span>${esc(label || "Run in SPARQL editor")}</span></button><button class="icon-btn small" data-copy-query="${id}" title="Copy query" aria-label="Copy query">${ICON.copy}</button></span>`;
    }

    function checkButton(s, check) {
      const q = S().withPrefixes(S().checkQuery(check), ctx.prefixes());
      const id = registerQuery(`${tokensToText(s.label)}: ${check.label}`, q);
      return `<button class="icon-btn small run-check" data-run="${id}" title="Find violations of ${esc(tokensToText(check.label))} with SPARQL" aria-label="Find violations of ${esc(tokensToText(check.label))} with SPARQL">${ICON.play}</button>`;
    }

    function sparqlConstraintQuery(s, c) {
      const focus = focusOf(s);
      const q = S().prepareConstraint(c.query, {
        path: s.path,
        shapeIri: s.iri,
        focus: /\$this\b/.test(c.query) ? focus && focus.replace(/\?this\b/g, "$this") : null,
      });
      const t = effectiveTargets(s);
      return S().withPrefixes(q, ctx.prefixes(), [...c.prefixes, ...(t ? S().targetPrefixes(t.targets) : [])]);
    }

    function validatorQuery(s, comp, v) {
      const params = {};
      for (const c of s.constraints) {
        if (c.component !== comp.id || !c.values[0]) continue;
        const name = c.param.replace(/^.*[#/]/, "");
        params[name] = c.values[0].sparql;
      }
      const focus = focusOf(s);
      let q = v.query;
      if (v.kind === "ask") {
        // ASK validators run per value node; turn them into a SELECT over all value nodes.
        const body = q.replace(/^[\s\S]*?\bASK\s*(WHERE\s*)?\{/i, "").replace(/\}\s*$/, "");
        q = `SELECT DISTINCT $this ?value\nWHERE {\n    ${s.path ? `$this $PATH ?value .` : "BIND ($this AS ?value)"}\n    FILTER NOT EXISTS {\n${body}\n    }\n}`;
      }
      q = S().prepareConstraint(q, {
        path: s.path,
        shapeIri: s.iri,
        params,
        focus: /\$this\b/.test(q) ? focus && focus.replace(/\?this\b/g, "$this") : null,
      });
      return S().withPrefixes(q, ctx.prefixes(), v.prefixes);
    }

    // ---------- rendering: list ----------

    function pills(e) {
      if (!e || !e.total) return "";
      return ["Violation", "Warning", "Info", "Other"].filter((s) => e[s]).map((s) => `<span class="pill ${ctx.sevClass[s]}"><span class="dot"></span>${e[s].toLocaleString()}</span>`).join("");
    }

    function searchText(s) {
      return `${tokensToText(s.label)} ${s.iri || ""} ${tokensToText(s.path || "")} ${s.name || ""} ${s.targets.map((t) => tokensToText(t.value)).join(" ")}`.toLowerCase();
    }

    function renderList() {
      const els = ctx.els;
      const q = st.query;
      let ids;
      if (q) {
        ids = st.model.shapes.filter((s) => !isInline(s) && searchText(s).includes(q)).map((s) => s.id);
      } else {
        ids = st.model.roots.filter((i) => !isInline(shape(i)));
      }
      const text = (i) => tokensToText(shape(i).label).toLowerCase();
      ids = ids.slice().sort((a, b) => (st.hasReport ? st.counts.get(b).total - st.counts.get(a).total : 0) || text(a).localeCompare(text(b)));
      els.shapeCount.textContent = q ? `${plural(ids.length, "match", "matches")}` : plural(ids.length, "shape");
      els.shapeList.innerHTML = "";
      if (!ids.length) {
        els.shapeList.innerHTML = '<li class="empty-facet">No shapes match</li>';
        return;
      }
      const frag = document.createDocumentFragment();
      for (const i of ids.slice(0, st.shown)) {
        const s = shape(i);
        const li = document.createElement("li");
        li.className = `shape-item${st.selected === i ? " active" : ""}${s.deactivated ? " deactivated" : ""}`;
        li.tabIndex = 0;
        li.dataset.shape = i;
        const sub = s.targets.length
          ? s.targets.map((t) => (t.kind === "sparql" ? "SPARQL target" : tokensToText(t.value))).join(", ")
          : s.path
            ? tokensToText(s.path)
            : s.parents.length ? `Used by ${plural(s.parents.length, "shape")}` : "No targets";
        const nProps = s.properties.length ? `<span class="shape-meta">${plural(s.properties.length, "property", "properties")}</span>` : "";
        li.innerHTML = `<span class="shape-kind ${s.kind}" title="${s.kind === "node" ? "Node shape" : "Property shape"}">${s.kind === "node" ? ICON.node : ICON.prop}</span>
          <span class="shape-text"><span class="shape-name">${esc(tokensToText(s.label))}</span><span class="shape-sub">${esc(sub)}</span></span>
          <span class="shape-side">${pills(st.counts.get(i))}${nProps}</span>`;
        frag.appendChild(li);
      }
      if (ids.length > st.shown) {
        const li = document.createElement("li");
        li.className = "more";
        li.textContent = `Show ${Math.min(LIST_PAGE, ids.length - st.shown)} more`;
        li.addEventListener("click", () => { st.shown += LIST_PAGE; renderList(); });
        frag.appendChild(li);
      }
      els.shapeList.appendChild(frag);
    }

    // ---------- rendering: detail ----------

    function resultsLine(s) {
      if (!st.hasReport) return "";
      const e = st.counts.get(s.id);
      if (!e || !e.total) return `<div class="callout ok-callout">No results in the report for this shape.</div>`;
      return `<div class="result-line">${pills(e)}<span>${plural(e.total, "result")} in the report</span><button class="btn small" data-results="${s.id}">${ICON.list}<span>Show results</span></button></div>`;
    }

    function rulesHtml(s) {
      const rules = describeConstraints(s);
      if (!rules.length) return "";
      const checks = S().coreChecks(s, focusOf(s), st.model);
      return `<ul class="rules">${rules
        .map((r) => {
          const c = checks.find((x) => r.params.includes(x.param));
          const btns = r.params
            .map((p) => checks.filter((x) => x.param === p))
            .flat()
            .map((x) => checkButton(s, x));
          const unique = [...new Set(btns)].slice(0, 2).join("");
          return `<li class="rule rule-${r.kind}"><span class="rule-text">${r.html}</span>${c ? unique : ""}</li>`;
        })
        .join("")}</ul>`;
    }

    function sparqlHtml(s) {
      if (!s.sparql.length) return "";
      return s.sparql
        .map((c, n) => {
          const q = sparqlConstraintQuery(s, c);
          const msg = c.messages.length ? `<p class="sq-msg">${c.messages.map(esc).join("<br>")}</p>` : "";
          return `<div class="sparql-card${c.deactivated ? " deactivated" : ""}">
            <div class="sq-head"><span class="badge">${ICON.code} SPARQL ${c.kind.toUpperCase()} constraint${s.sparql.length > 1 ? ` ${n + 1}` : ""}</span>${c.deactivated ? '<span class="badge muted">deactivated</span>' : ""}${queryButtons(`${tokensToText(s.label)}: SPARQL constraint`, q)}</div>
            ${msg}
            <pre class="query">${esc(S().dedent(c.query))}</pre>
          </div>`;
        })
        .join("");
    }

    function customHtml(s) {
      const comps = [...new Set(s.constraints.filter((c) => c.component !== undefined && c.component !== null).map((c) => c.component))];
      return comps
        .map((ci) => {
          const comp = st.model.components[ci];
          const validators = comp.validators.filter((v) => v.role === "validator" || v.role === (s.path ? "propertyValidator" : "nodeValidator"));
          return validators
            .map((v) => {
              const q = validatorQuery(s, comp, v);
              return `<div class="sparql-card">
                <div class="sq-head"><span class="badge">${ICON.code} ${esc(tokensToText(comp.label))} · SPARQL ${v.kind.toUpperCase()} validator</span>${queryButtons(`${tokensToText(s.label)}: ${tokensToText(comp.label)}`, q)}</div>
                ${v.messages.length ? `<p class="sq-msg">${v.messages.map(esc).join("<br>")}</p>` : ""}
                <pre class="query">${esc(S().dedent(v.query))}</pre>
              </div>`;
            })
            .join("");
        })
        .join("");
    }

    function propertyCard(i) {
      const p = shape(i);
      const e = st.counts.get(i);
      const title = p.name ? `${esc(p.name)} <span class="faint">${tokensToHtml(p.path || "", false)}</span>` : tokensToHtml(p.path || tokensToText(p.label), false);
      const own = p.iri ? `<button class="shape-link small" data-shape="${i}">${esc(tokensToText(p.iri ? `<${p.iri}>` : p.label))}</button>` : "";
      const sev = p.severity && p.severity !== "Violation" ? `<span class="pill ${ctx.sevClass[p.severity] || "sev-other"}">${esc(p.severity)}</span>` : "";
      const counts = st.hasReport && e && e.total ? `<button class="count-link" data-results="${i}" title="Show these results">${pills(e)}</button>` : "";
      const msg = p.messages.length ? `<p class="prop-msg">“${p.messages.map(esc).join("” · “")}”</p>` : "";
      const desc = p.description ? `<p class="prop-desc">${esc(p.description)}</p>` : "";
      const nested = p.properties.length ? `<p class="prop-nested">Has ${plural(p.properties.length, "nested property shape")}: <button class="shape-link small" data-shape="${i}">open</button></p>` : "";
      return `<div class="prop-card${p.deactivated ? " deactivated" : ""}" id="shape-${i}">
        <div class="prop-head"><span class="prop-title">${title}</span><span class="prop-side">${sev}${p.deactivated ? '<span class="badge muted">deactivated</span>' : ""}${counts}${own}</span></div>
        ${desc}${rulesHtml(p) || '<p class="faint small-text">No constraints.</p>'}${msg}${nested}${sparqlHtml(p)}${customHtml(p)}
      </div>`;
    }

    function renderDetail() {
      const els = ctx.els;
      const i = st.selected;
      if (i === null || i === undefined) {
        els.shapeDetail.innerHTML = `<div class="empty-results"><h3>Select a shape</h3><p>Pick a shape on the left to see its targets, every rule it checks, and SPARQL to find violations in your data.</p></div>`;
        return;
      }
      const s = shape(i);
      st.queries = new Map();
      let h = "";
      if (st.history.length) h += `<button class="link-btn back-link" data-back>${ICON.back} Back</button>`;
      h += `<div class="sd-head"><span class="shape-kind ${s.kind} big">${s.kind === "node" ? ICON.node : ICON.prop}</span><div class="sd-title-wrap"><h2 class="sd-title">${esc(tokensToText(s.label))}</h2>`;
      h += `<div class="sd-sub"><span>${s.kind === "node" ? "Node shape" : "Property shape"}</span>`;
      if (s.iri) h += `<span>${iriHtml(s.iri, true)}</span><button class="icon-btn small" data-copy="${esc(s.iri)}" title="Copy IRI" aria-label="Copy IRI">${ICON.copy}</button>`;
      if (s.severity) h += `<span class="pill ${ctx.sevClass[s.severity] || "sev-other"}">${esc(s.severity)}</span>`;
      if (s.deactivated) h += `<span class="badge muted">deactivated</span>`;
      h += `</div></div></div>`;
      if (s.description) h += `<p class="sd-desc">${esc(s.description)}</p>`;
      if (s.messages.length) h += `<p class="d-msg">${s.messages.map(esc).join("<br>")}</p>`;
      h += resultsLine(s);

      // Targets.
      h += `<section class="sd-section"><h3>Applies to</h3>`;
      if (s.targets.length) {
        h += `<ul class="targets">${s.targets
          .map((t) => {
            let li = `<li><span>${targetText(t)}</span>`;
            if (t.kind === "sparql") {
              const q = S().withPrefixes(t.value.trim(), ctx.prefixes(), t.prefixes);
              li += `${queryButtons(`${tokensToText(s.label)}: target`, q, "List these resources")}<pre class="query">${esc(t.value.trim())}</pre>`;
            }
            return li + "</li>";
          })
          .join("")}</ul>`;
        const focus = S().targetPattern(s.targets);
        const q = S().withPrefixes(`SELECT DISTINCT ?this\nWHERE {\n    ${focus.replace(/\n/g, "\n    ")}\n}\nLIMIT ${S().LIMIT}`, ctx.prefixes(), S().targetPrefixes(s.targets));
        h += `<div class="sd-actions">${queryButtons(`${tokensToText(s.label)}: focus nodes`, q, "List focus nodes")}</div>`;
      } else {
        const t = effectiveTargets(s);
        if (t && t.from !== s) {
          h += `<p>The focus nodes of ${shapeRef(t.from.id)}:</p><ul class="targets">${t.targets.map((x) => `<li>${targetText(x)}</li>`).join("")}</ul>`;
        } else if (s.parents.length) {
          h += `<p>No targets of its own. It is checked on the values of ${s.parents.map((p) => shapeRef(p)).join(", ")}.</p>`;
          const f = focusOf(s);
          if (f) {
            const q = S().withPrefixes(`SELECT DISTINCT ?this\nWHERE {\n    ${f.replace(/\n/g, "\n    ")}\n}\nLIMIT ${S().LIMIT}`, ctx.prefixes());
            h += `<div class="sd-actions">${queryButtons(`${tokensToText(s.label)}: focus nodes`, q, "List focus nodes")}</div>`;
          }
        } else {
          h += `<p class="faint">No targets, and no other shape refers to it, so validators never check it.</p>`;
        }
      }
      h += `</section>`;

      // Value nodes.
      if (s.path) {
        h += `<section class="sd-section"><h3>Property path</h3><p><span class="term">${tokensToHtml(s.path, true)}</span></p></section>`;
      }

      // Own rules.
      const own = rulesHtml(s);
      const sparqlOwn = sparqlHtml(s) + customHtml(s);
      if (own || sparqlOwn) {
        h += `<section class="sd-section"><h3>${s.path ? "Rules for each value" : "Rules for the resource itself"}</h3>${own}${sparqlOwn}</section>`;
      }

      // Property shapes.
      if (s.properties.length) {
        h += `<section class="sd-section"><h3>Properties <span class="faint">${s.properties.length}</span></h3>${s.properties.map(propertyCard).join("")}</section>`;
      }

      // Whole-shape query.
      const all = S().shapeQuery(s, st.model, focusOf, tokensToText);
      if (all) {
        h += `<section class="sd-section"><h3>Check with SPARQL</h3><p class="small-text">One query that lists the violations of every SHACL Core rule above, straight from your triple store. Nested shapes and logical rules (and, or, not, xone, qualified shapes) are left to the validator${s.sparql.length ? "; the SPARQL constraints have their own buttons" : ""}.</p>${queryButtons(`${tokensToText(s.label)}: all rules`, S().withPrefixes(all, ctx.prefixes()), "Find all violations")}</section>`;
      }

      // Used by.
      if (s.parents.length) {
        h += `<section class="sd-section"><h3>Used by</h3><p>${s.parents.map((p) => shapeRef(p)).join(", ")}</p></section>`;
      }
      els.shapeDetail.innerHTML = h;
      els.shapeDetail.scrollTop = 0;
    }

    // ---------- events ----------

    function select(i, opts) {
      if (st.selected !== null && st.selected !== i && !(opts && opts.noHistory)) st.history.push(st.selected);
      // Property shapes without a name of their own open on their parent, scrolled into view.
      const s = shape(i);
      if (s.kind === "property" && !s.iri && s.parents.length && !s.properties.length && !(opts && opts.exact)) {
        const parent = s.parents[0];
        st.selected = parent;
        renderList();
        renderDetail();
        const card = document.getElementById(`shape-${i}`);
        if (card) {
          card.scrollIntoView({ block: "center" });
          card.classList.add("flash");
          setTimeout(() => card.classList.remove("flash"), 1400);
        }
        return;
      }
      st.selected = i;
      renderList();
      renderDetail();
      ctx.els.shapesView.classList.add("detail-open");
    }

    function bind() {
      const els = ctx.els;
      let timer = null;
      els.shapeSearch2.addEventListener("input", () => {
        clearTimeout(timer);
        timer = setTimeout(() => {
          st.query = els.shapeSearch2.value.trim().toLowerCase();
          st.shown = LIST_PAGE;
          renderList();
        }, 100);
      });
      const pick = (e) => {
        const li = e.target.closest("[data-shape]");
        if (li) select(Number(li.dataset.shape));
      };
      els.shapeList.addEventListener("click", pick);
      els.shapeList.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " ") { e.preventDefault(); pick(e); }
      });
      els.shapeDetail.addEventListener("click", (e) => {
        const link = e.target.closest("[data-shape]");
        if (link) { select(Number(link.dataset.shape), { exact: link.classList.contains("small") }); return; }
        const run = e.target.closest("[data-run]");
        if (run) {
          const q = st.queries.get(run.dataset.run);
          if (q) ctx.runQuery(q.name, q.query);
          return;
        }
        const cp = e.target.closest("[data-copy-query]");
        if (cp) {
          const q = st.queries.get(cp.dataset.copyQuery);
          if (q) ctx.copy(q.query);
          return;
        }
        const c = e.target.closest("[data-copy]");
        if (c) { ctx.copy(c.dataset.copy); return; }
        const r = e.target.closest("[data-results]");
        if (r) {
          const s = shape(Number(r.dataset.results));
          const e2 = st.counts.get(s.id);
          ctx.showResults(new Set(e2.keys), tokensToText(s.label));
          return;
        }
        if (e.target.closest("[data-back]")) {
          const prev = st.history.pop();
          if (prev !== undefined) select(prev, { noHistory: true, exact: true });
        }
      });
      els.shapesBack.addEventListener("click", () => els.shapesView.classList.remove("detail-open"));
    }

    // ---------- public ----------

    let bound = false;
    return {
      setModel(model, rows, hasReport) {
        st.model = model;
        st.hasReport = hasReport;
        st.counts = countResults(rows);
        st.selected = null;
        st.history = [];
        st.query = "";
        st.shown = LIST_PAGE;
        ctx.els.shapeSearch2.value = "";
        if (!bound) { bind(); bound = true; }
        renderList();
        // Start on the shape with the most results, else the first one.
        const firstItem = ctx.els.shapeList.querySelector("[data-shape]");
        if (firstItem) {
          st.selected = Number(firstItem.dataset.shape);
          renderList();
        }
        renderDetail();
      },
      /** Open the shape a result came from. */
      showKey(key) {
        const s = st.model && st.model.shapes.find((x) => x.key === key);
        if (s) select(s.id);
        return !!s;
      },
      describeConstraints,
    };
  }

  window.ShaclShapes = { create };
})();
