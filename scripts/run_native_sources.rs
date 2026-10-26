use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use wait_timeout::ChildExt;

const SAFE_ENV_KEYS: &[&str] = &["PATH","HOME","TMPDIR","LANG","LC_ALL","TZ","JAVA_HOME"];
const MAX_SOURCE_BYTES: u64 = 1_048_576;
const MAX_OUTPUT_BYTES: usize = 1_048_576;

fn validate_source_file(source: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(source).map_err(|_| format!("missing Oreslang source: {}", source.display()))?;
    if meta.file_type().is_symlink() || !meta.is_file() || source.extension().and_then(|x|x.to_str()) != Some("ores") {
        return Err(format!("Oreslang source must be a regular non-symlink .ores file: {}", source.display()));
    }
    if meta.len() > MAX_SOURCE_BYTES {
        return Err(format!("Oreslang source exceeds {} bytes: {}", MAX_SOURCE_BYTES, source.display()));
    }
    Ok(())
}

fn validate_executable(executable: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(executable)
        .map_err(|_| "runner executable must be an existing absolute regular file".to_string())?;
    if !executable.is_absolute() || meta.file_type().is_symlink() || !meta.is_file() {
        return Err("runner executable must be an existing absolute regular non-symlink file".into());
    }
    #[cfg(unix)]
    if meta.permissions().mode() & 0o111 == 0 {
        return Err("runner executable must have an executable mode bit".into());
    }
    Ok(())
}

fn read_bounded<R: Read>(reader: R) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = reader.take((MAX_OUTPUT_BYTES + 1) as u64).read_to_end(&mut bytes);
    bytes
}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    let _ = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
}

pub const CASES: &[(&str,&str,&str)] = &[
    ("orestra","examples/placement-smoke.ores","orestra-placement-ok"),
    ("orestra","examples/edge-guards.ores","orestra-edge-guards-ok"),
    ("orestra","examples/workload-state-smoke.ores","orestra-workload-state-ok"),
    ("orestra","examples/control-plane-smoke.ores","orestra-control-plane-ok"),
    ("sdk","examples/sdk-smoke.ores","orestra-sdk-ok"),
];

pub fn runner_environment() -> HashMap<String,String> {
    let mut out = HashMap::new();
    for key in SAFE_ENV_KEYS {
        if let Ok(value)=env::var(key) { out.insert((*key).to_string(), value); }
    }
    out.insert("ORESTRA_SOURCE_TEST".into(),"1".into());
    out
}

pub fn run_case(command:&[String], source:&Path, sentinel:&str, timeout:Duration) -> Result<bool,String> {
    if command.is_empty() || command.iter().any(|v| v.is_empty()) {
        return Err("runner command must be nonempty argv list".into());
    }
    validate_source_file(source)?;
    let executable=Path::new(&command[0]);
    validate_executable(executable)?;
    let mut command_builder=Command::new(executable);
    command_builder
        .args(&command[1..]).arg(source)
        .env_clear().envs(runner_environment())
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    command_builder.process_group(0);
    let mut child=command_builder.spawn().map_err(|e|e.to_string())?;
    let mut stdout=child.stdout.take().unwrap();
    let mut stderr=child.stderr.take().unwrap();
    let out_thread=thread::spawn(move||read_bounded(&mut stdout));
    let err_thread=thread::spawn(move||read_bounded(&mut stderr));
    let status=match child.wait_timeout(timeout).map_err(|e|e.to_string())? {
        Some(status)=>status,
        None=>{
            #[cfg(unix)]
            kill_process_group(child.id());
            #[cfg(not(unix))]
            let _=child.kill();
            let _=child.wait();
            let _=out_thread.join();
            let _=err_thread.join();
            return Err("runner timed out".into());
        }
    };
    let stdout_bytes=out_thread.join().map_err(|_|"stdout reader panicked".to_string())?;
    let stderr_bytes=err_thread.join().map_err(|_|"stderr reader panicked".to_string())?;
    if stdout_bytes.len() > MAX_OUTPUT_BYTES || stderr_bytes.len() > MAX_OUTPUT_BYTES {
        return Err("runner output exceeded bounded capture limit".into());
    }
    let stdout=String::from_utf8_lossy(&stdout_bytes).into_owned();
    let stderr=String::from_utf8_lossy(&stderr_bytes).into_owned();
    let exact=stdout.lines().any(|line| line==sentinel);
    if !status.success() || !exact || stdout.contains("ERROR:") {
        return Err(format!("{}: exit={} expected={:?}\nstdout={}\nstderr={}",
            source.display(),status.code().unwrap_or(-1),sentinel,tail(&stdout,3000),tail(&stderr,3000)));
    }
    Ok(true)
}

fn tail(value:&str,max:usize)->String {
    if value.len()<=max { value.to_string() } else { value[value.len()-max..].to_string() }
}

pub fn run_cli(args:&[String])->Result<(),String>{
    let mut backend=None; let mut runner_json=None; let mut orestra=None; let mut sdk=None; let mut i=0;
    while i<args.len() {
        let value=args.get(i+1).ok_or_else(||format!("missing value for {}",args[i]))?.clone();
        match args[i].as_str() {
            "--backend"=>backend=Some(value),
            "--runner-json"=>runner_json=Some(value),
            "--orestra-root"=>orestra=Some(PathBuf::from(value)),
            "--sdk-root"=>sdk=Some(PathBuf::from(value)),
            other=>return Err(format!("unknown argument {other}")),
        }
        i+=2;
    }
    let backend=backend.ok_or("missing --backend")?;
    if backend!="graalvm" && backend!="llvm" {return Err("backend must be graalvm or llvm".into());}
    let runner_value:Value=serde_json::from_str(&runner_json.ok_or("missing --runner-json")?).map_err(|e|e.to_string())?;
    let runner=runner_value.as_array().ok_or("runner-json must be an argv array")?
        .iter().map(|v|v.as_str().map(str::to_string).ok_or("runner-json values must be strings")).collect::<Result<Vec<_>,_>>()?;
    let roots=HashMap::from([
        ("orestra",fs::canonicalize(orestra.ok_or("missing --orestra-root")?).map_err(|e|e.to_string())?),
        ("sdk",fs::canonicalize(sdk.ok_or("missing --sdk-root")?).map_err(|e|e.to_string())?),
    ]);
    for (location,relative,sentinel) in CASES {
        let root=&roots[location];
        let candidate=root.join(relative);
        validate_source_file(&candidate)?;
        let source=fs::canonicalize(&candidate).map_err(|_|format!("missing Oreslang source: {relative}"))?;
        if !source.starts_with(root){return Err(format!("fixture escapes configured source root: {relative}"));}
        run_case(&runner,&source,sentinel,Duration::from_secs(30))?;
        println!("PASS {backend} {relative}");
    }
    println!("Native Oreslang source gate PASS ({backend}, {} fixtures)",CASES.len());
    Ok(())
}

fn main(){
    let args:Vec<String>=env::args().skip(1).collect();
    if let Err(error)=run_cli(&args){eprintln!("NATIVE GATE FAILED: {error}");std::process::exit(1);}
}
