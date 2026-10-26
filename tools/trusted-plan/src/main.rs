use orestra_trusted_plan::run_plan;
use std::{env, path::PathBuf};

fn usage() -> ! {
    eprintln!("usage: orestra-trusted-plan [--receipt PATH] [--source-dir PATH]");
    std::process::exit(2)
}

fn main() {
    let mut receipt = PathBuf::from("ci/oreslang-receipt.json");
    let mut source = PathBuf::from("private-oreslang-source");
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--receipt") => {
                receipt = args.next().map(PathBuf::from).unwrap_or_else(|| usage())
            }
            Some("--source-dir") => {
                source = args.next().map(PathBuf::from).unwrap_or_else(|| usage())
            }
            _ => usage(),
        }
    }
    if let Err(e) = run_plan(&receipt, &source) {
        eprintln!("TEST_PLAN_DENIED: {e}");
        std::process::exit(1);
    }
}
