//! Embedded static assets for the web viewer.

pub struct Asset {
    pub path: &'static str,
    pub mime: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! txt {
    ($p:literal, $mime:literal, $file:literal) => {
        Asset {
            path: $p,
            mime: $mime,
            bytes: include_bytes!(concat!("../../", $file)),
        }
    };
}

pub static ASSETS: &[Asset] = &[
    txt!(
        "/assets/app/index.html",
        "text/html; charset=utf-8",
        "assets/app/index.html"
    ),
    txt!(
        "/assets/app/app.js",
        "application/javascript",
        "assets/app/app.js"
    ),
    txt!(
        "/assets/app/styles.css",
        "text/css; charset=utf-8",
        "assets/app/styles.css"
    ),
    txt!(
        "/assets/app/sparql.js",
        "application/javascript",
        "assets/app/sparql.js"
    ),
    txt!(
        "/assets/app/shapes.js",
        "application/javascript",
        "assets/app/shapes.js"
    ),
    txt!(
        "/assets/app/coverage.js",
        "application/javascript",
        "assets/app/coverage.js"
    ),
    txt!(
        "/assets/app/turtle-view.js",
        "application/javascript",
        "assets/app/turtle-view.js"
    ),
    txt!(
        "/assets/vendor/codemirror/codemirror.min.js",
        "application/javascript",
        "assets/vendor/codemirror/codemirror.min.js"
    ),
    txt!(
        "/assets/vendor/matgui/matgui.min.js",
        "application/javascript",
        "assets/vendor/matgui/matgui.min.js"
    ),
    txt!(
        "/assets/vendor/matgui/matgui.min.css",
        "text/css; charset=utf-8",
        "assets/vendor/matgui/matgui.min.css"
    ),
    txt!(
        "/assets/vendor/tabulator/tabulator.min.js",
        "application/javascript",
        "assets/vendor/tabulator/tabulator.min.js"
    ),
    txt!(
        "/assets/vendor/tabulator/tabulator.min.css",
        "text/css; charset=utf-8",
        "assets/vendor/tabulator/tabulator.min.css"
    ),
    // Demo data sets, loaded from the welcome screen or with `?demo=<id>`.
    txt!(
        "/assets/demos/library-shapes.ttl",
        "text/turtle; charset=utf-8",
        "assets/demos/library-shapes.ttl"
    ),
    txt!(
        "/assets/demos/library-data.ttl",
        "text/turtle; charset=utf-8",
        "assets/demos/library-data.ttl"
    ),
    txt!(
        "/assets/demos/library-report-pyshacl.ttl",
        "text/turtle; charset=utf-8",
        "assets/demos/library-report-pyshacl.ttl"
    ),
    txt!(
        "/assets/demos/railway-shapes.ttl",
        "text/turtle; charset=utf-8",
        "assets/demos/railway-shapes.ttl"
    ),
    txt!(
        "/assets/demos/railway-data.ttl",
        "text/turtle; charset=utf-8",
        "assets/demos/railway-data.ttl"
    ),
];

pub fn lookup(path: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|a| a.path == path)
}
