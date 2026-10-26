use chrono::{TimeZone,Utc};
use orestra_conformance::private_carrier::{canonical_digest,parse_expiry,tree_manifest,verify};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path,PathBuf};
use std::process::Command;
use std::sync::Mutex;
use tempfile::tempdir;
static CWD:Mutex<()>=Mutex::new(());

fn cmd(root:&Path,args:&[&str])->String{let out=Command::new("git").args(args).current_dir(root).output().unwrap();assert!(out.status.success(),"{:?}",String::from_utf8_lossy(&out.stderr));String::from_utf8_lossy(&out.stdout).trim().to_string()}
fn with_cwd<T>(root:&Path,f:impl FnOnce()->T)->T{let _g=CWD.lock().unwrap();let old=std::env::current_dir().unwrap();std::env::set_current_dir(root).unwrap();let v=f();std::env::set_current_dir(old).unwrap();v}
fn sha(data:&[u8])->String{format!("{:x}",Sha256::digest(data))}
struct Repo{_dir:tempfile::TempDir,root:PathBuf,head:String}
impl Repo{
 fn make(extra:Option<(&str,&str)>,mutate_workflow:bool,schema:&str)->Self{
  let dir=tempdir().unwrap();let root=dir.path().to_path_buf();cmd(&root,&["init","-q"]);cmd(&root,&["config","user.name","Synthetic CI"]);cmd(&root,&["config","user.email","ci@example.invalid"]);
  fs::create_dir_all(root.join(".github/workflows")).unwrap();fs::write(root.join(".github/workflows/oreslang-exact-tree-carrier.yml"),"name: synthetic carrier\n").unwrap();
  fs::write(root.join("README.md"),"carrier base\n").unwrap();fs::create_dir_all(root.join("ci/test-plans")).unwrap();
  let plan=json!({"schema":"ores-private-ci-test-plan-v1","source_repo":"synthetic/private","allowed_prs":[7],"commands":[{"argv":["mvn","--version"],"cwd":".","timeout_seconds":30}]});
  let plan_bytes=serde_json::to_vec(&plan).unwrap();fs::write(root.join("ci/test-plans/synthetic.json"),&plan_bytes).unwrap();let plan_sha=sha(&plan_bytes);
  cmd(&root,&["add","-A"]);cmd(&root,&["commit","-qm","base"]);let base=cmd(&root,&["rev-parse","HEAD"]);let workflow_blob=cmd(&root,&["rev-parse","HEAD:.github/workflows/oreslang-exact-tree-carrier.yml"]);
  fs::create_dir_all(root.join("private-oreslang-source/stdlib")).unwrap();fs::write(root.join("private-oreslang-source/core.txt"),"synthetic-only\n").unwrap();
  let runner=root.join("private-oreslang-source/stdlib/runner.sh");fs::write(&runner,"#!/bin/sh\nexit 0\n").unwrap();fs::set_permissions(&runner,fs::Permissions::from_mode(0o755)).unwrap();
  cmd(&root,&["add","private-oreslang-source"]);let source_tree=cmd(&root,&["write-tree"]);let subtree=cmd(&root,&["rev-parse",&format!("{source_tree}:private-oreslang-source")]);
  let manifest=with_cwd(&root,||tree_manifest(&subtree).unwrap().2);
  if mutate_workflow{fs::write(root.join(".github/workflows/oreslang-exact-tree-carrier.yml"),"name: mutated\n").unwrap();}
  if let Some((p,c))=extra{let target=root.join(p);if let Some(parent)=target.parent(){fs::create_dir_all(parent).unwrap();}fs::write(target,c).unwrap();}
  let mut receipt=json!({"schema":schema,"expires_at":"2099-01-02T00:00:00Z","carrier":{"workflow_blob_sha":workflow_blob},
   "destination":{"repo":"synthetic/carrier","repo_id":42,"base_sha":base},"mirror":{"root_tree_sha":subtree},
   "source":{"repo":"synthetic/private","repo_id":1,"pr":7,"head_sha":"1".repeat(40),"root_tree_sha":subtree,"manifest_sha256":manifest},
   "test_plan":{"path":"ci/test-plans/synthetic.json","sha256":plan_sha}});
  if schema=="ores-private-ci-receipt-v4"{receipt["authorization"]=json!({"repository":"ores-truffle-oreslang/ores-truffle-oreslang.github.io","repository_id":1409246164u64,"branch":"main","pr":52,"merge_sha":"4".repeat(40),"manifest_path":"ci/private-ci/authorizations/java-pr1668.json","manifest_blob_sha":"5".repeat(40),"manifest_sha256":"6".repeat(64)});}
  fs::write(root.join("ci/oreslang-receipt.json"),serde_json::to_vec(&receipt).unwrap()).unwrap();cmd(&root,&["add","-A"]);let digest=canonical_digest(&receipt);
  cmd(&root,&["commit","-qm",&format!("synthetic mirror\n\nOreslang-CI-Receipt-SHA256: {digest}")]);let head=cmd(&root,&["rev-parse","HEAD"]);Self{_dir:dir,root,head}
 }
 fn verify(&self)->Result<Value,String>{let tree=cmd(&self.root,&["rev-parse","HEAD:private-oreslang-source"]);with_cwd(&self.root,||verify(Path::new("ci/oreslang-receipt.json"),"private-oreslang-source",".github/workflows/oreslang-exact-tree-carrier.yml","synthetic/carrier",42,&self.head,&format!("ci/oreslang-{tree}"),Utc.with_ymd_and_hms(2099,1,1,0,0,0).unwrap()).map_err(|e|e.to_string()))}
}
#[test]
fn expiry_bounds(){let now=Utc.with_ymd_and_hms(2026,10,20,0,0,0).unwrap();assert!(parse_expiry("2026-10-21T00:00:00Z",now).is_ok());assert!(parse_expiry("2026-10-19T23:59:59Z",now).is_err());assert!(parse_expiry("2026-10-28T00:00:01Z",now).is_err());}
#[test]
fn complete_history_and_v4_pass(){for schema in ["ores-private-ci-receipt-v3","ores-private-ci-receipt-v4"]{let r=Repo::make(None,false,schema);let out=r.verify().unwrap();assert_eq!(out["source_repo"],"synthetic/private");assert_eq!(out["file_count"],2);assert_eq!(out["receipt_schema"],schema);}}
#[test]
fn unexpected_destination_and_workflow_mutation_fail(){let r=Repo::make(Some(("outside.txt","bad\n")),false,"ores-private-ci-receipt-v3");assert!(r.verify().unwrap_err().contains("unexpected destination change"));let r=Repo::make(None,true,"ores-private-ci-receipt-v3");let e=r.verify().unwrap_err();assert!(e.contains("unexpected destination change")||e.contains("workflow changed"));}
#[test]
fn symlink_git_entry_is_rejected(){let dir=tempdir().unwrap();let root=dir.path();cmd(root,&["init","-q"]);fs::write(root.join("target"),"x").unwrap();std::os::unix::fs::symlink("target",root.join("link")).unwrap();cmd(root,&["add","-A"]);let tree=cmd(root,&["write-tree"]);let e=with_cwd(root,||tree_manifest(&tree).unwrap_err().to_string());assert!(e.contains("unsupported Git entry"));}
