use orestra_conformance::control_dispatch::{verify, Denied, DISPATCH_PATH, SCHEMA};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

fn run(root: &Path, args: &[&str]) {
    let out = Command::new(args[0]).args(&args[1..]).current_dir(root).output().unwrap();
    assert!(out.status.success(), "{:?}", String::from_utf8_lossy(&out.stderr));
}
fn out(root: &Path, args: &[&str]) -> String {
    let out = Command::new(args[0]).args(&args[1..]).current_dir(root).output().unwrap();
    assert!(out.status.success(), "{:?}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    base: String,
    branch: String,
    target: String,
    source_tree: String,
    head: String,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        run(&root, &["git", "init", "-q"]);
        run(&root, &["git", "config", "user.name", "Synthetic CI"]);
        run(&root, &["git", "config", "user.email", "ci@example.invalid"]);
        fs::create_dir_all(root.join(".github/workflows")).unwrap();
        fs::write(root.join(".github/workflows/oreslang-exact-tree-carrier.yml"), "name: trusted-control\n").unwrap();
        fs::write(root.join("README.md"), "base\n").unwrap();
        run(&root, &["git", "add", "-A"]);
        run(&root, &["git", "commit", "-qm", "base"]);
        let base = out(&root, &["git", "rev-parse", "HEAD"]);
        let branch = out(&root, &["git", "branch", "--show-current"]);
        let target = "a".repeat(40);
        let source_tree = "b".repeat(40);
        let mut f = Self { _dir: dir, root, base, branch, target, source_tree, head: String::new() };
        f.write_dispatch(None);
        run(&f.root, &["git", "add", "-A"]);
        run(&f.root, &["git", "commit", "-qm", "dispatch"]);
        f.head = out(&f.root, &["git", "rev-parse", "HEAD"]);
        f
    }
    fn write_dispatch(&self, override_auth: Option<String>) {
        let doc = json!({
            "schema": SCHEMA,
            "carrier_commit_sha": self.target,
            "source_ref": format!("ci/oreslang-{}", self.source_tree),
            "authorization_ref": override_auth.unwrap_or_else(|| format!("ci/authorized-{}", self.target))
        });
        let mut raw = serde_json::to_vec(&doc).unwrap();
        raw.push(b'\n');
        let path = self.root.join(DISPATCH_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, raw).unwrap();
    }
    fn verify(&self, ref_name: Option<String>) -> Result<serde_json::Value, Denied> {
        verify(
            &self.root,
            DISPATCH_PATH,
            "orestra/orestra-conformance",
            1426277234,
            &self.head,
            &ref_name.unwrap_or_else(|| format!("ci/oreslang-control-{}", self.target)),
        )
    }
    fn recommit(&mut self, message: &str) {
        run(&self.root, &["git", "add", "-A"]);
        run(&self.root, &["git", "commit", "-qm", message]);
        self.head = out(&self.root, &["git", "rev-parse", "HEAD"]);
    }
}

#[test]
fn exact_source_free_dispatch_passes() {
    let f = Fixture::new();
    let result = f.verify(None).unwrap();
    assert_eq!(result["control_parent"], f.base);
    assert_eq!(result["carrier_commit_sha"], f.target);
}
#[test]
fn ref_and_authorization_must_bind_exact_target() {
    let f = Fixture::new();
    assert!(f.verify(Some(format!("ci/oreslang-control-{}", "c".repeat(40)))).unwrap_err().to_string().contains("ref/dispatch"));
    let mut f = Fixture::new();
    f.write_dispatch(Some(format!("ci/authorized-{}", "c".repeat(40))));
    f.recommit("bad auth");
    assert!(f.verify(None).unwrap_err().to_string().contains("authorization ref mismatch"));
}
#[test]
fn extra_change_and_workflow_mutation_fail() {
    let mut f = Fixture::new();
    fs::write(f.root.join("outside.txt"), "forbidden\n").unwrap();
    f.recommit("extra");
    assert!(f.verify(None).unwrap_err().to_string().contains("only dispatch metadata"));

    let mut f = Fixture::new();
    fs::write(f.root.join(".github/workflows/oreslang-exact-tree-carrier.yml"), "name: attacker\n").unwrap();
    f.recommit("workflow");
    let e = f.verify(None).unwrap_err().to_string();
    assert!(e.contains("only dispatch metadata") || e.contains("workflow changed"));
}
#[test]
fn merge_control_commit_fails() {
    let mut f = Fixture::new();
    run(&f.root, &["git", "checkout", "-qb", "other", &f.base]);
    fs::write(f.root.join("other.txt"), "x\n").unwrap();
    run(&f.root, &["git", "add", "-A"]);
    run(&f.root, &["git", "commit", "-qm", "other"]);
    run(&f.root, &["git", "checkout", "-q", &f.branch]);
    run(&f.root, &["git", "merge", "--no-ff", "-qm", "merge", "other"]);
    f.head = out(&f.root, &["git", "rev-parse", "HEAD"]);
    assert!(f.verify(None).unwrap_err().to_string().contains("exactly one parent"));
}
#[test]
fn noncanonical_or_wrong_repository_fails() {
    let mut f = Fixture::new();
    fs::write(
        f.root.join(DISPATCH_PATH),
        format!("{{ \"schema\":\"{SCHEMA}\",\"carrier_commit_sha\":\"{}\",\"source_ref\":\"ci/oreslang-{}\",\"authorization_ref\":\"ci/authorized-{}\" }}\n", f.target, f.source_tree, f.target),
    ).unwrap();
    f.recommit("noncanonical");
    assert!(f.verify(None).unwrap_err().to_string().contains("canonical JSON"));

    let f = Fixture::new();
    let e = verify(&f.root, DISPATCH_PATH, "other/repo", 42, &f.head, &format!("ci/oreslang-control-{}", f.target)).unwrap_err();
    assert!(e.to_string().contains("repository identity"));
}
