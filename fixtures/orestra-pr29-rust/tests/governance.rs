use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

fn root() -> &'static Path { Path::new(env!("CARGO_MANIFEST_DIR")) }

#[test]
fn codeowners_covers_control_dispatch() {
    let text = fs::read_to_string(root().join(".github/CODEOWNERS")).unwrap();
    let lines: BTreeSet<_> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    for required in [
        "/.github/workflows/oreslang-exact-tree-carrier.yml @ORESoftware",
        "/scripts/verify-oreslang-external-anchor.rs @ORESoftware",
        "/scripts/test-oreslang-external-anchor.rs @ORESoftware",
        "/scripts/verify-oreslang-private-carrier.rs @ORESoftware",
        "/scripts/test-oreslang-private-carrier.rs @ORESoftware",
        "/scripts/verify-oreslang-control-dispatch.rs @ORESoftware",
        "/scripts/test-oreslang-control-dispatch.rs @ORESoftware",
        "/tools/trusted-plan/ @ORESoftware",
        "/ci/test-plans/ @ORESoftware",
    ] {
        assert!(lines.contains(required), "missing CODEOWNER rule: {required}");
    }
}

#[test]
fn carrier_push_entry_is_control_ref_only() {
    let text = fs::read_to_string(root().join(".github/workflows/oreslang-exact-tree-carrier.yml")).unwrap();
    let trigger = &text[..text.find("permissions:").unwrap()];
    assert!(trigger.contains("pull_request:"));
    assert!(trigger.contains("push:"));
    assert!(trigger.contains("ci/oreslang-control-*"));
    assert!(!trigger.lines().any(|line| line.trim() == "- 'ci/oreslang-*'"));
    assert!(text.contains("github.event_name == 'push' && startsWith(github.ref_name, 'ci/oreslang-control-')"));
}

#[test]
fn private_checkout_occurs_only_after_source_free_authorization() {
    let text = fs::read_to_string(root().join(".github/workflows/oreslang-exact-tree-carrier.yml")).unwrap();
    let job = &text[text.find("  verify-carrier:").unwrap()..];
    let control_checkout = job.find("Checkout source-free control commit").unwrap();
    let dispatch = job.find("Verify source-free control dispatch with trusted parent").unwrap();
    let receipt = job.find("Fetch only the committed receipt before private checkout").unwrap();
    let authorization = job.find("Verify protected authorization before private checkout").unwrap();
    let setup = job.find("Set up GraalVM JDK 25 before private source checkout").unwrap();
    let private_checkout = job.find("Checkout exact authorized private carrier commit").unwrap();
    let provenance = job.find("Verify exact-tree carrier provenance").unwrap();
    let recheck = job.find("Recheck protected authorization after private checkout").unwrap();
    let execute = job.find("Execute immutable trusted test plan").unwrap();
    assert!(
        control_checkout < dispatch
            && dispatch < receipt
            && receipt < authorization
            && authorization < setup
            && setup < private_checkout
            && private_checkout < provenance
            && provenance < recheck
            && recheck < execute
    );
    assert!(job.contains("git worktree add --detach"));
    assert!(job.contains("GIT_NO_REPLACE_OBJECTS=1"));
    assert!(job.contains("ORESLANG_AUTHORIZATION_REF"));
    assert!(job.contains("ORESLANG_CONTROL_PARENT"));
    assert!(job.contains("application/vnd.github.raw+json"));
}

#[test]
fn generic_push_workflows_ignore_carrier_refs() {
    for path in [".github/workflows/conformance.yml", ".github/workflows/native-oreslang-policy.yml"] {
        let text = fs::read_to_string(root().join(path)).unwrap();
        assert!(text.contains("branches-ignore:"), "{path}");
        assert!(text.contains("ci/oreslang-*"), "{path}");
        assert!(text.contains("ci/oreslang-control-*"), "{path}");
    }
}

#[test]
fn workflow_uses_rust_only_control_plane() {
    let text = fs::read_to_string(root().join(".github/workflows/oreslang-exact-tree-carrier.yml")).unwrap();
    assert!(!text.contains("python3"));
    assert!(text.contains("--bin verify-oreslang-control-dispatch"));
    assert!(text.contains("--bin verify-oreslang-external-anchor"));
    assert!(text.contains("--bin verify-oreslang-private-carrier"));
    assert!(text.contains("tools/trusted-plan/Cargo.toml"));
}
