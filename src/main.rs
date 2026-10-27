//! Structural-only Oreslang source checks, not a compiler or runtime parity gate.
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn imports(source: &str) -> Vec<String> {
    // Match the legacy check's from\s+"..." pattern without a regex dependency.
    let chars: Vec<char> = source.chars().collect();
    let mut matches = Vec::new();
    let mut i = 0;
    while i + 4 <= chars.len() {
        if chars[i..i + 4] == ['f', 'r', 'o', 'm'] {
            let mut j = i + 4;
            if j < chars.len() && chars[j].is_whitespace() {
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '"' {
                    j += 1;
                    let start = j;
                    while j < chars.len() && chars[j] != '"' {
                        j += 1;
                    }
                    if j < chars.len() {
                        matches.push(chars[start..j].iter().collect());
                    }
                }
            }
        }
        i += 1;
    }
    matches
}

fn collect(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
        if kind.is_symlink() {
            return Err(format!("symlink in source inventory: {}", path.display()));
        }
        if kind.is_dir() && recursive {
            collect(&path, true, out)?;
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "ores") {
            out.push(path);
        }
    }
    Ok(())
}

fn check_source(root: &Path, path: &Path) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let deny = |reason: &str| format!("{}: {reason}", path.display());
    if source.matches('{').count() != source.matches('}').count() {
        return Err(deny("unbalanced brace counts"));
    }
    if !source.contains("return") {
        return Err(deny("missing return"));
    }
    if source.contains('\t') {
        return Err(deny("tab character"));
    }
    let canonical_root = fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    for import in imports(&source) {
        if import.starts_with('.') {
            let candidate = path.parent().unwrap_or(Path::new(".")).join(&import);
            let metadata = fs::symlink_metadata(&candidate)
                .map_err(|_| deny(&format!("missing local import {import:?}")))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(deny(&format!("local import must be a regular non-symlink file: {import:?}")));
            }
            let resolved = fs::canonicalize(&candidate)
                .map_err(|_| deny(&format!("missing local import {import:?}")))?;
            if !resolved.starts_with(&canonical_root) {
                return Err(deny(&format!("local import escapes repository root: {import:?}")));
            }
        }
    }
    Ok(())
}

fn check(root: &Path) -> Result<String, String> {
    let mut sources = Vec::new();
    let mut examples = Vec::new();
    collect(&root.join("src"), true, &mut sources)?;
    collect(&root.join("examples"), false, &mut examples)?;
    sources.sort();
    examples.sort();
    if sources.len() < 6 {
        return Err(format!("expected at least six src .ores modules, found {}", sources.len()));
    }
    if examples.len() < 4 {
        return Err(format!("expected at least four .ores examples, found {}", examples.len()));
    }
    for path in sources.iter().chain(&examples) {
        check_source(root, path)?;
    }
    Ok(format!(
        "structure/import checks passed for {} modules and {} examples (not a compiler test)",
        sources.len(),
        examples.len()
    ))
}

fn main() {
    let mut args = env::args_os().skip(1);
    let root = match args.next() {
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        Some(flag) if flag == "--root" => match args.next() {
            Some(root) if args.next().is_none() => PathBuf::from(root),
            _ => {
                eprintln!("usage: orestra-source-check [--root PATH]");
                std::process::exit(2)
            }
        },
        _ => {
            eprintln!("usage: orestra-source-check [--root PATH]");
            std::process::exit(2)
        }
    };
    match check(&root) {
        Ok(message) => println!("{message}"),
        Err(error) => {
            eprintln!("source check DENIED: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("orestra-source-check-{}-{nanos}", std::process::id()));
        fs::create_dir_all(root.join("src/deep")).unwrap();
        fs::create_dir_all(root.join("examples")).unwrap();
        for i in 0..6 {
            fs::write(root.join(format!("src/deep/{i}.ores")), "return {}\n").unwrap();
        }
        for i in 0..4 {
            fs::write(root.join(format!("examples/{i}.ores")), "return {}\n").unwrap();
        }
        root
    }

    #[test]
    fn import_scanner_preserves_legacy_match_semantics() {
        assert_eq!(imports("from \"./a.ores\" from\n \"../b.ores\""), ["./a.ores", "../b.ores"]);
        assert!(imports("from \"unfinished").is_empty());
    }

    #[test]
    fn accepts_valid_tree_and_rejects_missing_import() {
        let root = fixture();
        assert!(check(&root).unwrap().contains("6 modules and 4 examples"));
        fs::write(root.join("src/deep/0.ores"), "from \"./missing.ores\"\nreturn {}").unwrap();
        assert!(check(&root).unwrap_err().contains("missing local import"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_bad_source_and_missing_inventory() {
        let root = fixture();
        fs::write(root.join("examples/0.ores"), "return {\t").unwrap();
        assert!(check(&root).is_err());
        fs::remove_dir_all(root.join("examples")).unwrap();
        assert!(check(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_local_import_that_escapes_repository_root() {
        let root = fixture();
        let outside = root.parent().unwrap().join(format!(
            "orestra-outside-import-{}.ores",
            std::process::id()
        ));
        fs::write(&outside, "return {}\n").unwrap();
        let escaped = format!(
            "from \"../../{}\"\nreturn {{}}\n",
            outside.file_name().unwrap().to_string_lossy()
        );
        fs::write(root.join("examples/0.ores"), escaped).unwrap();

        assert!(check(&root)
            .unwrap_err()
            .contains("local import escapes repository root"));

        fs::remove_file(outside).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_oreslang_source_inventory() {
        use std::os::unix::fs::symlink;

        let root = fixture();
        let target = root.join("src/deep/0.ores");
        symlink(&target, root.join("src/deep/linked.ores")).unwrap();
        assert!(check(&root).unwrap_err().contains("symlink in source inventory"));
        fs::remove_dir_all(root).unwrap();
    }
}
