use orestra_state_oracle::quorum::{QuorumCluster, QuorumError};

#[test]
fn minority_cannot_elect_or_commit() {
    let mut c = QuorumCluster::three();
    assert!(matches!(c.elect("a", &["a"]), Err(QuorumError::NoQuorum(_))));
    let term = c.elect("a", &["a", "b"]).unwrap();
    assert!(matches!(
        c.propose("a", term, "x", &["a"]),
        Err(QuorumError::NoQuorum(_))
    ));
    assert!(c.committed_values().unwrap().is_empty());
}

#[test]
fn majority_commit_survives_leader_change() {
    let mut c = QuorumCluster::three();
    let t1 = c.elect("a", &["a", "b"]).unwrap();
    assert_eq!(c.propose("a", t1, "one", &["a", "b"]).unwrap(), 1);
    assert_eq!(c.committed_values().unwrap(), ["one"]);
    let t2 = c.elect("b", &["b", "c"]).unwrap();
    assert!(t2 > t1);
    assert_eq!(c.committed_values().unwrap(), ["one"]);
    assert_eq!(c.propose("b", t2, "two", &["b", "c"]).unwrap(), 2);
    assert_eq!(c.committed_values().unwrap(), ["one", "two"]);
}

#[test]
fn stale_leader_is_fenced_after_new_term() {
    let mut c = QuorumCluster::three();
    let old = c.elect("a", &["a", "b"]).unwrap();
    c.propose("a", old, "one", &["a", "b"]).unwrap();
    let new = c.elect("c", &["b", "c"]).unwrap();
    assert!(matches!(
        c.propose("a", old, "stale", &["a", "b"]),
        Err(QuorumError::NotLeader)
    ));
    c.propose("c", new, "two", &["b", "c"]).unwrap();
    assert_eq!(c.committed_values().unwrap(), ["one", "two"]);
}

#[test]
fn committed_prefix_never_changes() {
    let mut c = QuorumCluster::three();
    let t1 = c.elect("a", &["a", "b"]).unwrap();
    c.propose("a", t1, "v1", &["a", "b"]).unwrap();
    let t2 = c.elect("b", &["b", "c"]).unwrap();
    c.propose("b", t2, "v2", &["b", "c"]).unwrap();
    let t3 = c.elect("a", &["a", "c"]).unwrap();
    c.propose("a", t3, "v3", &["a", "c"]).unwrap();
    assert_eq!(c.committed_values().unwrap(), ["v1", "v2", "v3"]);
    c.assert_invariants().unwrap();
}
