use orestra_api_reference::validate::{check_all, load_schema, validate_document};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

fn workload() -> Value {
    serde_json::from_str(
        &fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/workload.json"))
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn all_schemas_are_draft_2020_12_objects() {
    let (schemas, examples) = check_all().unwrap();
    assert_eq!(schemas, 7);
    assert!(examples >= 4);
    for kind in [
        "workload",
        "node",
        "deployment",
        "reservation",
        "lease",
        "service",
        "namespace",
    ] {
        assert_eq!(load_schema(kind).unwrap()["type"], "object");
    }
}

#[test]
fn invalid_workload_mutations_are_rejected() {
    let original = workload();
    let mutations: Vec<Box<dyn Fn(&mut Value)>> = vec![
        Box::new(|d| {
            d.as_object_mut().unwrap().remove("metadata");
        }),
        Box::new(|d| d["metadata"]["generation"] = json!(true)),
        Box::new(|d| d["spec"]["containers"][0]["resources"]["cpuMillis"] = json!(-1)),
        Box::new(|d| d["spec"]["containers"][0]["resources"]["memoryBytes"] = json!(0)),
        Box::new(|d| d["spec"]["unknownCapability"] = json!("privileged")),
        Box::new(|d| d["metadata"]["name"] = json!("..")),
        Box::new(|d| d["metadata"]["namespace"] = json!("../admin")),
        Box::new(|d| d["kind"] = json!("Node")),
        Box::new(|d| d["force"] = json!(true)),
    ];
    for mutation in mutations {
        let mut candidate = original.clone();
        mutation(&mut candidate);
        assert!(
            validate_document(&candidate).is_err(),
            "invalid document accepted: {candidate}"
        );
    }
}

#[test]
fn unknown_kind_and_invalid_service_port_are_rejected() {
    let mut document = workload();
    document["kind"] = json!("Pod");
    assert!(validate_document(&document).is_err());

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut service: Value = serde_json::from_str(
        &fs::read_to_string(root.join("examples/service.json")).unwrap(),
    )
    .unwrap();
    validate_document(&service).unwrap();
    service["spec"]["ports"][0]["port"] = json!(65536);
    assert!(validate_document(&service).is_err());
}
