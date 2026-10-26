use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{TimeZone,Utc};
use orestra_conformance::external_anchor::*;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path,PathBuf};
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
fn item(path:&str,raw:&[u8])->Value{json!({"path":path,"type":"file","encoding":"base64","content":STANDARD.encode(raw),"sha":git_blob_sha(raw)})}
struct Fixture{_dir:tempfile::TempDir,root:PathBuf,routes:HashMap<String,Value>,manifest:Value,plan:Vec<u8>}
impl Fixture{
 fn new()->Self{
  let dir=tempdir().unwrap();let root=dir.path().to_path_buf();let plan=b"{\"schema\":\"ores-private-ci-test-plan-v1\"}\n".to_vec();
  let manifest=json!({"schema":MANIFEST_SCHEMA,"anchor_repository_id":ANCHOR_REPO_ID,
   "source":{"repository":SOURCE_REPO,"repository_id":SOURCE_REPO_ID,"base_branch":"main","pr":SOURCE_PR,"head_sha":SOURCE_HEAD,"tree_sha":TREE},
   "destination":{"repository":CARRIER_REPO,"repository_id":CARRIER_REPO_ID,"base_sha":BASE,"workflow_path":".github/workflows/oreslang-exact-tree-carrier.yml","workflow_blob_sha":WORKFLOW_BLOB,"test_plan_path":"ci/test-plans/java-pr1668.json","test_plan_blob_sha":git_blob_sha(&plan)},
   "constraints":{"source_must_be_private":true,"destination_must_be_private":true,"source_tree_prefix":"private-oreslang-source","source_ref_template":"ci/oreslang-{source_tree_sha}","authorization_ref_template":"ci/authorized-{carrier_commit_sha}","receipt_schema":"ores-private-ci-receipt-v4","expires_at":"2026-10-27T00:00:00Z"}});
  let manifest_raw=serde_json::to_vec(&manifest).unwrap();let mut routes=HashMap::new();
  routes.insert(format!("repos/{CARRIER_REPO}"),json!({"id":CARRIER_REPO_ID,"full_name":CARRIER_REPO,"private":true,"visibility":"private","archived":false,"disabled":false,"fork":false}));
  routes.insert(format!("repos/{CARRIER_REPO}/git/commits/{HEAD}"),json!({"sha":HEAD,"parents":[{"sha":BASE}]}));
  routes.insert(format!("repos/{CARRIER_REPO}/git/ref/heads/ci/authorized-{HEAD}"),json!({"ref":format!("refs/heads/ci/authorized-{HEAD}"),"object":{"type":"commit","sha":HEAD}}));
  routes.insert(format!("repos/{ANCHOR_REPO}"),json!({"id":ANCHOR_REPO_ID,"full_name":ANCHOR_REPO,"private":false,"visibility":"public","default_branch":"main","archived":false,"disabled":false,"fork":false}));
  routes.insert(format!("repos/{ANCHOR_REPO}/branches/main"),json!({"name":"main","protected":true,"commit":{"sha":ANCHOR_SHA}}));
  routes.insert(format!("repos/{ANCHOR_REPO}/pulls/{AUTHORIZATION_PR}"),json!({"number":AUTHORIZATION_PR,"state":"closed","draft":false,"merged_at":"2026-10-21T01:00:00Z","merge_commit_sha":ANCHOR_SHA,"user":{"id":100},"head":{"sha":ANCHOR_PR_HEAD,"repo":{"full_name":ANCHOR_REPO}},"base":{"ref":"main"}}));
  routes.insert(format!("repos/{ANCHOR_REPO}/pulls/{AUTHORIZATION_PR}/reviews?per_page={MAX_REVIEWS}"),json!([{"id":200,"state":"APPROVED","commit_id":ANCHOR_PR_HEAD,"submitted_at":"2026-10-21T00:59:00Z","user":{"id":101}}]));
  routes.insert(format!("repos/{ANCHOR_REPO}/contents/{MANIFEST_PATH}?ref={ANCHOR_SHA}"),item(MANIFEST_PATH,&manifest_raw));
  routes.insert(format!("repos/{CARRIER_REPO}/contents/.github/workflows/oreslang-exact-tree-carrier.yml?ref={BASE}"),json!({"path":".github/workflows/oreslang-exact-tree-carrier.yml","type":"file","sha":WORKFLOW_BLOB}));
  routes.insert(format!("repos/{CARRIER_REPO}/contents/ci/test-plans/java-pr1668.json?ref={BASE}"),item("ci/test-plans/java-pr1668.json",&plan));
  let f=Self{_dir:dir,root,routes,manifest,plan};f.write_receipt(None);f
 }
 fn manifest_raw(&self)->Vec<u8>{serde_json::to_vec(&self.manifest).unwrap()}
 fn refresh_manifest_route(&mut self){let raw=self.manifest_raw();self.routes.insert(format!("repos/{ANCHOR_REPO}/contents/{MANIFEST_PATH}?ref={ANCHOR_SHA}"),item(MANIFEST_PATH,&raw));}
 fn write_receipt(&self,merge_override:Option<&str>){let raw=self.manifest_raw();let receipt=json!({"schema":"ores-private-ci-receipt-v4","expires_at":"2026-10-22T00:00:00Z",
  "authorization":{"repository":ANCHOR_REPO,"repository_id":ANCHOR_REPO_ID,"branch":"main","pr":AUTHORIZATION_PR,"merge_sha":merge_override.unwrap_or(ANCHOR_SHA),"manifest_path":MANIFEST_PATH,"manifest_blob_sha":git_blob_sha(&raw),"manifest_sha256":sha(&raw)},
  "carrier":{"workflow_blob_sha":WORKFLOW_BLOB},"destination":{"repo":CARRIER_REPO,"repo_id":CARRIER_REPO_ID,"base_sha":BASE},"mirror":{"root_tree_sha":TREE},
  "source":{"repo":SOURCE_REPO,"repo_id":SOURCE_REPO_ID,"pr":SOURCE_PR,"head_sha":SOURCE_HEAD,"root_tree_sha":TREE,"manifest_sha256":"3".repeat(64)},
  "test_plan":{"path":"ci/test-plans/java-pr1668.json","sha256":sha(&self.plan)}});
  fs::write(self.root.join("receipt.json"),serde_json::to_vec(&receipt).unwrap()).unwrap();}
 fn run(&self)->Result<Value,String>{let _g=ENV_LOCK.lock().unwrap();std::env::set_var("GITHUB_REPOSITORY",CARRIER_REPO);std::env::set_var("GITHUB_REPOSITORY_ID",CARRIER_REPO_ID.to_string());std::env::set_var("GITHUB_SHA",HEAD);std::env::set_var("GITHUB_REF_NAME",format!("ci/oreslang-{TREE}"));
  let routes=self.routes.clone();verify_with_api(&self.root.join("receipt.json"),move|r|routes.get(r).cloned().ok_or_else(||Denied(format!("unexpected route {r}"))),Utc.with_ymd_and_hms(2026,10,21,0,0,0).unwrap()).map_err(|e|e.to_string())}
}
#[test]
fn exact_reviewed_anchor_passes(){let f=Fixture::new();let r=f.run().unwrap();assert_eq!(r["carrier_base"],BASE);assert_eq!(r["source_tree"],TREE);assert_eq!(r["anchor_sha"],ANCHOR_SHA);}
#[test]
fn unprotected_or_non_tip_anchor_fails(){let mut f=Fixture::new();f.routes.get_mut(&format!("repos/{ANCHOR_REPO}/branches/main")).unwrap()["protected"]=json!(false);assert!(f.run().unwrap_err().contains("not protected"));
 let mut f=Fixture::new();f.routes.get_mut(&format!("repos/{ANCHOR_REPO}/pulls/{AUTHORIZATION_PR}")).unwrap()["merge_commit_sha"]=json!("9".repeat(40));assert!(f.run().unwrap_err().contains("exact merged anchor tip"));}
#[test]
fn stale_and_latest_changes_requested_reviews_do_not_authorize(){let mut f=Fixture::new();let key=format!("repos/{ANCHOR_REPO}/pulls/{AUTHORIZATION_PR}/reviews?per_page={MAX_REVIEWS}");
 f.routes.insert(key.clone(),json!([{"id":201,"state":"APPROVED","commit_id":"8".repeat(40),"submitted_at":"x","user":{"id":101}}]));assert!(f.run().unwrap_err().contains("independent exact-head approval"));
 let mut f=Fixture::new();f.routes.insert(key,json!([{"id":200,"state":"APPROVED","commit_id":ANCHOR_PR_HEAD,"submitted_at":"x","user":{"id":101}},{"id":202,"state":"CHANGES_REQUESTED","commit_id":ANCHOR_PR_HEAD,"submitted_at":"x","user":{"id":101}}]));assert!(f.run().unwrap_err().contains("independent exact-head approval"));}
#[test]
fn carrier_base_and_receipt_anchor_bindings_are_exact(){let mut f=Fixture::new();f.manifest["destination"]["base_sha"]=json!("7".repeat(40));f.refresh_manifest_route();assert!(f.run().unwrap_err().contains("base/identity"));
 let f=Fixture::new();f.write_receipt(Some(&"6".repeat(40)));assert!(f.run().unwrap_err().contains("receipt authorization binding"));}
#[test]
fn anchor_move_during_validation_fails_closed(){let f=Fixture::new();let _g=ENV_LOCK.lock().unwrap();std::env::set_var("GITHUB_REPOSITORY",CARRIER_REPO);std::env::set_var("GITHUB_REPOSITORY_ID",CARRIER_REPO_ID.to_string());std::env::set_var("GITHUB_SHA",HEAD);std::env::set_var("GITHUB_REF_NAME",format!("ci/oreslang-{TREE}"));
 let routes=f.routes.clone();let branch_key=format!("repos/{ANCHOR_REPO}/branches/main");let mut count=0;let result=verify_with_api(&f.root.join("receipt.json"),move|r|{let mut v=routes.get(r).cloned().ok_or_else(||Denied("unexpected route".into()))?;if r==branch_key{count+=1;if count==2{v["commit"]["sha"]=json!("5".repeat(40));}}Ok(v)},Utc.with_ymd_and_hms(2026,10,21,0,0,0).unwrap());assert!(result.unwrap_err().to_string().contains("moved during validation"));}
