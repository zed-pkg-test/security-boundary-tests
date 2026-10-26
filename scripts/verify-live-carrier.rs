use serde_json::Value;
use std::fs;
fn load(path:&str)->Result<Value,String>{
    let raw=fs::read(path).map_err(|e|e.to_string())?;
    if raw.is_empty()||raw.len()>1024*1024||raw.starts_with(&[0xef,0xbb,0xbf]){return Err("invalid JSON metadata".into());}
    serde_json::from_slice(&raw).map_err(|e|e.to_string())
}
fn repository(path:&str)->Result<(),String>{
    let repo=load(path)?;
    if repo["id"]!=1426277234u64||repo["full_name"]!="orestra/orestra-conformance"||repo["private"]!=true
       ||repo["visibility"]!="private"||repo["default_branch"]!="main"||repo["archived"]!=false||repo["disabled"]!=false||repo["fork"]!=false{
        return Err("live carrier identity/privacy no longer eligible".into());
    }
    println!("CARRIER_LIVE_PRIVATE_PASS");Ok(())
}
fn authorization(path:&str,reference:&str,sha:&str)->Result<(),String>{
    let doc=load(path)?;
    if doc["ref"]!=format!("refs/heads/{reference}")||doc["object"]["type"]!="commit"||doc["object"]["sha"]!=sha{
        return Err("authorization ref does not bind event payload".into());
    }
    println!("CARRIER_AUTHORIZATION_REF_PASS");Ok(())
}
fn main(){
    let args:Vec<String>=std::env::args().skip(1).collect();
    let result=match args.as_slice(){
        [mode,path] if mode=="repository"=>repository(path),
        [mode,path,reference,sha] if mode=="authorization"=>authorization(path,reference,sha),
        _=>Err("usage: verify-live-carrier repository <json> | authorization <json> <ref> <sha>".into()),
    };
    if let Err(e)=result{eprintln!("{e}");std::process::exit(1);}
}
