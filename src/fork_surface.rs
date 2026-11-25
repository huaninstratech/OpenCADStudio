//! What this fork ships without. Upstream asks for donations and shows its
//! sponsor: a startup prompt, a `DONATE` command, a supporters list fetched
//! from its site, a "Sponsors" section on the Start page, a sponsor button
//! and the sponsor's analytics script on its website. It also sends its
//! update notice and its `WEBVERSION` command to its own site. None of that
//! belongs in this build, and a merge from upstream brings it back without
//! any conflict, so these tests read the tree itself and fail when any of it
//! reappears.

use std::path::{Path, PathBuf};

/// This fork's latest release. Every update path must lead here.
const FORK_LATEST_RELEASE: &str = "https://github.com/huaninstratech/OpenCADStudio/releases/latest";

/// This fork's web build, the page the `WEBVERSION` command opens.
const FORK_WEB_BUILD: &str = "https://huaninstratech.github.io/OpenCADStudio/app/";

/// Where an update or download address of the product can be written down:
/// the application with what it builds and ships, the page of its web build,
/// and the pages of its website.
const UPDATE_PATH_TREES: [&str; 11] = [
    "src",
    "crates",
    "packaging",
    ".github",
    "web",
    "plugins",
    "build.rs",
    "web-app.html",
    "index.html",
    "download.html",
    "site",
];

/// Addresses this fork's build and pages never name: upstream's release list
/// and the release API under the same path, the `github.io` host that
/// redirects to its site, its download page, and the host its sponsor's
/// analytics script is loaded from. The font repository and the plugin
/// registry under the same account are data sources, not releases.
const UPSTREAM_ADDRESSES: [&str; 4] = [
    "hakanseven12/opencadstudio/releases",
    "hakanseven12.github.io",
    "opencadstudio.com/download",
    "open-aec.com",
];

/// Words only the donation feature uses: `donat` also covers `Donate` and
/// `DonationPrompt`, `patrons` the `PatronsFetched` message, `sponsor` the
/// Start page's `Sponsors` section and the website's sponsor button.
const DONATION_WORDS: [&str; 5] = ["donat", "patreon", "patrons", "supporters", "sponsor"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Every UTF-8 file under `dir`, as a repository-relative path with `/`
/// separators and its text, in the order the directory yields them. Build
/// output and tool caches are not part of the tree; binary files (images,
/// fonts in binary formats) are skipped by the UTF-8 check.
fn text_files(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if !matches!(
                name.to_str(),
                Some("target" | ".git" | ".gitnexus" | "node_modules" | "__pycache__")
            ) {
                text_files(&path, out);
            }
        } else if let Ok(text) = String::from_utf8(std::fs::read(&path).unwrap_or_default()) {
            let shown = path
                .strip_prefix(repo_root())
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((shown, text));
        }
    }
}

/// The files under `roots`, in path order.
fn tree_files(roots: &[&str]) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for root in roots {
        let path = repo_root().join(root);
        if path.is_dir() {
            text_files(&path, &mut files);
        } else {
            files.push((
                root.to_string(),
                String::from_utf8(std::fs::read(&path).unwrap_or_default()).unwrap_or_default(),
            ));
        }
    }
    files.sort();
    files
}

/// Whether `path` is the release notes. They are history, and keep what they
/// say about the features they describe.
fn is_release_notes(path: &str) -> bool {
    path == "CHANGELOG.md" || path.starts_with("changelogs/")
}

/// `where_` (a path, or `path:line`) and the offending text of every file
/// name and line of `files` that `offends`, twenty at most per file.
fn offences(files: &[(String, String)], offends: impl Fn(&str, &str) -> bool) -> Vec<String> {
    let mut found = Vec::new();
    for (path, text) in files {
        // This file has to name what it forbids.
        if path.ends_with("/fork_surface.rs") {
            continue;
        }
        if offends(path, path) {
            found.push(format!("{path} (file name)"));
        }
        let mut per_file = 0;
        for (index, line) in text.lines().enumerate() {
            if per_file == 20 {
                found.push(format!("{path}: (further lines left out)"));
                break;
            }
            if offends(path, line) {
                per_file += 1;
                found.push(format!("{path}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    found
}

fn report(found: &[String]) -> String {
    let mut shown: Vec<&str> = found.iter().take(30).map(String::as_str).collect();
    if found.len() > shown.len() {
        shown.push("…");
    }
    shown.join("\n    ")
}

/// Whether the line, or a file name, belongs to the donation feature. A
/// translation can hold a look-alike (Spanish and Dutch "patterns" are
/// "patrones" and "patronen"), so a locale file is judged by its message and
/// attribute names alone.
fn solicits_donations(path: &str, line: &str) -> bool {
    let searched = if path.starts_with("locales/") || path.ends_with(".ftl") {
        line.split('=').next().unwrap_or("")
    } else {
        line
    };
    let lower = searched.to_ascii_lowercase();
    DONATION_WORDS.iter().any(|word| lower.contains(word))
}

/// Every file of the repository that is the product: all but the release
/// notes, in path order.
fn product_files() -> Vec<(String, String)> {
    let mut files = Vec::new();
    text_files(&repo_root(), &mut files);
    files.sort();
    files.retain(|(path, _)| !is_release_notes(path));
    files
}

#[test]
fn no_file_in_the_tree_solicits_donations() {
    let found = offences(&product_files(), |path, line| {
        solicits_donations(path, path) || solicits_donations(path, line)
    });
    assert!(
        found.is_empty(),
        "{} place(s) still carry upstream's donation solicitation:\n    {}",
        found.len(),
        report(&found)
    );
}

#[test]
fn every_update_path_stays_on_this_fork_s_releases() {
    // The notice's own link, the page the Start page and the Plugin Manager
    // open for the desktop build, and the web build the WEBVERSION command
    // opens.
    assert_eq!(crate::io::update_check::RELEASES_PAGE, FORK_LATEST_RELEASE);
    assert_eq!(
        crate::io::update_check::DOWNLOAD_PAGE,
        crate::io::update_check::RELEASES_PAGE
    );
    assert_eq!(crate::io::update_check::WEB_PAGE, FORK_WEB_BUILD);
    let found = offences(&tree_files(&UPDATE_PATH_TREES), |_, line| {
        let lower = line.to_ascii_lowercase();
        UPSTREAM_ADDRESSES
            .iter()
            .any(|address| lower.contains(address))
    });
    assert!(
        found.is_empty(),
        "{} place(s) still lead to upstream's site or releases, or load its sponsor's script:\n    {}",
        found.len(),
        report(&found)
    );
}

/// A configuration saved by an earlier build names a Start-page section this
/// one no longer has; it must open on the Welcome page, not fail to load.
#[test]
fn a_saved_start_section_this_build_lacks_opens_the_welcome_page() {
    use crate::app::StartSection;

    let saved: StartSection = serde_json::from_str("\"Supporters\"").expect("an unknown section loads");
    assert_eq!(saved, StartSection::Welcome);
    let kept: StartSection = serde_json::from_str("\"Recent\"").expect("a known section loads");
    assert_eq!(kept, StartSection::Recent);
}
