use orestra_conformance::native_runner::{run_case,runner_environment};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::time::Duration;
use tempfile::tempdir;

fn runner(dir:&Path,body:&str)->String{
    let p=dir.join("runner");fs::write(&p,format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p,fs::Permissions::from_mode(0o700)).unwrap();
    p.to_string_lossy().into_owned()
}
#[test]
fn requires_real_source_and_nonempty_absolute_runner(){
    let dir=tempdir().unwrap();let command=vec![runner(dir.path(),"echo expected")];
    assert!(run_case(&command,Path::new("/nonexistent/test.ores"),"expected",Duration::from_secs(2)).is_err());
    let source=dir.path().join("test.ores");fs::write(&source,"placeholder").unwrap();
    assert!(run_case(&[],&source,"expected",Duration::from_secs(2)).is_err());
    assert!(run_case(&["runner".into()],&source,"expected",Duration::from_secs(2)).is_err());
}
#[test]
fn success_requires_exact_sentinel_line_and_no_error_marker(){
    let dir=tempdir().unwrap();let source=dir.path().join("test.ores");fs::write(&source,"placeholder").unwrap();
    for body in ["echo wrong","echo prefix-expected-suffix","echo expected; echo 'ERROR: no'"]{
        let command=vec![runner(dir.path(),body)];assert!(run_case(&command,&source,"expected",Duration::from_secs(2)).is_err());
    }
    let command=vec![runner(dir.path(),"echo expected")];assert!(run_case(&command,&source,"expected",Duration::from_secs(2)).unwrap());
}
#[test]
fn environment_drops_startup_injection(){
    for key in ["BASH_ENV","PYTHONPATH","LD_PRELOAD","JAVA_TOOL_OPTIONS"]{std::env::set_var(key,"/tmp/inject");}
    let env=runner_environment();for key in ["BASH_ENV","PYTHONPATH","LD_PRELOAD","JAVA_TOOL_OPTIONS"]{assert!(!env.contains_key(key));}
    assert_eq!(env["ORESTRA_SOURCE_TEST"],"1");
}

#[test]
fn rejects_symlinked_and_oversized_sources(){
    let dir=tempdir().unwrap();
    let command=vec![runner(dir.path(),"echo expected")];
    let real=dir.path().join("real.ores");
    fs::write(&real,"placeholder").unwrap();
    let linked=dir.path().join("linked.ores");
    symlink(&real,&linked).unwrap();
    assert!(run_case(&command,&linked,"expected",Duration::from_secs(2)).is_err());

    let huge=dir.path().join("huge.ores");
    fs::write(&huge,vec![b'x';1_048_577]).unwrap();
    assert!(run_case(&command,&huge,"expected",Duration::from_secs(2)).is_err());
}

#[test]
fn rejects_symlinked_non_executable_and_unbounded_runner_output(){
    let dir=tempdir().unwrap();
    let source=dir.path().join("test.ores");
    fs::write(&source,"placeholder").unwrap();

    let real=runner(dir.path(),"echo expected");
    let linked=dir.path().join("runner-link");
    symlink(&real,&linked).unwrap();
    assert!(run_case(&[linked.to_string_lossy().into_owned()],&source,"expected",Duration::from_secs(2)).is_err());

    let not_exec=dir.path().join("not-exec");
    fs::write(&not_exec,"#!/bin/sh\necho expected\n").unwrap();
    fs::set_permissions(&not_exec,fs::Permissions::from_mode(0o600)).unwrap();
    assert!(run_case(&[not_exec.to_string_lossy().into_owned()],&source,"expected",Duration::from_secs(2)).is_err());

    let noisy=vec![runner(dir.path(),"head -c 1048577 /dev/zero | tr '\\0' x; echo expected")];
    assert!(run_case(&noisy,&source,"expected",Duration::from_secs(3)).is_err());
}
