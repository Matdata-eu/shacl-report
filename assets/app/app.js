// shacl-report viewer: summary dashboard, grouped result cards, table view
// and a details drawer, driven by the report JSON (served or embedded).
(function () {
  "use strict";

  const SH = "http://www.w3.org/ns/shacl#";
  const XSD = "http://www.w3.org/2001/XMLSchema#";
  const SEVERITIES = ["Violation", "Warning", "Info", "Other"];
  const SEV_CLASS = { Violation: "sev-violation", Warning: "sev-warning", Info: "sev-info", Other: "sev-other" };
  const GROUPS_PAGE = 30;
  const ROWS_PAGE = 20;
  const FACET_PAGE = 12;
  const THEME_KEY = "shacl-report-theme";

  const ICON = {
    check: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>',
    cross: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>',
    help: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M9.5 9.5a2.5 2.5 0 114 2c-.9.6-1.5 1.1-1.5 2.2M12 17h.01" /></svg>',
    chev: '<svg class="chev" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>',
    right: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>',
    copy: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 00-2-2H6a2 2 0 00-2 2v8a2 2 0 002 2h2" /></svg>',
    search: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6" /><path d="M20 20l-4.5-4.5" /></svg>',
    party: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>',
    sun: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4" /><path d="M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4l1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" /></svg>',
    moon: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 14.5A8 8 0 019.5 4a8 8 0 1010.5 10.5z" /></svg>',
  };

  // Plain-language name for each SHACL Core constraint component.
  const PROBLEM = {
    MinCount: "Missing value",
    MaxCount: "Too many values",
    Datatype: "Wrong datatype",
    Class: "Wrong class",
    NodeKind: "Wrong kind of node",
    MinInclusive: "Out of range",
    MaxInclusive: "Out of range",
    MinExclusive: "Out of range",
    MaxExclusive: "Out of range",
    MinLength: "Too short",
    MaxLength: "Too long",
    Pattern: "Doesn't match pattern",
    LanguageIn: "Language not allowed",
    UniqueLang: "Duplicate language",
    In: "Value not allowed",
    HasValue: "Required value missing",
    Closed: "Unexpected property",
    Equals: "Values differ",
    Disjoint: "Values overlap",
    LessThan: "Wrong value order",
    LessThanOrEquals: "Wrong value order",
    Node: "Nested shape failed",
    Property: "Property shape failed",
    QualifiedMinCount: "Too few qualifying values",
    QualifiedMaxCount: "Too many qualifying values",
    And: "Logical rule failed",
    Or: "Logical rule failed",
    Not: "Logical rule failed",
    Xone: "Logical rule failed",
    SPARQL: "Custom rule failed",
  };

  const state = {
    standalone: !!window.SHACL_REPORT,
    report: null,
    prefixes: [],
    rows: [],
    byFocus: new Map(),
    filtered: [],
    order: [],
    sevOn: new Set(SEVERITIES),
    problem: null,
    shape: null,
    resource: null,
    search: "",
    groupBy: "shape",
    view: "cards",
    groupsShown: GROUPS_PAGE,
    rowsShown: new Map(),
    collapsed: new Set(),
    shapeFacetAll: false,
    shapeFacetQuery: "",
    selected: null,
    table: null,
    shapeKeys: null,
    shapeKeysLabel: "",
    tab: "results",
    explorer: null,
    editor: null,
  };

  const $ = (id) => document.getElementById(id);
  const els = {
    version: $("version"), fileName: $("file-name"), openFiles: $("open-files"),
    exportBtn: $("export-btn"), exportMenu: $("export-menu"), themeBtn: $("theme-btn"),
    filters: $("filters"), toggleFilters: $("toggle-filters"), closeFilters: $("close-filters"), scrim: $("scrim"),
    sevFilter: $("severity-filter"), problemFilter: $("problem-filter"), shapeFilter: $("shape-filter"),
    shapeSearch: $("shape-search"), resetFilters: $("reset-filters"),
    welcome: $("welcome"), summary: $("summary"), statusCard: $("status-card"), statCards: $("stat-cards"),
    topShapes: $("top-shapes"), topProblems: $("top-problems"),
    results: $("results"), search: $("search"), groupBy: $("group-by"), viewToggle: $("view-toggle"),
    resultCount: $("result-count"), activeFilters: $("active-filters"),
    groups: $("groups"), tableWrap: $("table-wrap"), table: $("table"),
    drawer: $("drawer"), drawerBody: $("drawer-body"), drawerPos: $("drawer-pos"),
    prev: $("prev-result"), next: $("next-result"), closeDrawer: $("close-drawer"),
    modal: $("modal"), closeModal: $("close-modal"), formSlot: $("open-form-slot"), formTpl: $("open-form-tpl"),
    toast: $("toast"), loading: $("loading"), loadingMsg: $("loading-msg"),
    tabs: $("tabs"), resultsView: $("results-view"), shapesView: $("shapes-view"), sparqlView: $("sparql-view"),
    shapeList: $("shape-list"), shapeDetail: $("shape-detail"), shapeSearch2: $("shape-search-2"), shapeCount: $("shape-count"),
    shapesBack: $("shapes-back"),
  };

  // ---------- text helpers ----------

  function esc(s) {
    return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  }

  function shorten(iri) {
    for (const [name, base] of state.prefixes) {
      if (base && iri.startsWith(base)) {
        const local = iri.slice(base.length);
        if (local === "" || /^[A-Za-z_][\w.\-]*$/.test(local)) return `${name}:${local}`;
      }
    }
    return null;
  }
  const iriText = (iri) => shorten(iri) || `<${iri}>`;

  const TOKEN_RE = /<([A-Za-z][\w+.\-]*:[^\s<>"]*)>/g;
  const tokensToText = (s) => (s ? s.replace(TOKEN_RE, (_, iri) => iriText(iri)) : "");

  function iriHtml(iri, link) {
    const text = esc(iriText(iri));
    if (link && /^https?:\/\//i.test(iri)) {
      return `<a href="${esc(iri)}" target="_blank" rel="noopener noreferrer" title="${esc(iri)}">${text}</a>`;
    }
    return `<span class="iri" title="${esc(iri)}">${text}</span>`;
  }

  function tokensToHtml(s, link) {
    if (!s) return "";
    let out = "";
    let last = 0;
    s.replace(TOKEN_RE, (m, iri, offset) => {
      out += esc(s.slice(last, offset)) + iriHtml(iri, link);
      last = offset + m.length;
      return m;
    });
    return out + esc(s.slice(last));
  }

  function termText(o) {
    if (!o) return "";
    if (o.t === "iri") return iriText(o.v);
    if (o.t === "bnode") return `_:${o.v}`;
    if (o.lng) return `"${o.v}"@${o.lng}`;
    if (o.dt && o.dt !== XSD + "string") return `"${o.v}"^^${iriText(o.dt)}`;
    return `"${o.v}"`;
  }

  function termHtml(o, link) {
    if (!o) return "";
    if (o.t === "iri") return iriHtml(o.v, link);
    if (o.t === "bnode") return `<span class="faint">_:${esc(o.v)}</span>`;
    const v = `"${esc(o.v)}"`;
    if (o.lng) return `${v}<span class="lit-dt">@${esc(o.lng)}</span>`;
    if (o.dt && o.dt !== XSD + "string") return `${v}<span class="lit-dt">^^${esc(iriText(o.dt))}</span>`;
    return v;
  }

  // Inline "chip" rendering used inside sentences.
  const chipTerm = (o) => `<span class="term ${o && o.t === "lit" ? "lit" : ""}">${termHtml(o, false)}</span>`;
  const chipText = (s) => `<span class="term">${tokensToHtml(s, false)}</span>`;

  function termKey(o) {
    if (o.t === "iri") return `<${o.v}>`;
    if (o.t === "bnode") return `_:${o.v}`;
    return `"${o.v}"`;
  }

  const plural = (n, one, many) => `${n.toLocaleString()} ${n === 1 ? one : many || one + "s"}`;
  const sevKey = (s) => (s === "Violation" || s === "Warning" || s === "Info" ? s : "Other");
  const sevRank = (s) => SEVERITIES.indexOf(sevKey(s));
  const sevLabel = (s) => (sevKey(s) === "Other" ? iriText(s) : s);

  function componentLocal(iri) {
    if (!iri) return "";
    if (iri.startsWith(SH)) return iri.slice(SH.length).replace(/ConstraintComponent$/, "");
    return iriText(iri);
  }

  // ---------- plain-language explanation ----------

  function explain(d) {
    const c = d.local;
    const p = d.params || [];
    const n = p[0];
    const path = d.path ? chipText(d.path) : "this resource";
    const val = d.value ? chipTerm(d.value) : "The value";
    const list = (items) => items.map((i) => chipText(i)).join(", ");
    switch (c) {
      case "MinCount":
        if (n === "1") return `Missing a value for ${path}`;
        return n ? `Needs at least ${esc(n)} values for ${path}` : `Too few values for ${path}`;
      case "MaxCount":
        if (n === "0") return `Must not have any value for ${path}`;
        return n ? `Has too many values for ${path}; at most ${esc(n)} allowed` : `Has too many values for ${path}`;
      case "Datatype":
        return n ? `${val} should be of type ${chipText(n)}` : `${val} has the wrong datatype`;
      case "Class":
        return n ? `${val} should be an instance of ${chipText(n)}` : `${val} is not of the expected class`;
      case "NodeKind": {
        const kinds = { IRI: "an IRI", Literal: "a literal", BlankNode: "a blank node", BlankNodeOrIRI: "an IRI or blank node", BlankNodeOrLiteral: "a blank node or literal", IRIOrLiteral: "an IRI or literal" };
        const k = n ? kinds[tokensToText(n).replace(/^sh:/, "")] : null;
        return k ? `${val} should be ${k}` : `${val} is the wrong kind of node`;
      }
      case "MinInclusive": return n ? `${val} is below the minimum of ${esc(n)}` : `${val} is too small`;
      case "MaxInclusive": return n ? `${val} is above the maximum of ${esc(n)}` : `${val} is too large`;
      case "MinExclusive": return n ? `${val} must be greater than ${esc(n)}` : `${val} is too small`;
      case "MaxExclusive": return n ? `${val} must be less than ${esc(n)}` : `${val} is too large`;
      case "MinLength": return n ? `${val} is shorter than ${esc(n)} characters` : `${val} is too short`;
      case "MaxLength": return n ? `${val} is longer than ${esc(n)} characters` : `${val} is too long`;
      case "Pattern": return n ? `${val} doesn't match the pattern ${chipText(n)}` : `${val} doesn't match the required pattern`;
      case "In": return p.length ? `${val} is not one of the allowed values: ${list(p)}` : `${val} is not an allowed value`;
      case "HasValue": return n ? `Must have the value ${chipText(n)} for ${path}` : `Is missing a required value for ${path}`;
      case "LanguageIn": return p.length ? `${val} has a language outside ${list(p)}` : `${val} has a language that is not allowed`;
      case "UniqueLang": return `Has more than one value in the same language for ${path}`;
      case "Closed": return `Uses ${path}, which the shape does not allow`;
      case "Equals": return n ? `Values of ${path} must equal those of ${chipText(n)}` : `Values of ${path} must be equal`;
      case "Disjoint": return n ? `Values of ${path} must not overlap with ${chipText(n)}` : `Values of ${path} must not overlap`;
      case "LessThan": return n ? `${val} must be less than the values of ${chipText(n)}` : `${val} is not less than required`;
      case "LessThanOrEquals": return n ? `${val} must be at most the values of ${chipText(n)}` : `${val} is larger than allowed`;
      case "Node": return n ? `${d.value ? val : "The resource"} doesn't conform to ${chipText(n)}` : `${val} doesn't conform to the nested shape`;
      case "QualifiedMinCount": return n ? `Needs at least ${esc(n)} qualifying values for ${path}` : `Too few qualifying values for ${path}`;
      case "QualifiedMaxCount": return n ? `Has more than ${esc(n)} qualifying values for ${path}` : `Too many qualifying values for ${path}`;
      case "Not": return `${d.value ? val : "The resource"} matches a shape it must not match`;
      case "And": return `${d.value ? val : "The resource"} doesn't satisfy all required shapes`;
      case "Or": return `${d.value ? val : "The resource"} doesn't satisfy any of the alternative shapes`;
      case "Xone": return `${d.value ? val : "The resource"} must match exactly one of the alternative shapes`;
      default:
        if (d.messages.length) return esc(d.messages[0]);
        return `${esc(componentLocal(d.component) || "Constraint")} not satisfied`;
    }
  }

  // ---------- data preparation ----------

  function decorate(results) {
    return results.map((r) => {
      const local = componentLocal(r.component);
      const d = {
        ...r,
        sev: sevKey(r.severity),
        local,
        problem: PROBLEM[local] || (local ? local.replace(/([a-z])([A-Z])/g, "$1 $2") : "Other"),
        focusKey: termKey(r.focus),
        focusText: termText(r.focus),
        pathText: r.path ? tokensToText(r.path) : "",
        valueText: termText(r.value),
        shapeText: tokensToText(r.shape_label),
      };
      d.explanation = explain(d);
      d.explanationText = d.explanation.replace(/<[^>]+>/g, "").replace(/&quot;/g, '"').replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
      d.msgText = d.messages.join(" · ");
      d.hay = `${d.focusText} ${r.focus.v} ${d.pathText} ${d.valueText} ${d.msgText} ${d.explanationText} ${d.shapeText} ${d.problem} ${local}`.toLowerCase();
      return d;
    });
  }

  function matches(d, skip) {
    if (skip !== "sev" && !state.sevOn.has(d.sev)) return false;
    if (skip !== "problem" && state.problem !== null && d.problem !== state.problem) return false;
    if (skip !== "shape" && state.shape !== null && d.shape_label !== state.shape) return false;
    if (state.shapeKeys !== null && !state.shapeKeys.has(d.shape_key)) return false;
    if (state.resource !== null && d.focusKey !== state.resource) return false;
    if (state.search && !d.hay.includes(state.search)) return false;
    return true;
  }

  function countBy(rows, key) {
    const m = new Map();
    for (const d of rows) {
      const k = d[key];
      let e = m.get(k);
      if (!e) m.set(k, (e = { key: k, total: 0, Violation: 0, Warning: 0, Info: 0, Other: 0, sample: d }));
      e.total += 1;
      e[d.sev] += 1;
    }
    return [...m.values()];
  }

  function worst(e) {
    return e.Violation ? 0 : e.Warning ? 1 : e.Info ? 2 : 3;
  }

  function sortGroups(list) {
    return list.sort((a, b) => worst(a) - worst(b) || b.total - a.total || String(a.key).localeCompare(String(b.key)));
  }

  function rowSort(a, b) {
    return sevRank(a.severity) - sevRank(b.severity) || a.focusText.localeCompare(b.focusText) || a.pathText.localeCompare(b.pathText) || a.id - b.id;
  }

  // ---------- rendering: summary ----------

  function renderSummary() {
    const r = state.report;
    const s = r.stats;
    const src = r.sources || {};
    const base = (p) => (p || "").split(/[\\/]/).pop();
    let cls, icon, title, sub;
    if (r.conforms === true || (r.conforms === null && s.total === 0)) {
      cls = "ok"; icon = ICON.check; title = "Data conforms";
      sub = s.total ? `${plural(s.total, "non-blocking result")} reported` : "No problems found. Every resource satisfies its shapes.";
    } else if (r.conforms === false) {
      cls = "fail"; icon = ICON.cross; title = "Data does not conform";
      sub = `${plural(s.total, "problem")} on ${plural(s.focus_nodes, "resource")}, from ${plural(s.shapes, "shape")}`;
    } else {
      cls = "unknown"; icon = ICON.help; title = "Conformance not stated";
      sub = `${plural(s.total, "result")} on ${plural(s.focus_nodes, "resource")}`;
    }
    const srcParts = [];
    if (src.shapes && src.shapes.length) srcParts.push(`Shapes: ${src.shapes.map(base).join(", ")}`);
    if (src.data && src.data.length) srcParts.push(`Data: ${src.data.map(base).join(", ")}`);
    if (!r.has_shapes) srcParts.push("Add the shapes file for shape names and broken rules");
    els.statusCard.className = `card status-card ${cls}`;
    els.statusCard.innerHTML = `<div class="status-icon">${icon}</div><div><p class="status-title">${title}</p><p class="status-sub">${sub}</p><p class="status-sources">${esc(srcParts.join(" · "))}</p></div>`;

    const cards = [
      ["Violation", s.violations, "Must be fixed"],
      ["Warning", s.warnings, "Should be reviewed"],
      ["Info", s.infos + s.other, "For information"],
    ];
    els.statCards.innerHTML = "";
    for (const [sev, n, hint] of cards) {
      const b = document.createElement("button");
      const target = sev === "Info" ? ["Info", "Other"] : [sev];
      const only = state.sevOn.size === target.length && target.every((t) => state.sevOn.has(t));
      const off = !state.sevOn.has(sev);
      b.className = `stat ${SEV_CLASS[sev]}${n === 0 ? " zero" : ""}${off ? " off" : ""}`;
      b.innerHTML = `<div class="stat-label"><span class="dot"></span>${sev === "Info" ? "Info" : sev + "s"}</div><div class="stat-value">${n.toLocaleString()}</div><div class="stat-hint">${only ? "Showing only these · click to show all" : hint}</div>`;
      b.title = only ? "Show all severities" : `Show only ${sev.toLowerCase()} results`;
      b.addEventListener("click", () => {
        if (only) state.sevOn = new Set(SEVERITIES);
        else state.sevOn = new Set(target);
        update();
      });
      els.statCards.appendChild(b);
    }
    renderBars(els.topShapes, countBy(state.rows, "shape_label"), "shape", (k) => tokensToText(k));
    renderBars(els.topProblems, countBy(state.rows, "problem"), "problem", (k) => k);
  }

  function renderBars(el, entries, field, label) {
    entries.sort((a, b) => b.total - a.total || worst(a) - worst(b));
    const top = entries.slice(0, 5);
    const max = top.length ? top[0].total : 1;
    el.innerHTML = top.length ? "" : '<p class="none">Nothing to show.</p>';
    for (const e of top) {
      const b = document.createElement("button");
      b.className = `bar-row${state[field] === e.key ? " active" : ""}`;
      const segs = SEVERITIES.filter((s) => e[s]).map((s) => `<span class="bar-seg seg-${s.toLowerCase()}" style="width:${(e[s] / max) * 100}%"></span>`).join("");
      b.innerHTML = `<span class="bar-label" title="${esc(label(e.key))}">${esc(label(e.key))}</span><span class="bar-count">${e.total.toLocaleString()}</span><span class="bar-track">${segs}</span>`;
      b.addEventListener("click", () => {
        state[field] = state[field] === e.key ? null : e.key;
        update();
        els.results.scrollIntoView({ block: "start" });
      });
      el.appendChild(b);
    }
  }

  // ---------- rendering: filters ----------

  function renderFilters() {
    // Severity checkboxes.
    const sevCounts = { Violation: 0, Warning: 0, Info: 0, Other: 0 };
    for (const d of state.rows) if (matches(d, "sev")) sevCounts[d.sev] += 1;
    els.sevFilter.innerHTML = "";
    for (const sev of SEVERITIES) {
      if (sev === "Other" && !state.report.stats.other) continue;
      const lab = document.createElement("label");
      lab.className = `check ${SEV_CLASS[sev]}`;
      lab.style.setProperty("--sev-color", `var(--${sev.toLowerCase()})`);
      lab.innerHTML = `<input type="checkbox" ${state.sevOn.has(sev) ? "checked" : ""} /><span class="box">${ICON.check}</span><span class="label">${sev === "Other" ? "Other" : sev}</span><span class="count">${sevCounts[sev].toLocaleString()}</span>`;
      lab.querySelector("input").addEventListener("change", (e) => {
        if (e.target.checked) state.sevOn.add(sev);
        else state.sevOn.delete(sev);
        update();
      });
      els.sevFilter.appendChild(lab);
    }

    renderFacet(els.problemFilter, countBy(state.rows.filter((d) => matches(d, "problem")), "problem"), "problem", (k) => esc(k), Infinity);

    let shapes = countBy(state.rows.filter((d) => matches(d, "shape")), "shape_label");
    els.shapeSearch.classList.toggle("hidden", countBy(state.rows, "shape_label").length <= FACET_PAGE);
    if (state.shapeFacetQuery) {
      const q = state.shapeFacetQuery;
      shapes = shapes.filter((e) => tokensToText(e.key).toLowerCase().includes(q));
    }
    renderFacet(els.shapeFilter, shapes, "shape", (k) => esc(tokensToText(k)), state.shapeFacetAll ? Infinity : FACET_PAGE);

    const any = state.sevOn.size !== SEVERITIES.length || state.problem !== null || state.shape !== null || state.shapeKeys !== null || state.resource !== null || state.search;
    els.resetFilters.classList.toggle("hidden", !any);
  }

  function renderFacet(el, entries, field, label, limit) {
    entries.sort((a, b) => b.total - a.total || String(a.key).localeCompare(String(b.key)));
    const current = state[field];
    if (current !== null && !entries.some((e) => e.key === current)) entries.unshift({ key: current, total: 0 });
    el.innerHTML = "";
    if (!entries.length) {
      el.innerHTML = '<li class="empty-facet">None</li>';
      return;
    }
    for (const e of entries.slice(0, limit)) {
      const li = document.createElement("li");
      li.className = e.key === current ? "active" : "";
      li.tabIndex = 0;
      li.title = e.key === current ? "Click to clear this filter" : "Show only these results";
      li.innerHTML = `<span class="facet-label" title="${label(e.key)}">${label(e.key)}</span><span class="facet-count">${e.total.toLocaleString()}</span>`;
      const pick = () => {
        state[field] = e.key === current ? null : e.key;
        update();
      };
      li.addEventListener("click", pick);
      li.addEventListener("keydown", (ev) => {
        if (ev.key === "Enter" || ev.key === " ") { ev.preventDefault(); pick(); }
      });
      el.appendChild(li);
    }
    if (entries.length > limit) {
      const li = document.createElement("li");
      li.className = "more";
      li.textContent = `Show all ${entries.length}`;
      li.addEventListener("click", () => { state.shapeFacetAll = true; renderFilters(); });
      el.appendChild(li);
    }
  }

  function renderActiveFilters() {
    const chips = [];
    if (state.sevOn.size !== SEVERITIES.length) {
      chips.push([`Severity: ${SEVERITIES.filter((s) => state.sevOn.has(s)).join(", ") || "none"}`, () => (state.sevOn = new Set(SEVERITIES))]);
    }
    if (state.problem !== null) chips.push([`Problem: ${state.problem}`, () => (state.problem = null)]);
    if (state.shape !== null) chips.push([`Shape: ${tokensToText(state.shape)}`, () => (state.shape = null)]);
    if (state.shapeKeys !== null) chips.push([`Shape: ${state.shapeKeysLabel} (with its properties)`, () => (state.shapeKeys = null)]);
    if (state.resource !== null) {
      const d = state.byFocus.get(state.resource);
      chips.push([`Resource: ${d ? d[0].focusText : state.resource}`, () => (state.resource = null)]);
    }
    if (state.search) chips.push([`Search: “${state.search}”`, () => { state.search = ""; els.search.value = ""; }]);
    els.activeFilters.innerHTML = "";
    for (const [text, clear] of chips) {
      const b = document.createElement("button");
      b.className = "chip";
      b.title = "Remove this filter";
      b.innerHTML = `<span>${esc(text)}</span>${ICON.cross}`;
      b.addEventListener("click", () => { clear(); update(); });
      els.activeFilters.appendChild(b);
    }
    const total = state.rows.length;
    const shown = state.filtered.length;
    els.resultCount.textContent = shown === total ? plural(total, "result") : `${shown.toLocaleString()} of ${plural(total, "result")}`;
  }

  // ---------- rendering: results ----------

  function groupKey(d) {
    switch (state.groupBy) {
      case "shape": return d.shape_label;
      case "resource": return d.focusKey;
      case "problem": return d.problem;
      default: return "";
    }
  }

  function groupTitle(key, sample) {
    switch (state.groupBy) {
      case "shape": return esc(tokensToText(key));
      case "resource": return esc(sample.focusText);
      case "problem": return esc(key);
      default: return "All results";
    }
  }

  function buildGroups() {
    const map = new Map();
    for (const d of state.filtered) {
      const k = groupKey(d);
      let g = map.get(k);
      if (!g) map.set(k, (g = { key: k, rows: [], total: 0, Violation: 0, Warning: 0, Info: 0, Other: 0 }));
      g.rows.push(d);
      g.total += 1;
      g[d.sev] += 1;
    }
    const groups = sortGroups([...map.values()]);
    for (const g of groups) g.rows.sort(rowSort);
    state.order = groups.flatMap((g) => g.rows);
    return groups;
  }

  function pills(g) {
    return SEVERITIES.filter((s) => g[s])
      .map((s) => `<span class="pill ${SEV_CLASS[s]}"><span class="dot"></span>${g[s].toLocaleString()}</span>`)
      .join("");
  }

  function rowHtml(d) {
    let primary;
    let secondary;
    if (state.groupBy === "resource") {
      primary = esc(d.shapeText);
      secondary = esc(d.problem);
    } else if (state.groupBy === "problem") {
      primary = esc(d.focusText);
      secondary = esc(d.shapeText);
    } else {
      primary = esc(d.focusText);
      secondary = esc(d.problem);
    }
    const msg = d.messages.length && d.local in PROBLEM ? `<span class="row-msg">${esc(d.msgText)}</span>` : "";
    const sel = state.selected && state.selected.id === d.id ? " selected" : "";
    return `<button class="row ${SEV_CLASS[d.sev]}${sel}" data-id="${d.id}">
      <span class="row-bar"></span>
      <span class="row-main">
        <span class="row-top"><span class="row-focus" title="${esc(d.focus.v)}">${primary}</span><span class="row-problem">${secondary}</span></span>
        <span class="row-text">${d.explanation}</span>
        ${msg}
      </span>
      <span class="row-side"><span class="pill ${SEV_CLASS[d.sev]}">${esc(sevLabel(d.severity))}</span>${ICON.right}</span>
    </button>`;
  }

  function renderGroups() {
    const groups = buildGroups();
    els.groups.innerHTML = "";
    if (!state.filtered.length) {
      els.groups.innerHTML = state.rows.length
        ? `<div class="empty-results">${ICON.search}<h3>No results match your filters</h3><p>Try a different search, or <button class="link-btn" data-action="reset">reset all filters</button>.</p></div>`
        : `<div class="empty-results">${ICON.party}<h3>Nothing to fix</h3><p>The report contains no validation results.</p></div>`;
      return;
    }
    const frag = document.createDocumentFragment();
    for (const g of groups.slice(0, state.groupsShown)) {
      const sec = document.createElement("section");
      const collapsed = state.collapsed.has(g.key);
      sec.className = `group${collapsed ? " collapsed" : ""}`;
      const limit = state.rowsShown.get(g.key) || ROWS_PAGE;
      const head = state.groupBy === "none"
        ? ""
        : `<button class="group-head" aria-expanded="${!collapsed}">${ICON.chev}<span class="group-title" title="${groupTitle(g.key, g.rows[0])}">${groupTitle(g.key, g.rows[0])}<span class="group-sub">${plural(g.total, "result")}</span></span><span class="group-pills">${pills(g)}</span></button>`;
      const rows = g.rows.slice(0, limit).map(rowHtml).join("");
      const more = g.rows.length > limit ? `<button class="group-more">Show ${Math.min(ROWS_PAGE * 5, g.rows.length - limit)} more of ${(g.rows.length - limit).toLocaleString()} remaining</button>` : "";
      sec.innerHTML = `${head}<div class="group-body">${rows}${more}</div>`;
      const h = sec.querySelector(".group-head");
      if (h) h.addEventListener("click", () => {
        if (state.collapsed.has(g.key)) state.collapsed.delete(g.key);
        else state.collapsed.add(g.key);
        sec.classList.toggle("collapsed");
        h.setAttribute("aria-expanded", String(!sec.classList.contains("collapsed")));
      });
      const m = sec.querySelector(".group-more");
      if (m) m.addEventListener("click", () => { state.rowsShown.set(g.key, limit + ROWS_PAGE * 5); renderGroups(); });
      frag.appendChild(sec);
    }
    if (groups.length > state.groupsShown) {
      const b = document.createElement("button");
      b.className = "btn groups-more";
      b.textContent = `Show more groups (${(groups.length - state.groupsShown).toLocaleString()} remaining)`;
      b.addEventListener("click", () => { state.groupsShown += GROUPS_PAGE; renderGroups(); });
      frag.appendChild(b);
    }
    els.groups.appendChild(frag);
  }

  function ensureTable() {
    if (state.table) return Promise.resolve(state.table);
    return new Promise((resolve) => {
      const t = new Tabulator(els.table, {
        height: "100%",
        layout: "fitColumns",
        placeholder: "No results match your filters",
        selectableRows: false,
        index: "id",
        columns: [
          { title: "Severity", field: "sev", width: 110, sorter: (a, b) => SEVERITIES.indexOf(a) - SEVERITIES.indexOf(b),
            formatter: (c) => `<span class="pill ${SEV_CLASS[c.getValue()]}"><span class="dot"></span>${esc(sevLabel(c.getRow().getData().severity))}</span>` },
          { title: "Resource", field: "focusText", minWidth: 150, formatter: (c) => termHtml(c.getRow().getData().focus, false) },
          { title: "Problem", field: "explanationText", minWidth: 260, widthGrow: 2, formatter: (c) => c.getRow().getData().explanation },
          { title: "Path", field: "pathText", minWidth: 110, formatter: (c) => esc(c.getValue() || "—") },
          { title: "Value", field: "valueText", minWidth: 110, formatter: (c) => termHtml(c.getRow().getData().value, false) },
          { title: "Shape", field: "shapeText", minWidth: 150, formatter: (c) => esc(c.getValue()) },
          { title: "Type", field: "problem", width: 150 },
        ],
      });
      t.on("rowClick", (_e, row) => openDrawer(row.getData()));
      t.on("tableBuilt", () => { state.table = t; resolve(t); });
    });
  }

  function renderResults() {
    const cards = state.view === "cards";
    els.groups.classList.toggle("hidden", !cards);
    els.tableWrap.classList.toggle("hidden", cards);
    if (cards) {
      renderGroups();
    } else {
      buildGroups();
      ensureTable().then((t) => t.replaceData(state.order));
    }
  }

  function update(opts) {
    state.filtered = state.rows.filter((d) => matches(d));
    if (!opts || !opts.keepPaging) {
      state.groupsShown = GROUPS_PAGE;
      state.rowsShown = new Map();
    }
    renderSummary();
    renderFilters();
    renderActiveFilters();
    renderResults();
    if (state.selected) syncDrawerNav();
  }

  // ---------- drawer ----------

  function openDrawer(d) {
    state.selected = d;
    renderDrawer();
    els.drawer.classList.add("open");
    els.drawer.setAttribute("aria-hidden", "false");
    for (const el of els.groups.querySelectorAll(".row.selected")) el.classList.remove("selected");
    const rowEl = els.groups.querySelector(`.row[data-id="${d.id}"]`);
    if (rowEl) {
      rowEl.classList.add("selected");
      rowEl.scrollIntoView({ block: "nearest" });
    }
  }

  function closeDrawer() {
    state.selected = null;
    els.drawer.classList.remove("open");
    els.drawer.setAttribute("aria-hidden", "true");
    for (const el of els.groups.querySelectorAll(".row.selected")) el.classList.remove("selected");
  }

  function syncDrawerNav() {
    const i = state.order.findIndex((x) => x.id === state.selected.id);
    els.drawerPos.textContent = i >= 0 ? `${(i + 1).toLocaleString()} of ${state.order.length.toLocaleString()}` : "Filtered out";
    els.prev.disabled = i <= 0;
    els.next.disabled = i < 0 || i >= state.order.length - 1;
  }

  function step(delta) {
    if (!state.selected) return;
    const i = state.order.findIndex((x) => x.id === state.selected.id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= state.order.length) return;
    const d = state.order[j];
    // Make sure the target row is rendered (expanded group, enough rows).
    const k = groupKey(d);
    if (state.view === "cards") {
      let changed = state.collapsed.delete(k);
      const pos = state.order.filter((x) => groupKey(x) === k).indexOf(d);
      const limit = state.rowsShown.get(k) || ROWS_PAGE;
      if (pos >= limit) { state.rowsShown.set(k, pos + ROWS_PAGE); changed = true; }
      if (changed) renderGroups();
    }
    openDrawer(d);
  }

  function copyBtn(text) {
    return `<button class="icon-btn copy" data-copy="${esc(text)}" title="Copy" aria-label="Copy">${ICON.copy}</button>`;
  }

  function renderDrawer() {
    const d = state.selected;
    const r = state.report;
    let h = `<div class="d-pills"><span class="pill ${SEV_CLASS[d.sev]}"><span class="dot"></span>${esc(sevLabel(d.severity))}</span><span class="pill d-pill-neutral">${esc(d.problem)}</span></div>`;
    h += `<h2 class="d-title">${d.explanation}</h2>`;
    if (d.messages.length) h += `<p class="d-msg">${d.messages.map(esc).join("<br>")}</p>`;

    h += `<section class="d-section"><h3>Where</h3><dl class="kv">`;
    h += `<dt>Resource</dt><dd><span>${termHtml(d.focus, true)}</span>${d.focus.t === "iri" ? copyBtn(d.focus.v) : ""}</dd>`;
    h += `<dt>Path</dt><dd>${d.path ? tokensToHtml(d.path, true) : '<span class="faint">The resource itself</span>'}</dd>`;
    h += `<dt>Value</dt><dd>${d.value ? `<span>${termHtml(d.value, true)}</span>${copyBtn(d.value.v)}` : '<span class="faint">None</span>'}</dd>`;
    h += `</dl></section>`;

    h += `<section class="d-section"><h3>Rule</h3><dl class="kv">`;
    h += `<dt>Shape</dt><dd>${tokensToHtml(d.shape_label, true)}${d.shape_key && r.shapes ? `<br><button class="link-btn" data-action="explore-shape">Explore this shape and run it as SPARQL</button>` : ""}</dd>`;
    if (d.shape && d.shape.t === "iri" && !d.shape_label.includes(d.shape.v)) h += `<dt>Shape IRI</dt><dd>${iriHtml(d.shape.v, true)}</dd>`;
    h += `<dt>Constraint</dt><dd>${d.constraint ? tokensToHtml(d.constraint, true) : r.has_shapes ? '<span class="faint">Not found in the shapes</span>' : '<span class="faint">Load the shapes file to see it</span>'}</dd>`;
    h += `<dt>Component</dt><dd>${d.component ? iriHtml(d.component, true) : '<span class="faint">Not stated</span>'}</dd>`;
    if (d.details) h += `<dt>Details</dt><dd>${plural(d.details, "nested result")}</dd>`;
    h += `</dl></section>`;

    h += `<section class="d-section"><h3>Resource data</h3>`;
    if (!r.has_data) {
      h += `<div class="callout">Load the data file to see this resource's triples here.</div>`;
    } else if (d.focus.t !== "iri") {
      h += `<div class="callout">Blank nodes can't be matched between the report and the data file.</div>`;
    } else {
      const triples = r.focus[d.focusKey] || [];
      if (!triples.length) {
        h += `<div class="callout">This resource has no triples of its own in the data.</div>`;
      } else {
        h += `<table class="triples"><tbody>`;
        let hit = false;
        for (const t of triples) {
          const onPath = d.path_iri && t.p === d.path_iri;
          const isHit = onPath && d.value && t.o.t === d.value.t && t.o.v === d.value.v;
          if (isHit) hit = true;
          h += `<tr class="${isHit ? "hit" : onPath ? "on-path" : ""}"><td>${iriHtml(t.p, true)}</td><td>${termHtml(t.o, true)}</td></tr>`;
        }
        h += `</tbody></table>`;
        if (d.path_iri && !triples.some((t) => t.p === d.path_iri)) {
          h += `<div class="callout missing">This resource has no ${esc(iriText(d.path_iri))} at all.</div>`;
        } else if (hit) {
          h += `<div class="callout">The highlighted row holds the offending value.</div>`;
        }
      }
    }
    h += `</section>`;

    const same = state.byFocus.get(d.focusKey) || [];
    if (same.length > 1) {
      const active = state.resource === d.focusKey;
      h += `<section class="d-section"><div class="d-related"><span>${plural(same.length, "problem")} on this resource</span><button class="btn" data-action="${active ? "clear-resource" : "only-resource"}">${active ? "Show all resources" : "Show only these"}</button></div></section>`;
    }
    els.drawerBody.innerHTML = h;
    els.drawerBody.scrollTop = 0;
    syncDrawerNav();
  }

  // ---------- export ----------

  function exportCsv() {
    const cols = [
      ["severity", (d) => sevLabel(d.severity)],
      ["resource", (d) => d.focus.v],
      ["problem", (d) => d.problem],
      ["explanation", (d) => d.explanationText],
      ["path", (d) => d.pathText],
      ["value", (d) => (d.value ? d.value.v : "")],
      ["message", (d) => d.msgText],
      ["shape", (d) => d.shapeText],
      ["constraint", (d) => tokensToText(d.constraint || "")],
      ["component", (d) => d.component || ""],
    ];
    const q = (v) => `"${String(v).replace(/"/g, '""')}"`;
    const lines = [cols.map((c) => c[0]).join(",")];
    for (const d of state.order) lines.push(cols.map((c) => q(c[1](d))).join(","));
    const blob = new Blob(["﻿" + lines.join("\r\n")], { type: "text/csv;charset=utf-8" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = "shacl-report.csv";
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
    toast(`Exported ${plural(state.order.length, "result")}`);
  }

  // ---------- open form (server mode) ----------

  function mountOpenForm(slot) {
    slot.innerHTML = "";
    slot.appendChild(els.formTpl.content.cloneNode(true));
    const form = slot.querySelector("form");
    const files = { report: [], shapes: [], data: [] };
    const submit = form.querySelector("button[type=submit]");
    const msg = form.querySelector(".form-msg");
    const pathReport = form.querySelector("[name=path-report]");
    const pathShapes = form.querySelector("[name=path-shapes]");

    const refresh = () => {
      submit.disabled = !(files.report.length || files.shapes.length || pathReport.value.trim() || pathShapes.value.trim());
    };

    for (const drop of form.querySelectorAll(".drop")) {
      const slotName = drop.dataset.slot;
      const input = drop.querySelector("input");
      const list = drop.querySelector(".drop-files");
      const setFiles = (fl) => {
        const arr = [...fl];
        files[slotName] = slotName === "report" ? arr.slice(0, 1) : arr;
        list.textContent = files[slotName].map((f) => f.name).join(", ");
        drop.classList.toggle("filled", files[slotName].length > 0);
        refresh();
      };
      input.addEventListener("change", () => setFiles(input.files));
      drop.addEventListener("dragover", (e) => { e.preventDefault(); drop.classList.add("dragover"); });
      drop.addEventListener("dragleave", () => drop.classList.remove("dragover"));
      drop.addEventListener("drop", (e) => {
        e.preventDefault();
        drop.classList.remove("dragover");
        if (e.dataTransfer && e.dataTransfer.files.length) setFiles(e.dataTransfer.files);
      });
    }
    pathReport.addEventListener("input", refresh);
    pathShapes.addEventListener("input", refresh);

    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      msg.className = "form-msg busy";
      msg.textContent = "Opening…";
      submit.disabled = true;
      try {
        let resp;
        if (pathReport.value.trim() || pathShapes.value.trim()) {
          const split = (s) => s.split(",").map((x) => x.trim()).filter(Boolean);
          resp = await fetch("/api/load", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              report: pathReport.value.trim() || null,
              shapes: split(pathShapes.value),
              data: split(form.querySelector("[name=path-data]").value),
            }),
          });
        } else {
          const enc = async (f) => ({ name: f.name, content: await toBase64(f) });
          msg.textContent = "Reading files…";
          const body = {
            report: files.report.length ? await enc(files.report[0]) : null,
            shapes: await Promise.all(files.shapes.map(enc)),
            data: await Promise.all(files.data.map(enc)),
          };
          msg.textContent = files.report.length ? "Analysing report…" : "Reading shapes…";
          resp = await fetch("/api/upload", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify(body),
          });
        }
        if (!resp.ok) throw new Error(await resp.text());
        hideModal();
        await loadFromServer();
      } catch (err) {
        msg.className = "form-msg";
        msg.textContent = err.message;
        submit.disabled = false;
      }
    });
    return form;
  }

  function toBase64(file) {
    return new Promise((resolve, reject) => {
      const fr = new FileReader();
      fr.onload = () => resolve(String(fr.result).replace(/^data:[^,]*,/, ""));
      fr.onerror = () => reject(new Error(`Could not read ${file.name}`));
      fr.readAsDataURL(file);
    });
  }

  function showModal() {
    mountOpenForm(els.formSlot);
    els.modal.classList.remove("hidden");
    const first = els.formSlot.querySelector(".drop");
    if (first) first.focus();
  }

  function hideModal() {
    els.modal.classList.add("hidden");
    els.formSlot.innerHTML = "";
  }

  // ---------- misc ui ----------

  let toastTimer = null;
  function toast(text) {
    els.toast.textContent = text;
    els.toast.classList.remove("hidden");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => els.toast.classList.add("hidden"), 2200);
  }

  function setLoading(on, text) {
    els.loadingMsg.textContent = text || "Loading report…";
    els.loading.classList.toggle("hidden", !on);
  }

  function storedTheme() {
    try { return localStorage.getItem(THEME_KEY); } catch (_) { return null; }
  }

  function applyTheme(t) {
    if (t === "light" || t === "dark") document.documentElement.setAttribute("data-theme", t);
    else document.documentElement.removeAttribute("data-theme");
    const dark = t ? t === "dark" : window.matchMedia("(prefers-color-scheme: dark)").matches;
    els.themeBtn.innerHTML = dark ? ICON.sun : ICON.moon;
    els.themeBtn.title = dark ? "Switch to light theme" : "Switch to dark theme";
  }

  function toggleTheme() {
    const cur = storedTheme();
    const dark = cur ? cur === "dark" : window.matchMedia("(prefers-color-scheme: dark)").matches;
    const next = dark ? "light" : "dark";
    try { localStorage.setItem(THEME_KEY, next); } catch (_) {}
    applyTheme(next);
  }

  function openFilters(open) {
    els.filters.classList.toggle("open", open);
    els.scrim.classList.toggle("hidden", !open);
  }

  function setSegmented(el, value) {
    for (const b of el.querySelectorAll("button")) {
      const on = b.dataset.value === value;
      b.classList.toggle("active", on);
      b.setAttribute("aria-checked", String(on));
    }
  }

  // ---------- loading ----------

  function setReport(report) {
    state.report = report;
    state.prefixes = (report.prefixes || []).slice().sort((a, b) => b[1].length - a[1].length);
    state.rows = decorate(report.results || []);
    state.byFocus = new Map();
    for (const d of state.rows) {
      const list = state.byFocus.get(d.focusKey);
      if (list) list.push(d);
      else state.byFocus.set(d.focusKey, [d]);
    }
    state.sevOn = new Set(SEVERITIES);
    state.problem = state.shape = state.resource = null;
    state.search = "";
    els.search.value = "";
    state.collapsed = new Set();
    state.shapeFacetAll = false;
    closeDrawer();
    state.shapeKeys = null;
    const src = report.sources || {};
    const base = (p) => (p || "").split(/[\\/]/).pop();
    els.fileName.textContent = report.has_report === false ? (src.shapes || []).map(base).join(", ") : base(src.report);
    document.title = `${els.fileName.textContent || "SHACL report"} · shacl-report`;
    els.welcome.classList.add("hidden");
    els.exportBtn.parentElement.classList.remove("hidden");
    els.summary.classList.remove("hidden");
    els.results.classList.toggle("hidden", state.rows.length === 0 && report.conforms !== false);
    els.filters.classList.remove("hidden");
    const hasReport = report.has_report !== false;
    const hasShapes = !!report.shapes;
    els.tabs.classList.toggle("hidden", !hasShapes && !hasReport);
    els.tabs.querySelector('[data-tab="results"]').classList.toggle("hidden", !hasReport);
    els.tabs.querySelector('[data-tab="shapes"]').classList.toggle("hidden", !hasShapes);
    els.tabs.querySelector('[data-tab="sparql"]').classList.toggle("hidden", !hasShapes);
    els.exportMenu.querySelector('[data-export="csv"]').classList.toggle("hidden", !hasReport);
    if (hasShapes) {
      if (!state.explorer) {
        state.explorer = window.ShaclShapes.create({
          els, esc, tokensToHtml, tokensToText, iriHtml, iriText, plural, sevClass: SEV_CLASS,
          prefixes: () => state.report.prefixes || [],
          copy: copyText,
          showResults: (keys, label) => {
            state.shapeKeys = keys;
            state.shapeKeysLabel = label;
            state.shape = null;
            switchTab("results");
            update();
            els.results.scrollIntoView({ block: "start" });
          },
          runQuery: (name, query) => {
            switchTab("sparql");
            state.editor.open(name, query);
          },
        });
      }
      if (!state.editor) {
        state.editor = new window.ShaclSparql.Editor(els.sparqlView, {
          server: !state.standalone,
          preset: report.sparql_endpoint,
          toast,
          onNeedEndpoint: () => toast("Enter the SPARQL endpoint of your data; the query runs as soon as you press Use endpoint"),
        });
      }
      state.explorer.setModel(report.shapes, state.rows, hasReport);
    }
    if (hasReport) update();
    switchTab(hasReport ? "results" : "shapes");
  }

  function switchTab(tab) {
    state.tab = tab;
    document.body.dataset.tab = tab;
    els.resultsView.classList.toggle("hidden", tab !== "results");
    els.shapesView.classList.toggle("hidden", tab !== "shapes");
    els.sparqlView.classList.toggle("hidden", tab !== "sparql");
    for (const b of els.tabs.querySelectorAll("button")) {
      const on = b.dataset.tab === tab;
      b.classList.toggle("active", on);
      b.setAttribute("aria-selected", String(on));
    }
    if (tab !== "results") closeDrawer();
    if (tab === "sparql" && state.editor) state.editor.ensure();
  }

  async function copyText(text) {
    try {
      await navigator.clipboard.writeText(text);
      toast("Copied to clipboard");
    } catch (_) {
      toast("Copy is not available here");
    }
  }

  function showWelcome() {
    els.tabs.classList.add("hidden");
    switchTab("results");
    els.summary.classList.add("hidden");
    els.results.classList.add("hidden");
    els.filters.classList.add("hidden");
    els.exportBtn.parentElement.classList.add("hidden");
    els.welcome.innerHTML = `<div class="welcome-hero"><h1>Make sense of SHACL reports and shapes</h1><p>See which shapes fail and why in plain language, explore every rule in your shapes, and run them as SPARQL against your triple store.</p></div><div id="welcome-form"></div>`;
    els.welcome.classList.remove("hidden");
    mountOpenForm(document.getElementById("welcome-form"));
  }

  async function loadFromServer() {
    const meta = await (await fetch("/api/meta")).json();
    if (meta.version) els.version.textContent = `v${meta.version}`;
    els.openFiles.classList.remove("hidden");
    for (const el of document.querySelectorAll(".server-only")) el.classList.remove("hidden");
    if (!meta.loaded) {
      showWelcome();
      return;
    }
    setLoading(true);
    try {
      const resp = await fetch("/api/report");
      if (!resp.ok) throw new Error(await resp.text());
      setReport(await resp.json());
    } finally {
      setLoading(false);
    }
  }

  // ---------- events ----------

  function bind() {
    let timer = null;
    els.search.addEventListener("input", () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        state.search = els.search.value.trim().toLowerCase();
        update();
      }, 120);
    });
    els.shapeSearch.addEventListener("input", () => {
      state.shapeFacetQuery = els.shapeSearch.value.trim().toLowerCase();
      renderFilters();
    });
    els.tabs.addEventListener("click", (e) => {
      const b = e.target.closest("[data-tab]");
      if (b) switchTab(b.dataset.tab);
    });
    els.groupBy.addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (!b) return;
      state.groupBy = b.dataset.value;
      state.collapsed = new Set();
      setSegmented(els.groupBy, state.groupBy);
      update();
    });
    els.viewToggle.addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (!b) return;
      state.view = b.dataset.value;
      setSegmented(els.viewToggle, state.view);
      renderResults();
    });
    els.groups.addEventListener("click", (e) => {
      const row = e.target.closest(".row");
      if (row) {
        const d = state.rows[Number(row.dataset.id)];
        if (d) openDrawer(d);
        return;
      }
      if (e.target.closest("[data-action=reset]")) resetFilters();
    });
    els.drawerBody.addEventListener("click", async (e) => {
      const c = e.target.closest("[data-copy]");
      if (c) {
        try {
          await navigator.clipboard.writeText(c.dataset.copy);
          toast("Copied to clipboard");
        } catch (_) {
          toast("Copy is not available here");
        }
        return;
      }
      const a = e.target.closest("[data-action]");
      if (!a || !state.selected) return;
      if (a.dataset.action === "explore-shape") {
        const key = state.selected.shape_key;
        switchTab("shapes");
        state.explorer.showKey(key);
        return;
      }
      state.resource = a.dataset.action === "only-resource" ? state.selected.focusKey : null;
      update();
      renderDrawer();
    });
    els.prev.addEventListener("click", () => step(-1));
    els.next.addEventListener("click", () => step(1));
    els.closeDrawer.addEventListener("click", closeDrawer);
    els.resetFilters.addEventListener("click", resetFilters);
    els.themeBtn.addEventListener("click", toggleTheme);
    els.toggleFilters.addEventListener("click", () => openFilters(true));
    els.closeFilters.addEventListener("click", () => openFilters(false));
    els.scrim.addEventListener("click", () => openFilters(false));
    els.openFiles.addEventListener("click", showModal);
    els.closeModal.addEventListener("click", hideModal);
    els.modal.addEventListener("click", (e) => { if (e.target === els.modal) hideModal(); });
    els.exportBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = els.exportMenu.classList.toggle("hidden") === false;
      els.exportBtn.setAttribute("aria-expanded", String(open));
    });
    els.exportMenu.addEventListener("click", (e) => {
      const b = e.target.closest("[data-export]");
      if (!b) return;
      els.exportMenu.classList.add("hidden");
      if (!state.report) return;
      if (b.dataset.export === "csv") exportCsv();
      else window.location.href = "/api/export.html";
    });
    document.addEventListener("click", () => els.exportMenu.classList.add("hidden"));

    document.addEventListener("keydown", (e) => {
      const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement && document.activeElement.tagName);
      if (e.key === "Escape") {
        if (!els.modal.classList.contains("hidden")) hideModal();
        else if (els.filters.classList.contains("open")) openFilters(false);
        else if (state.selected) closeDrawer();
        else if (typing) document.activeElement.blur();
        return;
      }
      if (typing || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === "/") {
        e.preventDefault();
        if (state.tab === "shapes") els.shapeSearch2.focus();
        else if (state.tab === "results") els.search.focus();
      } else if (state.selected && (e.key === "j" || e.key === "ArrowDown")) {
        e.preventDefault();
        step(1);
      } else if (state.selected && (e.key === "k" || e.key === "ArrowUp")) {
        e.preventDefault();
        step(-1);
      }
    });
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => applyTheme(storedTheme()));
  }

  function resetFilters() {
    state.sevOn = new Set(SEVERITIES);
    state.problem = state.shape = state.resource = null;
    state.shapeKeys = null;
    state.search = "";
    els.search.value = "";
    update();
  }

  function init() {
    applyTheme(storedTheme());
    bind();
    if (state.standalone) {
      if (window.SHACL_REPORT_VERSION) els.version.textContent = `v${window.SHACL_REPORT_VERSION}`;
      setReport(window.SHACL_REPORT);
      return;
    }
    loadFromServer().catch((e) => {
      els.welcome.innerHTML = `<div class="empty-results"><h3>Could not load the report</h3><p>${esc(e.message)}</p></div>`;
      els.welcome.classList.remove("hidden");
    });
  }

  init();
})();
