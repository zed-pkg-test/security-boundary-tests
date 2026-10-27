use orestra_cli_tools::local::{capacity_plan, run, validate};
use serde_json::json;

#[test]
fn valid_manifest_and_generation_type() {
    let mut example = json!({
        "apiVersion":"orestra.io/v1alpha1",
        "kind":"Workload",
        "metadata":{
            "name":"x","namespace":"default","uid":"a",
            "generation":1,"resourceVersion":"1"
        },
        "spec":{"containers":[]}
    });
    validate(&example).unwrap();
    example["metadata"]["generation"] = json!(true);
    assert!(validate(&example).is_err());
}

#[test]
fn offline_placement_ranks_only_viable_nodes() {
    let nodes = json!([
        {"name":"b","ready":true,"cpuMillis":1000,"memoryBytes":1000,"usedCpuMillis":900},
        {"name":"a","ready":true,"cpuMillis":1000,"memoryBytes":1000,"usedCpuMillis":200},
        {"name":"c","ready":false,"cpuMillis":1000,"memoryBytes":1000}
    ]);
    assert_eq!(capacity_plan(&nodes, 200, 300).unwrap(), ["a"]);
    assert!(capacity_plan(&nodes, 2000, 300).unwrap().is_empty());
}

#[test]
fn duplicate_nodes_and_invalid_types_fail_closed() {
    let inventories = [
        json!([
            {"name":"n1","ready":true,"cpuMillis":1000,"memoryBytes":1000},
            {"name":"n1","ready":true,"cpuMillis":1000,"memoryBytes":1000}
        ]),
        json!([{"name":"n1","ready":true,"cpuMillis":true,"memoryBytes":1000}]),
        json!([{"name":"n1","ready":true,"cpuMillis":1000,"memoryBytes":1000,"usedCpuMillis":1001}]),
        json!([{"name":"n1","ready":1,"cpuMillis":1000,"memoryBytes":1000}]),
        json!([{"name":"n1","ready":true,"cpuMillis":"1000","memoryBytes":1000}]),
        json!([{"name":"n1","ready":true,"cpuMillis":1000,"memoryBytes":1000,"unschedulable":"false"}]),
        json!([{"name":"","ready":true,"cpuMillis":1000,"memoryBytes":1000}]),
    ];
    for inventory in inventories {
        assert!(capacity_plan(&inventory, 100, 100).is_err(), "{inventory}");
    }
}

#[test]
fn invalid_requests_are_rejected() {
    let node = json!([{"name":"a","ready":true,"cpuMillis":1000,"memoryBytes":1000}]);
    for (cpu, memory) in [(-1,10),(10,0)] {
        assert!(capacity_plan(&node, cpu, memory).is_err());
    }
}

#[cfg(unix)]
#[test]
fn cli_rejects_symlinked_json_inputs() {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("orestra-cli-symlink-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&root).unwrap();

    let real = root.join("manifest.json");
    fs::write(
        &real,
        r#"{"apiVersion":"orestra.io/v1alpha1","kind":"Workload","metadata":{"name":"x","namespace":"default","uid":"u","generation":1,"resourceVersion":"1"},"spec":{}}"#,
    )
    .unwrap();
    let link = root.join("manifest-link.json");
    symlink(&real, &link).unwrap();

    assert!(run(&["lint".into(), link.to_string_lossy().into_owned()]).is_err());
    fs::remove_dir_all(&root).unwrap();
}
