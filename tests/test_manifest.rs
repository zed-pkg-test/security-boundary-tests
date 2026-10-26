use serde_json::Value;
use std::fs;
use std::path::Path;
#[test]
fn scenarios_are_well_formed(){
    let dir=Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");let allowed=["create_node","deploy","reconcile","node_failure","leader_change","assert"];let mut count=0;
    for entry in fs::read_dir(dir).unwrap(){let path=entry.unwrap().path();if path.extension().and_then(|x|x.to_str())!=Some("json"){continue;}count+=1;
        let v:Value=serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();assert!(v["name"].as_str().is_some_and(|s|!s.is_empty()));
        let steps=v["steps"].as_array().filter(|x|!x.is_empty()).expect("steps");for step in steps{assert!(allowed.contains(&step["operation"].as_str().unwrap()));}}
    assert!(count>0);
}
