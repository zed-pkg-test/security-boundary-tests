use chrono::{DateTime,Duration,Utc};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::fs;
use std::path::{Component,Path};
use std::process::Command;

pub const MAX_FILES:usize=1024;
pub const MAX_TOTAL:u64=40*1024*1024;
pub const MAX_FILE:u64=10*1024*1024;

#[derive(Debug,Clone)]
pub struct Denied(pub String);
impl std::fmt::Display for Denied{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{write!(f,"{}",self.0)}}
impl std::error::Error for Denied{}
fn deny(s:impl Into<String>)->Denied{Denied(s.into())}

pub fn is_hex(value:&str,len:usize)->bool{value.len()==len&&value.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())}
pub fn require_hex(value:Option<&str>,len:usize,label:&str)->Result<String,Denied>{
    let value=value.ok_or_else(||deny(label))?;if !is_hex(value,len){return Err(deny(label));}Ok(value.into())
}
fn git(args:&[&str])->Result<String,Denied>{
    let out=Command::new("git").args(args).output().map_err(|_|deny("required Git metadata unavailable"))?;
    if !out.status.success(){return Err(deny("required Git metadata unavailable"));}
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
pub fn canonical_digest(value:&Value)->String{
    let raw=serde_json::to_vec(value).expect("JSON serializable");format!("{:x}",Sha256::digest(raw))
}
pub fn parse_expiry(value:&str,now:DateTime<Utc>)->Result<DateTime<Utc>,Denied>{
    if !value.ends_with('Z'){return Err(deny("receipt expiry"));}
    let expiry=DateTime::parse_from_rfc3339(value).map_err(|_|deny("receipt expiry"))?.with_timezone(&Utc);
    if expiry<=now{return Err(deny("receipt expired"));}
    if expiry-now>Duration::days(7){return Err(deny("receipt retention window exceeds seven days"));}
    Ok(expiry)
}
fn safe_path(path:&str)->bool{
    !path.starts_with('/')&&!path.contains('\\')&&!Path::new(path).components().any(|c|matches!(c,Component::ParentDir)||matches!(c,Component::Normal(x) if x==".git"))
}
pub fn tree_manifest(tree_sha:&str)->Result<(Vec<Value>,u64,String),Denied>{
    require_hex(Some(tree_sha),40,"invalid source tree SHA")?;
    let out=Command::new("git").args(["ls-tree","-r","-z",tree_sha]).output().map_err(|_|deny("source tree unavailable"))?;
    if !out.status.success(){return Err(deny("source tree unavailable"));}
    let mut rows=Vec::<Value>::new();let mut total=0u64;
    for entry in out.stdout.split(|b|*b==0).filter(|x|!x.is_empty()){
        let tab=entry.iter().position(|b|*b==b'\t').ok_or_else(||deny("malformed tree entry"))?;
        let meta=std::str::from_utf8(&entry[..tab]).map_err(|_|deny("malformed tree entry"))?;
        let path=std::str::from_utf8(&entry[tab+1..]).map_err(|_|deny("malformed tree entry"))?;
        let parts:Vec<_>=meta.split(' ').collect();if parts.len()!=3{return Err(deny("malformed tree entry"));}
        let (mode,kind,sha)=(parts[0],parts[1],parts[2]);
        if kind!="blob"||!matches!(mode,"100644"|"100755"){return Err(deny(format!("unsupported Git entry: {path}")));}
        if !safe_path(path){return Err(deny(format!("unsafe source path: {path}")));}
        require_hex(Some(sha),40,"invalid blob SHA")?;
        let size=git(&["cat-file","-s",sha])?.parse::<u64>().map_err(|_|deny("invalid blob size"))?;
        if size>MAX_FILE{return Err(deny("per-file size bound"));}total=total.checked_add(size).ok_or_else(||deny("aggregate size bound"))?;
        if total>MAX_TOTAL{return Err(deny("aggregate size bound"));}
        rows.push(json!([path,mode,sha,size]));if rows.len()>MAX_FILES{return Err(deny("file-count bound"));}
    }
    if rows.is_empty(){return Err(deny("empty source tree"));}
    rows.sort_by(|a,b|a[0].as_str().cmp(&b[0].as_str()));
    let value=Value::Array(rows.clone());let digest=canonical_digest(&value);
    Ok((rows,total,digest))
}
fn safe_plan_path(path:&str)->bool{path.starts_with("ci/test-plans/")&&path.ends_with(".json")&&safe_path(path)}

pub fn verify(receipt_path:&Path,source_dir:&str,workflow_path:&str,repository:&str,repository_id:u64,head_sha:&str,ref_name:&str,now:DateTime<Utc>)->Result<Value,Denied>{
    if !ref_name.starts_with("ci/oreslang-"){return Err(deny("unexpected carrier branch"));}
    if !repository.contains('/'){return Err(deny("destination repository context"));}
    if receipt_path.is_symlink()||!receipt_path.is_file(){return Err(deny("receipt missing or unsafe"));}
    let raw=fs::read(receipt_path).map_err(|_|deny("invalid receipt"))?;
    let receipt:Value=serde_json::from_slice(&raw).map_err(|_|deny("invalid receipt"))?;
    let schema=receipt["schema"].as_str().ok_or_else(||deny("receipt schema"))?;
    if schema!="ores-private-ci-receipt-v3"&&schema!="ores-private-ci-receipt-v4"{return Err(deny("receipt schema"));}
    if schema=="ores-private-ci-receipt-v4"{
        let auth=receipt["authorization"].as_object().ok_or_else(||deny("v4 authorization receipt"))?;
        let keys=["repository","repository_id","branch","pr","merge_sha","manifest_path","manifest_blob_sha","manifest_sha256"];
        if auth.len()!=keys.len()||keys.iter().any(|k|!auth.contains_key(*k))
            ||auth["repository"]!="ores-truffle-oreslang/ores-truffle-oreslang.github.io"
            ||auth["repository_id"]!=1409246164u64||auth["branch"]!="main"||auth["pr"]!=52
            ||auth["manifest_path"]!="ci/private-ci/authorizations/java-pr1668.json"{return Err(deny("v4 authorization receipt"));}
        require_hex(auth["merge_sha"].as_str(),40,"v4 authorization merge SHA")?;
        require_hex(auth["manifest_blob_sha"].as_str(),40,"v4 authorization manifest blob")?;
        require_hex(auth["manifest_sha256"].as_str(),64,"v4 authorization manifest digest")?;
    }
    parse_expiry(receipt["expires_at"].as_str().ok_or_else(||deny("receipt expiry"))?,now)?;
    let actual=require_hex(Some(&git(&["rev-parse","HEAD"])?),40,"invalid carrier head")?;
    if actual!=head_sha{return Err(deny("head/context mismatch"));}
    let parent=require_hex(Some(&git(&["rev-parse","HEAD^"])?),40,"invalid carrier parent")?;
    let dest=receipt["destination"].as_object().ok_or_else(||deny("destination receipt"))?;
    if dest["repo"]!=repository||dest["repo_id"]!=repository_id{return Err(deny("destination identity"));}
    if dest["base_sha"]!=parent{return Err(deny("destination base commit"));}
    let diff=Command::new("git").args(["diff","--name-only","-z",&parent,&actual]).output().map_err(|_|deny("required Git metadata unavailable"))?;
    if !diff.status.success(){return Err(deny("required Git metadata unavailable"));}
    let changed:Vec<String>=diff.stdout.split(|b|*b==0).filter(|x|!x.is_empty()).map(|x|String::from_utf8_lossy(x).into_owned()).collect();
    let receipt_name=receipt_path.to_string_lossy();
    if !changed.iter().any(|p|p==receipt_name.as_ref()){return Err(deny("receipt not committed with mirror"));}
    if !changed.iter().any(|p|p.starts_with(&format!("{source_dir}/"))){return Err(deny("source subtree not changed"));}
    for p in &changed{if p!=receipt_name.as_ref()&&!p.starts_with(&format!("{source_dir}/")){return Err(deny(format!("unexpected destination change: {p}")));}}
    let workflow=git(&["rev-parse",&format!("HEAD:{workflow_path}")])?;let parent_workflow=git(&["rev-parse",&format!("HEAD^:{workflow_path}")])?;
    require_hex(Some(&workflow),40,"carrier workflow blob")?;require_hex(Some(&parent_workflow),40,"parent carrier workflow blob")?;
    if workflow!=parent_workflow{return Err(deny("carrier workflow changed in source-bearing commit"));}
    if receipt["carrier"]["workflow_blob_sha"]!=workflow{return Err(deny("carrier workflow receipt binding"));}
    let source_tree=git(&["rev-parse",&format!("HEAD:{source_dir}")])?;require_hex(Some(&source_tree),40,"source root tree")?;
    if ref_name!=format!("ci/oreslang-{source_tree}"){return Err(deny("carrier branch/source tree mismatch"));}
    let source=receipt["source"].as_object().ok_or_else(||deny("source/mirror receipt"))?;let mirror=receipt["mirror"].as_object().ok_or_else(||deny("source/mirror receipt"))?;
    if source["root_tree_sha"]!=source_tree||mirror["root_tree_sha"]!=source_tree{return Err(deny("source root tree mismatch"));}
    if source["repo"].as_str().is_none_or(|v|!v.contains('/')){return Err(deny("source repository identity"));}
    if source["repo_id"].as_u64().is_none_or(|v|v==0){return Err(deny("source repository id"));}
    if source["pr"].as_u64().is_none_or(|v|v==0){return Err(deny("source PR number"));}
    require_hex(source["head_sha"].as_str(),40,"source head SHA")?;
    let (rows,total,manifest)=tree_manifest(&source_tree)?;
    if source["manifest_sha256"]!=manifest{return Err(deny("manifest digest mismatch"));}
    let test=receipt["test_plan"].as_object().ok_or_else(||deny("test plan receipt"))?;let plan_path=test["path"].as_str().ok_or_else(||deny("test plan path"))?;
    if !safe_plan_path(plan_path){return Err(deny("unsafe test plan path"));}
    let file=Path::new(plan_path);if file.is_symlink()||!file.is_file(){return Err(deny("test plan missing or unsafe"));}
    let plan_blob=git(&["rev-parse",&format!("HEAD:{plan_path}")])?;let parent_plan=git(&["rev-parse",&format!("HEAD^:{plan_path}")])?;
    if plan_blob!=parent_plan{return Err(deny("test plan changed in source-bearing commit"));}
    let plan_bytes=fs::read(file).map_err(|_|deny("invalid test plan JSON"))?;let plan_digest=format!("{:x}",Sha256::digest(&plan_bytes));
    if test["sha256"]!=plan_digest{return Err(deny("test plan digest mismatch"));}
    let plan:Value=serde_json::from_slice(&plan_bytes).map_err(|_|deny("invalid test plan JSON"))?;
    if plan["schema"]!="ores-private-ci-test-plan-v1"{return Err(deny("test plan schema"));}
    if plan["source_repo"]!=source["repo"]{return Err(deny("test plan source repository mismatch"));}
    let pr=source["pr"].as_u64().unwrap();let allowed=plan["allowed_prs"].as_array().ok_or_else(||deny("source PR not authorized by test plan"))?;
    if allowed.iter().any(|v|v.as_u64().is_none_or(|x|x==0))||!allowed.iter().any(|v|v.as_u64()==Some(pr)){return Err(deny("source PR not authorized by test plan"));}
    let receipt_digest=canonical_digest(&receipt);require_hex(Some(&receipt_digest),64,"receipt digest")?;
    let message=git(&["log","-1","--format=%B"])?;
    if !message.lines().any(|l|l==format!("Oreslang-CI-Receipt-SHA256: {receipt_digest}")){return Err(deny("commit/receipt digest binding"));}
    Ok(json!({"source_repo":source["repo"],"source_pr":pr,"source_head":source["head_sha"],"source_tree":source_tree,
        "file_count":rows.len(),"byte_count":total,"manifest_sha256":manifest,"receipt_sha256":receipt_digest,
        "workflow_blob_sha":workflow,"test_plan_path":plan_path,"test_plan_sha256":plan_digest,"receipt_schema":schema}))
}
fn main(){
    let args:Vec<String>=std::env::args().skip(1).collect();let mut receipt="ci/oreslang-receipt.json".to_string();let mut source="private-oreslang-source".to_string();let mut workflow=".github/workflows/oreslang-exact-tree-carrier.yml".to_string();let mut i=0;
    while i<args.len(){let v=args.get(i+1).cloned().unwrap_or_default();match args[i].as_str(){"--receipt"=>receipt=v,"--source-dir"=>source=v,"--workflow"=>workflow=v,_=>{eprintln!("CARRIER_PROVENANCE_DENIED: unknown argument");std::process::exit(1)}}i+=2;}
    let repo=std::env::var("GITHUB_REPOSITORY").unwrap_or_default();let id=std::env::var("GITHUB_REPOSITORY_ID").ok().and_then(|v|v.parse().ok()).unwrap_or(0);
    let head=std::env::var("GITHUB_SHA").unwrap_or_default();let r=std::env::var("GITHUB_REF_NAME").unwrap_or_default();
    match verify(Path::new(&receipt),&source,&workflow,&repo,id,&head,&r,Utc::now()){
        Ok(v)=>{for (k,val) in v.as_object().unwrap(){println!("{}={}",k.to_ascii_uppercase(),val.as_str().map(str::to_string).unwrap_or_else(||val.to_string()));}println!("CARRIER_PROVENANCE=PASS");}
        Err(e)=>{eprintln!("CARRIER_PROVENANCE_DENIED: {e}");std::process::exit(1);}
    }
}
