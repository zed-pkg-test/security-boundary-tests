//! Static/demo UI fixture admission, not a browser, accessibility, or runtime security proof.
use serde_json::Value;
use std::{collections::BTreeSet, env, fs, path::{Path, PathBuf}};

const MAX_FIXTURE_BYTES: u64 = 1_048_576;
const MAX_DEMO_ITEMS: usize = 4096;

fn check_demo(data: &Value) -> Result<(), &'static str> {
    for field in ["nodes", "workloads"] {
        let items = data[field].as_array().ok_or("demo section must be a JSON array")?;
        if items.is_empty() {
            return Err("demo section must not be empty");
        }
        if items.len() > MAX_DEMO_ITEMS {
            return Err("demo section exceeds resource count bound");
        }
        let mut identities = BTreeSet::new();
        for item in items {
            let metadata = item["metadata"]
                .as_object()
                .ok_or("demo item is missing metadata")?;
            let name = metadata
                .get("name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or("demo item is missing metadata.name")?;
            let namespace = metadata
                .get("namespace")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or("demo item is missing metadata.namespace")?;
            if !identities.insert((namespace, name)) {
                return Err("duplicate demo resource identity");
            }
            if !item["status"].as_object().is_some_and(|status| status.contains_key("phase")) {
                return Err("demo item is missing status.phase");
            }
        }
    }
    Ok(())
}

fn check_assets(html: &str, js: &str) -> Result<(), &'static str> {
    for expected in [r#"lang="en""#, r#"role="alert""#, r#"id="rows""#, r#"id="query""#] {
        if !html.contains(expected) {
            return Err("required HTML accessibility marker missing");
        }
    }
    if !js.contains("replaceChildren") {
        return Err("safe DOM mutation marker missing");
    }
    if js.contains("innerHTML") {
        return Err("innerHTML forbidden in UI code");
    }
    Ok(())
}

fn read_regular(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!("fixture must be a regular non-symlink file: {}", path.display()));
    }
    if meta.len() > MAX_FIXTURE_BYTES {
        return Err(format!("fixture exceeds {MAX_FIXTURE_BYTES} bytes: {}", path.display()));
    }
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_regular_text(path: &Path) -> Result<String, String> {
    String::from_utf8(read_regular(path)?).map_err(|e| format!("{}: {e}", path.display()))
}

fn check(root: &Path) -> Result<(), String> {
    let data = read_regular(&root.join("data/demo.json"))?;
    let json: Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    check_demo(&json).map_err(String::from)?;
    let html = read_regular_text(&root.join("index.html"))?;
    let js = read_regular_text(&root.join("assets/app.js"))?;
    check_assets(&html, &js).map_err(String::from)
}

fn main() {
    let mut args = env::args_os().skip(1);
    let root = match args.next() {
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        Some(flag) if flag == "--root" => match args.next() {
            Some(value) if args.next().is_none() => PathBuf::from(value),
            _ => { eprintln!("usage: orestra-ui-assets-check [--root PATH]"); std::process::exit(2); }
        },
        _ => { eprintln!("usage: orestra-ui-assets-check [--root PATH]"); std::process::exit(2); }
    };
    match check(&root) {
        Ok(()) => println!("UI demo and accessibility markers validated (static-only)"),
        Err(reason) => { eprintln!("UI assets DENIED: {reason}"); std::process::exit(1); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_missing_demo_sections_or_phase() {
        assert!(check_demo(&json!({"nodes":[],"workloads":[]})).is_err());
        assert!(check_demo(&json!({
            "nodes":[{"metadata":{"name":"n1","namespace":"system"},"status":{}}],
            "workloads":[{"metadata":{"name":"w1","namespace":"default"},"status":{"phase":"Ready"}}]
        })).is_err());
        assert!(check_demo(&json!({
            "nodes":[{"metadata":{"name":"n1","namespace":"system"},"status":{"phase":"Ready"}}],
            "workloads":[{"metadata":{"name":"w1","namespace":"default"},"status":{"phase":"Ready"}}]
        })).is_ok());
    }

    #[test]
    fn rejects_missing_or_duplicate_demo_identities() {
        let valid = json!({
            "nodes":[{"metadata":{"name":"n1","namespace":"system"},"status":{"phase":"Ready"}}],
            "workloads":[{"metadata":{"name":"w1","namespace":"default"},"status":{"phase":"Running"}}]
        });
        assert!(check_demo(&valid).is_ok());

        let missing = json!({
            "nodes":[{"metadata":{"namespace":"system"},"status":{"phase":"Ready"}}],
            "workloads":[{"metadata":{"name":"w1","namespace":"default"},"status":{"phase":"Running"}}]
        });
        assert!(check_demo(&missing).is_err());

        let duplicate = json!({
            "nodes":[
                {"metadata":{"name":"n1","namespace":"system"},"status":{"phase":"Ready"}},
                {"metadata":{"name":"n1","namespace":"system"},"status":{"phase":"Ready"}}
            ],
            "workloads":[{"metadata":{"name":"w1","namespace":"default"},"status":{"phase":"Running"}}]
        });
        assert!(check_demo(&duplicate).is_err());
    }

    #[test]
    fn rejects_excessive_demo_resource_count() {
        let item = json!({
            "metadata":{"name":"n","namespace":"system"},
            "status":{"phase":"Ready"}
        });
        let nodes = vec![item; MAX_DEMO_ITEMS + 1];
        let data = json!({
            "nodes":nodes,
            "workloads":[{"metadata":{"name":"w","namespace":"default"},"status":{"phase":"Running"}}]
        });
        assert!(check_demo(&data).is_err());
    }

    #[test]
    fn rejects_inner_html_and_missing_dom_markers() {
        let html = r#"<html lang="en"><div role="alert" id="rows"></div><input id="query"></html>"#;
        assert!(check_assets(html, "replaceChildren").is_ok());
        assert!(check_assets(html, "replaceChildren; innerHTML").is_err());
        assert!(check_assets("", "replaceChildren").is_err());
    }

    #[test]
    fn rejects_each_missing_accessibility_marker_individually() {
        let html = r#"<html lang="en"><div role="alert" id="rows"></div><input id="query"></html>"#;
        for marker in [r#"lang="en""#, r#"role="alert""#, r#"id="rows""#, r#"id="query""#] {
            let missing = html.replace(marker, "");
            assert!(check_assets(&missing, "replaceChildren").is_err(), "missing marker admitted: {marker}");
        }
    }

    #[test]
    fn rejects_oversized_fixture_files() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("orestra-ui-size-{}-{nonce}", std::process::id()));
        fs::create_dir_all(root.join("data")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("data/demo.json"),
            vec![b'x'; (MAX_FIXTURE_BYTES + 1) as usize],
        )
        .unwrap();
        fs::write(root.join("index.html"), r#"<html lang="en"><div role="alert" id="rows"></div><input id="query"></html>"#).unwrap();
        fs::write(root.join("assets/app.js"), "replaceChildren").unwrap();

        assert!(check(&root).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_fixture_files() {
        use std::os::unix::fs::symlink;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let parent = env::temp_dir().join(format!("orestra-ui-symlink-{}-{nonce}", std::process::id()));
        let root = parent.join("repo");
        fs::create_dir_all(root.join("data")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("data/demo.json"), r#"{"nodes":[{"status":{"phase":"Ready"}}],"workloads":[{"status":{"phase":"Ready"}}]}"#).unwrap();
        fs::write(root.join("index.html"), r#"<html lang="en"><div role="alert" id="rows"></div><input id="query"></html>"#).unwrap();
        let outside = parent.join("outside.js");
        fs::write(&outside, "replaceChildren").unwrap();
        symlink(&outside, root.join("assets/app.js")).unwrap();

        assert!(check(&root).is_err());
        fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn repo_assets_pass_static_gate() {
        assert!(check(Path::new(env!("CARGO_MANIFEST_DIR"))).is_ok());
    }
}
