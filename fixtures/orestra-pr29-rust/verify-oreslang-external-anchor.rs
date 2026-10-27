use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime,Duration,Utc};
use serde_json::{json,Value};
use sha1::{Digest as Sha1Digest,Sha1};
use sha2::{Digest as Sha2Digest,Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

pub const ANCHOR_REPO:&str="ores-truffle-oreslang/ores-truffle-oreslang.github.io";
pub const ANCHOR_REPO_ID:u64=1409246164;
pub const ANCHOR_BRANCH:&str="main";
pub const MANIFEST_SCHEMA:&str="ores-private-ci-external-authorization-v1";
pub const CARRIER_REPO:&str="orestra/orestra-conformance";
pub const CARRIER_REPO_ID:u64=1426277234;
pub const SOURCE_REPO:&str="ores-truffle-oreslang/oreslang-source.java";
pub const SOURCE_REPO_ID:u64=1400792274;
pub const MAX_REVIEWS:usize=100;
pub const SECURITY_REVIEWER_ID:u64=11139560;
const MAX_CHANGED_FILES:usize=100;
const MAX_RECEIPT:u64=64*1024;
const MAX_JSON:usize=1024*1024;
const MAX_MANIFEST:usize=64*1024;

#[derive(Debug,Clone)]
pub struct Denied(pub String);
impl std::fmt::Display for Denied{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{write!(f,"{}",self.0)}}
impl std::error::Error for Denied{}
fn deny(s:impl Into<String>)->Denied{Denied(s.into())}
pub fn is_sha(v:&str)->bool{v.len()==40&&v.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())}
pub fn require_sha(v:Option<&str>,label:&str)->Result<String,Denied>{let v=v.ok_or_else(||deny(label))?;if !is_sha(v){return Err(deny(label));}Ok(v.into())}
pub fn git_blob_sha(data:&[u8])->String{
    let mut h=Sha1::new();h.update(format!("blob {}\0",data.len()).as_bytes());h.update(data);format!("{:x}",h.finalize())
}
fn sha256(data:&[u8])->String{format!("{:x}",Sha256::digest(data))}
pub fn canonical_digest(value:&Value)->String{sha256(&serde_json::to_vec(value).expect("json"))}

pub fn gh_json(route:&str)->Result<Value,Denied>{
    let mut cmd=Command::new("gh");cmd.args(["api","--method","GET",route,"--header","Accept: application/vnd.github+json"]);
    cmd.env("GH_HOST","github.com").env("GH_PROMPT_DISABLED","1");
    let out=cmd.output().map_err(|_|deny("GitHub API unavailable"))?;
    if !out.status.success(){return Err(deny("GitHub API request failed"));}
    if out.stdout.len()>MAX_JSON{return Err(deny("GitHub API response oversized"));}
    serde_json::from_slice(&out.stdout).map_err(|_|deny("GitHub API returned invalid JSON"))
}
pub fn decode_contents(item:&Value,path:&str,max_bytes:usize)->Result<(Vec<u8>,String),Denied>{
    if item["path"]!=path||item["type"]!="file"||item["encoding"]!="base64"{return Err(deny("anchored file metadata invalid"));}
    let encoded=item["content"].as_str().ok_or_else(||deny("anchored file content missing"))?;
    let compact:String=encoded.chars().filter(|c|!c.is_whitespace()).collect();
    let raw=STANDARD.decode(compact).map_err(|_|deny("anchored file base64 invalid"))?;
    if raw.is_empty()||raw.len()>max_bytes{return Err(deny("anchored file missing or oversized"));}
    let blob=require_sha(item["sha"].as_str(),"anchored file blob SHA invalid")?;
    if git_blob_sha(&raw)!=blob{return Err(deny("anchored file blob digest mismatch"));}
    Ok((raw,blob))
}
pub fn parse_expiry(value:&str,now:DateTime<Utc>)->Result<DateTime<Utc>,Denied>{
    if !value.ends_with('Z'){return Err(deny("authorization expiry invalid"));}
    let expiry=DateTime::parse_from_rfc3339(value).map_err(|_|deny("authorization expiry invalid"))?.with_timezone(&Utc);
    if expiry<=now{return Err(deny("authorization expired"));}
    if expiry-now>Duration::days(7){return Err(deny("authorization retention exceeds seven days"));}
    Ok(expiry)
}
fn exact_keys(value:&Value,keys:&[&str])->bool{
    value.as_object().is_some_and(|o|o.len()==keys.len()&&keys.iter().all(|k|o.contains_key(*k)))
}
fn env_required(key:&str)->String{std::env::var(key).unwrap_or_default()}
fn env_optional(key:&str)->Option<String>{
    std::env::var(key).ok().filter(|v|!v.is_empty())
}
fn read_receipt(path:&Path)->Result<Value,Denied>{
    let meta=fs::symlink_metadata(path).map_err(|_|deny("v4 receipt missing or unsafe"))?;
    if meta.file_type().is_symlink()||!meta.is_file()||meta.len()==0||meta.len()>MAX_RECEIPT{
        return Err(deny("v4 receipt missing or oversized"));
    }
    let raw=fs::read(path).map_err(|_|deny("v4 receipt unreadable"))?;
    serde_json::from_slice(&raw).map_err(|_|deny("v4 receipt invalid JSON"))
}
fn load_authorization_hint(path:&Path)->Result<(u64,u64,String),Denied>{
    let receipt=read_receipt(path)?;
    if receipt["schema"]!="ores-private-ci-receipt-v4"{return Err(deny("v4 receipt schema mismatch"));}
    let source=&receipt["source"];
    let source_pr=source["pr"].as_u64().filter(|v|*v>0).ok_or_else(||deny("v4 receipt source hint invalid"))?;
    if source["repo"]!=SOURCE_REPO||source["repo_id"]!=SOURCE_REPO_ID{return Err(deny("v4 receipt source hint invalid"));}
    let auth=&receipt["authorization"];
    if !exact_keys(auth,&["repository","repository_id","branch","pr","merge_sha","manifest_path","manifest_blob_sha","manifest_sha256"])
       ||auth["repository"]!=ANCHOR_REPO||auth["repository_id"]!=ANCHOR_REPO_ID||auth["branch"]!=ANCHOR_BRANCH{
        return Err(deny("v4 receipt authorization hint invalid"));
    }
    let authorization_pr=auth["pr"].as_u64().filter(|v|*v>0).ok_or_else(||deny("v4 receipt authorization hint invalid"))?;
    require_sha(auth["merge_sha"].as_str(),"v4 receipt authorization hint invalid")?;
    require_sha(auth["manifest_blob_sha"].as_str(),"v4 receipt authorization hint invalid")?;
    let digest=auth["manifest_sha256"].as_str().ok_or_else(||deny("v4 receipt authorization hint invalid"))?;
    if digest.len()!=64||!digest.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase()){
        return Err(deny("v4 receipt authorization hint invalid"));
    }
    let manifest_path=format!("ci/private-ci/authorizations/java-pr{source_pr}.json");
    if auth["manifest_path"]!=manifest_path{return Err(deny("v4 receipt manifest path/source PR mismatch"));}
    Ok((source_pr,authorization_pr,manifest_path))
}

fn carrier_context<F>(api:&mut F)->Result<(String,String,String),Denied> where F:FnMut(&str)->Result<Value,Denied>{
    let repository=env_required("GITHUB_REPOSITORY");let repository_id=env_required("GITHUB_REPOSITORY_ID");
    let control_head=env_optional("ORESLANG_CARRIER_HEAD");
    let control_source=env_optional("ORESLANG_SOURCE_REF");
    let control_auth=env_optional("ORESLANG_AUTHORIZATION_REF");
    let control_parent=env_optional("ORESLANG_CONTROL_PARENT");
    let control_mode=control_head.is_some()||control_source.is_some()||control_auth.is_some()||control_parent.is_some();
    if control_mode&&!(control_head.is_some()&&control_source.is_some()&&control_auth.is_some()&&control_parent.is_some()){
        return Err(deny("control dispatch context incomplete"));
    }
    let event_head=env_required("GITHUB_SHA");
    let head=require_sha(Some(control_head.as_deref().unwrap_or(&event_head)),"carrier event SHA invalid")?;
    let ref_name=control_source.clone().unwrap_or_else(||env_required("GITHUB_REF_NAME"));
    if repository!=CARRIER_REPO||repository_id!=CARRIER_REPO_ID.to_string(){return Err(deny("carrier event repository identity mismatch"));}
    let suffix=ref_name.strip_prefix("ci/oreslang-").ok_or_else(||deny("carrier event ref is not an isolated source ref"))?;
    let ref_tree=require_sha(Some(suffix),"carrier source-tree ref suffix invalid")?;
    let repo=api(&format!("repos/{CARRIER_REPO}"))?;
    if repo["id"]!=CARRIER_REPO_ID||repo["full_name"]!=CARRIER_REPO||repo["private"]!=true||repo["visibility"]!="private"
       ||repo["archived"]!=false||repo["disabled"]!=false||repo["fork"]!=false{return Err(deny("carrier repository identity/privacy invalid"));}
    let commit=api(&format!("repos/{CARRIER_REPO}/git/commits/{head}"))?;
    let parents=commit["parents"].as_array().ok_or_else(||deny("carrier source commit must have exactly one parent"))?;
    if commit["sha"]!=head||parents.len()!=1{return Err(deny("carrier source commit must have exactly one parent"));}
    let base=require_sha(parents[0]["sha"].as_str(),"carrier base SHA invalid")?;
    if control_mode{
        let expected_auth=format!("ci/authorized-{head}");
        if control_auth.as_deref()!=Some(expected_auth.as_str()){
            return Err(deny("control dispatch authorization ref mismatch"));
        }
        let expected_parent=require_sha(control_parent.as_deref(),"control dispatch parent invalid")?;
        if base!=expected_parent{return Err(deny("carrier/control parent mismatch"));}
    }
    let source_ref=api(&format!("repos/{CARRIER_REPO}/git/ref/heads/{ref_name}"))?;
    if source_ref["ref"]!=format!("refs/heads/{ref_name}")||source_ref["object"]["type"]!="commit"||source_ref["object"]["sha"]!=head{
        return Err(deny("source-tree ref missing or mismatched"));
    }
    let auth=api(&format!("repos/{CARRIER_REPO}/git/ref/heads/ci/authorized-{head}"))?;
    if auth["ref"]!=format!("refs/heads/ci/authorized-{head}")||auth["object"]["type"]!="commit"||auth["object"]["sha"]!=head{
        return Err(deny("dispatcher authorization ref missing or mismatched"));
    }
    Ok((head,ref_tree,base))
}

pub fn validate_anchor_rules(rules:&Value)->Result<(),Denied>{
    let rules=rules.as_array().ok_or_else(||deny("anchor effective rules missing or unbounded"))?;
    if rules.is_empty()||rules.len()>=100{return Err(deny("anchor effective rules missing or unbounded"));}
    let mut secure_pr=false;let mut deletion=false;let mut non_fast_forward=false;
    for rule in rules{
        let kind=rule["type"].as_str().ok_or_else(||deny("anchor effective rule malformed"))?;
        match kind{
            "deletion"=>deletion=true,
            "non_fast_forward"=>non_fast_forward=true,
            "pull_request"=>{
                let p=&rule["parameters"];
                if p["required_approving_review_count"].as_u64().is_some_and(|v|v>=1)
                   &&p["dismiss_stale_reviews_on_push"]==true
                   &&p["require_code_owner_review"]==true
                   &&p["require_last_push_approval"]==true
                   &&p["required_review_thread_resolution"]==true{secure_pr=true;}
            },
            _=>{}
        }
    }
    if !secure_pr{return Err(deny("anchor pull-request rule is not strict enough"));}
    if !deletion{return Err(deny("anchor deletion protection missing"));}
    if !non_fast_forward{return Err(deny("anchor force-push protection missing"));}
    Ok(())
}

pub fn effective_independent_approval(reviews:&Value,pr:&Value)->Result<bool,Denied>{
    let reviews=reviews.as_array().ok_or_else(||deny("authorization review set missing or unbounded"))?;
    if reviews.len()>=MAX_REVIEWS{return Err(deny("authorization review set missing or unbounded"));}
    let author=pr["user"]["id"].as_u64().ok_or_else(||deny("authorization PR author identity missing"))?;
    let head=require_sha(pr["head"]["sha"].as_str(),"authorization PR head SHA invalid")?;
    let mut latest:HashMap<u64,&Value>=HashMap::new();
    for review in reviews{
        let user=review["user"]["id"].as_u64().ok_or_else(||deny("authorization review identity invalid"))?;
        let id=review["id"].as_u64().ok_or_else(||deny("authorization review identity invalid"))?;
        if latest.get(&user).and_then(|r|r["id"].as_u64()).is_none_or(|old|id>old){latest.insert(user,review);}
    }
    Ok(latest.into_iter().any(|(id,r)|id!=author&&id==SECURITY_REVIEWER_ID&&r["state"]=="APPROVED"&&r["commit_id"]==head
       &&r["submitted_at"].as_str().is_some_and(|s|!s.is_empty())))
}

fn validate_manifest(doc:&Value,base:&str,tree:&str,now:DateTime<Utc>,source_pr:u64)->Result<(Value,Value),Denied>{
    if !exact_keys(doc,&["schema","anchor_repository_id","source","destination","constraints"]){return Err(deny("authorization manifest fields are not exact"));}
    if doc["schema"]!=MANIFEST_SCHEMA{return Err(deny("authorization manifest schema mismatch"));}
    if doc["anchor_repository_id"]!=ANCHOR_REPO_ID{return Err(deny("authorization anchor repository id mismatch"));}
    let source=&doc["source"];
    if !exact_keys(source,&["repository","repository_id","base_branch","pr","head_sha","tree_sha"]){return Err(deny("authorization source fields are not exact"));}
    if source["repository"]!=SOURCE_REPO||source["repository_id"]!=SOURCE_REPO_ID||source["base_branch"]!="main"||source["pr"]!=source_pr{
        return Err(deny("authorization source identity mismatch"));
    }
    require_sha(source["head_sha"].as_str(),"authorization source head invalid")?;
    let source_tree=require_sha(source["tree_sha"].as_str(),"authorization source tree invalid")?;
    if source_tree!=tree{return Err(deny("carrier ref does not match authorized source tree"));}
    let dest=&doc["destination"];
    if !exact_keys(dest,&["repository","repository_id","base_sha","workflow_path","workflow_blob_sha","test_plan_path","test_plan_blob_sha"]){return Err(deny("authorization destination fields are not exact"));}
    if dest["repository"]!=CARRIER_REPO||dest["repository_id"]!=CARRIER_REPO_ID||dest["base_sha"]!=base{return Err(deny("authorization carrier base/identity mismatch"));}
    require_sha(dest["workflow_blob_sha"].as_str(),"authorization workflow blob invalid")?;
    require_sha(dest["test_plan_blob_sha"].as_str(),"authorization test-plan blob invalid")?;
    if dest["workflow_path"]!=".github/workflows/oreslang-exact-tree-carrier.yml"{return Err(deny("authorization workflow path mismatch"));}
    if dest["test_plan_path"]!=format!("ci/test-plans/java-pr{source_pr}.json"){return Err(deny("authorization test-plan path mismatch"));}
    let c=&doc["constraints"];
    if !exact_keys(c,&["source_must_be_private","destination_must_be_private","source_tree_prefix","source_ref_template","authorization_ref_template","control_ref_template","receipt_schema","expires_at"]){return Err(deny("authorization constraint fields are not exact"));}
    if c["source_must_be_private"]!=true||c["destination_must_be_private"]!=true||c["source_tree_prefix"]!="private-oreslang-source"
       ||c["source_ref_template"]!="ci/oreslang-{source_tree_sha}"||c["authorization_ref_template"]!="ci/authorized-{carrier_commit_sha}"
       ||c["control_ref_template"]!="ci/oreslang-control-{carrier_commit_sha}"||c["receipt_schema"]!="ores-private-ci-receipt-v4"{return Err(deny("authorization constraints mismatch"));}
    parse_expiry(c["expires_at"].as_str().ok_or_else(||deny("authorization expiry invalid"))?,now)?;
    Ok((source.clone(),dest.clone()))
}
fn verify_receipt(path:&Path,anchor:&str,blob:&str,digest:&str,source:&Value,dest:&Value,plan_digest:&str,authorization_pr:u64,manifest_path:&str)->Result<(),Denied>{
    let receipt=read_receipt(path)?;
    if receipt["schema"]!="ores-private-ci-receipt-v4"{return Err(deny("v4 receipt schema mismatch"));}
    let expected=json!({"repository":ANCHOR_REPO,"repository_id":ANCHOR_REPO_ID,"branch":ANCHOR_BRANCH,"pr":authorization_pr,
        "merge_sha":anchor,"manifest_path":manifest_path,"manifest_blob_sha":blob,"manifest_sha256":digest});
    if receipt["authorization"]!=expected{return Err(deny("v4 receipt authorization binding mismatch"));}
    for (k,expected) in [("repo",&source["repository"]),("repo_id",&source["repository_id"]),("pr",&source["pr"]),("head_sha",&source["head_sha"]),("root_tree_sha",&source["tree_sha"])]{
        if receipt["source"][k]!=*expected{return Err(deny("v4 receipt source binding mismatch"));}
    }
    if receipt["destination"]["repo"]!=dest["repository"]||receipt["destination"]["repo_id"]!=dest["repository_id"]||receipt["destination"]["base_sha"]!=dest["base_sha"]{
        return Err(deny("v4 receipt destination binding mismatch"));
    }
    if receipt["carrier"]["workflow_blob_sha"]!=dest["workflow_blob_sha"]{return Err(deny("v4 receipt workflow binding mismatch"));}
    if receipt["test_plan"]["path"]!=dest["test_plan_path"]||receipt["test_plan"]["sha256"]!=plan_digest{return Err(deny("v4 receipt test-plan binding mismatch"));}
    if receipt["mirror"]["root_tree_sha"]!=source["tree_sha"]{return Err(deny("v4 receipt mirror binding mismatch"));}
    Ok(())
}
pub fn verify_with_api<F>(receipt:&Path,mut api:F,now:DateTime<Utc>)->Result<Value,Denied> where F:FnMut(&str)->Result<Value,Denied>{
    let (source_pr,authorization_pr,manifest_path)=load_authorization_hint(receipt)?;
    let (head,tree,base)=carrier_context(&mut api)?;
    let anchor=api(&format!("repos/{ANCHOR_REPO}"))?;
    if anchor["id"]!=ANCHOR_REPO_ID||anchor["full_name"]!=ANCHOR_REPO||anchor["private"]!=false||anchor["visibility"]!="public"
       ||anchor["default_branch"]!=ANCHOR_BRANCH||anchor["archived"]!=false||anchor["disabled"]!=false||anchor["fork"]!=false{
        return Err(deny("public authorization anchor identity/state invalid"));
    }
    let branch=api(&format!("repos/{ANCHOR_REPO}/branches/{ANCHOR_BRANCH}"))?;
    let anchor_sha=require_sha(branch["commit"]["sha"].as_str(),"authorization anchor main SHA invalid")?;
    if branch["name"]!=ANCHOR_BRANCH||branch["protected"]!=true{return Err(deny("authorization anchor main is not protected"));}
    validate_anchor_rules(&api(&format!("repos/{ANCHOR_REPO}/rules/branches/{ANCHOR_BRANCH}"))?)?;
    let pr=api(&format!("repos/{ANCHOR_REPO}/pulls/{authorization_pr}"))?;
    if pr["number"]!=authorization_pr||pr["state"]!="closed"||pr["draft"]!=false||pr["merged_at"].as_str().is_none_or(str::is_empty)
       ||pr["merge_commit_sha"]!=anchor_sha||pr["head"]["repo"]["full_name"]!=ANCHOR_REPO||pr["base"]["ref"]!=ANCHOR_BRANCH{
        return Err(deny("authorization PR is not the exact merged anchor tip"));
    }
    let reviews=api(&format!("repos/{ANCHOR_REPO}/pulls/{authorization_pr}/reviews?per_page={MAX_REVIEWS}"))?;
    if !effective_independent_approval(&reviews,&pr)?{return Err(deny("authorization PR lacks approved security-reviewer exact-head approval"));}
    let item=api(&format!("repos/{ANCHOR_REPO}/contents/{manifest_path}?ref={anchor_sha}"))?;
    let (manifest_raw,manifest_blob)=decode_contents(&item,&manifest_path,MAX_MANIFEST)?;
    let changed_files=api(&format!("repos/{ANCHOR_REPO}/pulls/{authorization_pr}/files?per_page={MAX_CHANGED_FILES}"))?;
    let changed_files=changed_files.as_array().ok_or_else(||deny("authorization PR changed-file set missing or unbounded"))?;
    if changed_files.len()>=MAX_CHANGED_FILES{return Err(deny("authorization PR changed-file set missing or unbounded"));}
    let reviewed:Vec<_>=changed_files.iter().filter(|e|e["filename"]==manifest_path).collect();
    if reviewed.len()!=1||!(reviewed[0]["status"]=="added"||reviewed[0]["status"]=="modified")||reviewed[0]["sha"]!=manifest_blob{
        return Err(deny("authorization PR did not review exact manifest blob"));
    }
    let manifest:Value=serde_json::from_slice(&manifest_raw).map_err(|_|deny("authorization manifest JSON invalid"))?;
    let (source,dest)=validate_manifest(&manifest,&base,&tree,now,source_pr)?;
    let workflow_path=dest["workflow_path"].as_str().unwrap();
    let workflow=api(&format!("repos/{CARRIER_REPO}/contents/{workflow_path}?ref={base}"))?;
    if workflow["path"]!=workflow_path||workflow["type"]!="file"||workflow["sha"]!=dest["workflow_blob_sha"]{return Err(deny("carrier workflow differs from public authorization"));}
    let plan_path=dest["test_plan_path"].as_str().unwrap();let plan=api(&format!("repos/{CARRIER_REPO}/contents/{plan_path}?ref={base}"))?;
    let (plan_raw,plan_blob)=decode_contents(&plan,plan_path,64*1024)?;
    if plan_blob!=dest["test_plan_blob_sha"]{return Err(deny("carrier test plan differs from public authorization"));}
    let plan_digest=sha256(&plan_raw);
    let after=api(&format!("repos/{ANCHOR_REPO}/branches/{ANCHOR_BRANCH}"))?;
    if after["protected"]!=true||after["commit"]["sha"]!=anchor_sha{return Err(deny("authorization anchor moved during validation"));}
    validate_anchor_rules(&api(&format!("repos/{ANCHOR_REPO}/rules/branches/{ANCHOR_BRANCH}"))?)?;
    let manifest_digest=sha256(&manifest_raw);
    verify_receipt(receipt,&anchor_sha,&manifest_blob,&manifest_digest,&source,&dest,&plan_digest,authorization_pr,&manifest_path)?;
    Ok(json!({"carrier_head":head,"carrier_base":base,"source_pr":source_pr,"authorization_pr":authorization_pr,"source_head":source["head_sha"],"source_tree":source["tree_sha"],
        "anchor_sha":anchor_sha,"manifest_blob_sha":manifest_blob,"manifest_sha256":manifest_digest,
        "workflow_blob_sha":dest["workflow_blob_sha"],"test_plan_blob_sha":dest["test_plan_blob_sha"],"test_plan_sha256":plan_digest}))
}
pub fn verify(receipt:&Path)->Result<Value,Denied>{verify_with_api(receipt,gh_json,Utc::now())}
fn main(){
    let args:Vec<String>=std::env::args().skip(1).collect();let receipt=if args.len()==2&&args[0]=="--receipt"{args[1].clone()}else{"ci/oreslang-receipt.json".into()};
    match verify(Path::new(&receipt)){Ok(v)=>{for(k,val)in v.as_object().unwrap(){println!("{}={}",k.to_ascii_uppercase(),val.as_str().unwrap_or(&val.to_string()));}println!("EXTERNAL_ANCHOR=PASS");}
    Err(e)=>{eprintln!("EXTERNAL_ANCHOR_DENIED: {e}");std::process::exit(1);}}
}
