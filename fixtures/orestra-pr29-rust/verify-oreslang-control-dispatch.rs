use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Command;

pub const DISPATCH_PATH: &str = "ci/oreslang-dispatch.json";
pub const SCHEMA: &str = "ores-private-ci-control-v1";
const CARRIER_REPO: &str = "orestra/orestra-conformance";
const CARRIER_REPO_ID: u64 = 1426277234;
const WORKFLOW: &str = ".github/workflows/oreslang-exact-tree-carrier.yml";
const MAX_DISPATCH: u64 = 16 * 1024;

#[derive(Debug, Clone)]
pub struct Denied(pub String);
impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}
impl std::error::Error for Denied {}
fn deny(s: impl Into<String>) -> Denied { Denied(s.into()) }

fn is_sha(v: &str) -> bool {
    v.len() == 40 && v.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn require_sha(v: &str, label: &str) -> Result<String, Denied> {
    if !is_sha(v) { return Err(deny(label)); }
    Ok(v.to_string())
}
fn git(root: &Path, args: &[&str]) -> Result<String, Denied> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .output()
        .map_err(|_| deny("required Git metadata unavailable"))?;
    if !out.status.success() { return Err(deny("required Git metadata unavailable")); }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
fn exact_keys(v: &Value, keys: &[&str]) -> bool {
    v.as_object().is_some_and(|o| o.len() == keys.len() && keys.iter().all(|k| o.contains_key(*k)))
}

pub fn verify(root: &Path, path: &str, repository: &str, repository_id: u64, head_sha: &str, ref_name: &str) -> Result<Value, Denied> {
    if repository != CARRIER_REPO || repository_id != CARRIER_REPO_ID {
        return Err(deny("control repository identity"));
    }
    let actual = require_sha(&git(root, &["rev-parse", "HEAD"])?, "control head invalid")?;
    let event_head = require_sha(head_sha, "event head invalid")?;
    if actual != event_head { return Err(deny("control head/context mismatch")); }

    let target = ref_name.strip_prefix("ci/oreslang-control-").ok_or_else(|| deny("unexpected control ref"))?;
    let target = require_sha(target, "control ref target invalid")?;

    let parents = git(root, &["rev-list", "--parents", "-n", "1", "HEAD"])?;
    let parts: Vec<_> = parents.split_whitespace().collect();
    if parts.len() != 2 || parts[0] != actual {
        return Err(deny("control commit must have exactly one parent"));
    }
    let parent = require_sha(parts[1], "control parent invalid")?;

    let diff = Command::new("git")
        .args(["diff", "--name-only", "-z", &parent, &actual])
        .current_dir(root)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .output()
        .map_err(|_| deny("required Git metadata unavailable"))?;
    if !diff.status.success() { return Err(deny("required Git metadata unavailable")); }
    let changed: Vec<String> = diff.stdout.split(|b| *b == 0).filter(|x| !x.is_empty())
        .map(|x| String::from_utf8_lossy(x).into_owned()).collect();
    if changed != [path.to_string()] {
        return Err(deny("control commit may change only dispatch metadata"));
    }

    let dispatch = root.join(path);
    let meta = fs::symlink_metadata(&dispatch).map_err(|_| deny("dispatch metadata missing or unsafe"))?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() == 0 || meta.len() > MAX_DISPATCH {
        return Err(deny("dispatch metadata missing or oversized"));
    }
    let raw = fs::read(&dispatch).map_err(|_| deny("dispatch metadata unreadable"))?;
    let doc: Value = serde_json::from_slice(&raw).map_err(|_| deny("dispatch metadata invalid JSON"))?;
    let mut canonical = serde_json::to_vec(&doc).map_err(|_| deny("dispatch metadata invalid JSON"))?;
    canonical.push(b'\n');
    if raw != canonical {
        return Err(deny("dispatch metadata must be canonical JSON"));
    }
    if !exact_keys(&doc, &["schema", "carrier_commit_sha", "source_ref", "authorization_ref"]) {
        return Err(deny("dispatch fields are not exact"));
    }
    if doc["schema"] != SCHEMA { return Err(deny("dispatch schema mismatch")); }
    let carrier = require_sha(doc["carrier_commit_sha"].as_str().ok_or_else(|| deny("dispatch carrier SHA invalid"))?, "dispatch carrier SHA invalid")?;
    if carrier != target { return Err(deny("control ref/dispatch target mismatch")); }

    let source_ref = doc["source_ref"].as_str().ok_or_else(|| deny("dispatch source ref invalid"))?;
    let source_tree = source_ref.strip_prefix("ci/oreslang-").ok_or_else(|| deny("dispatch source ref invalid"))?;
    require_sha(source_tree, "dispatch source ref invalid")?;

    let authorization_ref = doc["authorization_ref"].as_str().ok_or_else(|| deny("dispatch authorization ref mismatch"))?;
    if authorization_ref != format!("ci/authorized-{target}") {
        return Err(deny("dispatch authorization ref mismatch"));
    }

    let workflow = git(root, &["rev-parse", &format!("HEAD:{WORKFLOW}")])?;
    let parent_workflow = git(root, &["rev-parse", &format!("HEAD^:{WORKFLOW}")])?;
    if workflow != parent_workflow { return Err(deny("control workflow changed")); }

    Ok(json!({
        "control_head": actual,
        "control_parent": parent,
        "carrier_commit_sha": target,
        "source_ref": source_ref,
        "authorization_ref": authorization_ref
    }))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut root = ".".to_string();
    let mut dispatch = DISPATCH_PATH.to_string();
    let mut i = 0;
    while i < args.len() {
        if i + 1 >= args.len() {
            eprintln!("CONTROL_DISPATCH_DENIED: missing argument value");
            std::process::exit(1);
        }
        match args[i].as_str() {
            "--repo-root" => root = args[i + 1].clone(),
            "--dispatch" => dispatch = args[i + 1].clone(),
            _ => {
                eprintln!("CONTROL_DISPATCH_DENIED: unknown argument");
                std::process::exit(1);
            }
        }
        i += 2;
    }
    let repo = std::env::var("GITHUB_REPOSITORY").unwrap_or_default();
    let id = std::env::var("GITHUB_REPOSITORY_ID").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let head = std::env::var("GITHUB_SHA").unwrap_or_default();
    let ref_name = std::env::var("GITHUB_REF_NAME").unwrap_or_default();
    match verify(Path::new(&root), &dispatch, &repo, id, &head, &ref_name) {
        Ok(v) => {
            for (k, val) in v.as_object().unwrap() {
                let rendered = val.as_str().map(str::to_string).unwrap_or_else(|| val.to_string());
                println!("{}={}", k.to_ascii_uppercase(), rendered);
            }
            println!("CONTROL_DISPATCH=PASS");
        }
        Err(e) => {
            eprintln!("CONTROL_DISPATCH_DENIED: {e}");
            std::process::exit(1);
        }
    }
}
