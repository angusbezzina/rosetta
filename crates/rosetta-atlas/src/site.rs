//! Read-only static Atlas export that opens from a file or a static host.

use std::path::Path;

use crate::atlas;
use crate::open::Brand;

const TEMPLATE: &str = include_str!("../viewer-dist/site-index.html");
const SCRIPT: &[u8] = include_bytes!("../viewer-dist/site-viewer.js");
const STYLES: &[u8] = include_bytes!("../viewer-dist/atlas-viewer.css");
const DATA_MARKER: &str = "__ROSETTA_INLINE_DATA__";

#[derive(Debug, thiserror::Error)]
pub enum SiteError {
    #[error(transparent)]
    Atlas(#[from] atlas::AtlasLoadError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("bundled static page has no data marker")]
    MissingDataMarker,
}

fn write(out: &Path, name: &str, bytes: &[u8]) -> Result<(), SiteError> {
    let path = out.join(name);
    std::fs::write(&path, bytes).map_err(|source| SiteError::Write {
        path: path.display().to_string(),
        source,
    })
}

/// Write one snapshot and its complete standalone viewer. Existing generated
/// filenames are updated, and unrelated files in `out` are left alone.
pub fn export(root: &Path, source_dir: &str, brand: &Brand, out: &Path) -> Result<(), SiteError> {
    let bundle = atlas::bundle_repo_from(root, source_dir)?;
    let bundle_json = serde_json::to_string_pretty(&bundle)?;
    let brand_json = serde_json::to_string_pretty(brand)?;
    // A repository name, label or excerpt can contain `</script>`. Escape `<`
    // before embedding JSON in an executable script so it stays data.
    let inline_bundle = serde_json::to_string(&bundle)?.replace('<', "\\u003c");
    let inline_brand = serde_json::to_string(brand)?.replace('<', "\\u003c");
    if !TEMPLATE.contains(DATA_MARKER) {
        return Err(SiteError::MissingDataMarker);
    }
    let page = TEMPLATE.replace(
        DATA_MARKER,
        &format!(
            "window.__ATLAS__={{bundle:{inline_bundle},brand:{inline_brand},staticSite:true}};"
        ),
    );

    std::fs::create_dir_all(out).map_err(|source| SiteError::Write {
        path: out.display().to_string(),
        source,
    })?;
    write(out, "index.html", page.as_bytes())?;
    write(out, "site-viewer.js", SCRIPT)?;
    write(out, "atlas-viewer.css", STYLES)?;
    write(out, "bundle.json", bundle_json.as_bytes())?;
    write(out, "brand.json", brand_json.as_bytes())?;
    Ok(())
}
