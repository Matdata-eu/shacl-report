// shacl-report: the coverage view. Shows which resources of the data the
// shapes look at (by class) and which of their property values a rule
// checks, broken down by class, property and constraint category.
(function () {
  "use strict";

  const XSD = "http://www.w3.org/2001/XMLSchema#";
  const RDF_LANG = "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString";
  const S = () => window.ShaclSparql;

  const HOW = {
    path: ["Property path", "ok"],
    inverse: ["Inverse path", "info"],
    sparql: ["SPARQL (estimated)", "info"],
  };

  const ICON = {
    right: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>',
    info: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 11v5M12 8h.01" /></svg>',
    code: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 7l-5 5 5 5M15 7l5 5-5 5" /></svg>',
    copy: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 00-2-2H6a2 2 0 00-2 2v8a2 2 0 002 2h2" /></svg>',
  };

  /**
   * @param {object} ctx helpers and callbacks from app.js:
   *   els, esc, iriHtml, iriText, termHtml, tokensToHtml, plural, copy(text),
   *   prefixes(), shapeLabel(key), showShape(key), runQuery(name, query)
   */
  function create(ctx) {
    const { esc, iriHtml, iriText, termHtml, plural } = ctx;
    const el = ctx.els;
    const st = {
      report: null,
      cov: null,
      view: "classes",
      open: new Set(),
      selected: null,
      gapsOnly: false,
      ignoreAnnotations: false,
      query: "",
      hintsHidden: false,
    };
    let bound = false;

    // ---------- numbers ----------

    const pct = (part, whole) => (whole ? (part * 100) / whole : 100);
    const pctText = (part, whole) => {
      if (!whole) return "–";
      const p = pct(part, whole);
      if (part < whole && p > 99.5) return "99.5 %";
      if (part > 0 && p < 0.5) return "<1 %";
      return `${Math.round(p)} %`;
    };
    const num = (n) => n.toLocaleString();

    const props = (row) => row.properties.filter((p) => !(st.ignoreAnnotations && p.annotation));

    function sums(row) {
      const ps = props(row);
      const s = { triples: 0, covered: 0, cats: st.cov.categories.map(() => 0) };
      for (const p of ps) {
        s.triples += p.triples;
        s.covered += p.covered;
        p.categories.forEach((n, i) => (s.cats[i] += n));
      }
      return s;
    }

    function totals() {
      const t = st.cov.totals;
      const triples = st.ignoreAnnotations ? t.triples - t.annotation_triples : t.triples;
      const covered = st.ignoreAnnotations ? t.covered - t.annotation_covered : t.covered;
      return { ...t, triples, covered };
    }

    // ---------- names ----------

    function rowName(row) {
      if (row.iri) return iriText(row.iri);
      if (row.via) return `(blank nodes) ${row.via.class ? iriText(row.via.class) + " › " : "› "}${iriText(row.via.predicate)}`;
      return row.kind === "untyped" ? "(no type)" : "(blank nodes)";
    }

    function rowNameHtml(row) {
      if (row.iri) return iriHtml(row.iri, false);
      return esc(rowName(row));
    }

    function rowSub(row) {
      if (row.kind === "untyped") return "IRIs without rdf:type";
      if (row.kind === "blank") return row.via ? "untyped values of this property" : "untyped blank nodes";
      if (!row.shapes.length) return "no shape applies";
      const first = ctx.tokensToText(ctx.shapeLabel(row.shapes[0].shape));
      return row.targeted ? `targeted by ${first}${row.shapes.length > 1 ? ` +${row.shapes.length - 1}` : ""}` : `reached through ${first}`;
    }

    const shapeLink = (key) => `<button class="link-btn cov-shape" data-shape-key="${esc(key)}">${ctx.tokensToHtml(ctx.shapeLabel(key), false)}</button>`;

    // ---------- tree ----------

    /** Rows in tree order with their depth; subclasses under their parent. */
    function treeRows() {
      const rows = st.cov.classes;
      const byKey = new Map(rows.map((r) => [r.key, r]));
      const kids = new Map();
      const roots = [];
      for (const r of rows) {
        const parent = r.parents.find((p) => byKey.has(p));
        if (parent) {
          if (!kids.has(parent)) kids.set(parent, []);
          kids.get(parent).push(r);
        } else roots.push(r);
      }
      const out = [];
      const seen = new Set();
      const walk = (r, depth) => {
        if (seen.has(r.key)) return;
        seen.add(r.key);
        out.push({ row: r, depth });
        for (const k of kids.get(r.key) || []) walk(k, depth + 1);
      };
      for (const r of roots) walk(r, 0);
      for (const r of rows) walk(r, 0);
      return out;
    }

    function hasGap(row) {
      const s = sums(row);
      return row.uncovered > 0 || s.covered < s.triples;
    }

    // ---------- rendering ----------

    function bar(parts) {
      return `<div class="cov-bar">${parts.map(([cls, w]) => (w > 0 ? `<span class="${cls}" style="width:${w}%"></span>` : "")).join("")}</div>`;
    }

    function squares(counts, total, exact) {
      return `<span class="cov-squares">${st.cov.categories
        .map((c, i) => {
          const n = counts[i];
          const p = pct(n, total);
          const title = `${c.label}: ${exact ? (n ? "yes" : "no") : pctText(n, total)}`;
          if (!n) return `<span class="sq empty" title="${esc(title)}"></span>`;
          const mix = exact ? 100 : Math.round(30 + p * 0.7);
          return `<span class="sq" style="background:color-mix(in srgb, var(--accent) ${mix}%, var(--surface-3))" title="${esc(title)}"></span>`;
        })
        .join("")}</span>`;
    }

    function renderTiles() {
      const t = totals();
      const covered = t.targeted + t.reached;
      const noRule = st.cov.classes.filter((r) => r.targeted + r.reached === 0);
      const noRuleRes = noRule.reduce((a, r) => a + r.resources, 0);
      const names = noRule.slice(0, 3).map(rowName).join(", ") + (noRule.length > 3 ? ` and ${noRule.length - 3} more` : "");
      return `<div class="cov-tiles">
        <div class="card cov-tile">
          <span class="cov-kicker">Resources covered</span>
          <div class="cov-big"><strong>${pctText(covered, t.resources)}</strong><span>${num(covered)} of ${num(t.resources)}</span></div>
          ${bar([["t", pct(t.targeted, t.resources)], ["r", pct(t.reached, t.resources)], ["u", pct(t.uncovered, t.resources)]])}
          <div class="cov-legend"><span><i class="t"></i>Targeted ${num(t.targeted)}</span><span><i class="r"></i>Reached ${num(t.reached)}</span><span><i class="u"></i>Not covered ${num(t.uncovered)}</span></div>
        </div>
        <div class="card cov-tile">
          <span class="cov-kicker">Property values checked</span>
          <div class="cov-big"><strong>${pctText(t.covered, t.triples)}</strong><span>${num(t.covered)} of ${num(t.triples)}</span></div>
          ${bar([["f", pct(t.covered, t.triples)]])}
          <span class="cov-note">Triples that at least one rule checks</span>
        </div>
        <button class="card cov-tile cov-tile-btn" data-action="gaps"${noRule.length ? "" : " disabled"}>
          <span class="cov-kicker">Classes without rules</span>
          <div class="cov-big"><strong class="${noRule.length ? "gap" : ""}">${num(noRule.length)}</strong>${noRule.length ? `<span>${plural(noRuleRes, "resource")}</span>` : ""}</div>
          <span class="cov-note">${noRule.length ? esc(names) : "Every class is looked at by a shape"}</span>
        </button>
        <button class="card cov-tile cov-tile-btn" data-action="unused"${st.cov.unused.length ? "" : " disabled"}>
          <span class="cov-kicker">Rules that match nothing</span>
          <div class="cov-big"><strong>${num(st.cov.unused.length)}</strong></div>
          <span class="cov-note">${st.cov.unused.length ? "Dead shapes, or a typo in a path" : "Every rule applies to some data"}</span>
        </button>
      </div>`;
    }

    function renderHints() {
      if (st.hintsHidden || !st.cov.hints.length) return "";
      return st.cov.hints
        .map(
          (h) => `<div class="cov-hint">${ICON.info}<div class="cov-hint-text"><strong>${plural(h.resources, "resource")} of ${iriHtml(h.class, false)} could be checked by ${ctx.tokensToHtml(ctx.shapeLabel(h.shape), false)}</strong>
          <span><code>${esc(iriText(h.class))} rdfs:subClassOf ${esc(iriText(h.superclass))}</code> is in the shapes graph, but SHACL only follows subclasses that are in the data graph. Add the triple (or the inferred types) to the data to have them checked.</span></div>
          <div class="cov-hint-actions"><button class="btn small" data-select-class="${esc("<" + h.class + ">")}">Show</button><button class="icon-btn" data-action="hide-hints" aria-label="Dismiss">${'<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>'}</button></div></div>`
        )
        .join("");
    }

    function matchesQuery(row) {
      const q = st.query;
      if (!q) return { row: true, props: null };
      const name = rowName(row).toLowerCase();
      if (name.includes(q) || (row.iri || "").toLowerCase().includes(q)) return { row: true, props: null };
      const ps = props(row).filter((p) => p.iri.toLowerCase().includes(q) || iriText(p.iri).toLowerCase().includes(q));
      return ps.length ? { row: true, props: ps } : { row: false };
    }

    function howChips(p) {
      if (!p.covered) {
        if (p.declared) return `<span class="chip-s gap">Declared, no constraints</span>`;
        return `<span class="chip-s gap">No rule</span>`;
      }
      const chips = p.how.map((h) => `<span class="chip-s ${HOW[h][1]}">${HOW[h][0]}</span>`);
      if (p.covered < p.triples) chips.push(`<span class="chip-s gap">Some resources</span>`);
      return chips.join("");
    }

    function renderClasses() {
      let h = `<div class="card cov-table"><div class="cov-scroll"><div class="cov-grid-wrap">`;
      h += `<div class="cov-row cov-head"><span>Class</span><span>Resources</span><span>Resources covered</span><span>Values checked</span><span>Rule categories</span><span>Covered by</span></div>`;
      let shown = 0;
      for (const { row, depth } of treeRows()) {
        const m = matchesQuery(row);
        if (!m.row) continue;
        if (st.gapsOnly && !hasGap(row)) continue;
        shown++;
        const s = sums(row);
        const open = st.open.has(row.key) || !!m.props;
        const sel = st.selected && st.selected.key === row.key && !st.selected.prop;
        const covered = row.targeted + row.reached;
        const resText = row.uncovered === 0 ? (row.targeted ? "all targeted" : "all reached via a nested shape") : covered ? `${num(row.uncovered)} not covered` : "none covered";
        h += `<div class="cov-class${open ? " open" : ""}">`;
        h += `<div class="cov-row cov-body${sel ? " selected" : ""}" data-key="${esc(row.key)}">
          <span class="cov-name" style="padding-left:${depth * 22}px">
            <button class="icon-btn cov-toggle" data-toggle="${esc(row.key)}" aria-expanded="${open}" aria-label="${open ? "Hide" : "Show"} properties of ${esc(rowName(row))}">${ICON.right}</button>
            ${depth ? '<span class="faint cov-sub-mark">↳</span>' : ""}
            <span class="cov-name-text"><button class="cov-link" data-select="${esc(row.key)}">${rowNameHtml(row)}</button><span class="cov-subline">${esc(rowSub(row))}${row.closed ? ` · ${num(row.closed)} in a closed shape` : ""}</span></span>
          </span>
          <span class="num">${num(row.resources)}</span>
          <span class="cov-cell">${bar([["t", pct(row.targeted, row.resources)], ["r", pct(row.reached, row.resources)], ["u", pct(row.uncovered, row.resources)]])}<span class="cov-note">${resText}</span></span>
          <span class="cov-cell">${bar([["f", pct(s.covered, s.triples)]])}<span class="cov-note">${pctText(s.covered, s.triples)} of ${num(s.triples)}</span></span>
          <span>${squares(s.cats, s.triples, false)}</span>
          <span class="cov-shapes">${row.shapes.slice(0, 2).map((x) => esc(ctx.tokensToText(ctx.shapeLabel(x.shape)))).join(", ") || '<span class="faint">—</span>'}${row.shapes.length > 2 ? ` +${row.shapes.length - 2}` : ""}</span>
        </div>`;
        if (open) {
          let ps = m.props || props(row);
          if (st.gapsOnly) ps = ps.filter((p) => p.covered < p.triples);
          h += `<div class="cov-row cov-subhead"><span>Property</span><span>Used by</span><span>How it is checked</span><span>Values checked</span><span>Categories</span><span>Shape</span></div>`;
          if (!ps.length) h += `<div class="cov-empty-props">${row.properties.length ? "No properties match" : "These resources have no properties besides their type"}</div>`;
          for (const p of ps) {
            const psel = st.selected && st.selected.key === row.key && st.selected.prop === p.iri;
            h += `<div class="cov-row cov-prop${psel ? " selected" : ""}">
              <span class="cov-name cov-prop-name"><button class="cov-link" data-select="${esc(row.key)}" data-prop="${esc(p.iri)}">${iriHtml(p.iri, false)}</button>${p.annotation ? '<span class="chip-s neutral">annotation</span>' : ""}</span>
              <span class="num">${num(p.resources)}${p.triples > p.resources ? `<span class="faint"> · ${num(p.triples)} values</span>` : ""}</span>
              <span class="cov-chips">${howChips(p)}</span>
              <span class="cov-cell">${bar([["f", pct(p.covered, p.triples)]])}<span class="cov-note">${pctText(p.covered, p.triples)}</span></span>
              <span>${squares(p.categories, p.covered, true)}</span>
              <span class="cov-shapes">${p.shapes.slice(0, 1).map((k) => esc(ctx.tokensToText(ctx.shapeLabel(k)))).join("") || '<span class="faint">—</span>'}${p.shapes.length > 1 ? ` +${p.shapes.length - 1}` : ""}</span>
            </div>`;
          }
        }
        h += `</div>`;
      }
      if (!shown) h += `<div class="cov-empty-props">Nothing matches. Clear the search or turn off “Only show gaps”.</div>`;
      h += `</div></div></div>`;
      return h;
    }

    function renderCategories() {
      const cats = st.cov.categories;
      const t = totals();
      const all = cats.map(() => 0);
      for (const r of st.cov.classes) sums(r).cats.forEach((n, i) => (all[i] += n));
      let h = `<div class="card cov-cat-summary"><div class="cov-cat-head"><h2>Which kinds of rules check your data</h2><span class="muted">Share of all property values checked by at least one rule of that kind</span></div><div class="cov-cat-bars">`;
      // Class rows can share triples (resources with several types), so the
      // overall share is an upper bound when that happens.
      cats.forEach((c, i) => {
        const n = Math.min(all[i], t.triples);
        h += `<div class="cov-cat-bar" title="${esc(c.components)}"><div class="cov-cat-line"><span>${esc(c.label)}</span><strong>${pctText(n, t.triples)}</strong></div>${bar([["f", pct(n, t.triples)]])}</div>`;
      });
      h += `</div></div>`;
      h += `<div class="card cov-table"><div class="cov-scroll"><div class="cov-grid-wrap">`;
      h += `<div class="cov-mx cov-head"><span>Class</span>${cats.map((c) => `<span class="cov-mx-h" title="${esc(c.components)}">${esc(c.label)}</span>`).join("")}<span class="cov-mx-h">Any rule</span></div>`;
      for (const { row, depth } of treeRows()) {
        if (!matchesQuery(row).row) continue;
        if (st.gapsOnly && !hasGap(row)) continue;
        const s = sums(row);
        const sel = st.selected && st.selected.key === row.key && !st.selected.prop;
        h += `<div class="cov-mx cov-body${sel ? " selected" : ""}"><span class="cov-name" style="padding-left:${depth * 18}px"><span class="cov-name-text"><button class="cov-link" data-select="${esc(row.key)}">${rowNameHtml(row)}</button><span class="cov-subline">${plural(s.triples, "value")}</span></span></span>`;
        cats.forEach((c, i) => {
          const n = s.cats[i];
          if (!n) {
            h += `<button class="cov-cell-mx zero" data-select="${esc(row.key)}" title="${esc(c.label)}: no rule">–</button>`;
          } else {
            const p = pct(n, s.triples);
            const mix = Math.round(18 + p * 0.82);
            h += `<button class="cov-cell-mx${mix > 60 ? " hi" : ""}" data-select="${esc(row.key)}" style="background:color-mix(in srgb, var(--accent) ${mix}%, var(--surface))" title="${esc(c.label)}: ${pctText(n, s.triples)} of the values">${pctText(n, s.triples)}</button>`;
          }
        });
        h += `<strong class="num cov-any">${pctText(s.covered, s.triples)}</strong></div>`;
      }
      h += `</div></div></div>`;
      h += `<p class="cov-foot">Read a row as: “of all values of this class, this share is checked by a rule of that kind”. A value checked by several kinds of rules counts in each column. An empty cell means no rule of that kind touches the class.</p>`;
      return h;
    }

    function renderUnused() {
      const u = st.cov.unused;
      let h = `<section id="cov-unused" class="card cov-unused"><div class="cov-cat-head"><h2>Rules that match nothing</h2><span class="muted">Shapes without focus nodes, and paths that never have a value</span></div>`;
      if (!u.length) return h + `<p class="muted cov-unused-none">Every rule applies to some of the data.</p></section>`;
      for (const r of u) {
        let text, chip;
        if (r.kind === "no-focus") {
          text = "Its targets match nothing in the data.";
          chip = `<span class="chip-s neutral">No focus nodes</span>`;
        } else if (r.kind === "no-values") {
          text = `None of its focus nodes has a value for this path.${r.suggestion ? ` Did you mean ${iriHtml(r.suggestion, false)}? The data uses that one.` : ""}`;
          chip = r.suggestion ? `<span class="chip-s gap">Probably a typo</span>` : `<span class="chip-s neutral">No values</span>`;
        } else {
          text = "It uses SPARQL-based targets (sh:target), which shacl-report does not evaluate. Coverage may be too low.";
          chip = `<span class="chip-s info">Targets not evaluated</span>`;
        }
        h += `<div class="cov-unused-row"><span class="cov-unused-name">${shapeLink(r.shape)}</span><span class="cov-unused-text muted">${text}</span>${chip}</div>`;
      }
      return h + `</section>`;
    }

    function render() {
      if (!st.cov) return;
      const t = st.cov.totals;
      el.coverageAnnot.checked = st.ignoreAnnotations;
      el.coverageGaps.checked = st.gapsOnly;
      el.coverageAnnotCount.textContent = num(t.annotation_triples);
      const ig = st.cov.ignored;
      el.coverageIgnored.innerHTML = [
        ["rdf:type triples", ig.rdf_type],
        ["RDF list cells", ig.list_cells],
        ["Ontology and shapes in the data", ig.schema + ig.shapes_graph],
        ["Left out on the command line", ig.user + ig.annotations],
      ]
        .filter(([, n], i) => n || i < 3)
        .map(([l, n]) => `<li><span>${esc(l)}</span><span class="num">${num(n)}</span></li>`)
        .join("");
      el.coverageCats.innerHTML = st.cov.categories.map((c, i) => `<li><span class="cov-cat-n">${i + 1}</span><span><strong>${esc(c.label)}</strong><span class="cov-note">${esc(c.components)}</span></span></li>`).join("");
      let h = renderTiles();
      h += renderHints();
      h += `<div class="toolbar cov-toolbar"><div class="search">${'<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6" /><path d="M20 20l-4.5-4.5" /></svg>'}<input id="cov-search" type="search" placeholder="Find a class or property…" aria-label="Find a class or property" value="${esc(st.query)}" /></div>
        <div class="toolbar-group"><span class="toolbar-label">Break down by</span><div class="segmented" role="radiogroup" aria-label="Break down by">
        <button data-view="classes" role="radio" class="${st.view === "classes" ? "active" : ""}" aria-checked="${st.view === "classes"}">Class</button>
        <button data-view="categories" role="radio" class="${st.view === "categories" ? "active" : ""}" aria-checked="${st.view === "categories"}">Rule category</button></div></div></div>`;
      h += st.view === "classes" ? renderClasses() : renderCategories();
      h += renderUnused();
      const focused = document.activeElement && document.activeElement.id === "cov-search";
      const caret = focused ? document.activeElement.selectionStart : null;
      el.coverageContent.innerHTML = h;
      if (focused) {
        const input = document.getElementById("cov-search");
        input.focus();
        if (caret !== null) input.setSelectionRange(caret, caret);
      }
      renderDrawer();
    }

    // ---------- details drawer ----------

    function findRow(key) {
      return st.cov.classes.find((r) => r.key === key);
    }

    function starterProperty(row, p) {
      const shape = row.shapes.length ? ctx.tokensToText(ctx.shapeLabel(row.shapes[0].shape)) : row.iri ? shapeNameFor(row.iri) : "ex:NewShape";
      const objs = p.examples.map((e) => e.o);
      let typeLine;
      if (objs.length && objs.every((o) => o.t === "iri")) typeLine = "  sh:nodeKind sh:IRI ;";
      else if (objs.length && objs.every((o) => o.t === "bnode")) typeLine = "  sh:nodeKind sh:BlankNode ;";
      else {
        const dts = new Set(objs.map((o) => (o.lng ? RDF_LANG : o.dt || XSD + "string")));
        typeLine = dts.size === 1 ? `  sh:datatype ${iriText([...dts][0])} ;` : "  # values of several types: add sh:datatype or sh:class";
      }
      const lines = [`${shape} sh:property [`, `  sh:path ${iriText(p.iri)} ;`, typeLine];
      if (p.triples <= p.resources) lines.push("  sh:maxCount 1 ;  # never more than one today");
      if (row.resources && p.resources === row.resources) lines.push("  sh:minCount 1 ;  # every resource has one today");
      lines.push("] .");
      return lines.join("\n");
    }

    function starterClass(row) {
      if (!row.iri) return null;
      const name = shapeNameFor(row.iri);
      const lines = [`${name} a sh:NodeShape ;`, `  sh:targetClass ${iriText(row.iri)} ;`];
      const ps = props(row).filter((p) => !p.annotation).slice(0, 6);
      ps.forEach((p, i) => {
        const objs = p.examples.map((e) => e.o);
        const checks = [];
        if (objs.length && objs.every((o) => o.t === "iri")) checks.push("sh:nodeKind sh:IRI");
        else if (objs.length && objs.every((o) => o.t === "lit")) {
          const dts = new Set(objs.map((o) => (o.lng ? RDF_LANG : o.dt || XSD + "string")));
          if (dts.size === 1) checks.push(`sh:datatype ${iriText([...dts][0])}`);
        }
        if (p.resources === row.resources) checks.push("sh:minCount 1");
        if (p.triples <= p.resources) checks.push("sh:maxCount 1");
        lines.push("  sh:property [", `    sh:path ${iriText(p.iri)} ;`, ...checks.map((c) => `    ${c} ;`), `  ]${i === ps.length - 1 ? " ." : " ;"}`);
      });
      if (!ps.length) lines[1] = lines[1].replace(/ ;$/, " .");
      return lines.join("\n");
    }

    /** A name for a new shape of class `iri`: `ex:Car` gives `ex:CarShape`. */
    function shapeNameFor(iri) {
      const short = iriText(iri);
      return short.startsWith("<") ? `<${iri}Shape>` : `${short}Shape`;
    }

    function classQuery(row) {
      const T = S();
      if (row.iri) return T.withPrefixes(`SELECT DISTINCT ?this\nWHERE {\n    ?this a <${row.iri}> .\n}\nLIMIT ${T.LIMIT}`, ctx.prefixes());
      if (row.via) {
        const cls = row.via.class ? `\n    ?parent a <${row.via.class}> .` : "";
        return T.withPrefixes(`SELECT DISTINCT ?this\nWHERE {${cls}\n    ?parent <${row.via.predicate}> ?this .\n    FILTER (isBlank(?this))\n    FILTER NOT EXISTS { ?this a ?type }\n}\nLIMIT ${T.LIMIT}`, ctx.prefixes());
      }
      return T.withPrefixes(`SELECT DISTINCT ?this\nWHERE {\n    ?this ?p ?o .\n    FILTER (isIRI(?this))\n    FILTER NOT EXISTS { ?this a ?type }\n}\nLIMIT ${T.LIMIT}`, ctx.prefixes());
    }

    function propQuery(row, p) {
      const T = S();
      const sel = row.iri ? `?this a <${row.iri}> .\n    ` : "";
      return T.withPrefixes(`SELECT ?this ?value\nWHERE {\n    ${sel}?this <${p.iri}> ?value .\n}\nLIMIT ${T.LIMIT}`, ctx.prefixes());
    }

    function stat(n, label) {
      return `<div class="cov-stat"><strong class="num">${n}</strong><span>${esc(label)}</span></div>`;
    }

    function renderDrawer() {
      const sel = st.selected;
      const row = sel && findRow(sel.key);
      if (!row) {
        el.coverageDrawer.classList.remove("open");
        el.coverageDrawer.setAttribute("aria-hidden", "true");
        return;
      }
      el.coverageDrawer.classList.add("open");
      el.coverageDrawer.setAttribute("aria-hidden", "false");
      const p = sel.prop && row.properties.find((x) => x.iri === sel.prop);
      el.coverageDrawerBody.innerHTML = p ? propDetail(row, p) : classDetail(row);
    }

    function classDetail(row) {
      const covered = row.targeted + row.reached;
      const s = sums(row);
      const status = row.uncovered === 0 ? (row.targeted ? ["Targeted", "ok"] : ["Reached through a nested shape", "ok"]) : covered ? ["Partly covered", "gap"] : ["Not covered", "gap"];
      let h = `<div class="d-pills"><span class="chip-s ${status[1]}">${status[0]}</span>${row.closed ? `<span class="chip-s info">${num(row.closed)} in a closed shape</span>` : ""}</div>`;
      h += `<h2 class="d-title">${rowNameHtml(row)}</h2>`;
      h += `<div class="cov-stats">${stat(num(row.resources), "resources")}${stat(pctText(covered, row.resources), "covered")}${stat(pctText(s.covered, s.triples), "values checked")}</div>`;
      let why;
      if (row.uncovered === 0 && row.targeted === row.resources) why = "Every resource of this class is a focus node of a shape.";
      else if (row.uncovered === 0) why = "No shape targets these resources, but each one is validated as the value of a property whose rule applies a nested shape (sh:node, sh:property, sh:qualifiedValueShape or a logical constraint).";
      else if (covered) why = `${plural(row.uncovered, "resource")} of this group ${row.uncovered === 1 ? "is" : "are"} neither a focus node nor the value of a nested shape.`;
      else if (row.kind === "blank" && row.via) why = `These blank nodes are values of ${iriText(row.via.predicate)}. A rule on that property may count them, but no shape looks inside them.`;
      else why = "No shape targets these resources, and no rule reaches them as a value. Nothing about them is checked.";
      const hint = row.iri && st.cov.hints.find((x) => x.class === row.iri);
      if (hint) why += ` ${iriText(hint.superclass)} is a superclass of ${iriText(row.iri)} in the shapes graph only; SHACL does not use it for targeting.`;
      h += `<section class="d-section"><h3>Why</h3><p class="d-msg">${esc(why)}</p></section>`;
      if (row.shapes.length) {
        h += `<section class="d-section"><h3>Checked by</h3><ul class="cov-list">${row.shapes.map((x) => `<li>${shapeLink(x.shape)}<span class="faint">${plural(x.resources, "resource")}</span></li>`).join("")}</ul></section>`;
      }
      h += examplesHtml(row);
      if (!covered) {
        const starter = starterClass(row);
        if (starter) h += starterHtml(starter, "Starter shape, from the data");
      }
      h += `<div class="cov-actions"><button class="btn primary" data-action="class-query">${ICON.code}Find these in the endpoint</button>${row.properties.length ? `<button class="btn" data-toggle="${esc(row.key)}">${st.open.has(row.key) ? "Hide" : "Show"} properties</button>` : ""}</div>`;
      return h;
    }

    function examplesHtml(row) {
      if (!row.examples.length) return "";
      const focus = (st.report && st.report.focus) || {};
      let h = `<section class="d-section"><h3>${row.uncovered ? "Examples, those not covered first" : "Examples"}</h3>`;
      for (const ex of row.examples.slice(0, 4)) {
        const key = ex.t === "iri" ? `<${ex.v}>` : `_:${ex.v}`;
        const triples = focus[key] || [];
        h += `<details class="cov-example"><summary>${termHtml(ex, true)}<span class="faint">${plural(triples.length, "triple")}</span></summary>`;
        if (triples.length) {
          h += `<table class="triples"><tbody>${triples.map((t) => `<tr><td>${iriHtml(t.p, true)}</td><td>${termHtml(t.o, true)}</td></tr>`).join("")}</tbody></table>`;
        }
        h += `</details>`;
      }
      return h + `</section>`;
    }

    function propDetail(row, p) {
      const status = !p.covered ? (p.declared ? ["Declared, no constraints", "gap"] : ["Not covered", "gap"]) : p.covered < p.triples ? ["Partly covered", "gap"] : ["Covered", "ok"];
      let h = `<div class="d-pills"><span class="chip-s ${status[1]}">${status[0]}</span>${p.how.map((x) => `<span class="chip-s ${HOW[x][1]}">${HOW[x][0]}</span>`).join("")}${p.annotation ? '<span class="chip-s neutral">annotation</span>' : ""}</div>`;
      h += `<h2 class="d-title">${rowNameHtml(row)} › ${iriHtml(p.iri, false)}</h2>`;
      h += `<div class="cov-stats">${stat(num(p.resources), "resources use it")}${stat(num(p.triples), p.triples === 1 ? "value" : "values")}${stat(pctText(p.covered, p.triples), "checked")}</div>`;
      let why;
      if (!p.covered && p.declared) why = "A property shape names this path but has no constraint, so every value passes. Add a constraint to have it checked.";
      else if (!p.covered) why = row.targeted + row.reached ? `No property shape that applies to these resources has a path that reaches ${iriText(p.iri)}. Its values are never checked.` : "No shape applies to these resources at all, so none of their properties is checked.";
      else if (p.covered < p.triples) why = "Only some of the resources that use this property are focus nodes of the shape that checks it.";
      else if (p.how.includes("sparql") && p.how.length === 1) why = "A SPARQL constraint reads this property. shacl-report cannot know exactly what a query checks, so this is an estimate from the query text.";
      else if (p.how.includes("inverse")) why = "Checked from the other side: a property shape walks this property backwards (sh:inversePath) from the resources it points to.";
      else why = "Every value is checked by a property shape.";
      h += `<section class="d-section"><h3>Why</h3><p class="d-msg">${esc(why)}</p></section>`;
      if (p.covered) {
        const kinds = st.cov.categories.filter((c, i) => p.categories[i]).map((c) => c.label);
        h += `<section class="d-section"><h3>Checked by</h3><ul class="cov-list">${p.shapes.map((k) => `<li>${shapeLink(k)}</li>`).join("")}</ul>${kinds.length ? `<p class="cov-note">Kinds of rules: ${esc(kinds.join(", "))}</p>` : ""}</section>`;
      }
      if (p.examples.length) {
        h += `<section class="d-section"><h3>${p.covered < p.triples ? "Values nobody checks" : "Example values"}</h3><table class="triples"><tbody>`;
        for (const e of p.examples) {
          h += `<tr class="${e.covered ? "" : "cov-gap"}"><td>${termHtml(e.s, true)}</td><td>${termHtml(e.o, true)}</td></tr>`;
        }
        h += `</tbody></table></section>`;
      }
      if (p.covered < p.triples) h += starterHtml(starterProperty(row, p), "Starter rule, from the data");
      h += `<div class="cov-actions"><button class="btn primary" data-action="prop-query">${ICON.code}Find these in the endpoint</button></div>`;
      return h;
    }

    function starterHtml(text, title) {
      return `<section class="d-section"><div class="cov-starter-head"><h3>${esc(title)}</h3><button class="btn small" data-copy-starter>${ICON.copy}Copy</button></div><pre class="cov-code">${esc(text)}</pre><p class="cov-note">Datatypes and counts are what the data has today. Review before adding it to your shapes.</p></section>`;
    }

    // ---------- events ----------

    function select(key, prop) {
      st.selected = { key, prop: prop || null };
      render();
    }

    function bind() {
      el.coverageAnnot.addEventListener("change", () => {
        st.ignoreAnnotations = el.coverageAnnot.checked;
        render();
      });
      el.coverageGaps.addEventListener("change", () => {
        st.gapsOnly = el.coverageGaps.checked;
        render();
      });
      el.coverageContent.addEventListener("input", (e) => {
        if (e.target.id !== "cov-search") return;
        st.query = e.target.value.trim().toLowerCase();
        render();
      });
      const onClick = (e) => {
        if (e.ctrlKey || e.metaKey) return;
        const t = e.target;
        const toggle = t.closest("[data-toggle]");
        if (toggle) {
          const k = toggle.dataset.toggle;
          if (st.open.has(k)) st.open.delete(k);
          else st.open.add(k);
          render();
          return;
        }
        const sel = t.closest("[data-select]");
        if (sel) return select(sel.dataset.select, sel.dataset.prop);
        const selClass = t.closest("[data-select-class]");
        if (selClass) {
          st.view = "classes";
          return select(selClass.dataset.selectClass);
        }
        const view = t.closest("[data-view]");
        if (view) {
          st.view = view.dataset.view;
          return render();
        }
        const shape = t.closest("[data-shape-key]");
        if (shape) return ctx.showShape(shape.dataset.shapeKey);
        const action = t.closest("[data-action]");
        if (action) {
          const a = action.dataset.action;
          if (a === "gaps") {
            st.gapsOnly = true;
            st.view = "classes";
            render();
          } else if (a === "unused") {
            document.getElementById("cov-unused").scrollIntoView({ behavior: "smooth", block: "start" });
          } else if (a === "hide-hints") {
            st.hintsHidden = true;
            render();
          } else if (a === "class-query" || a === "prop-query") {
            const row = findRow(st.selected.key);
            const p = st.selected.prop && row.properties.find((x) => x.iri === st.selected.prop);
            const q = p ? propQuery(row, p) : classQuery(row);
            ctx.runQuery(`Coverage: ${p ? rowName(row) + " › " + iriText(p.iri) : rowName(row)}`, q);
          }
          return;
        }
        if (t.closest("[data-copy-starter]")) {
          const pre = el.coverageDrawerBody.querySelector(".cov-code");
          if (pre) ctx.copy(pre.textContent);
        }
      };
      el.coverageContent.addEventListener("click", onClick);
      el.coverageDrawerBody.addEventListener("click", onClick);
      el.coverageClose.addEventListener("click", () => {
        st.selected = null;
        render();
      });
      el.coverageToggleOptions.addEventListener("click", () => el.coverageOptions.classList.toggle("open"));
    }

    return {
      setModel(report) {
        st.report = report;
        st.cov = report.coverage || null;
        st.selected = null;
        st.open = new Set();
        st.query = "";
        st.hintsHidden = false;
        if (!st.cov) return;
        if (!bound) {
          bind();
          bound = true;
        }
        render();
      },
      closeDrawer() {
        if (!st.selected) return false;
        st.selected = null;
        render();
        return true;
      },
    };
  }

  window.ShaclCoverage = { create };
})();
