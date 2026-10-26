//! Fail-closed manifest validator: checks declarations, never loads or executes drivers.
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{Map, Number, Value};
use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

// Unlike serde_json::Value's default deserialization, reject duplicate JSON keys
// at every nesting level. A manifest cannot smuggle conflicting policy entries.
struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value without duplicate object keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(Number::from(v))))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(Number::from(v))))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("invalid floating-point JSON number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.to_owned())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
                StrictValue::deserialize(d)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some(item) = seq.next_element::<StrictValue>()? {
                    out.push(item.0);
                }
                Ok(StrictValue(Value::Array(out)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = Map::new();
                while let Some((key, value)) = map.next_entry::<String, StrictValue>()? {
                    if out.insert(key.clone(), value.0).is_some() {
                        return Err(de::Error::custom(format!("duplicate JSON key: {key}")));
                    }
                }
                Ok(StrictValue(Value::Object(out)))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}

fn exact_keys(v: &Value, expected: &[&str]) -> bool {
    v.as_object().is_some_and(|o|
        o.len() == expected.len() && expected.iter().all(|key| o.contains_key(*key))
    )
}

fn registry_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn version(value: &str) -> bool {
    let core = value.strip_suffix("-alpha").unwrap_or(value);
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p|
        !p.is_empty() && (p.len() == 1 || !p.starts_with('0')) &&
        p.as_bytes().iter().all(u8::is_ascii_digit)
    )
}

fn labels(value: &Value) -> bool {
    let Some(arr) = value.as_array() else { return false };
    let mut seen = HashSet::new();
    !arr.is_empty() && arr.iter().all(|v|
        v.as_str().is_some_and(|s| registry_name(s) && seen.insert(s))
    )
}

fn validate(v: &Value) -> Result<(), &'static str> {
    if !exact_keys(v, &["apiVersion", "kind", "metadata", "spec"]) {
        return Err("unexpected manifest keys");
    }
    if v["apiVersion"].as_str() != Some("orestra.io/v1alpha1") {
        return Err("api version");
    }
    if v["kind"].as_str() != Some("DriverManifest") {
        return Err("kind");
    }
    let meta = &v["metadata"];
    if !exact_keys(meta, &["name", "version"]) {
        return Err("metadata shape");
    }
    if !meta["name"].as_str().is_some_and(registry_name) {
        return Err("invalid plugin name");
    }
    if !meta["version"].as_str().is_some_and(version) {
        return Err("invalid plugin version");
    }
    let spec = &v["spec"];
    if !exact_keys(spec, &[
        "category", "abi", "platforms", "implementation",
        "runtimeContract", "sandbox", "status",
    ]) {
        return Err("spec shape");
    }
    if !matches!(
        (spec["category"].as_str(), spec["runtimeContract"].as_str()),
        (Some("runtime"), Some("OCI"))
            | (Some("network"), Some("CNI"))
            | (Some("volume"), Some("CSI"))
    ) {
        return Err("incompatible category/contract");
    }
    let abi = &spec["abi"];
    if !exact_keys(abi, &["min", "max"]) {
        return Err("invalid ABI");
    }
    let (Some(min), Some(max)) = (abi["min"].as_u64(), abi["max"].as_u64()) else {
        return Err("invalid ABI");
    };
    if min == 0 || min > max || max > 65535 {
        return Err("invalid ABI");
    }
    let Some(platforms) = spec["platforms"].as_array() else {
        return Err("invalid platform set");
    };
    let mut seen = HashSet::new();
    if platforms.is_empty() || !platforms.iter().all(|v|
        v.as_str().is_some_and(|s|
            matches!(s, "linux/amd64" | "linux/arm64") && seen.insert(s)
        )
    ) {
        return Err("invalid platform set");
    }
    if !spec["implementation"].as_str().is_some_and(registry_name) {
        return Err("implementation must be symbolic, not a path");
    }
    let sandbox = &spec["sandbox"];
    if !exact_keys(sandbox, &["requires", "mustDeny"])
        || !labels(&sandbox["requires"]) || !labels(&sandbox["mustDeny"]) {
        return Err("invalid or duplicate sandbox policy labels");
    }
    if spec["status"].as_str() != Some("planned") {
        return Err("unverified driver is not ready");
    }
    Ok(())
}

fn validate_bytes(bytes: &[u8]) -> Result<Value, String> {
    let StrictValue(value) = serde_json::from_slice::<StrictValue>(bytes)
        .map_err(|e| format!("invalid/ambiguous JSON: {e}"))?;
    validate(&value).map_err(String::from)?;
    Ok(value)
}

fn check_all(root: &Path) -> Result<usize, String> {
    let dir = root.join("manifests");
    let mut paths = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let kind = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
            if !kind.is_file() || kind.is_symlink() {
                return Err(format!("manifest must be a regular file: {}", path.display()));
            }
            paths.push(path);
        }
    }
    paths.sort();
    if paths.len() < 3 {
        return Err("expected OCI/CNI/CSI contracts".into());
    }
    let mut names = HashSet::new();
    for path in &paths {
        let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
        if size > 65_536 {
            return Err(format!("{}: manifest exceeds 64 KiB", path.display()));
        }
        let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let value = validate_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = value["metadata"]["name"].as_str().ok_or("invalid plugin name")?;
        if !names.insert(name.to_string()) {
            return Err(format!("{}: duplicate manifest name", path.display()));
        }
    }
    Ok(paths.len())
}

fn main() {
    let mut args = env::args_os().skip(1);
    let root = match args.next() {
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        Some(flag) if flag == "--root" => match args.next() {
            Some(path) if args.next().is_none() => PathBuf::from(path),
            _ => {
                eprintln!("usage: orestra-driver-manifest [--root PATH]");
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!("usage: orestra-driver-manifest [--root PATH]");
            std::process::exit(2);
        }
    };
    match check_all(&root) {
        Ok(count) => println!("{count} driver manifests checked"),
        Err(error) => {
            eprintln!("driver manifests DENIED: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid() -> Value {
        serde_json::from_str(include_str!("../../manifests/oci-linux.json")).unwrap()
    }

    #[test]
    fn validates_all_repo_manifests() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(check_all(root).unwrap() >= 3);
    }

    #[test]
    fn keeps_valid_manifest_unchanged() {
        let manifest = valid();
        assert_eq!(validate(&manifest), Ok(()));
        assert_eq!(validate_bytes(serde_json::to_string(&manifest).unwrap().as_bytes()).unwrap(), manifest);
    }

    #[test]
    fn rejects_invalid_abi_plugin_status_and_policy() {
        for case in 0..14 {
            let mut m = valid();
            match case {
                0 => { m["spec"]["abi"]["min"] = json!(2); m["spec"]["abi"]["max"] = json!(1); }
                1 => m["spec"]["abi"]["min"] = json!(true),
                2 => m["spec"]["status"] = json!("ready"),
                3 => m["spec"]["implementation"] = json!("../../bin/sh"),
                4 => m["spec"]["category"] = json!("network"),
                5 => m["spec"]["sandbox"]["requires"] = json!([]),
                6 => m["spec"]["sandbox"]["mustDeny"] = json!(["allow-all", "allow-all"]),
                7 => m["spec"]["platforms"] = json!(["linux/windows"]),
                8 => m["shellCommand"] = json!("/bin/sh"),
                9 => m["metadata"]["name"] = json!("../escape"),
                10 => m["metadata"]["version"] = json!("01.2.3"),
                11 => m["spec"]["platforms"] = json!(["linux/amd64", "linux/amd64"]),
                12 => m["spec"]["abi"]["max"] = json!(65536),
                _ => m["spec"]["sandbox"]["requires"] = json!(["bad/path"]),
            }
            assert!(validate(&m).is_err(), "mutation {case} unexpectedly admitted");
        }
    }

    #[test]
    fn rejects_ambiguous_json_and_wrong_types() {
        assert!(validate_bytes(br#"{"x":1,"x":2}"#).is_err());
        let serialized = serde_json::to_string(&valid()).unwrap();
        let ambiguous = serialized.replace(
            r#""name":"oci-linux""#,
            r#""name":"oci-linux","name":"evil""#
        );
        assert!(validate_bytes(ambiguous.as_bytes()).is_err());
        let mut m = valid();
        m["spec"]["abi"]["min"] = json!(1.0);
        assert!(validate(&m).is_err());
        assert!(!version("1.02.3"));
        assert!(version("0.1.0-alpha"));
    }
}
