use orestra_cli_tools::local::{capacity_plan, cpu_millis, import_k8s_deployment, memory_bytes, validate};
use serde_json::{json, Value};

fn deployment() -> Value {
    json!({
        "apiVersion":"apps/v1",
        "kind":"Deployment",
        "metadata":{"name":"web","namespace":"default","generation":3},
        "spec":{
            "replicas":2,
            "selector":{"matchLabels":{"app":"web"}},
            "template":{
                "metadata":{"labels":{"app":"web"}},
                "spec":{
                    "containers":[{
                        "name":"web",
                        "image":"nginx:stable",
                        "resources":{
                            "requests":{"cpu":"250m","memory":"64Mi"},
                            "limits":{"cpu":"250m","memory":"64Mi"}
                        }
                    }]
                }
            },
            "strategy":{
                "type":"RollingUpdate",
                "rollingUpdate":{"maxSurge":1,"maxUnavailable":0}
            }
        }
    })
}

#[test]
fn safe_deployment_conversion() {
    let output = import_k8s_deployment(&deployment()).unwrap();
    assert_eq!(output["apiVersion"], "orestra.io/v1alpha1");
    assert_eq!(output["kind"], "Deployment");
    assert_eq!(output["metadata"]["generation"], 3);
    assert_eq!(output["metadata"]["resourceVersion"], "0");
    assert_eq!(output["spec"]["replicas"], 2);
    assert_eq!(output["spec"]["template"]["containers"][0]["resources"]["cpuMillis"], 250);
    assert_eq!(
        output["spec"]["template"]["containers"][0]["resources"]["memoryBytes"],
        64 * 1024 * 1024
    );
    assert_eq!(output["spec"]["strategy"]["maxUnavailable"], 0);
}

#[test]
fn security_sensitive_pod_fields_are_rejected() {
    for (field, value) in [
        ("hostNetwork", json!(true)),
        ("hostPID", json!(true)),
        ("hostIPC", json!(true)),
        ("volumes", json!([{"hostPath":{"path":"/"}}])),
        ("securityContext", json!({"runAsUser":0})),
        ("serviceAccountName", json!("admin")),
    ] {
        let mut document = deployment();
        document["spec"]["template"]["spec"][field] = value;
        assert!(import_k8s_deployment(&document).is_err(), "{field}");
    }
}

#[test]
fn privileged_and_unmodelled_container_fields_are_rejected() {
    for (field, value) in [
        ("securityContext", json!({"privileged":true})),
        ("volumeMounts", json!([{"name":"host","mountPath":"/host"}])),
        ("ports", json!([{"containerPort":80}])),
        ("envFrom", json!([{"secretRef":{"name":"x"}}])),
    ] {
        let mut document = deployment();
        document["spec"]["template"]["spec"]["containers"][0][field] = value;
        assert!(import_k8s_deployment(&document).is_err(), "{field}");
    }
}

#[test]
fn percentage_rollout_missing_requests_and_selector_drift_are_rejected() {
    let mut percentage = deployment();
    percentage["spec"]["strategy"]["rollingUpdate"]["maxSurge"] = json!("25%");
    assert!(import_k8s_deployment(&percentage).is_err());

    let mut missing = deployment();
    missing["spec"]["template"]["spec"]["containers"][0]["resources"]["requests"]
        .as_object_mut().unwrap().remove("memory");
    assert!(import_k8s_deployment(&missing).is_err());

    let mut drift = deployment();
    drift["spec"]["template"]["metadata"]["labels"] = json!({"app":"other"});
    assert!(import_k8s_deployment(&drift).is_err());
}

#[test]
fn quantity_subset_is_intentionally_narrow() {
    assert_eq!(cpu_millis(&json!("2")).unwrap(), 2000);
    assert_eq!(memory_bytes(&json!("1Gi")).unwrap(), 1024i64.pow(3));

    for value in ["0.5", "100u"] {
        assert!(cpu_millis(&json!(value)).is_err(), "{value}");
    }
    for value in ["1.5Gi", "1G"] {
        assert!(memory_bytes(&json!(value)).is_err(), "{value}");
    }
}

#[test]
fn duplicate_container_names_are_rejected() {
    let mut document = deployment();
    let duplicate = document["spec"]["template"]["spec"]["containers"][0].clone();
    document["spec"]["template"]["spec"]["containers"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(import_k8s_deployment(&document).is_err());
}


#[test]
fn excessive_container_count_is_rejected() {
    let mut document = deployment();
    let template = document["spec"]["template"]["spec"]["containers"][0].clone();
    let containers = document["spec"]["template"]["spec"]["containers"]
        .as_array_mut()
        .unwrap();
    containers.clear();
    for index in 0..257 {
        let mut container = template.clone();
        container["name"] = json!(format!("c{index}"));
        containers.push(container);
    }
    assert!(import_k8s_deployment(&document).is_err());
}


#[test]
fn oversized_or_nul_identity_strings_are_rejected() {
    let mut manifest = json!({
        "apiVersion":"orestra.io/v1alpha1",
        "kind":"Workload",
        "metadata":{
            "name":"x",
            "namespace":"default",
            "uid":"u",
            "resourceVersion":"1",
            "generation":1
        },
        "spec":{}
    });
    manifest["metadata"]["name"] = json!("x".repeat(257));
    assert!(validate(&manifest).is_err());
    manifest["metadata"]["name"] = json!("bad\0name");
    assert!(validate(&manifest).is_err());

    let mut document = deployment();
    document["metadata"]["namespace"] = json!("n".repeat(257));
    assert!(import_k8s_deployment(&document).is_err());

    let mut document = deployment();
    document["spec"]["template"]["spec"]["containers"][0]["name"] = json!("c".repeat(257));
    assert!(import_k8s_deployment(&document).is_err());

    let mut document = deployment();
    document["spec"]["template"]["spec"]["containers"][0]["image"] = json!("i".repeat(2049));
    assert!(import_k8s_deployment(&document).is_err());
}


#[test]
fn capacity_plan_rejects_oversized_or_nul_node_names() {
    for name in ["x".repeat(257), "bad\0node".to_string()] {
        let nodes = json!([{
            "name":name,
            "cpuMillis":1000,
            "memoryBytes":1024,
            "usedCpuMillis":0,
            "usedMemoryBytes":0,
            "ready":true
        }]);
        assert!(capacity_plan(&nodes, 1, 1).is_err());
    }
}


#[test]
fn selector_label_maps_and_values_are_bounded() {
    let mut excessive = deployment();
    let mut labels = serde_json::Map::new();
    for index in 0..257 {
        labels.insert(format!("k{index}"), json!("v"));
    }
    excessive["spec"]["selector"]["matchLabels"] = Value::Object(labels.clone());
    excessive["spec"]["template"]["metadata"]["labels"] = Value::Object(labels);
    assert!(import_k8s_deployment(&excessive).is_err());

    let mut oversized_key = deployment();
    let mut labels = serde_json::Map::new();
    labels.insert("x".repeat(257), json!("v"));
    let labels = Value::Object(labels);
    oversized_key["spec"]["selector"]["matchLabels"] = labels.clone();
    oversized_key["spec"]["template"]["metadata"]["labels"] = labels;
    assert!(import_k8s_deployment(&oversized_key).is_err());

    let mut oversized_value = deployment();
    let mut labels = serde_json::Map::new();
    labels.insert("app".into(), json!("v".repeat(257)));
    let labels = Value::Object(labels);
    oversized_value["spec"]["selector"]["matchLabels"] = labels.clone();
    oversized_value["spec"]["template"]["metadata"]["labels"] = labels;
    assert!(import_k8s_deployment(&oversized_value).is_err());

    let mut non_text = deployment();
    non_text["spec"]["selector"]["matchLabels"] = json!({"app": 7});
    non_text["spec"]["template"]["metadata"]["labels"] = json!({"app": 7});
    assert!(import_k8s_deployment(&non_text).is_err());
}
