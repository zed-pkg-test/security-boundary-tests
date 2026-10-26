use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
fn root()->&'static Path{Path::new(env!("CARGO_MANIFEST_DIR"))}
#[test]
fn codeowners_covers_security_control_plane(){
 let text=fs::read_to_string(root().join(".github/CODEOWNERS")).unwrap();let lines:BTreeSet<_>=text.lines().map(str::trim).filter(|l|!l.is_empty()&&!l.starts_with('#')).collect();
 for required in ["* @ORESoftware","/.github/CODEOWNERS @ORESoftware","/.github/workflows/oreslang-exact-tree-carrier.yml @ORESoftware","/Cargo.toml @ORESoftware","/scripts/verify-oreslang-private-carrier.rs @ORESoftware","/scripts/test-oreslang-private-carrier.rs @ORESoftware","/scripts/test-carrier-governance-policy.rs @ORESoftware","/scripts/verify-oreslang-external-anchor.rs @ORESoftware","/scripts/test-oreslang-external-anchor.rs @ORESoftware","/scripts/verify-live-carrier.rs @ORESoftware","/tools/trusted-plan/ @ORESoftware","/ci/test-plans/ @ORESoftware"]{assert!(lines.contains(required),"missing {required}");}
}
#[test]
fn workflow_orders_anchor_before_provenance_and_rechecks_before_execution(){
 let text=fs::read_to_string(root().join(".github/workflows/oreslang-exact-tree-carrier.yml")).unwrap();let source=&text[text.find("  verify-carrier:").unwrap()..];
 let checkout=source.find("- uses: actions/checkout@").unwrap();let anchor=source.find("Verify protected public external authorization").unwrap();let provenance=source.find("Verify exact-tree carrier provenance").unwrap();
 assert!(checkout<anchor&&anchor<provenance);assert!(!source.contains("repos/orestra/orestra-conformance/branches/main/protection"));
 let first=source.find("--bin verify-oreslang-external-anchor").unwrap();let second=source[first+1..].find("--bin verify-oreslang-external-anchor").unwrap()+first+1;
 let execution=source.find("Execute immutable trusted test plan").unwrap();assert!(first<second&&second<execution);
}
#[test]
fn governance_changes_trigger_validation_and_repository_python_is_not_executed(){
 let text=fs::read_to_string(root().join(".github/workflows/oreslang-exact-tree-carrier.yml")).unwrap();
 for required in ["- '.github/CODEOWNERS'","- 'scripts/test-carrier-governance-policy.rs'","- 'scripts/verify-oreslang-external-anchor.rs'","- 'scripts/test-oreslang-external-anchor.rs'","- 'tools/trusted-plan/**'"]{assert!(text.contains(required));}
 assert!(!text.contains("python3 scripts/"),"carrier workflow must not execute tracked Python tooling");
}
