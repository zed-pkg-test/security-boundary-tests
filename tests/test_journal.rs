use orestra_state_oracle::journal::{
    JournalError, JournaledStore, SyncStrategy,
};
use orestra_state_oracle::store::StoreError;
use serde_json::json;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::tempdir;

#[derive(Default)]
struct ToggleSync {
    fail_file: AtomicBool,
    fail_directory: AtomicBool,
}

impl SyncStrategy for ToggleSync {
    fn sync_file(&self, file: &File) -> io::Result<()> {
        if self.fail_file.load(Ordering::SeqCst) {
            Err(io::Error::other("disk failure"))
        } else {
            file.sync_all()
        }
    }

    fn sync_directory(&self, directory: &File) -> io::Result<()> {
        if self.fail_directory.load(Ordering::SeqCst) {
            Err(io::Error::other("directory sync failed"))
        } else {
            directory.sync_all()
        }
    }
}

#[test]
fn acknowledged_write_survives_reopen() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.wal");
    let store = JournaledStore::new(&path).unwrap();
    let lease = store.elect("cluster", "controller-a", 10, 20).unwrap();
    let revision = store
        .put(
            "Workload/default/a",
            json!({"metadata":{"uid":"a"}}),
            None,
            "cluster",
            &lease,
            11,
        )
        .unwrap();

    let reopened = JournaledStore::new(&path).unwrap();
    assert_eq!(reopened.revision().unwrap(), revision);
    let got = reopened.get("Workload/default/a").unwrap().unwrap();
    assert_eq!(got.0, revision);
    assert_eq!(got.1["metadata"]["uid"], "a");
    assert_eq!(got.1["metadata"]["resourceVersion"], revision.to_string());
}

#[test]
fn truncated_tail_recovers_last_complete_frame_and_is_removed_before_append() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.wal");
    let store = JournaledStore::new(&path).unwrap();
    let lease = store.elect("cluster", "leader", 10, 20).unwrap();
    store.put("a", json!({"v":1}), None, "cluster", &lease, 11).unwrap();

    let mut stream = OpenOptions::new().append(true).open(&path).unwrap();
    stream.write_all(br#"{"payload":{"version":1"#).unwrap();
    stream.sync_all().unwrap();
    drop(stream);

    let recovered = JournaledStore::new(&path).unwrap();
    assert_eq!(recovered.get("a").unwrap().unwrap().1, json!({"v":1}));
    recovered.put("b", json!(2), None, "cluster", &lease, 12).unwrap();

    let reopened = JournaledStore::new(&path).unwrap();
    assert_eq!(reopened.revision().unwrap(), 2);
    assert_eq!(reopened.get("a").unwrap().unwrap().1, json!({"v":1}));
    assert_eq!(reopened.get("b").unwrap().unwrap().1, json!(2));
}

#[test]
fn complete_corrupt_frame_fails_closed() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.wal");
    let store = JournaledStore::new(&path).unwrap();
    store.elect("cluster", "controller-a", 10, 20).unwrap();
    let mut stream = OpenOptions::new().append(true).open(&path).unwrap();
    stream
        .write_all(br#"{"payload":{"version":1,"state":{}},"sha256":"bad"}
"#)
        .unwrap();
    stream.sync_all().unwrap();
    drop(stream);

    assert!(matches!(
        JournaledStore::new(&path),
        Err(JournalError::Corrupt(_))
    ));
}

#[test]
fn failed_staging_sync_does_not_ack_or_mutate_state() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.wal");
    let initial = JournaledStore::new(&path).unwrap();
    let lease = initial.elect("cluster", "controller-a", 10, 20).unwrap();
    drop(initial);

    let sync = Arc::new(ToggleSync::default());
    let store = JournaledStore::with_sync_strategy(&path, sync.clone()).unwrap();
    sync.fail_file.store(true, Ordering::SeqCst);
    assert!(matches!(
        store.put("a", json!({"v":1}), None, "cluster", &lease, 11),
        Err(JournalError::Io(_))
    ));
    sync.fail_file.store(false, Ordering::SeqCst);
    assert_eq!(store.get("a").unwrap(), None);

    let reopened = JournaledStore::new(&path).unwrap();
    assert_eq!(reopened.get("a").unwrap(), None);
}

#[test]
fn atomic_reservation_watch_resume_and_compaction_survive_restart() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.wal");
    let store = JournaledStore::new(&path).unwrap();
    let lease = store.elect("cluster", "controller-a", 10, 20).unwrap();
    let node_version = store
        .put(
            "Node/system/n",
            json!({
                "metadata":{"uid":"node"},
                "spec":{"capacity":{"cpuMillis":1000,"memoryBytes":1000}},
                "status":{"phase":"Ready"}
            }),
            None,
            "cluster",
            &lease,
            11,
        )
        .unwrap();
    let work_version = store
        .put(
            "Workload/default/w",
            json!({"metadata":{"uid":"work"},"spec":{},"status":{"phase":"Pending"}}),
            None,
            "cluster",
            &lease,
            11,
        )
        .unwrap();
    let before = store.revision().unwrap();
    let committed = store
        .reserve(
            "Workload/default/w",
            work_version,
            "work",
            "Node/system/n",
            "node",
            node_version,
            "Reservation/default/w",
            400,
            400,
            "cluster",
            &lease,
            12,
        )
        .unwrap();
    let events = store.watch_after(before, "").unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| event.revision == committed));

    let reopened = JournaledStore::new(&path).unwrap();
    assert_eq!(reopened.revision().unwrap(), committed);
    assert_eq!(
        reopened.get("Workload/default/w").unwrap().unwrap().1["status"]["phase"],
        "Assigned"
    );
    assert!(reopened.get("Reservation/default/w").unwrap().is_some());
    let remaining = reopened.watch_resume(&events[0].cursor(), "").unwrap();
    assert_eq!(remaining.iter().map(|event| event.key.as_str()).collect::<Vec<_>>(), ["Reservation/default/w"]);

    reopened.compact_through(1).unwrap();
    let compacted = JournaledStore::new(&path).unwrap();
    assert!(matches!(
        compacted.watch_after(0, ""),
        Err(JournalError::Store(StoreError::ResetRequired(_)))
    ));
}

#[test]
fn two_open_stores_do_not_lose_committed_revisions() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.wal");
    let first = JournaledStore::new(&path).unwrap();
    let lease = first.elect("cluster", "leader", 10, 20).unwrap();
    let second = JournaledStore::new(&path).unwrap();

    first.put("a", json!(1), None, "cluster", &lease, 11).unwrap();
    second.put("b", json!(2), None, "cluster", &lease, 11).unwrap();

    assert_eq!(first.get("b").unwrap().unwrap().1, json!(2));
    assert_eq!(first.revision().unwrap(), 2);
    let current = JournaledStore::new(&path).unwrap();
    assert_eq!(current.revision().unwrap(), 2);
    assert_eq!(current.get("a").unwrap().unwrap().1, json!(1));
    assert_eq!(current.get("b").unwrap().unwrap().1, json!(2));
}

#[test]
fn valid_checksum_cannot_hide_revision_rewind() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.wal");
    let store = JournaledStore::new(&path).unwrap();
    let lease = store.elect("cluster", "leader", 10, 20).unwrap();
    store.put("a", json!(1), None, "cluster", &lease, 11).unwrap();

    let bytes = fs::read(&path).unwrap();
    let first_end = bytes.iter().position(|b| *b == b'\n').unwrap() + 1;
    let first_frame = bytes[..first_end].to_vec();
    let mut stream = OpenOptions::new().append(true).open(&path).unwrap();
    stream.write_all(&first_frame).unwrap();
    stream.sync_all().unwrap();
    drop(stream);

    let error = match JournaledStore::new(&path) {
        Ok(_) => panic!("revision rewind accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("non-monotonic"));
}

#[test]
fn uncertain_directory_sync_poisoned_until_reopen() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.wal");
    let initial = JournaledStore::new(&path).unwrap();
    let lease = initial.elect("cluster", "leader", 10, 20).unwrap();
    drop(initial);

    let sync = Arc::new(ToggleSync::default());
    let store = JournaledStore::with_sync_strategy(&path, sync.clone()).unwrap();
    sync.fail_directory.store(true, Ordering::SeqCst);
    assert!(matches!(
        store.put("a", json!(1), None, "cluster", &lease, 11),
        Err(JournalError::UncertainCommit)
    ));
    assert!(matches!(
        store.put("b", json!(2), None, "cluster", &lease, 11),
        Err(JournalError::UncertainCommit)
    ));
    assert!(matches!(store.get("a"), Err(JournalError::UncertainCommit)));
    assert!(matches!(store.revision(), Err(JournalError::UncertainCommit)));

    let reopened = JournaledStore::new(&path).unwrap();
    assert_eq!(reopened.get("a").unwrap().unwrap().1, json!(1));
}

#[test]
fn independent_stores_cannot_overcommit_same_node() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.wal");
    let first = Arc::new(JournaledStore::new(&path).unwrap());
    let lease = first.elect("cluster", "leader", 10, 30).unwrap();
    let node_version = first
        .put(
            "Node/system/n",
            json!({
                "metadata":{"uid":"node-1"},
                "spec":{"capacity":{"cpuMillis":1000,"memoryBytes":1000}},
                "status":{"phase":"Ready"}
            }),
            None,
            "cluster",
            &lease,
            11,
        )
        .unwrap();
    let a_version = first
        .put(
            "Workload/default/a",
            json!({"metadata":{"uid":"work-a"},"spec":{},"status":{"phase":"Pending"}}),
            None,
            "cluster",
            &lease,
            11,
        )
        .unwrap();
    let b_version = first
        .put(
            "Workload/default/b",
            json!({"metadata":{"uid":"work-b"},"spec":{},"status":{"phase":"Pending"}}),
            None,
            "cluster",
            &lease,
            11,
        )
        .unwrap();
    let second = Arc::new(JournaledStore::new(&path).unwrap());
    let barrier = Arc::new(Barrier::new(2));

    let launch = |store: Arc<JournaledStore>, name: &'static str, version: u64| {
        let barrier = barrier.clone();
        let lease = lease.clone();
        thread::spawn(move || {
            barrier.wait();
            store
                .reserve(
                    &format!("Workload/default/{name}"),
                    version,
                    &format!("work-{name}"),
                    "Node/system/n",
                    "node-1",
                    node_version,
                    &format!("Reservation/default/{name}"),
                    600,
                    600,
                    "cluster",
                    &lease,
                    12,
                )
                .is_ok()
        })
    };
    let a = launch(first.clone(), "a", a_version);
    let b = launch(second.clone(), "b", b_version);
    let accepted = [a.join().unwrap(), b.join().unwrap()]
        .into_iter()
        .filter(|value| *value)
        .count();
    assert_eq!(accepted, 1);

    let recovered = JournaledStore::new(&path).unwrap();
    assert_eq!(recovered.revision().unwrap(), 4);
    let (_, reservations) = recovered.list("Reservation/").unwrap();
    assert_eq!(reservations.len(), 1);
    assert_eq!(reservations[0].2["spec"]["cpuMillis"], 600);
}


#[test]
fn persisted_wal_disappearance_fails_closed() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.wal");
    let store = JournaledStore::new(&path).unwrap();
    let lease = store.elect("cluster", "leader", 10, 20).unwrap();
    store.put("a", json!(1), None, "cluster", &lease, 11).unwrap();

    fs::remove_file(&path).unwrap();

    let read_error = store.revision().unwrap_err();
    assert!(read_error
        .to_string()
        .contains("previously persisted WAL disappeared"));

    let write_error = store
        .put("b", json!(2), None, "cluster", &lease, 12)
        .unwrap_err();
    assert!(write_error
        .to_string()
        .contains("previously persisted WAL disappeared"));
}
