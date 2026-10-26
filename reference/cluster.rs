use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub cpu: i64,
    pub memory: i64,
    pub ready: bool,
    pub generation: u64,
    pub agent_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deployment {
    pub name: String,
    pub replicas: usize,
    pub cpu: i64,
    pub memory: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub uid: u64,
    pub owner: String,
    pub node: String,
    pub cpu: i64,
    pub memory: i64,
    pub phase: String,
    pub generation: u64,
    pub node_generation: u64,
    pub agent_epoch: u64,
}

#[derive(Debug, Default)]
pub struct Cluster {
    pub nodes: BTreeMap<String, Node>,
    pub deployments: BTreeMap<String, Deployment>,
    pub instances: BTreeMap<u64, Instance>,
    next_uid: u64,
    pub epoch: u64,
}

impl Cluster {
    pub fn new() -> Self {
        Self { next_uid: 1, ..Self::default() }
    }

    pub fn add_node(&mut self, name: &str, cpu: i64, memory: i64) -> Result<(), String> {
        if name.is_empty() || self.nodes.contains_key(name) || cpu <= 0 || memory <= 0 {
            return Err("invalid node".into());
        }
        self.nodes.insert(name.to_string(), Node {
            name: name.to_string(), cpu, memory, ready: true, generation: 1, agent_epoch: 1,
        });
        Ok(())
    }

    pub fn deploy(&mut self, name: &str, replicas: usize, cpu: i64, memory: i64) -> Result<(), String> {
        if name.is_empty() || cpu <= 0 || memory <= 0 {
            return Err("invalid deployment".into());
        }
        self.deployments.insert(name.to_string(), Deployment {
            name: name.to_string(), replicas, cpu, memory,
        });
        Ok(())
    }

    pub fn set_ready(&mut self, name: &str, ready: bool) -> Result<(), String> {
        let node = self.nodes.get_mut(name).ok_or_else(|| "unknown node".to_string())?;
        node.ready = ready;
        if !ready {
            for instance in self.instances.values_mut() {
                if instance.node == name && matches!(instance.phase.as_str(), "Assigned" | "Running") {
                    instance.phase = "Unknown".into();
                }
            }
        }
        Ok(())
    }

    pub fn restart_agent(&mut self, name: &str) -> Result<u64, String> {
        let node = self.nodes.get_mut(name).ok_or_else(|| "unknown node".to_string())?;
        node.agent_epoch = node.agent_epoch.checked_add(1).ok_or("agent epoch exhausted")?;
        for instance in self.instances.values_mut() {
            if instance.node == name && matches!(instance.phase.as_str(), "Assigned" | "Running") {
                instance.phase = "Unknown".into();
            }
        }
        Ok(node.agent_epoch)
    }

    pub fn agent_start(&mut self, uid: u64, agent_epoch: u64, process_observed: bool) -> Result<(), String> {
        let node_name = self.instances.get(&uid).ok_or("unknown instance")?.node.clone();
        let node = self.nodes.get(&node_name).ok_or("unknown node")?.clone();
        let instance = self.instances.get_mut(&uid).ok_or("unknown instance")?;
        if agent_epoch != node.agent_epoch || instance.agent_epoch != agent_epoch {
            return Err("stale node-agent epoch".into());
        }
        if instance.node_generation != node.generation {
            return Err("stale node generation".into());
        }
        if !node.ready || instance.phase != "Assigned" {
            return Err("workload is not startable".into());
        }
        if !process_observed {
            return Err("runtime process was not observed".into());
        }
        instance.phase = "Running".into();
        Ok(())
    }

    pub fn start_assigned(&mut self) -> usize {
        let uids: Vec<u64> = self.instances.values()
            .filter(|i| i.phase == "Assigned")
            .map(|i| i.uid).collect();
        let mut started = 0;
        for uid in uids {
            let node_name = self.instances[&uid].node.clone();
            let Some(node) = self.nodes.get(&node_name) else { continue };
            if !node.ready { continue; }
            let epoch = node.agent_epoch;
            if self.agent_start(uid, epoch, true).is_ok() { started += 1; }
        }
        started
    }

    pub fn agent_confirm_absent(&mut self, uid: u64, agent_epoch: u64) -> Result<(), String> {
        let node_name = self.instances.get(&uid).ok_or("unknown instance")?.node.clone();
        let node = self.nodes.get(&node_name).ok_or("unknown node")?;
        if agent_epoch != node.agent_epoch {
            return Err("stale node-agent epoch".into());
        }
        let instance = self.instances.get_mut(&uid).ok_or("unknown instance")?;
        if !node.ready || !matches!(instance.phase.as_str(), "Unknown" | "Terminating") {
            return Err("absence proof is not authoritative".into());
        }
        instance.phase = "Stopped".into();
        Ok(())
    }

    pub fn confirm_fenced(&mut self, name: &str, fence_proven: bool) -> Result<usize, String> {
        let node = self.nodes.get(name).ok_or("unknown node")?;
        if !fence_proven || node.ready {
            return Err("physical fence not established".into());
        }
        let mut stopped = 0;
        for instance in self.instances.values_mut() {
            if instance.node == name && matches!(instance.phase.as_str(), "Unknown" | "Terminating") {
                instance.phase = "Stopped".into();
                stopped += 1;
            }
        }
        Ok(stopped)
    }

    pub fn takeover(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    pub fn active(&self, owner: Option<&str>) -> Vec<&Instance> {
        self.instances.values().filter(|i| i.phase == "Running" && owner.is_none_or(|o| i.owner == o)).collect()
    }

    pub fn assigned(&self, owner: Option<&str>) -> Vec<&Instance> {
        self.instances.values().filter(|i| i.phase == "Assigned" && owner.is_none_or(|o| i.owner == o)).collect()
    }

    pub fn claims(&self, owner: Option<&str>) -> Vec<&Instance> {
        self.instances.values().filter(|i| matches!(i.phase.as_str(), "Assigned" | "Running" | "Unknown" | "Terminating") && owner.is_none_or(|o| i.owner == o)).collect()
    }

    pub fn usage(&self, node: &str) -> (i64, i64) {
        self.instances.values()
            .filter(|i| i.node == node && matches!(i.phase.as_str(), "Assigned" | "Running" | "Unknown" | "Terminating"))
            .fold((0,0), |(cpu,mem), i| (cpu + i.cpu, mem + i.memory))
    }

    pub fn confirm_stopped(&mut self, uid: u64, stopped_proven: bool) -> Result<(), String> {
        if !stopped_proven { return Err("runtime termination is not proven".into()); }
        let instance = self.instances.get_mut(&uid).ok_or("unknown instance")?;
        if instance.phase != "Terminating" { return Err("instance is not terminating".into()); }
        instance.phase = "Stopped".into();
        Ok(())
    }

    fn fit_nodes(&self, deployment: &Deployment) -> Vec<(i64, String)> {
        let mut candidates = Vec::new();
        for node in self.nodes.values() {
            if !node.ready { continue; }
            let (used_cpu, used_memory) = self.usage(&node.name);
            if deployment.cpu <= node.cpu - used_cpu && deployment.memory <= node.memory - used_memory {
                let score = (used_cpu + deployment.cpu) * 1000 / node.cpu
                    + (used_memory + deployment.memory) * 1000 / node.memory;
                candidates.push((-score, node.name.clone()));
            }
        }
        candidates.sort();
        candidates
    }

    pub fn reconcile(&mut self, epoch: u64) -> Result<(), String> {
        if epoch != self.epoch || epoch == 0 { return Err("fenced scheduler".into()); }
        let names: Vec<String> = self.deployments.keys().cloned().collect();
        for name in names {
            let deployment = self.deployments[&name].clone();
            let mut desired_claims: Vec<u64> = self.instances.values()
                .filter(|i| i.owner == deployment.name && matches!(i.phase.as_str(), "Assigned" | "Running" | "Unknown"))
                .map(|i| i.uid).collect();
            desired_claims.sort();

            let mut excess = desired_claims.len().saturating_sub(deployment.replicas);
            for uid in desired_claims.iter().rev().copied() {
                if excess == 0 { break; }
                let instance = self.instances.get_mut(&uid).unwrap();
                if matches!(instance.phase.as_str(), "Assigned" | "Running") {
                    instance.phase = "Terminating".into();
                    excess -= 1;
                }
            }

            let remaining = self.instances.values()
                .filter(|i| i.owner == deployment.name && matches!(i.phase.as_str(), "Assigned" | "Running" | "Unknown"))
                .count();
            for _ in 0..deployment.replicas.saturating_sub(remaining) {
                let choices = self.fit_nodes(&deployment);
                let Some((_, node_name)) = choices.first() else { break };
                let node = self.nodes[node_name].clone();
                let uid = self.next_uid;
                self.next_uid += 1;
                self.instances.insert(uid, Instance {
                    uid, owner: deployment.name.clone(), node: node_name.clone(),
                    cpu: deployment.cpu, memory: deployment.memory, phase: "Assigned".into(),
                    generation: 1, node_generation: node.generation, agent_epoch: node.agent_epoch,
                });
            }
        }
        self.assert_invariants()
    }

    pub fn assert_invariants(&self) -> Result<(), String> {
        let uids: BTreeSet<_> = self.instances.keys().copied().collect();
        if uids.len() != self.instances.len() { return Err("duplicate instance uid".into()); }
        for node in self.nodes.values() {
            let (cpu, memory) = self.usage(&node.name);
            if cpu < 0 || cpu > node.cpu || memory < 0 || memory > node.memory {
                return Err(format!("capacity invariant failed for {}", node.name));
            }
        }
        for deployment in self.deployments.values() {
            let known = self.instances.values()
                .filter(|i| i.owner == deployment.name && matches!(i.phase.as_str(), "Assigned" | "Running"))
                .count();
            if known > deployment.replicas { return Err("known claims exceed desired replicas".into()); }
        }
        Ok(())
    }
}
