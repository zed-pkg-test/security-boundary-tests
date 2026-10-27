use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const VALID_KINDS: [&str; 5] = ["Workload", "Deployment", "Node", "Lease", "Reservation"];
const MAX_CONTAINERS: usize = 256;
const MAX_IDENTITY_BYTES: usize = 256;
const MAX_IMAGE_BYTES: usize = 2048;

fn bounded_text<'a>(
    value: Option<&'a Value>,
    label: &str,
    max_bytes: usize,
) -> Result<&'a str, String> {
    let text = value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{label} must be nonempty text"))?;
    if text.len() > max_bytes || text.contains('\0') {
        return Err(format!("{label} exceeds supported identity bound"));
    }
    Ok(text)
}

pub fn validate(value: &Value) -> Result<(), String> {
    let object = value.as_object().ok_or_else(|| "manifest must be an object".to_string())?;
    if object.get("apiVersion").and_then(Value::as_str) != Some("orestra.io/v1alpha1") {
        return Err("unsupported apiVersion".into());
    }
    let kind = object.get("kind").and_then(Value::as_str).ok_or_else(|| "unknown kind".to_string())?;
    if !VALID_KINDS.contains(&kind) {
        return Err("unknown kind".into());
    }
    let metadata = object
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| "missing metadata".to_string())?;
    for key in ["name", "namespace", "uid", "resourceVersion"] {
        bounded_text(metadata.get(key), &format!("metadata.{key}"), MAX_IDENTITY_BYTES)?;
    }
    if metadata.get("generation").and_then(Value::as_i64).is_none_or(|generation| generation < 1) {
        return Err("metadata.generation must be positive integer".into());
    }
    if object.get("spec").and_then(Value::as_object).is_none() {
        return Err("missing spec".into());
    }
    Ok(())
}

pub fn cpu_millis(value: &Value) -> Result<i64, String> {
    if let Some(amount) = value.as_i64() {
        if amount <= 0 {
            return Err("CPU must be positive".into());
        }
        return amount.checked_mul(1000).ok_or_else(|| "CPU quantity overflow".to_string());
    }
    let text = value
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| "unsupported Kubernetes CPU quantity".to_string())?;
    if let Some(number) = text.strip_suffix('m') {
        if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) {
            let amount = number.parse::<i64>().map_err(|_| "unsupported Kubernetes CPU quantity")?;
            if amount > 0 {
                return Ok(amount);
            }
        }
    } else if text.bytes().all(|b| b.is_ascii_digit()) {
        let amount = text.parse::<i64>().map_err(|_| "unsupported Kubernetes CPU quantity")?;
        if amount > 0 {
            return amount.checked_mul(1000).ok_or_else(|| "CPU quantity overflow".to_string());
        }
    }
    Err("supported CPU quantities are positive cores or millicores".into())
}

pub fn memory_bytes(value: &Value) -> Result<i64, String> {
    if let Some(amount) = value.as_i64() {
        if amount > 0 {
            return Ok(amount);
        }
        return Err("memory must be positive".into());
    }
    let text = value
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| "unsupported Kubernetes memory quantity".to_string())?;
    for (suffix, multiplier) in [("Ki", 1024i64), ("Mi", 1024i64.pow(2)), ("Gi", 1024i64.pow(3))] {
        if let Some(number) = text.strip_suffix(suffix) {
            if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) {
                let amount = number.parse::<i64>().map_err(|_| "unsupported Kubernetes memory quantity")?;
                if amount > 0 {
                    return amount.checked_mul(multiplier).ok_or_else(|| "memory quantity overflow".to_string());
                }
            }
        }
    }
    if text.bytes().all(|b| b.is_ascii_digit()) {
        let amount = text.parse::<i64>().map_err(|_| "unsupported Kubernetes memory quantity")?;
        if amount > 0 {
            return Ok(amount);
        }
    }
    Err("supported memory quantities are bytes, Ki, Mi or Gi".into())
}

fn exact_or_subset(map: &Map<String, Value>, allowed: &[&str]) -> bool {
    map.keys().all(|key| allowed.contains(&key.as_str()))
}

pub fn import_k8s_deployment(value: &Value) -> Result<Value, String> {
    let root = value.as_object().ok_or_else(|| "only apps/v1 Deployment is supported".to_string())?;
    if root.get("apiVersion").and_then(Value::as_str) != Some("apps/v1")
        || root.get("kind").and_then(Value::as_str) != Some("Deployment")
    {
        return Err("only apps/v1 Deployment is supported".into());
    }
    if !exact_or_subset(root, &["apiVersion", "kind", "metadata", "spec"]) {
        return Err("unsupported top-level Kubernetes fields".into());
    }
    let metadata = root.get("metadata").and_then(Value::as_object)
        .ok_or_else(|| "metadata/spec must be objects".to_string())?;
    let spec = root.get("spec").and_then(Value::as_object)
        .ok_or_else(|| "metadata/spec must be objects".to_string())?;
    if !exact_or_subset(metadata, &["name", "namespace", "generation", "labels"]) {
        return Err("unsupported Kubernetes metadata".into());
    }
    let name = bounded_text(metadata.get("name"), "Kubernetes metadata.name", MAX_IDENTITY_BYTES)?;
    let namespace = metadata.get("namespace").and_then(Value::as_str).unwrap_or("default");
    if namespace.is_empty()
        || namespace.len() > MAX_IDENTITY_BYTES
        || namespace.contains('\0')
    {
        return Err("invalid Kubernetes metadata".into());
    }
    let generation = metadata.get("generation").map(Value::as_i64).unwrap_or(Some(1))
        .filter(|x| *x > 0).ok_or_else(|| "invalid Kubernetes metadata".to_string())?;

    if !exact_or_subset(spec, &["replicas", "selector", "template", "strategy"]) {
        return Err("unsupported Kubernetes Deployment fields".into());
    }
    let replicas = spec.get("replicas").map(Value::as_i64).unwrap_or(Some(1))
        .filter(|x| *x >= 0).ok_or_else(|| "replicas must be a nonnegative integer".to_string())?;
    let selector = spec.get("selector").and_then(Value::as_object)
        .ok_or_else(|| "only matchLabels selectors are supported".to_string())?;
    if selector.len() != 1 || selector.get("matchLabels").and_then(Value::as_object).is_none() {
        return Err("only matchLabels selectors are supported".into());
    }
    let match_labels = selector["matchLabels"].as_object().unwrap();
    let template = spec.get("template").and_then(Value::as_object)
        .ok_or_else(|| "unsupported pod template fields".to_string())?;
    if !exact_or_subset(template, &["metadata", "spec"]) {
        return Err("unsupported pod template fields".into());
    }
    let pod_metadata = template.get("metadata").and_then(Value::as_object)
        .cloned().unwrap_or_default();
    if !exact_or_subset(&pod_metadata, &["labels"]) {
        return Err("invalid pod template".into());
    }
    let pod_spec = template.get("spec").and_then(Value::as_object)
        .ok_or_else(|| "invalid pod template".to_string())?;
    let pod_labels = pod_metadata.get("labels").and_then(Value::as_object)
        .cloned().unwrap_or_default();
    if &pod_labels != match_labels {
        return Err("template labels must exactly match selector in import subset".into());
    }
    for forbidden in [
        "hostNetwork","hostPID","hostIPC","volumes","securityContext",
        "nodeName","serviceAccountName","initContainers"
    ] {
        if pod_spec.contains_key(forbidden) {
            return Err("security-sensitive/unsupported pod fields are rejected".into());
        }
    }
    if !exact_or_subset(pod_spec, &["containers", "restartPolicy"]) {
        return Err("unsupported pod spec fields".into());
    }
    if pod_spec.get("restartPolicy").and_then(Value::as_str).unwrap_or("Always") != "Always" {
        return Err("Deployment import requires restartPolicy Always".into());
    }
    let containers = pod_spec.get("containers").and_then(Value::as_array)
        .filter(|items| !items.is_empty()).ok_or_else(|| "containers must be a nonempty array".to_string())?;
    if containers.len() > MAX_CONTAINERS {
        return Err(format!("containers exceed supported bound of {MAX_CONTAINERS}"));
    }
    let mut out_containers = Vec::new();
    let mut container_names = HashSet::new();
    for container in containers {
        let container = container.as_object().ok_or_else(|| "container must be an object".to_string())?;
        if !exact_or_subset(container, &["name", "image", "resources"]) {
            return Err("unsupported container field".into());
        }
        let container_name =
            bounded_text(container.get("name"), "container name", MAX_IDENTITY_BYTES)?;
        if !container_names.insert(container_name.to_string()) {
            return Err("duplicate container name".into());
        }
        let image = bounded_text(container.get("image"), "container image", MAX_IMAGE_BYTES)?;
        let resources = container.get("resources").and_then(Value::as_object)
            .ok_or_else(|| "resource requests are required".to_string())?;
        if !exact_or_subset(resources, &["requests", "limits"]) {
            return Err("resource requests are required".into());
        }
        let requests = resources.get("requests").and_then(Value::as_object)
            .ok_or_else(|| "exact CPU and memory requests are required".to_string())?;
        if requests.len() != 2 || !requests.contains_key("cpu") || !requests.contains_key("memory") {
            return Err("exact CPU and memory requests are required".into());
        }
        if let Some(limits) = resources.get("limits") {
            if limits != resources.get("requests").unwrap() {
                return Err("import subset requires limits to equal requests when present".into());
            }
        }
        out_containers.push(json!({
            "name":container_name,
            "image":image,
            "resources":{
                "cpuMillis":cpu_millis(&requests["cpu"])?,
                "memoryBytes":memory_bytes(&requests["memory"])?
            }
        }));
    }

    let mut output = json!({
        "apiVersion":"orestra.io/v1alpha1",
        "kind":"Deployment",
        "metadata":{
            "name":name,
            "namespace":namespace,
            "uid":format!("k8s-{namespace}-{name}"),
            "generation":generation,
            "resourceVersion":"0"
        },
        "spec":{
            "replicas":replicas,
            "template":{"containers":out_containers}
        }
    });

    if let Some(strategy) = spec.get("strategy") {
        let strategy = strategy.as_object().ok_or_else(|| "unsupported strategy".to_string())?;
        if !exact_or_subset(strategy, &["type", "rollingUpdate"]) {
            return Err("unsupported strategy".into());
        }
        if strategy.get("type").and_then(Value::as_str).unwrap_or("RollingUpdate") != "RollingUpdate" {
            return Err("only RollingUpdate strategy is supported".into());
        }
        let rolling = match strategy.get("rollingUpdate") {
            Some(value) => value.as_object().ok_or_else(|| "unsupported rollingUpdate fields".to_string())?,
            None => {
                output["spec"]["strategy"] = json!({"maxSurge":1,"maxUnavailable":1});
                return Ok(output);
            }
        };
        if !exact_or_subset(rolling, &["maxSurge", "maxUnavailable"]) {
            return Err("unsupported rollingUpdate fields".into());
        }
        let surge = rolling.get("maxSurge").map(Value::as_i64).unwrap_or(Some(1))
            .filter(|x| *x >= 0).ok_or_else(|| "percentage rollout values are not supported".to_string())?;
        let unavailable = rolling.get("maxUnavailable").map(Value::as_i64).unwrap_or(Some(1))
            .filter(|x| *x >= 0).ok_or_else(|| "percentage rollout values are not supported".to_string())?;
        output["spec"]["strategy"] = json!({"maxSurge":surge,"maxUnavailable":unavailable});
    }
    Ok(output)
}

pub fn capacity_plan(nodes: &Value, cpu: i64, memory: i64) -> Result<Vec<String>, String> {
    if cpu <= 0 || memory <= 0 {
        return Err("resource requests must be positive signed i64 values".into());
    }
    let nodes = nodes.as_array().ok_or_else(|| "nodes must be a bounded array".to_string())?;
    if nodes.len() > 100_000 {
        return Err("nodes must be a bounded array".into());
    }
    let mut viable = Vec::new();
    let mut names = HashSet::new();
    for node in nodes {
        let node = node.as_object().ok_or_else(|| "node must be an object".to_string())?;
        let name = bounded_text(node.get("name"), "node name", MAX_IDENTITY_BYTES)?;
        if !names.insert(name.to_string()) {
            return Err("empty or duplicate node name".into());
        }
        let cpu_capacity = quantity(node, "cpuMillis", false)?;
        let memory_capacity = quantity(node, "memoryBytes", false)?;
        let used_cpu = quantity(node, "usedCpuMillis", true)?;
        let used_memory = quantity(node, "usedMemoryBytes", true)?;
        let ready = node.get("ready").and_then(Value::as_bool)
            .ok_or_else(|| "ready must be a boolean".to_string())?;
        let unschedulable = match node.get("unschedulable") {
            Some(value) => value.as_bool().ok_or_else(|| "unschedulable must be a boolean".to_string())?,
            None => false,
        };
        if cpu_capacity <= 0 || memory_capacity <= 0 || used_cpu > cpu_capacity || used_memory > memory_capacity {
            return Err("invalid node capacity/accounting".into());
        }
        if !ready || unschedulable {
            continue;
        }
        if cpu <= cpu_capacity - used_cpu && memory <= memory_capacity - used_memory {
            let cpu_score = used_cpu.checked_add(cpu).and_then(|x| x.checked_mul(1000))
                .ok_or_else(|| "resource score overflow".to_string())? / cpu_capacity;
            let mem_score = used_memory.checked_add(memory).and_then(|x| x.checked_mul(1000))
                .ok_or_else(|| "resource score overflow".to_string())? / memory_capacity;
            let score = cpu_score.checked_add(mem_score).ok_or_else(|| "resource score overflow".to_string())?;
            viable.push((-score, name.to_string()));
        }
    }
    viable.sort();
    Ok(viable.into_iter().map(|(_, name)| name).collect())
}

fn quantity(node: &Map<String, Value>, key: &str, optional: bool) -> Result<i64, String> {
    let Some(value) = node.get(key) else {
        return if optional { Ok(0) } else { Err(format!("invalid signed i64 quantity: {key}")) };
    };
    let value = value.as_i64().filter(|x| *x >= 0)
        .ok_or_else(|| format!("invalid signed i64 quantity: {key}"))?;
    Ok(value)
}

fn read_json_bounded(path: &Path, label: &str) -> Result<Value, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} must be a regular non-symlink file"));
    }
    if metadata.len() > 4 * 1024 * 1024 {
        return Err(format!("{label} larger than 4 MiB"));
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

pub fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: orestra-local <lint|import-k8s|plan> ...".into());
    }
    match args[0].as_str() {
        "lint" if args.len() == 2 => {
            let value = read_json_bounded(Path::new(&args[1]), "manifest")?;
            validate(&value)?;
            println!(
                "OK: {} {}/{}",
                value["kind"].as_str().unwrap(),
                value["metadata"]["namespace"].as_str().unwrap(),
                value["metadata"]["name"].as_str().unwrap()
            );
            Ok(())
        }
        "import-k8s" if args.len() == 2 => {
            let source = read_json_bounded(Path::new(&args[1]), "manifest")?;
            let output = import_k8s_deployment(&source)?;
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
            Ok(())
        }
        "plan" => {
            if args.len() != 6 || args[2] != "--cpu-millis" || args[4] != "--memory-bytes" {
                return Err("usage: orestra-local plan <nodes.json> --cpu-millis N --memory-bytes N".into());
            }
            let nodes = read_json_bounded(Path::new(&args[1]), "node inventory")?;
            let cpu = args[3].parse::<i64>().map_err(|_| "invalid cpu millis")?;
            let memory = args[5].parse::<i64>().map_err(|_| "invalid memory bytes")?;
            println!("{}", serde_json::to_string_pretty(&json!({
                "candidateNodes":capacity_plan(&nodes, cpu, memory)?
            })).unwrap());
            Ok(())
        }
        _ => Err("invalid command or arguments".into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = run(&args) {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}
