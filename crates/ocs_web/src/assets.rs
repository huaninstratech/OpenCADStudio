//! The web bundle: the files `trunk build` produces, served from
//! [`crate::Config::static_dir`].
//!
//! The bundle is a fixed set of files whose names are known after the build,
//! so it is read once at start-up into memory — the server then answers from
//! RAM, with a long cache for the hashed asset names and no cache for
//! `index.html`.

use std::collections::HashMap;
use std::path::Path;

/// One file of the bundle.
#[derive(Debug, Clone)]
pub struct File {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
    /// Whether the name is content-addressed (`OpenCADStudio-<hash>.js`) and
    /// may be cached by the browser forever.
    pub immutable: bool,
}

/// The bundle, by request path (`OpenCADStudio/app/OpenCADStudio-abc.js`).
#[derive(Debug, Default)]
pub struct Bundle {
    files: HashMap<String, File>,
}

impl Bundle {
    /// Reads every file under `dir`. A missing or unreadable folder gives an
    /// empty bundle rather than a failure: the drawings can still be served,
    /// and the operator sees the empty page and the log line.
    pub fn load(dir: Option<&Path>) -> Self {
        let Some(dir) = dir else {
            return Self::default();
        };
        let mut files = HashMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(folder) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Ok(relative) = path.strip_prefix(dir) else {
                    continue;
                };
                let request_path = relative
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
                files.insert(
                    request_path.clone(),
                    File {
                        bytes,
                        content_type: content_type(file_name),
                        immutable: is_hashed_asset(file_name),
                    },
                );
            }
        }
        let mut bundle = Self { files };
        bundle.rebase_published_paths();
        bundle
    }

    /// Points the bundle at this server.
    ///
    /// A bundle built with `--public-url /OpenCADStudio/app/` — the address of
    /// the fork's GitHub Pages copy — names its program and data by absolute
    /// paths. Served from this server they would be fetched from the wrong
    /// place, so every `/OpenCADStudio/app/` in the page, its manifest and its
    /// worker is rewritten to `/`. A bundle that already asks for `/` is left
    /// as it is.
    ///
    /// The rewrite is textual and only touches the files that can name an
    /// address: HTML, JavaScript, JSON and the web manifest. It runs once, at
    /// start-up, on the bytes kept in memory.
    fn rebase_published_paths(&mut self) {
        const PUBLISHED: &[u8] = b"/OpenCADStudio/app/";
        for (path, file) in self.files.iter_mut() {
            let text_kind = matches!(
                path.rsplit('.').next().unwrap_or_default(),
                "html" | "js" | "json" | "webmanifest"
            );
            if !text_kind || !file.bytes.windows(PUBLISHED.len()).any(|window| window == PUBLISHED) {
                continue;
            }
            if let Ok(text) = std::str::from_utf8(&file.bytes) {
                file.bytes = text.replace("/OpenCADStudio/app/", "/").into_bytes();
            }
        }
    }

    /// The file at `path`, where `path` is relative to the bundle's root.
    pub fn get(&self, path: &str) -> Option<&File> {
        self.files.get(path.trim_start_matches('/'))
    }

    /// `index.html`, the page a directory request falls back to.
    pub fn index(&self) -> Option<&File> {
        self.get("index.html")
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// The `Content-Type` a browser needs for each kind of file in a bundle.
fn content_type(file_name: &str) -> &'static str {
    let extension = file_name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    match extension.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        // Without this type a browser refuses to instantiate the module.
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

/// Whether the name carries a content hash (`name-0123456789abcdef.js`), which
/// makes the file safe to cache forever.
fn is_hashed_asset(file_name: &str) -> bool {
    if file_name.ends_with(".html") {
        return false;
    }
    // The extension is stripped first: a hashed wasm is
    // `name-0123456789abcdef_bg.wasm`, where the hash is not the last part
    // before the dot.
    let stem = file_name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(file_name);
    let Some((_, tail)) = stem.rsplit_once('-') else {
        return false;
    };
    // The hash runs to the end of the stem, or to a `_` suffix such as `_bg`.
    let hash = tail.split('_').next().unwrap_or_default();
    hash.len() >= 12 && hash.chars().all(|ch| ch.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    //! Reading a bundle and the headers each file earns.
    use super::{is_hashed_asset, Bundle};

    #[test]
    fn a_bundle_reads_its_files_with_the_types_a_browser_needs() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        std::fs::write(dir.path().join("index.html"), b"<html></html>").unwrap();
        std::fs::write(dir.path().join("OpenCADStudio-0123456789abcdef.js"), b"js").unwrap();
        std::fs::write(dir.path().join("OpenCADStudio-0123456789abcdef_bg.wasm"), b"wasm").unwrap();
        std::fs::create_dir(dir.path().join("worker_pkg")).unwrap();
        std::fs::write(dir.path().join("worker_pkg/ocs_web_worker.js"), b"worker").unwrap();
        let bundle = Bundle::load(Some(dir.path()));
        assert_eq!(bundle.len(), 4, "four files, one of them in a subfolder");
        let wasm = bundle.get("/OpenCADStudio-0123456789abcdef_bg.wasm").expect("the wasm");
        assert_eq!(wasm.content_type, "application/wasm");
        assert!(wasm.immutable, "a hashed name is cached forever");
        assert!(bundle.get("OpenCADStudio-0123456789abcdef.js").is_some());
        assert!(bundle.get("worker_pkg/ocs_web_worker.js").is_some());
        assert_eq!(bundle.index().expect("the page").content_type, "text/html; charset=utf-8");
        assert!(!bundle.index().expect("the page").immutable, "the page is not hashed");
    }

    #[test]
    fn a_bundle_that_is_not_there_is_empty_rather_than_a_failure() {
        let bundle = Bundle::load(None);
        assert!(bundle.is_empty());
        assert!(bundle.get("index.html").is_none());
        let missing = Bundle::load(Some(std::path::Path::new("/no/such/folder")));
        assert!(missing.is_empty());
    }

    #[test]
    fn a_bundle_built_for_the_fork_s_page_is_repointed_at_this_server() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        // What `--public-url /OpenCADStudio/app/` writes: absolute addresses.
        std::fs::write(
            dir.path().join("index.html"),
            br#"<html><head><link href="/OpenCADStudio/app/app.webmanifest" />
            <script src="/OpenCADStudio/app/OpenCADStudio-0123456789abcdef.js"></script>
            <link rel="preload" href="/OpenCADStudio/app/OpenCADStudio-0123456789abcdef_bg.wasm" /></head></html>"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("app.webmanifest"),
            br#"{"start_url":"/OpenCADStudio/app/","scope":"/OpenCADStudio/app/"}"#,
        )
        .unwrap();
        let bundle = Bundle::load(Some(dir.path()));
        let page = String::from_utf8(bundle.index().expect("the page").bytes.clone()).unwrap();
        assert!(!page.contains("/OpenCADStudio/app/"), "the page was repointed");
        assert!(page.contains("src=\"/OpenCADStudio-0123456789abcdef.js\""));
        let manifest = String::from_utf8(bundle.get("app.webmanifest").expect("manifest").bytes.clone()).unwrap();
        assert!(manifest.contains("\"start_url\":\"/\""));
    }

    #[test]
    fn a_bundle_that_already_asks_for_the_root_is_left_alone() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let page = br#"<html><script src="/OpenCADStudio-0123456789abcdef.js"></script></html>"#;
        std::fs::write(dir.path().join("index.html"), page).unwrap();
        let bundle = Bundle::load(Some(dir.path()));
        assert_eq!(bundle.index().expect("the page").bytes, page.to_vec());
    }

    #[test]
    fn only_content_addressed_names_are_cached_forever() {
        assert!(is_hashed_asset("OpenCADStudio-0123456789abcdef.js"));
        assert!(
            is_hashed_asset("OpenCADStudio-0123456789abcdef_bg.wasm"),
            "a hashed wasm carries a suffix after its hash"
        );
        assert!(is_hashed_asset("logo-45c33c4b340416d6.svg"));
        assert!(!is_hashed_asset("index.html"));
        assert!(!is_hashed_asset("site-version.txt"));
        assert!(!is_hashed_asset("sw.js"));
        assert!(!is_hashed_asset("worker_pkg/ocs_web_worker.js"));
        assert!(!is_hashed_asset("app.webmanifest"));
    }
}
