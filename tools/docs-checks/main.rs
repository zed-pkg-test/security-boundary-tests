//! Local documentation and integration-lock shape checks, not a remote revision verifier.
use serde_json::Value;
use std::{collections::BTreeSet, env, fs, path::{Path, PathBuf}};

const MAX_DOC_BYTES: u64 = 1_048_576;
const MAX_DOC_FILES: usize = 4096;
const MAX_LINKS_PER_DOC: usize = 4096;
const MAX_LINK_TARGET_BYTES: usize = 4096;
const MAX_DOC_DEPTH: usize = 64;
const REPOSITORIES: [&str; 9] = [
    "orestra", "orestra-api", "orestra-cli", "orestra-sdk",
    "orestra-state", "orestra-drivers", "orestra-ui",
    "orestra-conformance", "orestra-website",
];

fn collect_md(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    collect_md_at(root, dir, files, 0)
}

fn collect_md_at(
    root: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_DOC_DEPTH {
        return Err(format!("documentation tree exceeds depth bound of {MAX_DOC_DEPTH}"));
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!("symlink in doc inventory: {}", path.display()));
        }
        if kind.is_dir() {
            if path.file_name().is_some_and(|n| n == ".git" || n == "target") {
                continue;
            }
            collect_md_at(root, &path, files, depth + 1)?;
        } else if kind.is_file() && path.extension().is_some_and(|e| e == "md") {
            if files.len() >= MAX_DOC_FILES {
                return Err(format!(
                    "documentation inventory exceeds {MAX_DOC_FILES} Markdown files"
                ));
            }
            files.push(path);
        }
    }
    Ok(())
}

fn markdown_targets(source: &str) -> Result<Vec<&str>, &'static str> {
    let mut matches = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = &rest[..end];
        if !target.is_empty() && !target.contains('\n') {
            if target.len() > MAX_LINK_TARGET_BYTES {
                return Err("Markdown link target exceeds size bound");
            }
            if matches.len() >= MAX_LINKS_PER_DOC {
                return Err("Markdown link count exceeds per-file bound");
            }
            matches.push(target);
        }
        rest = &rest[end + 1..];
    }
    Ok(matches)
}

fn check_links(root: &Path) -> Result<usize, String> {
    let canonical_root = fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let mut files = Vec::new();
    collect_md(root, root, &mut files)?;
    files.sort();
    let mut broken = Vec::new();
    for file in &files {
        let meta = fs::symlink_metadata(file).map_err(|e| format!("{}: {e}", file.display()))?;
        if meta.len() > MAX_DOC_BYTES {
            return Err(format!("documentation file exceeds {MAX_DOC_BYTES} bytes: {}", file.display()));
        }
        let src = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let targets = markdown_targets(&src)
            .map_err(|e| format!("{}: {e}", file.display()))?;
        for target in targets {
            if target.starts_with("https://") || target.starts_with("http://")
                || target.starts_with("mailto:") || target.starts_with('#') {
                continue;
            }
            let target = target.split('#').next().unwrap_or("");
            if !target.is_empty() {
                let candidate = file.parent().unwrap_or(root).join(target);
                match fs::canonicalize(&candidate) {
                    Ok(resolved) if resolved.starts_with(&canonical_root) => {}
                    _ => broken.push(format!("{}: {target}", file.strip_prefix(root).unwrap_or(file).display())),
                }
            }
        }
    }
    if !broken.is_empty() {
        return Err(broken.join("\n"));
    }
    Ok(files.len())
}

fn validate_lock(doc: &Value) -> Result<usize, &'static str> {
    if doc["format"].as_str() != Some("orestra.integration-lock/v1")
        || doc["releaseStatus"].as_str() != Some("pre-alpha-source-only") {
        return Err("unexpected lock format/status");
    }
    let revisions = doc["revisions"].as_object().ok_or("revisions must be an object")?;
    let expected: BTreeSet<_> = REPOSITORIES.into_iter().collect();
    let found: BTreeSet<_> = revisions.keys().map(String::as_str).collect();
    if expected != found {
        return Err("wrong revision identities");
    }
    for sha in revisions.values() {
        let sha = sha.as_str().ok_or("revision must be a string")?;
        if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
            return Err("revision must be a lowercase forty-digit hex SHA");
        }
    }
    if doc["verification"]["nativeOreslangCompile"].as_str() != Some("not_verified") {
        return Err("unsupported native compilation attestation");
    }
    Ok(REPOSITORIES.len())
}

fn check_lock(root: &Path) -> Result<usize, String> {
    let file = root.join("integration/compatibility.lock.json");
    let meta = fs::symlink_metadata(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!("integration lock must be a regular non-symlink file: {}", file.display()));
    }
    if meta.len() > MAX_DOC_BYTES {
        return Err(format!("integration lock exceeds {MAX_DOC_BYTES} bytes: {}", file.display()));
    }
    let bytes = fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let doc: Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    validate_lock(&doc).map_err(|e| format!("{}: {e}", file.display()))
}

fn main() {
    let mut args = env::args_os().skip(1);
    let mode = match args.next() {
        None => String::from("all"),
        Some(value) => value.to_string_lossy().into_owned(),
    };
    let root = match args.next() {
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        Some(value) if value == "--root" => match args.next() {
            Some(p) if args.next().is_none() => PathBuf::from(p),
            _ => { eprintln!("usage: orestra-docs-gates [all|links|lock] [--root PATH]"); std::process::exit(2); }
        },
        _ => { eprintln!("usage: orestra-docs-gates [all|links|lock] [--root PATH]"); std::process::exit(2); }
    };
    if !matches!(mode.as_str(), "all" | "links" | "lock") {
        eprintln!("unknown mode {mode:?}"); std::process::exit(2);
    }
    let result = (|| -> Result<(), String> {
        if mode == "all" || mode == "links" {
            println!("validated links in {} documentation files", check_links(&root)?);
        }
        if mode == "all" || mode == "lock" {
            println!("validated {} pinned repository revisions (shape only)", check_lock(&root)?);
        }
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("documentation gate DENIED: {e}"); std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_relative_markdown_links_without_touching_web_links() {
        assert_eq!(
            markdown_targets("[a](docs/a.md) [b](#local)").unwrap(),
            ["docs/a.md", "#local"]
        );
        assert!(markdown_targets("[incomplete](").unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_lock_sha_or_claim() {
        let mut doc: Value = serde_json::from_str(include_str!("../../integration/compatibility.lock.json")).unwrap();
        assert!(validate_lock(&doc).is_ok());
        doc["verification"]["nativeOreslangCompile"] = json!("verified");
        assert!(validate_lock(&doc).is_err());
        doc["verification"]["nativeOreslangCompile"] = json!("not_verified");
        doc["revisions"]["orestra"] = json!("NOT-A-GIT-SHA");
        assert!(validate_lock(&doc).is_err());
    }

    #[test]
    fn repository_docs_and_lock_are_admitted() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(check_links(root).unwrap() > 0);
        assert_eq!(check_lock(root), Ok(9));
    }

    #[test]
    fn rejects_lock_revision_set_and_sha_shape_mutations() {
        let base: Value = serde_json::from_str(include_str!("../../integration/compatibility.lock.json")).unwrap();

        let mut extra_repo = base.clone();
        extra_repo["revisions"]["unexpected"] = json!("0123456789abcdef0123456789abcdef01234567");
        assert!(validate_lock(&extra_repo).is_err());

        let mut missing_repo = base.clone();
        missing_repo["revisions"].as_object_mut().unwrap().remove("orestra-ui");
        assert!(validate_lock(&missing_repo).is_err());

        let mut uppercase = base;
        uppercase["revisions"]["orestra"] = json!("ABCDEF0123456789abcdef0123456789abcdef01");
        assert!(validate_lock(&uppercase).is_err());
    }

    #[test]
    fn rejects_relative_links_that_escape_repository_root() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let parent = env::temp_dir().join(format!("orestra-docs-escape-{}-{nonce}", std::process::id()));
        let root = parent.join("repo");
        fs::create_dir_all(&root).unwrap();
        fs::write(parent.join("outside.md"), "# outside\n").unwrap();
        fs::write(root.join("README.md"), "[escape](../outside.md)\n").unwrap();

        assert!(check_links(&root).is_err());
        fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn rejects_oversized_markdown_and_lock_inputs() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("orestra-docs-size-{}-{nonce}", std::process::id()));
        fs::create_dir_all(root.join("integration")).unwrap();
        fs::write(root.join("README.md"), vec![b'x'; (MAX_DOC_BYTES + 1) as usize]).unwrap();
        fs::write(root.join("integration/compatibility.lock.json"), "{}").unwrap();
        assert!(check_links(&root).is_err());

        fs::write(root.join("README.md"), "# ok\n").unwrap();
        fs::write(
            root.join("integration/compatibility.lock.json"),
            vec![b'x'; (MAX_DOC_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(check_lock(&root).is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rejects_oversized_markdown_inventory() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("orestra-docs-count-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("one.md"), "# one\n").unwrap();

        let mut files = vec![PathBuf::new(); MAX_DOC_FILES];
        assert!(collect_md(&root, &root, &mut files).is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rejects_excessive_documentation_tree_depth() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("orestra-docs-depth-{}-{nonce}", std::process::id()));
        let mut dir = root.clone();
        fs::create_dir_all(&dir).unwrap();
        for _ in 0..=MAX_DOC_DEPTH {
            dir = dir.join("nested");
            fs::create_dir(&dir).unwrap();
        }
        fs::write(dir.join("deep.md"), "# deep\n").unwrap();

        let mut files = Vec::new();
        assert!(collect_md(&root, &root, &mut files).is_err());

        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_markdown_inventory_entries() {
        use std::os::unix::fs::symlink;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("orestra-docs-symlink-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let real = root.join("real.md");
        fs::write(&real, "# real\n").unwrap();
        symlink(&real, root.join("linked.md")).unwrap();

        let mut files = Vec::new();
        assert!(collect_md(&root, &root, &mut files).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rejects_excessive_or_oversized_markdown_links() {
        let many = "[x](a.md) ".repeat(MAX_LINKS_PER_DOC + 1);
        assert!(markdown_targets(&many).is_err());

        let long = format!("[x]({})", "a".repeat(MAX_LINK_TARGET_BYTES + 1));
        assert!(markdown_targets(&long).is_err());
    }
}
