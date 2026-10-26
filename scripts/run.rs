use std::process::Command;
fn main(){
    let status=Command::new("cargo").args(["test","--all-targets"]).status().expect("run cargo test");
    std::process::exit(status.code().unwrap_or(1));
}
