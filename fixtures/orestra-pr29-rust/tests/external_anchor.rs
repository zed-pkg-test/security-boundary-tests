use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{TimeZone,Utc};
use orestra_conformance::external_anchor::*;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tempfile::tempdir;

static ENV_LOCK:Mutex<()>=Mutex::new(());
const HEAD:&str="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE:&str="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TREE:&str="cccccccccccccccccccccccccccccccccccccccc";
const SOURCE_HEAD:&str="dddddddddddddddddddddddddddddddddddddddd";
const ANCHOR_SHA:&str="eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const ANCHOR_PR_HEAD:&str="ffffffffffffffffffffffffffffffffffffffff";
const WORKFLOW_BLOB:&str="1111111111111111111111111111111111111111";

fn sha(data:&[u8])->String{format!("{:x}",Sha256::digest(data))}
fn item(path:&str,raw:&[u8])->Value{
    json!({"path":path,"type":"file","encoding":"base64","content":STANDARD.encode(raw),"sha":git_blob_sha(raw)})
}
fn strict_rules()->Value{
    json!([
        {"type":"pull_request","parameters":{
            "required_approving_review_count":1,
            "dismiss_stale_reviews_on_push":true,
            "require_code_owner_review":true,
            "require_last_push_approval":true,
            "required_review_thread_resolution":true
        }},
        {"type":"deletion"},
        {"type":"non_fast_forward"}
    ])
}

struct Fixture{
    _dir:tempfile::TempDir,
    root:PathBuf,
    routes:HashMap<String,Value>,
    manifest:Value,
    plan:Vec<u8>,
    source_pr:u64,
    auth_pr:u64,
}
impl Fixture{
    fn new()->Self{Self::new_for(1668,52)}
    fn new_for(source_pr:u64,auth_pr:u64)->Self{
        let dir=tempdir().unwrap();
        let root=dir.path().to_path_buf();
        let plan=b"{\"schema\":\"ores-private-ci-test-plan-v1\"}\n".to_vec();
        let plan_path=format!("ci/test-plans/java-pr{source_pr}.json");
        let manifest_path=format!("ci/private-ci/authorizations/java-pr{source_pr}.json");
        let manifest=json!({
            "schema":MANIFEST_SCHEMA,
            "anchor_repository_id":ANCHOR_REPO_ID,
            "source":{"repository":SOURCE_REPO,"repository_id":SOURCE_REPO_ID,"base_branch":"main","pr":source_pr,"head_sha":SOURCE_HEAD,"tree_sha":TREE},
            "destination":{"repository":CARRIER_REPO,"repository_id":CARRIER_REPO_ID,"base_sha":BASE,
                "workflow_path":".github/workflows/oreslang-exact-tree-carrier.yml","workflow_blob_sha":WORKFLOW_BLOB,
                "test_plan_path":plan_path,"test_plan_blob_sha":git_blob_sha(&plan)},
            "constraints":{"source_must_be_private":true,"destination_must_be_private":true,
                "source_tree_prefix":"private-oreslang-source","source_ref_template":"ci/oreslang-{source_tree_sha}",
                "authorization_ref_template":"ci/authorized-{carrier_commit_sha}",
                "control_ref_template":"ci/oreslang-control-{carrier_commit_sha}",
                "receipt_schema":"ores-private-ci-receipt-v4","expires_at":"2026-10-27T00:00:00Z"}
        });
        let manifest_raw=serde_json::to_vec(&manifest).unwrap();
        let mut routes=HashMap::new();
        routes.insert(format!("repos/{CARRIER_REPO}"),json!({"id":CARRIER_REPO_ID,"full_name":CARRIER_REPO,"private":true,"visibility":"private","archived":false,"disabled":false,"fork":false}));
        routes.insert(format!("repos/{CARRIER_REPO}/git/commits/{HEAD}"),json!({"sha":HEAD,"parents":[{"sha":BASE}]}));
        routes.insert(format!("repos/{CARRIER_REPO}/git/ref/heads/ci/oreslang-{TREE}"),json!({"ref":format!("refs/heads/ci/oreslang-{TREE}"),"object":{"type":"commit","sha":HEAD}}));
        routes.insert(format!("repos/{CARRIER_REPO}/git/ref/heads/ci/authorized-{HEAD}"),json!({"ref":format!("refs/heads/ci/authorized-{HEAD}"),"object":{"type":"commit","sha":HEAD}}));
        routes.insert(format!("repos/{ANCHOR_REPO}"),json!({"id":ANCHOR_REPO_ID,"full_name":ANCHOR_REPO,"private":false,"visibility":"public","default_branch":"main","archived":false,"disabled":false,"fork":false}));
        routes.insert(format!("repos/{ANCHOR_REPO}/branches/main"),json!({"name":"main","protected":true,"commit":{"sha":ANCHOR_SHA}}));
        routes.insert(format!("repos/{ANCHOR_REPO}/rules/branches/main"),strict_rules());
        routes.insert(format!("repos/{ANCHOR_REPO}/pulls/{auth_pr}"),json!({
            "number":auth_pr,"state":"closed","draft":false,"merged_at":"2026-10-21T01:00:00Z","merge_commit_sha":ANCHOR_SHA,
            "user":{"id":100},"head":{"sha":ANCHOR_PR_HEAD,"repo":{"full_name":ANCHOR_REPO}},"base":{"ref":"main"}
        }));
        routes.insert(format!("repos/{ANCHOR_REPO}/pulls/{auth_pr}/reviews?per_page={MAX_REVIEWS}"),json!([
            {"id":200,"state":"APPROVED","commit_id":ANCHOR_PR_HEAD,"submitted_at":"2026-10-21T00:59:00Z","user":{"id":SECURITY_REVIEWER_ID}}
        ]));
        routes.insert(format!("repos/{ANCHOR_REPO}/pulls/{auth_pr}/files?per_page=100"),json!([
            {"filename":manifest_path,"status":"added","sha":git_blob_sha(&manifest_raw)}
        ]));
        routes.insert(format!("repos/{ANCHOR_REPO}/contents/{manifest_path}?ref={ANCHOR_SHA}"),item(&manifest_path,&manifest_raw));
        routes.insert(format!("repos/{CARRIER_REPO}/contents/.github/workflows/oreslang-exact-tree-carrier.yml?ref={BASE}"),
            json!({"path":".github/workflows/oreslang-exact-tree-carrier.yml","type":"file","sha":WORKFLOW_BLOB}));
        routes.insert(format!("repos/{CARRIER_REPO}/contents/{plan_path}?ref={BASE}"),item(&plan_path,&plan));
        let f=Self{_dir:dir,root,routes,manifest,plan,source_pr,auth_pr};
        f.write_receipt(None);
        f
    }
    fn manifest_path(&self)->String{format!("ci/private-ci/authorizations/java-pr{}.json",self.source_pr)}
    fn plan_path(&self)->String{format!("ci/test-plans/java-pr{}.json",self.source_pr)}
    fn manifest_raw(&self)->Vec<u8>{serde_json::to_vec(&self.manifest).unwrap()}
    fn refresh_manifest(&mut self){
        let raw=self.manifest_raw();
        let path=self.manifest_path();
        self.routes.insert(format!("repos/{ANCHOR_REPO}/contents/{path}?ref={ANCHOR_SHA}"),item(&path,&raw));
        self.routes.insert(format!("repos/{ANCHOR_REPO}/pulls/{}/files?per_page=100",self.auth_pr),
            json!([{"filename":path,"status":"modified","sha":git_blob_sha(&raw)}]));
    }
    fn write_receipt(&self,merge_override:Option<&str>){
        let raw=self.manifest_raw();
        let receipt=json!({
            "schema":"ores-private-ci-receipt-v4","expires_at":"2026-10-22T00:00:00Z",
            "authorization":{"repository":ANCHOR_REPO,"repository_id":ANCHOR_REPO_ID,"branch":"main","pr":self.auth_pr,
                "merge_sha":merge_override.unwrap_or(ANCHOR_SHA),"manifest_path":self.manifest_path(),
                "manifest_blob_sha":git_blob_sha(&raw),"manifest_sha256":sha(&raw)},
            "carrier":{"workflow_blob_sha":WORKFLOW_BLOB},
            "destination":{"repo":CARRIER_REPO,"repo_id":CARRIER_REPO_ID,"base_sha":BASE},
            "mirror":{"root_tree_sha":TREE},
            "source":{"repo":SOURCE_REPO,"repo_id":SOURCE_REPO_ID,"pr":self.source_pr,"head_sha":SOURCE_HEAD,"root_tree_sha":TREE,"manifest_sha256":"3".repeat(64)},
            "test_plan":{"path":self.plan_path(),"sha256":sha(&self.plan)}
        });
        fs::write(self.root.join("receipt.json"),serde_json::to_vec(&receipt).unwrap()).unwrap();
    }
    fn run(&self)->Result<Value,String>{
        let _g=ENV_LOCK.lock().unwrap();
        std::env::remove_var("ORESLANG_CARRIER_HEAD");
        std::env::remove_var("ORESLANG_SOURCE_REF");
        std::env::set_var("GITHUB_REPOSITORY",CARRIER_REPO);
        std::env::set_var("GITHUB_REPOSITORY_ID",CARRIER_REPO_ID.to_string());
        std::env::set_var("GITHUB_SHA",HEAD);
        std::env::set_var("GITHUB_REF_NAME",format!("ci/oreslang-{TREE}"));
        let routes=self.routes.clone();
        verify_with_api(&self.root.join("receipt.json"),
            move|r|routes.get(r).cloned().ok_or_else(||Denied(format!("unexpected route {r}"))),
            Utc.with_ymd_and_hms(2026,10,21,0,0,0).unwrap()).map_err(|e|e.to_string())
    }
}

#[test]
fn exact_reviewed_anchor_passes(){
    let f=Fixture::new();
    let r=f.run().unwrap();
    assert_eq!(r["carrier_base"],BASE);
    assert_eq!(r["source_pr"],1668);
    assert_eq!(r["authorization_pr"],52);
}
#[test]
fn arbitrary_source_and_authorization_prs_pass_when_receipt_and_manifest_bind(){
    let f=Fixture::new_for(1711,53);
    let r=f.run().unwrap();
    assert_eq!(r["source_pr"],1711);
    assert_eq!(r["authorization_pr"],53);
}
#[test]
fn authorization_pr_must_review_exact_manifest_blob(){
    let mut f=Fixture::new();
    f.routes.insert(format!("repos/{ANCHOR_REPO}/pulls/52/files?per_page=100"),
        json!([{"filename":"README.md","status":"modified","sha":"7".repeat(40)}]));
    assert!(f.run().unwrap_err().contains("exact manifest blob"));
}
#[test]
fn protected_anchor_rules_are_strict(){
    let mut f=Fixture::new();
    f.routes.get_mut(&format!("repos/{ANCHOR_REPO}/rules/branches/main")).unwrap()[0]["parameters"]["dismiss_stale_reviews_on_push"]=json!(false);
    assert!(f.run().unwrap_err().contains("pull-request rule"));
    let mut f=Fixture::new();
    let key=format!("repos/{ANCHOR_REPO}/rules/branches/main");
    f.routes.get_mut(&key).unwrap().as_array_mut().unwrap().retain(|v|v["type"]!="non_fast_forward");
    assert!(f.run().unwrap_err().contains("force-push protection"));
}
#[test]
fn only_security_reviewer_exact_head_approval_authorizes(){
    let mut f=Fixture::new();
    f.routes.insert(format!("repos/{ANCHOR_REPO}/pulls/52/reviews?per_page={MAX_REVIEWS}"),json!([
        {"id":200,"state":"APPROVED","commit_id":ANCHOR_PR_HEAD,"submitted_at":"x","user":{"id":99999999}}
    ]));
    assert!(f.run().unwrap_err().contains("security-reviewer"));
    let mut f=Fixture::new();
    f.routes.insert(format!("repos/{ANCHOR_REPO}/pulls/52/reviews?per_page={MAX_REVIEWS}"),json!([
        {"id":200,"state":"APPROVED","commit_id":ANCHOR_PR_HEAD,"submitted_at":"x","user":{"id":SECURITY_REVIEWER_ID}},
        {"id":202,"state":"CHANGES_REQUESTED","commit_id":ANCHOR_PR_HEAD,"submitted_at":"x","user":{"id":SECURITY_REVIEWER_ID}}
    ]));
    assert!(f.run().unwrap_err().contains("security-reviewer"));
}
#[test]
fn receipt_manifest_path_must_follow_source_pr(){
    let f=Fixture::new_for(1711,53);
    let mut receipt:Value=serde_json::from_slice(&fs::read(f.root.join("receipt.json")).unwrap()).unwrap();
    receipt["authorization"]["manifest_path"]=json!("ci/private-ci/authorizations/java-pr1668.json");
    fs::write(f.root.join("receipt.json"),serde_json::to_vec(&receipt).unwrap()).unwrap();
    assert!(f.run().unwrap_err().contains("manifest path/source PR mismatch"));
}
#[test]
fn carrier_base_and_receipt_anchor_bindings_are_exact(){
    let mut f=Fixture::new();
    f.manifest["destination"]["base_sha"]=json!("7".repeat(40));
    f.refresh_manifest();
    assert!(f.run().unwrap_err().contains("base/identity"));
    let f=Fixture::new();
    f.write_receipt(Some(&"6".repeat(40)));
    assert!(f.run().unwrap_err().contains("exact merged anchor tip")||f.run().unwrap_err().contains("authorization binding"));
}
#[test]
fn control_dispatch_environment_overrides_event_context(){
    let f=Fixture::new();
    let _g=ENV_LOCK.lock().unwrap();
    std::env::set_var("GITHUB_REPOSITORY",CARRIER_REPO);
    std::env::set_var("GITHUB_REPOSITORY_ID",CARRIER_REPO_ID.to_string());
    std::env::set_var("GITHUB_SHA","9".repeat(40));
    std::env::set_var("GITHUB_REF_NAME",format!("ci/oreslang-control-{HEAD}"));
    std::env::set_var("ORESLANG_CARRIER_HEAD",HEAD);
    std::env::set_var("ORESLANG_SOURCE_REF",format!("ci/oreslang-{TREE}"));
    std::env::set_var("ORESLANG_AUTHORIZATION_REF",format!("ci/authorized-{HEAD}"));
    std::env::set_var("ORESLANG_CONTROL_PARENT",BASE);
    let routes=f.routes.clone();
    let r=verify_with_api(&f.root.join("receipt.json"),
        move|route|routes.get(route).cloned().ok_or_else(||Denied(format!("unexpected route {route}"))),
        Utc.with_ymd_and_hms(2026,10,21,0,0,0).unwrap()).unwrap();
    std::env::remove_var("ORESLANG_CARRIER_HEAD");
    std::env::remove_var("ORESLANG_SOURCE_REF");
    std::env::remove_var("ORESLANG_AUTHORIZATION_REF");
    std::env::remove_var("ORESLANG_CONTROL_PARENT");
    assert_eq!(r["carrier_head"],HEAD);
}

#[test]
fn partial_or_mismatched_control_context_fails_closed(){
    let f=Fixture::new();
    let _g=ENV_LOCK.lock().unwrap();
    std::env::set_var("GITHUB_REPOSITORY",CARRIER_REPO);
    std::env::set_var("GITHUB_REPOSITORY_ID",CARRIER_REPO_ID.to_string());
    std::env::set_var("GITHUB_SHA","9".repeat(40));
    std::env::set_var("GITHUB_REF_NAME",format!("ci/oreslang-control-{HEAD}"));
    std::env::set_var("ORESLANG_CARRIER_HEAD",HEAD);
    let routes=f.routes.clone();
    let e=verify_with_api(&f.root.join("receipt.json"),
        move|route|routes.get(route).cloned().ok_or_else(||Denied(format!("unexpected route {route}"))),
        Utc.with_ymd_and_hms(2026,10,21,0,0,0).unwrap()).unwrap_err();
    assert!(e.to_string().contains("control dispatch context incomplete"));
    std::env::set_var("ORESLANG_SOURCE_REF",format!("ci/oreslang-{TREE}"));
    std::env::set_var("ORESLANG_AUTHORIZATION_REF",format!("ci/authorized-{}", "8".repeat(40)));
    std::env::set_var("ORESLANG_CONTROL_PARENT",BASE);
    let routes=f.routes.clone();
    let e=verify_with_api(&f.root.join("receipt.json"),
        move|route|routes.get(route).cloned().ok_or_else(||Denied(format!("unexpected route {route}"))),
        Utc.with_ymd_and_hms(2026,10,21,0,0,0).unwrap()).unwrap_err();
    assert!(e.to_string().contains("authorization ref mismatch"));
    std::env::set_var("ORESLANG_AUTHORIZATION_REF",format!("ci/authorized-{HEAD}"));
    std::env::set_var("ORESLANG_CONTROL_PARENT","7".repeat(40));
    let routes=f.routes.clone();
    let e=verify_with_api(&f.root.join("receipt.json"),
        move|route|routes.get(route).cloned().ok_or_else(||Denied(format!("unexpected route {route}"))),
        Utc.with_ymd_and_hms(2026,10,21,0,0,0).unwrap()).unwrap_err();
    assert!(e.to_string().contains("carrier/control parent mismatch"));
    for key in ["ORESLANG_CARRIER_HEAD","ORESLANG_SOURCE_REF","ORESLANG_AUTHORIZATION_REF","ORESLANG_CONTROL_PARENT"]{
        std::env::remove_var(key);
    }
}
#[test]
fn anchor_move_during_validation_fails_closed(){
    let f=Fixture::new();
    let _g=ENV_LOCK.lock().unwrap();
    std::env::set_var("GITHUB_REPOSITORY",CARRIER_REPO);
    std::env::set_var("GITHUB_REPOSITORY_ID",CARRIER_REPO_ID.to_string());
    std::env::set_var("GITHUB_SHA",HEAD);
    std::env::set_var("GITHUB_REF_NAME",format!("ci/oreslang-{TREE}"));
    let routes=f.routes.clone();
    let key=format!("repos/{ANCHOR_REPO}/branches/main");
    let mut count=0;
    let result=verify_with_api(&f.root.join("receipt.json"),move|route|{
        let mut v=routes.get(route).cloned().ok_or_else(||Denied("unexpected route".into()))?;
        if route==key{count+=1;if count==2{v["commit"]["sha"]=json!("5".repeat(40));}}
        Ok(v)
    },Utc.with_ymd_and_hms(2026,10,21,0,0,0).unwrap());
    assert!(result.unwrap_err().to_string().contains("moved during validation"));
}
