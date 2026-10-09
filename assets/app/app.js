// shacl-report web viewer — Tabulator-driven SHACL validation report browser.
(function () {
  const SH = "http://www.w3.org/ns/shacl#";
  const XSD_STRING = "http://www.w3.org/2001/XMLSchema#string";
  const SEVERITIES = ["Violation", "Warning", "Info"];

  const state = {
    report: null,
    prefixes: [], // sorted by length desc for longest-prefix-wins shortening
    table: null,
    standalone: !!window.SHACL_REPORT,
    severityOff: new Set(), // severities hidden via the chips
    shapeFilter: null,
    componentFilter: null,
    search: "",
    selectedId: null,
  };

  const els = {
    version: document.getElementById("version"),
    conforms: document.getElementById("conforms"),
    meta: document.getElementById("meta"),
    chips: document.getElementById("severity-chips"),
    search: document.getElementById("search"),
    groupBy: document.getElementById("group-by"),
    csv: document.getElementById("download-csv"),
    openLoad: document.getElementById("open-load"),
    loader: document.getElementById("loader"),
    doLoad: document.getElementById("do-load"),
    pathReport: document.getElementById("path-report"),
    pathData: document.getElementById("path-data"),
    pathShapes: document.getElementById("path-shapes"),
    loaderMsg: document.getElementById("loader-msg"),
    shapeList: document.getElementById("shape-list"),
    componentList: document.getElementById("component-list"),
    empty: document.getElementById("empty-state"),
    table: document.getElementById("table"),
    details: document.getElementById("details"),
    detailsBody: document.getElementById("details-body"),
    closeDetails: document.getElementById("close-details"),
    overlay: document.getElementById("loading-overlay"),
    overlayMsg: document.getElementById("loading-msg"),
  };

  // ---------- rendering helpers ----------

  function escapeHtml(s) {
    return String(s)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function shortenIri(iri) {
    for (const [name, base] of state.prefixes) {
      if (base && iri.startsWith(base)) {
        const local = iri.slice(base.length);
        if (/^[A-Za-z_][\w.\-]*$/.test(local) || local === "") return `${name}:${local}`;
      }
    }
    return null;
  }

  function iriText(iri) {
    return shortenIri(iri) || `<${iri}>`;
  }

  function renderIri(iri) {
    const safeIri = escapeHtml(iri);
    const isLink = /^https?:\/\//i.test(iri);
    const text = escapeHtml(iriText(iri));
    if (!isLink) return `<span class="iri" title="${safeIri}">${text}</span>`;
    return `<a class="iri-link" href="${safeIri}" target="_blank" rel="noopener noreferrer" title="${safeIri}">${text}</a>`;
  }

  const TOKEN_RE = /<([A-Za-z][\w+.\-]*:[^\s<>"]*)>/g;

  // Text with <iri> tokens → plain text with shortened IRIs.
  function tokensToText(s) {
    if (!s) return "";
    return s.replace(TOKEN_RE, (_, iri) => iriText(iri));
  }

  // Text with <iri> tokens → HTML with linked, shortened IRIs.
  function tokensToHtml(s) {
    if (!s) return "";
    let out = "";
    let last = 0;
    s.replace(TOKEN_RE, (m, iri, offset) => {
      out += escapeHtml(s.slice(last, offset)) + renderIri(iri);
      last = offset + m.length;
      return m;
    });
    return out + escapeHtml(s.slice(last));
  }

  function termText(o) {
    if (!o) return "";
    if (o.t === "iri") return iriText(o.v);
    if (o.t === "bnode") return `_:${o.v}`;
    if (o.lng) return `"${o.v}"@${o.lng}`;
    if (o.dt && o.dt !== XSD_STRING) return `"${o.v}"^^${iriText(o.dt)}`;
    return `"${o.v}"`;
  }

  function renderTerm(o) {
    if (!o) return "";
    if (o.t === "iri") return renderIri(o.v);
    if (o.t === "bnode") return `<span class="bnode">_:${escapeHtml(o.v)}</span>`;
    const v = escapeHtml(o.v);
    if (o.lng) return `"${v}"<span class="lit-dt">@${escapeHtml(o.lng)}</span>`;
    if (o.dt && o.dt !== XSD_STRING) return `"${v}"<span class="lit-dt">^^${renderIri(o.dt)}</span>`;
    return `"${v}"`;
  }

  function termKey(o) {
    if (o.t === "iri") return `<${o.v}>`;
    if (o.t === "bnode") return `_:${o.v}`;
    return `"${o.v}"`;
  }

  function componentShort(iri) {
    if (!iri) return "";
    if (iri.startsWith(SH)) return iri.slice(SH.length).replace(/ConstraintComponent$/, "");
    return iriText(iri);
  }

  function severityClass(sev) {
    if (sev === "Violation") return "sev-violation";
    if (sev === "Warning") return "sev-warning";
    if (sev === "Info") return "sev-info";
    return "sev-other";
  }

  function severityLabel(sev) {
    return SEVERITIES.includes(sev) ? sev : iriText(sev);
  }

  function defaultMessage(r) {
    const c = componentShort(r.component) || "Constraint";
    return r.constraint ? `${c} violated: ${tokensToText(r.constraint)}` : `${c} violated`;
  }

  // ---------- table ----------

  function decorate(results) {
    // Precompute plain-text fields used for grouping, filtering and CSV.
    return results.map((r) => ({
      ...r,
      focus_key: termText(r.focus),
      path_text: r.path ? tokensToText(r.path) : "(node)",
      value_text: termText(r.value),
      message_text: r.messages.length ? r.messages.join(" · ") : defaultMessage(r),
      shape_text: tokensToText(r.shape_label),
      component_short: componentShort(r.component),
    }));
  }

  function severityFormatter(cell) {
    const v = cell.getValue();
    return `<span class="sev-badge ${severityClass(v)}">${escapeHtml(severityLabel(v))}</span>`;
  }

  function sevSorter(a, b) {
    const ra = SEVERITIES.indexOf(a);
    const rb = SEVERITIES.indexOf(b);
    return (ra < 0 ? 9 : ra) - (rb < 0 ? 9 : rb);
  }

  function groupHeader(value, count, data) {
    const field = els.groupBy.value;
    const sev = { Violation: 0, Warning: 0, Info: 0 };
    for (const d of data) if (d.severity in sev) sev[d.severity] += 1;
    const parts = SEVERITIES.filter((s) => sev[s] > 0).map(
      (s) => `<span class="sev-count ${severityClass(s)}">${sev[s]} ${s.toLowerCase()}${sev[s] === 1 || s === "Info" ? "" : "s"}</span>`
    );
    let label;
    if (field === "shape_label") label = tokensToHtml(value);
    else if (field === "severity") label = escapeHtml(severityLabel(value));
    else label = escapeHtml(value);
    return `<span class="group-label">${label}</span> <span class="group-count">${count} result${count === 1 ? "" : "s"}</span> ${parts.join(" ")}`;
  }

  function buildTable() {
    state.table = new Tabulator(els.table, {
      height: "100%",
      layout: "fitColumns",
      placeholder: "No results",
      selectableRows: 1,
      groupBy: els.groupBy.value || false,
      groupHeader: groupHeader,
      groupStartOpen: true,
      columns: [
        {
          title: "Severity",
          field: "severity",
          width: 105,
          formatter: severityFormatter,
          sorter: sevSorter,
        },
        {
          title: "Focus node",
          field: "focus_key",
          formatter: (c) => renderTerm(c.getRow().getData().focus),
          minWidth: 160,
        },
        {
          title: "Path",
          field: "path_text",
          formatter: (c) => {
            const d = c.getRow().getData();
            return d.path ? tokensToHtml(d.path) : '<span class="muted">(node)</span>';
          },
          minWidth: 120,
        },
        {
          title: "Value",
          field: "value_text",
          formatter: (c) => renderTerm(c.getRow().getData().value),
          minWidth: 120,
        },
        {
          title: "Message",
          field: "message_text",
          formatter: (c) => {
            const d = c.getRow().getData();
            const cls = d.messages.length ? "" : "muted";
            return `<span class="${cls}">${escapeHtml(c.getValue())}</span>`;
          },
          minWidth: 220,
          widthGrow: 2,
        },
        {
          title: "Shape",
          field: "shape_text",
          formatter: (c) => tokensToHtml(c.getRow().getData().shape_label),
          minWidth: 160,
        },
        { title: "Constraint", field: "component_short", width: 140 },
      ],
    });
    state.table.on("rowClick", (_e, row) => showDetails(row.getData()));
  }

  function rowMatches(d) {
    if (state.severityOff.has(SEVERITIES.includes(d.severity) ? d.severity : "Other")) return false;
    if (state.shapeFilter !== null && d.shape_label !== state.shapeFilter) return false;
    if (state.componentFilter !== null && d.component_short !== state.componentFilter) return false;
    if (state.search) {
      const hay = `${d.focus_key} ${d.focus.v} ${d.path_text} ${d.value_text} ${d.message_text} ${d.shape_text} ${d.component_short}`.toLowerCase();
      if (!hay.includes(state.search)) return false;
    }
    return true;
  }

  function applyFilters() {
    if (!state.table) return;
    state.table.setFilter(rowMatches);
    renderFacets();
  }

  // ---------- header, chips, facets ----------

  function renderHeader() {
    const r = state.report;
    els.conforms.classList.remove("hidden", "ok", "fail", "unknown");
    if (r.conforms === true) {
      els.conforms.textContent = "✓ Conforms";
      els.conforms.classList.add("ok");
    } else if (r.conforms === false) {
      els.conforms.textContent = "✗ Does not conform";
      els.conforms.classList.add("fail");
    } else {
      els.conforms.textContent = "Conformance not stated";
      els.conforms.classList.add("unknown");
    }
    const s = r.stats;
    const src = r.sources || {};
    const name = (p) => (p || "").split(/[\\/]/).pop();
    let meta = `${escapeHtml(name(src.report))} · ${s.total} result${s.total === 1 ? "" : "s"} on ${s.focus_nodes} focus node${s.focus_nodes === 1 ? "" : "s"}`;
    if (src.data && src.data.length) meta += ` · data: ${src.data.map((p) => escapeHtml(name(p))).join(", ")}`;
    if (src.shapes && src.shapes.length) meta += ` · shapes: ${src.shapes.map((p) => escapeHtml(name(p))).join(", ")}`;
    els.meta.innerHTML = meta;

    const counts = { Violation: s.violations, Warning: s.warnings, Info: s.infos, Other: s.other };
    els.chips.innerHTML = "";
    for (const sev of [...SEVERITIES, "Other"]) {
      if (!counts[sev]) continue;
      const b = document.createElement("button");
      b.className = `chip ${sev === "Other" ? "sev-other" : severityClass(sev)}`;
      if (state.severityOff.has(sev)) b.classList.add("off");
      b.textContent = `${counts[sev]} ${sev}${counts[sev] === 1 || sev === "Info" || sev === "Other" ? "" : "s"}`;
      b.title = `Show or hide ${sev.toLowerCase()} results`;
      b.addEventListener("click", () => {
        if (state.severityOff.has(sev)) state.severityOff.delete(sev);
        else state.severityOff.add(sev);
        b.classList.toggle("off");
        applyFilters();
      });
      els.chips.appendChild(b);
    }
  }

  function renderFacet(listEl, key, current, labelFn, onPick) {
    const counts = new Map();
    const saveShape = state.shapeFilter;
    const saveComp = state.componentFilter;
    // Facet counts honour every filter except the facet's own.
    if (key === "shape_label") state.shapeFilter = null;
    else state.componentFilter = null;
    for (const d of state.rows) {
      if (!rowMatches(d)) continue;
      counts.set(d[key], (counts.get(d[key]) || 0) + 1);
    }
    state.shapeFilter = saveShape;
    state.componentFilter = saveComp;

    const entries = [...counts.entries()].sort((a, b) => b[1] - a[1] || String(a[0]).localeCompare(String(b[0])));
    listEl.innerHTML = "";
    if (entries.length === 0) {
      listEl.innerHTML = '<li class="muted">None</li>';
      return;
    }
    for (const [value, n] of entries) {
      const li = document.createElement("li");
      li.className = value === current ? "active" : "";
      li.innerHTML = `<span class="facet-label">${labelFn(value)}</span><span class="facet-count">${n}</span>`;
      li.title = value === current ? "Click to clear this filter" : "Click to show only these results";
      li.addEventListener("click", (e) => {
        if (e.target.closest("a")) return;
        onPick(value === current ? null : value);
      });
      listEl.appendChild(li);
    }
  }

  function renderFacets() {
    renderFacet(els.shapeList, "shape_label", state.shapeFilter, tokensToHtml, (v) => {
      state.shapeFilter = v;
      applyFilters();
    });
    renderFacet(els.componentList, "component_short", state.componentFilter, escapeHtml, (v) => {
      state.componentFilter = v;
      applyFilters();
    });
  }

  // ---------- details panel ----------

  function field(label, html) {
    return `<dt>${escapeHtml(label)}</dt><dd>${html || '<span class="muted">—</span>'}</dd>`;
  }

  function showDetails(d) {
    state.selectedId = d.id;
    const r = state.report;
    let html = `<p><span class="sev-badge ${severityClass(d.severity)}">${escapeHtml(severityLabel(d.severity))}</span></p>`;
    html += "<dl>";
    html += field("Message", d.messages.length ? d.messages.map(escapeHtml).join("<br>") : `<span class="muted">${escapeHtml(defaultMessage(d))}</span>`);
    html += field("Focus node", renderTerm(d.focus));
    html += field("Path", d.path ? tokensToHtml(d.path) : "");
    html += field("Value", renderTerm(d.value));
    html += field("Shape", tokensToHtml(d.shape_label));
    if (d.shape && d.shape.t === "iri") html += field("Shape IRI", renderIri(d.shape.v));
    html += field("Constraint component", d.component ? renderIri(d.component) : "");
    html += field("Constraint", d.constraint ? tokensToHtml(d.constraint) : (r.has_shapes ? "" : '<span class="muted">pass --shapes to see the constraint</span>'));
    if (d.details) html += field("Nested details", `${d.details} sh:detail result(s)`);
    html += "</dl>";

    html += "<h3>Focus node in the data graph</h3>";
    if (!r.has_data) {
      html += '<p class="muted">Pass <code>--data</code> to see the focus node\'s triples here.</p>';
    } else if (d.focus.t !== "iri") {
      html += '<p class="muted">Blank-node focus nodes cannot be matched to the data graph.</p>';
    } else {
      const triples = r.focus[termKey(d.focus)] || [];
      if (triples.length === 0) {
        html += '<p class="muted">No triples with this focus node as subject.</p>';
      } else {
        let hasHit = false;
        html += '<table class="ctx"><tbody>';
        for (const t of triples) {
          const onPath = d.path_iri && t.p === d.path_iri;
          const isValue = onPath && d.value && t.o.v === d.value.v && t.o.t === d.value.t;
          if (isValue) hasHit = true;
          const cls = isValue ? "hit" : onPath ? "on-path" : "";
          html += `<tr class="${cls}"><td>${renderIri(t.p)}</td><td>${renderTerm(t.o)}</td></tr>`;
        }
        html += "</tbody></table>";
        if (d.path_iri && !triples.some((t) => t.p === d.path_iri)) {
          html += `<p class="note">The focus node has no value for ${renderIri(d.path_iri)}.</p>`;
        } else if (hasHit) {
          html += '<p class="note">The highlighted triple holds the offending value.</p>';
        }
      }
    }
    els.detailsBody.innerHTML = html;
    els.details.classList.remove("hidden");
  }

  function hideDetails() {
    els.details.classList.add("hidden");
    state.selectedId = null;
    if (state.table) state.table.deselectRow();
  }

  // ---------- loading ----------

  function setReport(report) {
    state.report = report;
    state.prefixes = (report.prefixes || []).slice().sort((a, b) => b[1].length - a[1].length);
    state.rows = decorate(report.results || []);
    state.shapeFilter = null;
    state.componentFilter = null;
    hideDetails();
    renderHeader();
    els.empty.classList.add("hidden");
    if (state.rows.length === 0) {
      els.empty.innerHTML = report.conforms === false
        ? "<p>The report states the data does not conform, but it lists no results.</p>"
        : "<p>No validation results. The data conforms to the shapes.</p>";
      els.empty.classList.remove("hidden");
    }
    state.table.setData(state.rows).then(applyFilters);
  }

  async function loadFromServer() {
    const metaResp = await fetch("/api/meta");
    if (!metaResp.ok) throw new Error("/api/meta failed");
    const meta = await metaResp.json();
    if (meta.version) els.version.textContent = `v${meta.version}`;
    els.openLoad.classList.remove("hidden");
    if (!meta.loaded) {
      els.meta.textContent = "no report loaded";
      els.empty.innerHTML = "<p>No report loaded. Pass a report on the command line or use the loader above.</p>";
      els.empty.classList.remove("hidden");
      els.loader.classList.remove("hidden");
      return;
    }
    els.overlay.classList.remove("hidden");
    try {
      const resp = await fetch("/api/report");
      if (!resp.ok) throw new Error(await resp.text());
      setReport(await resp.json());
    } finally {
      els.overlay.classList.add("hidden");
    }
  }

  function splitPaths(s) {
    return s.split(",").map((p) => p.trim()).filter(Boolean);
  }

  els.openLoad.addEventListener("click", () => els.loader.classList.toggle("hidden"));
  els.doLoad.addEventListener("click", async () => {
    const body = {
      report: els.pathReport.value.trim(),
      data: splitPaths(els.pathData.value),
      shapes: splitPaths(els.pathShapes.value),
    };
    if (!body.report) {
      els.loaderMsg.textContent = "A report file is required.";
      return;
    }
    els.loaderMsg.textContent = "Loading…";
    try {
      const r = await fetch("/api/load", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!r.ok) throw new Error(await r.text());
      els.loaderMsg.textContent = "";
      els.loader.classList.add("hidden");
      await loadFromServer();
    } catch (e) {
      els.loaderMsg.textContent = "Error: " + e.message;
    }
  });

  let searchTimer = null;
  els.search.addEventListener("input", () => {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      state.search = els.search.value.trim().toLowerCase();
      applyFilters();
    }, 150);
  });

  els.groupBy.addEventListener("change", () => {
    state.table.setGroupBy(els.groupBy.value || false);
  });

  els.csv.addEventListener("click", () => {
    if (!state.table) return;
    state.table.download("csv", "shacl-report.csv", {}, "active");
  });

  els.closeDetails.addEventListener("click", hideDetails);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") hideDetails();
  });

  function init() {
    buildTable();
    state.rows = [];
    if (state.standalone) {
      if (window.SHACL_REPORT_VERSION) els.version.textContent = `v${window.SHACL_REPORT_VERSION}`;
      // Tabulator builds asynchronously; wait for it before loading data.
      state.table.on("tableBuilt", () => setReport(window.SHACL_REPORT));
      return;
    }
    state.table.on("tableBuilt", () => {
      loadFromServer().catch((e) => {
        els.meta.textContent = "init error: " + e.message;
      });
    });
  }

  init();
})();
