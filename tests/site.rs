use regex::Regex;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

const MAX_HTML_BYTES: u64 = 1_048_576;
const MAX_HTML_FILES: usize = 4096;

fn collect_html(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!("symlink in static-site inventory: {}", path.display()));
        }
        if kind.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            collect_html(&path, out)?;
        } else if kind.is_file() && path.extension().is_some_and(|e| e == "html") {
            if out.len() >= MAX_HTML_FILES {
                return Err(format!("static-site inventory exceeds {MAX_HTML_FILES} HTML files"));
            }
            let metadata = entry.metadata().map_err(|e| format!("{}: {e}", path.display()))?;
            if metadata.len() > MAX_HTML_BYTES {
                return Err(format!(
                    "static-site HTML exceeds {MAX_HTML_BYTES} bytes: {}",
                    path.display()
                ));
            }
            out.push(path);
        }
    }
    Ok(())
}

fn parse_links_and_ids(html: &str) -> Result<(Vec<String>, BTreeSet<String>), String> {
    let tag_re = Regex::new(r#"(?is)<\s*([a-zA-Z][a-zA-Z0-9:-]*)\b([^>]*)>"#).unwrap();
    let attr_re = Regex::new(r#"(?is)\b([a-zA-Z_:][a-zA-Z0-9_:.:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap();
    let mut links = Vec::new();
    let mut ids = BTreeSet::new();
    for caps in tag_re.captures_iter(html) {
        let tag = caps.get(1).unwrap().as_str().to_ascii_lowercase();
        let attrs = caps.get(2).unwrap().as_str();
        for attr in attr_re.captures_iter(attrs) {
            let name = attr.get(1).unwrap().as_str().to_ascii_lowercase();
            let value = attr
                .get(2)
                .or_else(|| attr.get(3))
                .map(|m| m.as_str())
                .unwrap_or("");
            if name == "id" && !ids.insert(value.to_owned()) {
                return Err(format!("duplicate HTML id: {value}"));
            }
            if tag == "a" && name == "href" {
                links.push(value.to_owned());
            }
        }
    }
    Ok((links, ids))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                b'A'..=b'F' => Some(b - b'A' + 10),
                _ => None,
            };
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).expect("percent-decoded site path must be UTF-8")
}

fn is_external(href: &str) -> bool {
    if href.starts_with("//") {
        return true;
    }
    let end = href.find(|c| matches!(c, '/' | '?' | '#')).unwrap_or(href.len());
    href[..end]
        .find(':')
        .is_some_and(|colon| colon > 0 && href[..colon].bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.')))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(c.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(p) => out.push(p),
        }
    }
    out
}

#[test]
fn internal_links_resolve_and_anchors_exist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_html(root, &mut files).unwrap();
    files.sort();

    for file in files {
        let html = fs::read_to_string(&file).unwrap();
        let (links, _) = parse_links_and_ids(&html).unwrap();
        for href in links {
            if is_external(&href) {
                continue;
            }
            let (before_fragment, fragment) = href
                .split_once('#')
                .map_or((href.as_str(), None), |(a, b)| (a, Some(b)));
            let path_part = before_fragment.split('?').next().unwrap_or("");
            let rel = percent_decode(path_part);
            let target = if rel.is_empty() {
                file.clone()
            } else {
                normalize(&file.parent().unwrap().join(rel))
            };
            assert!(target.starts_with(root), "{}: navigation escaped site root via {href}", file.display());
            assert!(target.is_file(), "{}: missing link {href}", file.display());
            if let Some(fragment) = fragment.filter(|f| !f.is_empty()) {
                let target_html = fs::read_to_string(&target).unwrap();
                let (_, ids) = parse_links_and_ids(&target_html).unwrap();
                assert!(ids.contains(fragment), "{}: missing anchor #{fragment} in {}", file.display(), target.display());
            }
        }
    }
}

#[test]
fn no_external_assets_or_inline_scripting_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_html(root, &mut files);
    for file in files {
        let html = fs::read_to_string(&file).unwrap();
        assert!(html.contains("Content-Security-Policy"), "{}", file.display());
        assert!(!html.contains("<script>"), "{}", file.display());
    }
}

#[test]
fn entrypoints_and_release_claims_hold() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for rel in ["index.html", "pages/architecture.html", "pages/getting-started.html"] {
        let html = fs::read_to_string(root.join(rel)).unwrap();
        assert!(html.contains(r#"lang="en""#), "{rel}");
        assert!(html.contains(r#"name="viewport""#), "{rel}");
        assert!(html.to_ascii_lowercase().contains("orestra"), "{rel}");
    }
    let home = fs::read_to_string(root.join("index.html")).unwrap().to_ascii_lowercase();
    assert!(home.contains("pre-alpha"));
    assert!(home.contains("not yet a production"));
}

#[cfg(unix)]
#[test]
fn static_site_inventory_rejects_symlinks() {
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let parent = std::env::temp_dir().join(format!("orestra-site-symlink-{}-{nonce}", std::process::id()));
    let root = parent.join("site");
    fs::create_dir_all(&root).unwrap();
    let outside = parent.join("outside.html");
    fs::write(&outside, "<html></html>").unwrap();
    symlink(&outside, root.join("linked.html")).unwrap();

    let mut files = Vec::new();
    assert!(collect_html(&root, &mut files).is_err());
    fs::remove_dir_all(&parent).unwrap();
}


#[test]
fn static_site_inventory_and_html_size_are_bounded() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!(
        "orestra-site-bounds-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();

    fs::write(
        root.join("large.html"),
        vec![b'x'; (MAX_HTML_BYTES + 1) as usize],
    )
    .unwrap();
    let mut files = Vec::new();
    assert!(collect_html(&root, &mut files).is_err());

    fs::remove_file(root.join("large.html")).unwrap();
    fs::write(root.join("one.html"), "<html></html>").unwrap();
    let mut files = vec![PathBuf::new(); MAX_HTML_FILES];
    assert!(collect_html(&root, &mut files).is_err());

    fs::remove_dir_all(&root).unwrap();
}
