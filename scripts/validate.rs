use jsonschema::{Draft, JSONSchema};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub const KINDS: [&str; 7] = [
    "workload",
    "node",
    "deployment",
    "lease",
    "reservation",
    "service",
    "namespace",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

pub fn load_schema(kind: &str) -> Result<Value, String> {
    let kind = kind.to_ascii_lowercase();
    if !KINDS.contains(&kind.as_str()) {
        return Err("unsupported document kind".into());
    }
    let path = root().join("schemas/v1alpha1").join(format!("{kind}.schema.json"));
    let content = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let schema: Value = serde_json::from_str(&content)
        .map_err(|e| format!("{}: invalid JSON Schema: {e}", path.display()))?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| format!("{}: invalid JSON Schema: {e}", path.display()))?;
    if schema["$schema"].as_str() != Some("https://json-schema.org/draft/2020-12/schema") {
        return Err(format!("{}: wrong JSON Schema draft", path.display()));
    }
    if schema["properties"]["kind"]["const"]
        .as_str()
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some(kind.as_str())
    {
        return Err(format!("{}: kind const mismatch", path.display()));
    }
    Ok(schema)
}

pub fn validate_document(document: &Value) -> Result<(), String> {
    let object = document
        .as_object()
        .ok_or_else(|| "missing document kind".to_string())?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing document kind".to_string())?
        .to_ascii_lowercase();
    if !KINDS.contains(&kind.as_str()) {
        return Err("unsupported document kind".into());
    }
    let schema = load_schema(&kind)?;
    let compiled = JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .should_validate_formats(true)
        .compile(&schema)
        .map_err(|e| e.to_string())?;
    if let Err(errors) = compiled.validate(document) {
        let messages = errors.map(|e| e.to_string()).collect::<Vec<_>>();
        return Err(messages.join("; "));
    }
    Ok(())
}

pub fn check_all() -> Result<(usize, usize), String> {
    for kind in KINDS {
        let schema = load_schema(kind)?;
        if schema.get("additionalProperties").and_then(Value::as_bool) != Some(false) {
            return Err(format!("{kind}: additionalProperties must be false"));
        }
    }
    let examples = root().join("examples");
    let mut paths: Vec<PathBuf> = fs::read_dir(&examples)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|x| x.to_str()) == Some("json"))
        .collect();
    paths.sort();
    if paths.len() < 4 {
        return Err(format!("expected at least 4 examples, found {}", paths.len()));
    }
    for path in &paths {
        let document: Value = serde_json::from_str(
            &fs::read_to_string(path).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("{}: {e}", path.display()))?;
        validate_document(&document).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok((KINDS.len(), paths.len()))
}

fn main() {
    match check_all() {
        Ok((schemas, examples)) => {
            println!("validated {schemas} schemas and {examples} examples");
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
