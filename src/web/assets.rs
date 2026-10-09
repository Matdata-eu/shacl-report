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
        "/assets/vendor/tabulator/tabulator.min.js",
        "application/javascript",
        "assets/vendor/tabulator/tabulator.min.js"
    ),
    txt!(
        "/assets/vendor/tabulator/tabulator.min.css",
        "text/css; charset=utf-8",
        "assets/vendor/tabulator/tabulator.min.css"
    ),
];

pub fn lookup(path: &str) -> Option<&'static Asset> {
    ASSETS.iter().find(|a| a.path == path)
}
