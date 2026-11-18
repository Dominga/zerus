//! WhiteWh fork (owner, 2026-11-18): an HTML page an agent made on a peer
//! (a report, a preview) names its pictures, styles and scripts by relative
//! paths. Its downloaded copy alone opens without them, so Open copy of a
//! peer's page assembles a site folder here: the page and every file it (and
//! its style sheets) names inside the page's folder, at the same relative
//! paths, bounded in count and size. A file that cannot be read is skipped;
//! the page still opens.
use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// Files fetched besides the page.
pub const MAX_FILES: usize = 200;
/// Bytes fetched besides the page.
pub const MAX_BYTES: u64 = 128 * 1024 * 1024;
/// Downloads at once.
const WORKERS: usize = 4;

pub fn is_page(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_ascii_lowercase();
    name.ends_with(".html") || name.ends_with(".htm")
}

/// The relative references of an HTML page or a style sheet: `src`, `href`,
/// `poster` and `data` attributes and CSS `url(...)`, as normalized paths
/// inside the page's folder. Links that leave it (`..` above it, a root, a
/// scheme, a fragment only) are not files of this page.
pub fn references(text: &str) -> Vec<String> {
    let found: BTreeSet<String> = raw(text).iter().filter_map(|value| relative(value)).collect();
    found.into_iter().collect()
}

/// Every referenced value as written.
fn raw(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let lower = text.to_ascii_lowercase();
    for attribute in ["src=", "href=", "poster=", "data="] {
        let mut from = 0;
        while let Some(at) = lower[from..].find(attribute) {
            let start = from + at + attribute.len();
            from = start;
            // Only a whole attribute name: not `data-src=` read as `src=`.
            let before = lower[..from - attribute.len()].chars().next_back();
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                continue;
            }
            if let Some(value) = quoted(&text[start..]) {
                found.push(value.to_owned());
            }
        }
    }
    let mut from = 0;
    while let Some(at) = lower[from..].find("url(") {
        let start = from + at + 4;
        from = start;
        let rest = &text[start..];
        let Some(end) = rest.find(')') else { break };
        let value = rest[..end].trim().trim_matches(['"', '\'']);
        found.push(value.to_owned());
    }
    found
}

fn quoted(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let quote = text.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let body = &text[1..];
    body.find(quote).map(|end| &body[..end])
}

/// A reference as a path inside the page's folder, `/`-separated.
fn relative(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.split(['?', '#']).next().unwrap_or("");
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains(':')
        || value.chars().any(char::is_control)
        || value.len() > 1024
    {
        return None;
    }
    let value = value.replace("%20", " ").replace('\\', "/");
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            // Up stays inside the page's folder; above it is not this page's.
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The peer's folder of `page` and the separator it uses.
fn folder(page: &str) -> (&str, char) {
    match page.rfind(['/', '\\']) {
        Some(at) => (&page[..at], if page[at..].starts_with('\\') { '\\' } else { '/' }),
        None => ("", '/'),
    }
}

/// Assemble the site of the peer's `page` (already downloaded to `copy`) in a
/// folder under `sites`, fetching each named file with `fetch(remote, local)`.
/// Returns the local page to open.
pub fn assemble(
    host: &str,
    page: &str,
    copy: &Path,
    sites: &Path,
    fetch: Arc<dyn Fn(&str, &Path) -> Result<(), String> + Send + Sync>,
) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    let (remote_folder, separator) = folder(page);
    let name = page.rsplit(['/', '\\']).next().unwrap_or(page);
    let digest = format!("{:x}", Sha256::digest(format!("{host}\n{remote_folder}").as_bytes()));
    let root = sites.join(&digest[..16]);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|e| format!("Could not prepare the page folder: {e}"))?;
    let local_page = root.join(name);
    std::fs::copy(copy, &local_page).map_err(|e| format!("Could not copy the page: {e}"))?;
    let html = std::fs::read(&local_page).map_err(|e| e.to_string())?;
    let html = String::from_utf8_lossy(&html);
    let queue: VecDeque<String> = references(&html).into_iter().filter(|r| r != name).collect();
    let state = Arc::new(Mutex::new(Fetch {
        queue,
        seen: BTreeSet::new(),
        files: 0,
        bytes: 0,
    }));
    let remote = |relative: &str| -> String {
        let relative = if separator == '\\' { relative.replace('/', "\\") } else { relative.to_owned() };
        if remote_folder.is_empty() {
            relative
        } else {
            format!("{remote_folder}{separator}{relative}")
        }
    };
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            let state = state.clone();
            let fetch = fetch.clone();
            let root = root.clone();
            let remote = &remote;
            scope.spawn(move || loop {
                let next = {
                    let mut s = state.lock().unwrap_or_else(PoisonError::into_inner);
                    if s.files >= MAX_FILES || s.bytes >= MAX_BYTES {
                        None
                    } else {
                        loop {
                            match s.queue.pop_front() {
                                Some(r) if s.seen.insert(r.clone()) => {
                                    s.files += 1;
                                    break Some(r);
                                }
                                Some(_) => continue,
                                None => break None,
                            }
                        }
                    }
                };
                let Some(relative) = next else { break };
                let local = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
                if let Some(parent) = local.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if fetch(&remote(&relative), &local).is_err() {
                    continue;
                }
                let size = std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0);
                // A style sheet names its own pictures and fonts, relative to it.
                let more = if relative.to_ascii_lowercase().ends_with(".css") && size <= 4 * 1024 * 1024 {
                    let base = relative.rsplit_once('/').map(|(dir, _)| format!("{dir}/")).unwrap_or_default();
                    std::fs::read(&local)
                        .map(|css| raw(&String::from_utf8_lossy(&css)))
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|r| relative_join(&base, &r))
                        .collect()
                } else {
                    Vec::new()
                };
                let mut s = state.lock().unwrap_or_else(PoisonError::into_inner);
                s.bytes += size;
                s.queue.extend(more);
            });
        }
    });
    Ok(local_page)
}

fn relative_join(base: &str, reference: &str) -> Option<String> {
    relative(&format!("{base}{reference}"))
}

struct Fetch {
    queue: VecDeque<String>,
    seen: BTreeSet<String>,
    files: usize,
    bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_names_its_files_inside_its_folder() {
        let html = r##"<html><head><link rel="stylesheet" href="style/main.css?v=2">
            <script src='js/app.js'></script><link href="https://cdn.example.com/x.css">
            <a href="#top">top</a><a href="../secret.txt">up</a><a href="/abs.png">abs</a>
            </head><body style="background:url(img/bg%20one.png)">
            <img data-src="lazy.png" src="./img/chart.png"><video poster="img\poster.jpg"></video>
            <img src="data:image/png;base64,AAAA"><a href="report.html">self</a></body></html>"##;
        assert_eq!(
            references(html),
            ["img/bg one.png", "img/chart.png", "img/poster.jpg", "js/app.js", "report.html", "style/main.css"]
        );
        assert_eq!(relative_join("style/", "../img/x.png").as_deref(), Some("img/x.png"));
        assert_eq!(relative_join("style/", "../../x.png"), None);
        assert_eq!(relative_join("style/", "fonts/a.woff2").as_deref(), Some("style/fonts/a.woff2"));
        assert!(is_page("E:\\Out\\Report.HTML") && is_page("/tmp/a.htm") && !is_page("/tmp/a.png"));
    }

    #[test]
    fn the_site_folder_mirrors_the_peer_folder_and_skips_missing_files() {
        let temporary = tempfile::tempdir().unwrap();
        let copy = temporary.path().join("downloaded.html");
        std::fs::write(&copy, r#"<link href="css/site.css"><img src="img/a.png"><img src="missing.png">"#).unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let log = asked.clone();
        let fetch: Arc<dyn Fn(&str, &Path) -> Result<(), String> + Send + Sync> = Arc::new(move |remote: &str, local: &Path| {
            log.lock().unwrap().push(remote.to_owned());
            match remote {
                r"E:\Out\css\site.css" => std::fs::write(local, "body{background:url('../img/bg.png')} .x{background:url(icons/i.svg)}").map_err(|e| e.to_string()),
                r"E:\Out\img\a.png" | r"E:\Out\css\icons\i.svg" => std::fs::write(local, b"x").map_err(|e| e.to_string()),
                _ => Err("missing".into()),
            }
        });
        let sites = temporary.path().join("sites");
        let page = assemble("msi-old", r"E:\Out\report.html", &copy, &sites, fetch).unwrap();
        assert_eq!(page.file_name().unwrap(), "report.html");
        let root = page.parent().unwrap();
        assert!(root.join("css").join("site.css").is_file());
        assert!(root.join("img").join("a.png").is_file());
        assert!(root.join("css").join("icons").join("i.svg").is_file());
        assert!(!root.join("missing.png").exists());
        let mut asked = asked.lock().unwrap().clone();
        asked.sort();
        // The style sheet's `../img/bg.png` is the page's `img/bg.png`.
        assert_eq!(asked, [r"E:\Out\css\icons\i.svg", r"E:\Out\css\site.css", r"E:\Out\img\a.png", r"E:\Out\img\bg.png", r"E:\Out\missing.png"]);
    }
}
