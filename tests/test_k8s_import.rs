use orestra_cli_tools::local::{cpu_millis, import_k8s_deployment, memory_bytes};
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
