//! SDK structural checks only; never claims compiler or runtime validation.
use std::{env, fs, path::{Path, PathBuf}};

fn validate_source(source: &str) -> Result<(), &'static str> {
    if !source.contains("define module") || !source.contains("end") {
        return Err("missing module boundaries");
    }
    if !source.contains("pub fnc") {
        return Err("missing public function");
    }
    if source.bytes().filter(|b| *b == b'{').count()
        != source.bytes().filter(|b| *b == b'}').count()
    {
        return Err("brace counts differ");
    }
    let bytes = source.as_bytes();
    for (i, &byte) in bytes.iter().enumerate() {
        if byte == b'&' || byte == b'|' {
            let neighbor_left = i > 0 && bytes[i - 1] == byte;
            let neighbor_right = i + 1 < bytes.len() && bytes[i + 1] == byte;
            if !neighbor_left && !neighbor_right {
                return Err("standalone & or | forbidden in Oreslang source");
            }
        }
    }
    Ok(())
}

fn collect_sources(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            return Err(format!("symlink in SDK source inventory: {}", path.display()));
        }
        if file_type.is_dir() {
            collect_sources(&path, paths)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "ores") {
            paths.push(path);
        }
    }
    Ok(())
}

fn check(root: &Path) -> Result<usize, String> {
    let dir = root.join("src");
    let mut paths = Vec::new();
    collect_sources(&dir, &mut paths)?;
    paths.sort();
    if paths.len() < 4 {
        return Err(format!("expected >=4 SDK modules; found {}", paths.len()));
    }
    for path in &paths {
        let src = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        validate_source(&src).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(paths.len())
}

fn main() {
    let mut args = env::args_os().skip(1);
    let root = match args.next() {
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        Some(flag) if flag == "--root" => match args.next() {
            Some(value) if args.next().is_none() => PathBuf::from(value),
            _ => { eprintln!("usage: orestra-sdk-source-gate [--root PATH]"); std::process::exit(2); }
        },
        _ => { eprintln!("usage: orestra-sdk-source-gate [--root PATH]"); std::process::exit(2); }
    };
    match check(&root) {
        Ok(n) => println!("checked {n} Oreslang SDK modules (structural only)"),
        Err(reason) => { eprintln!("SDK source check DENIED: {reason}"); std::process::exit(1); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "define module demo as\npub fnc check(): bool { return true && false || true; }\nend\n";

    #[test]
    fn accepts_logical_and_or() {
        assert!(validate_source(VALID).is_ok());
    }

    #[test]
    fn rejects_standalone_bitwise_operators() {
        for bad in ["a & b", "a | b"] {
            assert!(validate_source(&format!("{VALID}\n{bad}")).is_err());
        }
    }

    #[test]
    fn rejects_missing_public_function_or_mismatched_braces() {
        assert!(validate_source("define module x as\nend").is_err());
        assert!(validate_source(&format!("{VALID}{{")).is_err());
    }

    #[test]
    fn actual_repository_source_admitted() {
        assert!(check(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap() >= 4);
    }

    #[test]
    fn recursively_checks_nested_sdk_modules() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("orestra-sdk-nested-{}-{nonce}", std::process::id()));
        let nested = root.join("src/deep");
        fs::create_dir_all(&nested).unwrap();
        for i in 0..4 {
            fs::write(nested.join(format!("module-{i}.ores")), VALID).unwrap();
        }
        assert_eq!(check(&root).unwrap(), 4);
        fs::write(nested.join("module-3.ores"), "define module broken as\nend\n").unwrap();
        assert!(check(&root).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_oreslang_source() {
        use std::os::unix::fs::symlink;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("orestra-sdk-symlink-{}-{nonce}", std::process::id()));
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        for i in 0..4 {
            fs::write(src.join(format!("module-{i}.ores")), VALID).unwrap();
        }
        let real = src.join("module-0.ores");
        symlink(&real, src.join("linked.ores")).unwrap();

        assert!(check(&root).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
