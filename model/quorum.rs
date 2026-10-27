use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub term: u64,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct Replica {
    pub name: String,
    pub term: u64,
    pub log: Vec<Entry>,
    pub commit_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuorumError {
    Invalid(&'static str),
    NotLeader,
    NoQuorum(&'static str),
    Invariant(&'static str),
}

impl fmt::Display for QuorumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) | Self::NoQuorum(message) | Self::Invariant(message) => write!(f, "{message}"),
            Self::NotLeader => write!(f, "stale leader or term"),
        }
    }
}
impl std::error::Error for QuorumError {}

pub struct QuorumCluster {
    pub replicas: BTreeMap<String, Replica>,
    pub term: u64,
    pub leader: Option<String>,
}

impl QuorumCluster {
    pub fn new(names: &[&str]) -> Result<Self, QuorumError> {
        if names.len() < 3 || names.len() % 2 == 0 {
            return Err(QuorumError::Invalid("oracle requires odd cluster size >= 3"));
        }
        let replicas = names
            .iter()
            .map(|name| {
                (
                    (*name).to_string(),
                    Replica {
                        name: (*name).to_string(),
                        term: 0,
                        log: Vec::new(),
                        commit_index: 0,
                    },
                )
            })
            .collect();
        Ok(Self { replicas, term: 0, leader: None })
    }

    pub fn three() -> Self {
        Self::new(&["a", "b", "c"]).unwrap()
    }

    pub fn majority(&self) -> usize {
        self.replicas.len() / 2 + 1
    }

    pub fn elect(&mut self, candidate: &str, voters: &[&str]) -> Result<u64, QuorumError> {
        let voters: BTreeSet<_> = voters.iter().copied().collect();
        if !self.replicas.contains_key(candidate) || !voters.contains(candidate) {
            return Err(QuorumError::Invalid("candidate must vote for itself"));
        }
        let valid_votes = voters.iter().filter(|name| self.replicas.contains_key(**name)).count();
        if valid_votes < self.majority() {
            return Err(QuorumError::NoQuorum("election lacks majority"));
        }
        self.term += 1;
        self.leader = Some(candidate.to_string());
        for replica in self.replicas.values_mut() {
            replica.term = replica.term.max(self.term);
        }
        self.repair_committed_prefix(candidate)?;
        Ok(self.term)
    }

    fn committed_prefix(&self) -> Result<Vec<Entry>, QuorumError> {
        let mut committed = Vec::new();
        let max_commit = self.replicas.values().map(|r| r.commit_index).max().unwrap_or(0);
        for index in 0..max_commit {
            let values: Vec<&Entry> = self
                .replicas
                .values()
                .filter(|replica| replica.commit_index > index && replica.log.len() > index)
                .map(|replica| &replica.log[index])
                .collect();
            if values.is_empty() {
                break;
            }
            let first = values[0];
            if values.iter().skip(1).any(|entry| *entry != first) {
                return Err(QuorumError::Invariant("committed-prefix divergence"));
            }
            committed.push(first.clone());
        }
        Ok(committed)
    }

    fn repair_committed_prefix(&mut self, leader: &str) -> Result<(), QuorumError> {
        let prefix = self.committed_prefix()?;
        let target = self.replicas.get_mut(leader).ok_or(QuorumError::Invalid("unknown leader"))?;
        let mismatched = target.log.len() < prefix.len()
            || prefix.iter().enumerate().any(|(i, entry)| target.log.get(i) != Some(entry));
        if mismatched {
            target.log = prefix.clone();
        }
        target.commit_index = target.commit_index.max(prefix.len());
        Ok(())
    }

    pub fn propose(
        &mut self,
        leader: &str,
        term: u64,
        value: &str,
        reachable: &[&str],
    ) -> Result<usize, QuorumError> {
        if self.leader.as_deref() != Some(leader) || term != self.term {
            return Err(QuorumError::NotLeader);
        }
        let reachable: BTreeSet<_> = reachable.iter().copied().collect();
        if !reachable.contains(leader) {
            return Err(QuorumError::Invalid("leader must reach itself"));
        }
        let valid = reachable.iter().filter(|name| self.replicas.contains_key(**name)).count();
        if valid < self.majority() {
            return Err(QuorumError::NoQuorum("proposal cannot commit without majority"));
        }

        let entry = Entry { term, value: value.to_string() };
        let index = self.replicas.get(leader).unwrap().log.len();
        self.replicas.get_mut(leader).unwrap().log.push(entry.clone());
        let mut acks = 1usize;
        for name in reachable.iter().copied() {
            if name == leader || !self.replicas.contains_key(name) {
                continue;
            }
            let follower = self.replicas.get_mut(name).unwrap();
            follower.log.truncate(index);
            follower.log.push(entry.clone());
            acks += 1;
        }
        if acks < self.majority() {
            return Err(QuorumError::NoQuorum("proposal lost quorum"));
        }

        let commit_index = index + 1;
        for name in reachable.iter().copied() {
            if let Some(replica) = self.replicas.get_mut(name) {
                if replica.log.len() >= commit_index {
                    replica.commit_index = replica.commit_index.max(commit_index);
                }
            }
        }
        self.assert_invariants()?;
        Ok(commit_index)
    }

    pub fn committed_values(&self) -> Result<Vec<String>, QuorumError> {
        Ok(self.committed_prefix()?.into_iter().map(|entry| entry.value).collect())
    }

    pub fn assert_invariants(&self) -> Result<(), QuorumError> {
        let prefix = self.committed_prefix()?;
        for replica in self.replicas.values() {
            if replica.term > self.term || replica.commit_index > replica.log.len() {
                return Err(QuorumError::Invariant("replica term/commit index invalid"));
            }
            for i in 0..replica.commit_index.min(prefix.len()) {
                if replica.log[i] != prefix[i] {
                    return Err(QuorumError::Invariant("replica committed prefix changed"));
                }
            }
        }
        Ok(())
    }
}
