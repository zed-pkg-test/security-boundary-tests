use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

const MAX_KEY_BYTES: usize = 1024;
const MAX_OBJECT_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub holder: String,
    pub epoch: u64,
    pub expires_tick: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub revision: u64,
    pub event_type: String,
    pub key: String,
    pub object: Value,
    #[serde(default)]
    pub ordinal: u64,
}

impl Event {
    pub fn cursor(&self) -> String {
        format!("{}:{}", self.revision, self.ordinal)
    }
}

#[derive(Debug)]
pub enum StoreError {
    Invalid(&'static str),
    Conflict(&'static str),
    ResetRequired(&'static str),
    Fenced(&'static str),
    RevisionExhausted,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message)
            | Self::Conflict(message)
            | Self::ResetRequired(message)
            | Self::Fenced(message) => write!(f, "{message}"),
            Self::RevisionExhausted => write!(f, "revision exhausted"),
        }
    }
}
impl std::error::Error for StoreError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct StoreState {
    pub revision: u64,
    pub compacted: u64,
    pub objects: BTreeMap<String, (u64, Value)>,
    pub leases: BTreeMap<String, Lease>,
    #[serde(rename = "epochCounters")]
    pub epoch_counters: BTreeMap<String, u64>,
    pub events: Vec<Event>,
}

impl Default for StoreState {
    fn default() -> Self {
        Self {
            revision: 0,
            compacted: 0,
            objects: BTreeMap::new(),
            leases: BTreeMap::new(),
            epoch_counters: BTreeMap::new(),
            events: Vec::new(),
        }
    }
}

pub struct MemoryStore {
    state: Mutex<StoreState>,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self { state: Mutex::new(StoreState::default()) }
    }

    pub(crate) fn snapshot(&self) -> StoreState {
        self.state.lock().unwrap().clone()
    }

    pub(crate) fn restore(&self, state: StoreState) {
        *self.state.lock().unwrap() = state;
    }

    pub fn revision(&self) -> u64 {
        self.state.lock().unwrap().revision
    }

    pub fn elect(&self, domain: &str, holder: &str, now: i64, ttl: i64) -> Result<Lease, StoreError> {
        if domain.is_empty() || holder.is_empty() || ttl <= 0 || now < 0 {
            return Err(StoreError::Invalid("invalid lease request"));
        }
        let requested_expiry = now
            .checked_add(ttl)
            .ok_or(StoreError::Invalid("lease expiry overflow"))?;
        let mut state = self.state.lock().unwrap();
        if let Some(previous) = state.leases.get(domain).cloned() {
            if now < previous.expires_tick {
                if previous.holder != holder {
                    return Err(StoreError::Fenced("lease is owned by another candidate"));
                }
                let renewed = Lease {
                    holder: holder.to_string(),
                    epoch: previous.epoch,
                    expires_tick: previous.expires_tick.max(requested_expiry),
                };
                state.leases.insert(domain.to_string(), renewed.clone());
                return Ok(renewed);
            }
        }
        let epoch = state
            .epoch_counters
            .get(domain)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(StoreError::Invalid("lease epoch exhausted"))?;
        state.epoch_counters.insert(domain.to_string(), epoch);
        let lease = Lease { holder: holder.to_string(), epoch, expires_tick: requested_expiry };
        state.leases.insert(domain.to_string(), lease.clone());
        Ok(lease)
    }

    fn check_leader(state: &StoreState, domain: &str, lease: &Lease, now: i64) -> Result<(), StoreError> {
        match state.leases.get(domain) {
            Some(current) if current == lease && now < current.expires_tick => Ok(()),
            _ => Err(StoreError::Fenced("stale leadership epoch or expired lease")),
        }
    }

    pub fn get(&self, key: &str) -> Option<(u64, Value)> {
        self.state.lock().unwrap().objects.get(key).cloned()
    }

    pub fn list(&self, prefix: &str) -> (u64, Vec<(String, u64, Value)>) {
        let state = self.state.lock().unwrap();
        let rows = state.objects.iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, (revision, value))| (key.clone(), *revision, value.clone()))
            .collect();
        (state.revision, rows)
    }

    pub fn put(
        &self,
        key: &str,
        object: Value,
        expected: Option<u64>,
        domain: &str,
        lease: &Lease,
        now: i64,
    ) -> Result<u64, StoreError> {
        if key.is_empty() || key.len() > MAX_KEY_BYTES || now < 0 {
            return Err(StoreError::Invalid("invalid object key or tick"));
        }
        let encoded = serde_json::to_vec(&object)
            .map_err(|_| StoreError::Invalid("invalid object encoding"))?;
        if encoded.len() > MAX_OBJECT_BYTES {
            return Err(StoreError::Invalid("object exceeds size bound"));
        }
        let mut state = self.state.lock().unwrap();
        Self::check_leader(&state, domain, lease, now)?;
        let old = state.objects.get(key).cloned();
        if (old.is_some() && expected.is_none())
            || expected.is_some() && old.as_ref().map(|x| x.0) != expected
        {
            return Err(StoreError::Conflict("resource version mismatch"));
        }
        if state.revision == i64::MAX as u64 {
            return Err(StoreError::RevisionExhausted);
        }

        state.revision += 1;
        let revision = state.revision;
        let mut value = object;
        if let Some(metadata) = value
            .as_object_mut()
            .and_then(|root| root.get_mut("metadata"))
            .and_then(Value::as_object_mut)
        {
            metadata.insert("resourceVersion".into(), Value::String(revision.to_string()));
        }
        state.objects.insert(key.to_string(), (revision, value.clone()));
        state.events.push(Event {
            revision,
            event_type: if old.is_none() { "ADDED" } else { "MODIFIED" }.to_string(),
            key: key.to_string(),
            object: value,
            ordinal: 0,
        });
        Ok(revision)
    }

    pub fn delete(
        &self,
        key: &str,
        expected: u64,
        domain: &str,
        lease: &Lease,
        now: i64,
    ) -> Result<u64, StoreError> {
        if now < 0 {
            return Err(StoreError::Invalid("invalid tick"));
        }
        let mut state = self.state.lock().unwrap();
        Self::check_leader(&state, domain, lease, now)?;
        let Some(old) = state.objects.get(key).cloned() else {
            return Err(StoreError::Conflict("resource version mismatch"));
        };
        if old.0 != expected {
            return Err(StoreError::Conflict("resource version mismatch"));
        }
        if state.revision == i64::MAX as u64 {
            return Err(StoreError::RevisionExhausted);
        }
        state.revision += 1;
        let revision = state.revision;
        state.objects.remove(key);
        state.events.push(Event {
            revision,
            event_type: "DELETED".into(),
            key: key.to_string(),
            object: old.1,
            ordinal: 0,
        });
        Ok(revision)
    }

    pub fn watch_after(&self, revision: u64, prefix: &str) -> Result<Vec<Event>, StoreError> {
        let state = self.state.lock().unwrap();
        if revision < state.compacted || revision > state.revision {
            return Err(StoreError::ResetRequired("invalid/compacted watch cursor: relist"));
        }
        Ok(state.events.iter()
            .filter(|event| event.revision > revision && event.key.starts_with(prefix))
            .cloned()
            .collect())
    }

    pub fn watch_resume(&self, cursor: &str, prefix: &str) -> Result<Vec<Event>, StoreError> {
        let mut parts = cursor.split(':');
        let Some(rev_text) = parts.next() else {
            return Err(StoreError::ResetRequired("invalid event cursor"));
        };
        let Some(ord_text) = parts.next() else {
            return Err(StoreError::ResetRequired("invalid event cursor"));
        };
        if parts.next().is_some()
            || rev_text.is_empty()
            || ord_text.is_empty()
            || !rev_text.bytes().all(|b| b.is_ascii_digit())
            || !ord_text.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(StoreError::ResetRequired("invalid event cursor"));
        }
        let revision = rev_text.parse::<u64>()
            .map_err(|_| StoreError::ResetRequired("invalid event cursor"))?;
        let ordinal = ord_text.parse::<u64>()
            .map_err(|_| StoreError::ResetRequired("invalid event cursor"))?;

        let state = self.state.lock().unwrap();
        if revision <= state.compacted || revision > state.revision {
            return Err(StoreError::ResetRequired("stale event cursor; relist"));
        }
        if !state.events.iter().any(|event| event.revision == revision && event.ordinal == ordinal) {
            return Err(StoreError::ResetRequired("event cursor no longer exists"));
        }
        Ok(state.events.iter()
            .filter(|event| (event.revision, event.ordinal) > (revision, ordinal)
                && event.key.starts_with(prefix))
            .cloned()
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reserve(
        &self,
        workload_key: &str,
        workload_version: u64,
        workload_uid: &str,
        node_key: &str,
        node_uid: &str,
        node_version: u64,
        reservation_key: &str,
        cpu: i64,
        memory: i64,
        domain: &str,
        lease: &Lease,
        now: i64,
    ) -> Result<u64, StoreError> {
        if workload_key.is_empty()
            || workload_uid.is_empty()
            || node_key.is_empty()
            || node_uid.is_empty()
            || reservation_key.is_empty()
            || cpu <= 0
            || memory <= 0
            || workload_version == 0
            || node_version == 0
            || now < 0
        {
            return Err(StoreError::Invalid(
                "invalid reservation identity, version, quantities or tick",
            ));
        }

        let work_parts: Vec<_> = workload_key.split('/').collect();
        let node_parts: Vec<_> = node_key.split('/').collect();
        if work_parts.len() != 3
            || work_parts[0] != "Workload"
            || work_parts[1].is_empty()
            || work_parts[2].is_empty()
            || node_parts.len() != 3
            || node_parts[0] != "Node"
            || node_parts[1].is_empty()
            || node_parts[2].is_empty()
            || reservation_key != format!("Reservation/{}/{}", work_parts[1], work_parts[2])
        {
            return Err(StoreError::Invalid("reservation must use the workload's canonical key"));
        }

        let mut state = self.state.lock().unwrap();
        Self::check_leader(&state, domain, lease, now)?;

        let work = state.objects.get(workload_key).cloned();
        let node = state.objects.get(node_key).cloned();
        let (Some((work_rev, mut workload)), Some((node_rev, node_object))) = (work, node) else {
            return Err(StoreError::Conflict("stale workload or node version"));
        };
        if work_rev != workload_version || node_rev != node_version {
            return Err(StoreError::Conflict("stale workload or node version"));
        }
        if state.objects.contains_key(reservation_key) {
            return Err(StoreError::Conflict("duplicate reservation"));
        }

        if pointer_str(&workload, "/metadata/uid") != Some(workload_uid)
            || pointer_str(&node_object, "/metadata/uid") != Some(node_uid)
            || pointer_str(&workload, "/status/phase") != Some("Pending")
            || workload.pointer("/spec/nodeName").is_some_and(truthy)
        {
            return Err(StoreError::Conflict("binding identity, phase or UID mismatch"));
        }
        if pointer_str(&node_object, "/status/phase") != Some("Ready")
            || node_object.pointer("/spec/unschedulable").is_some_and(truthy)
        {
            return Err(StoreError::Conflict("node is not schedulable"));
        }

        let capacity = node_object
            .pointer("/status/allocatable")
            .filter(|value| !value.is_null())
            .or_else(|| node_object.pointer("/spec/capacity"))
            .and_then(Value::as_object)
            .ok_or(StoreError::Conflict("node has invalid capacity"))?;
        let cap_cpu = capacity.get("cpuMillis").and_then(Value::as_i64)
            .filter(|value| *value >= 0)
            .ok_or(StoreError::Conflict("node has invalid capacity"))?;
        let cap_mem = capacity.get("memoryBytes").and_then(Value::as_i64)
            .filter(|value| *value >= 0)
            .ok_or(StoreError::Conflict("node has invalid capacity"))?;

        let mut total_cpu = 0i64;
        let mut total_mem = 0i64;
        for (key, (_, object)) in &state.objects {
            if !key.starts_with("Reservation/") {
                continue;
            }
            let existing_node_key = pointer_str(object, "/spec/nodeKey");
            let existing_node_uid = pointer_str(object, "/spec/nodeUid");
            if existing_node_key == Some(node_key) && existing_node_uid != Some(node_uid) {
                return Err(StoreError::Conflict("old node incarnation has unresolved reservations"));
            }
            if existing_node_uid == Some(node_uid) {
                let reserved_cpu = object.pointer("/spec/cpuMillis").and_then(Value::as_i64)
                    .filter(|value| *value >= 0)
                    .ok_or(StoreError::Conflict("existing reservation has invalid capacity"))?;
                let reserved_mem = object.pointer("/spec/memoryBytes").and_then(Value::as_i64)
                    .filter(|value| *value >= 0)
                    .ok_or(StoreError::Conflict("existing reservation has invalid capacity"))?;
                total_cpu = total_cpu.checked_add(reserved_cpu)
                    .ok_or(StoreError::Conflict("existing reservation has invalid capacity"))?;
                total_mem = total_mem.checked_add(reserved_mem)
                    .ok_or(StoreError::Conflict("existing reservation has invalid capacity"))?;
            }
        }

        if total_cpu > cap_cpu
            || total_mem > cap_mem
            || cpu > cap_cpu - total_cpu
            || memory > cap_mem - total_mem
        {
            return Err(StoreError::Conflict("insufficient unreserved capacity"));
        }
        if state.revision >= i64::MAX as u64 {
            return Err(StoreError::RevisionExhausted);
        }
        let revision = state.revision + 1;

        ensure_object_path(&mut workload, "spec")
            .insert("nodeName".into(), Value::String(node_key.to_string()));
        ensure_object_path(&mut workload, "status")
            .insert("phase".into(), Value::String("Assigned".into()));
        ensure_object_path(&mut workload, "metadata")
            .insert("resourceVersion".into(), Value::String(revision.to_string()));

        let reservation = json!({
            "kind": "Reservation",
            "metadata": {
                "uid": reservation_key,
                "resourceVersion": revision.to_string()
            },
            "spec": {
                "workloadUid": workload_uid,
                "nodeUid": node_uid,
                "nodeKey": node_key,
                "nodeVersion": node_version,
                "cpuMillis": cpu,
                "memoryBytes": memory,
                "leaseEpoch": lease.epoch
            }
        });

        state.objects.insert(workload_key.to_string(), (revision, workload.clone()));
        state.objects.insert(reservation_key.to_string(), (revision, reservation.clone()));
        state.revision = revision;
        state.events.push(Event {
            revision,
            event_type: "MODIFIED".into(),
            key: workload_key.to_string(),
            object: workload,
            ordinal: 0,
        });
        state.events.push(Event {
            revision,
            event_type: "ADDED".into(),
            key: reservation_key.to_string(),
            object: reservation,
            ordinal: 1,
        });
        Ok(revision)
    }

    pub fn compact_through(&self, revision: u64) -> Result<(), StoreError> {
        let mut state = self.state.lock().unwrap();
        if revision < state.compacted || revision > state.revision {
            return Err(StoreError::Invalid("invalid compaction revision"));
        }
        state.compacted = revision;
        state.events.retain(|event| event.revision > revision);
        Ok(())
    }

    #[cfg(test)]
    pub fn force_revision_for_test(&self, revision: u64) {
        self.state.lock().unwrap().revision = revision;
    }
}

fn pointer_str<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_i64().map(|n| n != 0)
            .or_else(|| number.as_u64().map(|n| n != 0))
            .or_else(|| number.as_f64().map(|n| n != 0.0))
            .unwrap_or(false),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn ensure_object_path<'a>(value: &'a mut Value, key: &str) -> &'a mut serde_json::Map<String, Value> {
    let root = value.as_object_mut().expect("resource must be an object");
    let entry = root.entry(key.to_string()).or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    entry.as_object_mut().unwrap()
}
