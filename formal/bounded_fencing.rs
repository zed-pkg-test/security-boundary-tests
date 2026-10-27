//! Bounded exploration of epoch fencing, not TLC or native Oreslang execution.
use std::{collections::{HashSet, VecDeque}, env};

const MAX_EPOCH: u8 = 3;
const MAX_COMMITS: usize = 2;
const WRITERS: [&str; 2] = ["alpha", "beta"];
type Commit = (&'static str, u8, u8, &'static str);

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
struct State {
    epoch: u8,
    holder: Option<&'static str>,
    commits: Vec<Commit>,
}

impl State {
    fn root() -> Self {
        Self { epoch: 0, holder: None, commits: vec![] }
    }

    fn validate(&self) -> Result<(), &'static str> {
        if self.epoch > MAX_EPOCH || self.commits.len() > MAX_COMMITS {
            return Err("state exceeds finite-model bounds");
        }
        if let Some(holder) = self.holder {
            if !WRITERS.contains(&holder) {
                return Err("unrecognized epoch holder");
            }
        } else if self.epoch != 0 {
            return Err("nonzero epoch without a writer");
        }
        if self.commits.iter().any(|(writer, claim, actual, owner)|
            writer != owner || claim != actual || *actual > self.epoch
        ) {
            return Err("stale or unauthorized commit");
        }
        Ok(())
    }

    fn successors(&self) -> Vec<Self> {
        let mut next = Vec::new();
        if self.epoch < MAX_EPOCH {
            for holder in WRITERS {
                next.push(Self {
                    epoch: self.epoch + 1,
                    holder: Some(holder),
                    commits: self.commits.clone(),
                });
            }
        }
        if self.commits.len() < MAX_COMMITS {
            if let Some(holder) = self.holder {
                let mut commits = self.commits.clone();
                commits.push((holder, self.epoch, self.epoch, holder));
                next.push(Self { epoch: self.epoch, holder: Some(holder), commits });
            }
        }
        next
    }
}

fn explore() -> Result<usize, String> {
    let root = State::root();
    let mut pending = VecDeque::from([root.clone()]);
    let mut visited = HashSet::from([root]);
    while let Some(state) = pending.pop_front() {
        state.validate().map_err(|e| format!("{state:?}: {e}"))?;
        for next in state.successors() {
            next.validate().map_err(|e| format!("{next:?}: {e}"))?;
            if visited.insert(next.clone()) {
                pending.push_back(next);
            }
        }
    }
    if visited.len() < 10 {
        return Err(format!("too few reachable states: {}", visited.len()));
    }
    Ok(visited.len())
}

fn main() {
    if env::args_os().len() != 1 {
        eprintln!("usage: orestra-fencing-model"); std::process::exit(2);
    }
    match explore() {
        Ok(count) => println!("explored {count} finite epoch/commit states"),
        Err(error) => { eprintln!("fencing model DENIED: {error}"); std::process::exit(1); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reachable_state_exploration_is_not_empty() {
        assert!(explore().unwrap() >= 10);
    }

    #[test]
    fn rejects_stale_claim_or_wrong_commit_owner() {
        let bad = State {
            epoch: 2, holder: Some("beta"),
            commits: vec![("alpha", 1, 2, "beta")],
        };
        assert!(bad.validate().is_err());
        let bad_owner = State {
            epoch: 1, holder: Some("alpha"),
            commits: vec![("alpha", 1, 1, "beta")],
        };
        assert!(bad_owner.validate().is_err());
    }

    #[test]
    fn rejects_epoch_overflow_or_invalid_holder() {
        assert!(State { epoch: 4, holder: Some("alpha"), commits: vec![] }.validate().is_err());
        assert!(State { epoch: 1, holder: Some("mallory"), commits: vec![] }.validate().is_err());
    }

    #[test]
    fn no_successor_advances_past_epoch_bound() {
        let terminal = State { epoch: MAX_EPOCH, holder: Some("alpha"), commits: vec![] };
        assert!(terminal.successors().iter().all(|s| s.epoch == MAX_EPOCH));
    }
}
