use std::collections::{HashSet, VecDeque};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct Commit {
    writer: &'static str,
    claim: u8,
    actual: u8,
    owner: &'static str,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct State {
    epoch: u8,
    holder: Option<&'static str>,
    commits: Vec<Commit>,
}

fn main() {
    const WRITERS: [&str; 2] = ["alpha", "beta"];
    const MAX_EPOCH: u8 = 3;
    const MAX_COMMITS: usize = 2;

    let root = State { epoch: 0, holder: None, commits: Vec::new() };
    let mut pending = VecDeque::from([root.clone()]);
    let mut seen = HashSet::from([root]);

    while let Some(state) = pending.pop_front() {
        assert!(state.epoch <= MAX_EPOCH);
        for commit in &state.commits {
            assert_eq!(commit.writer, commit.owner);
            assert_eq!(commit.claim, commit.actual);
        }

        let mut candidates = Vec::new();
        if state.epoch < MAX_EPOCH {
            for writer in WRITERS {
                candidates.push(State {
                    epoch: state.epoch + 1,
                    holder: Some(writer),
                    commits: state.commits.clone(),
                });
            }
        }
        if state.commits.len() < MAX_COMMITS {
            if let Some(holder) = state.holder {
                let mut commits = state.commits.clone();
                commits.push(Commit {
                    writer: holder,
                    claim: state.epoch,
                    actual: state.epoch,
                    owner: holder,
                });
                candidates.push(State { epoch: state.epoch, holder: Some(holder), commits });
            }
        }
        for candidate in candidates {
            if seen.insert(candidate.clone()) {
                pending.push_back(candidate);
            }
        }
    }

    assert!(seen.len() >= 10);
    println!("explored {} finite epoch/commit states", seen.len());
}
