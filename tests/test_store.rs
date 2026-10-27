use orestra_state_oracle::store::{Lease, MemoryStore, StoreError};
use serde_json::{json, Value};
use std::sync::{Arc, Barrier};
use std::thread;

fn setup() -> (MemoryStore, Lease) {
    let store = MemoryStore::new();
    let lease = store.elect("cluster", "controller-a", 10, 20).unwrap();
    (store, lease)
}

fn put(
    store: &MemoryStore,
    lease: &Lease,
    key: &str,
    value: Value,
    expected: Option<u64>,
    tick: i64,
) -> Result<u64, StoreError> {
    store.put(key, value, expected, "cluster", lease, tick)
}

#[test]
fn create_and_cas() {
    let (store, lease) = setup();
    let first = put(&store, &lease, "default/Workload/a", json!({"value":1}), None, 10).unwrap();
    assert_eq!(first, 1);
    assert!(matches!(
        put(&store, &lease, "default/Workload/a", json!({"value":2}), None, 10),
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        put(&store, &lease, "default/Workload/a", json!({"value":2}), Some(999), 10),
        Err(StoreError::Conflict(_))
    ));
    let second = put(&store, &lease, "default/Workload/a", json!({"value":2}), Some(first), 10).unwrap();
    assert_eq!(second, 2);
    assert_eq!(store.get("default/Workload/a"), Some((2, json!({"value":2}))));
}

#[test]
fn copy_on_read_and_write() {
    let (store, lease) = setup();
    let mut item = json!({"labels":["safe"]});
    put(&store, &lease, "a", item.clone(), None, 10).unwrap();
    item["labels"].as_array_mut().unwrap().push(json!("bad"));
    let mut got = store.get("a").unwrap().1;
    got["labels"].as_array_mut().unwrap().push(json!("bad"));
    assert_eq!(store.get("a").unwrap().1, json!({"labels":["safe"]}));
}

#[test]
fn atomic_list_then_watch_and_compaction() {
    let (store, lease) = setup();
    put(&store, &lease, "a", json!({"v":1}), None, 10).unwrap();
    let (snapshot, rows) = store.list("");
    assert_eq!(rows.len(), 1);
    put(&store, &lease, "b", json!({"v":2}), None, 10).unwrap();
    let events = store.watch_after(snapshot, "").unwrap();
    assert_eq!(events.iter().map(|e| (e.revision, e.key.as_str())).collect::<Vec<_>>(), [(2, "b")]);

    store.compact_through(1).unwrap();
    assert!(matches!(store.watch_after(0, ""), Err(StoreError::ResetRequired(_))));
    assert_eq!(
        store.watch_after(1, "").unwrap().iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
        ["b"]
    );
}

#[test]
fn stale_expired_and_renewed_epochs_are_fenced() {
    let (store, lease) = setup();
    assert!(matches!(
        store.elect("cluster", "controller-b", 15, 20),
        Err(StoreError::Fenced(_))
    ));
    let newer = store.elect("cluster", "controller-b", 31, 20).unwrap();
    assert_eq!(newer.epoch, lease.epoch + 1);
    assert!(matches!(
        put(&store, &lease, "a", json!(1), None, 31),
        Err(StoreError::Fenced(_))
    ));
    assert!(matches!(
        put(&store, &newer, "a", json!(1), None, 51),
        Err(StoreError::Fenced(_))
    ));
    assert_eq!(put(&store, &newer, "a", json!(1), None, 32).unwrap(), 1);

    let (renew_store, old) = setup();
    let renewed = renew_store.elect("cluster", "controller-a", 12, 40).unwrap();
    assert_eq!(renewed.epoch, old.epoch);
    assert!(matches!(
        put(&renew_store, &old, "a", json!(1), None, 13),
        Err(StoreError::Fenced(_))
    ));
    assert_eq!(put(&renew_store, &renewed, "a", json!(1), None, 13).unwrap(), 1);
}

#[test]
fn delete_versions_and_events() {
    let (store, lease) = setup();
    let rev = put(&store, &lease, "a", json!({"x":2}), None, 10).unwrap();
    assert!(matches!(
        store.delete("a", 0, "cluster", &lease, 11),
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(store.delete("a", rev, "cluster", &lease, 11).unwrap(), 2);
    assert_eq!(store.get("a"), None);
    assert_eq!(store.watch_after(1, "").unwrap()[0].event_type, "DELETED");
}

fn seed_node_and_workloads(store: &MemoryStore, lease: &Lease) {
    put(
        store,
        lease,
        "Node/system/n1",
        json!({
            "metadata":{"uid":"node-uid"},
            "spec":{"capacity":{"cpuMillis":1000,"memoryBytes":1000}},
            "status":{"phase":"Ready"}
        }),
        None,
        10,
    )
    .unwrap();
    for name in ["a", "b", "c"] {
        put(
            store,
            lease,
            &format!("Workload/default/{name}"),
            json!({"metadata":{"uid":format!("uid-{name}")},"spec":{},"status":{"phase":"Pending"}}),
            None,
            10,
        )
        .unwrap();
    }
}

#[test]
fn reservation_is_one_revision_and_prevents_overcommit() {
    let (store, lease) = setup();
    seed_node_and_workloads(&store, &lease);
    let snapshot = store.revision();
    let a = store.get("Workload/default/a").unwrap().0;
    let node_version = store.get("Node/system/n1").unwrap().0;
    let revision = store
        .reserve(
            "Workload/default/a", a, "uid-a", "Node/system/n1", "node-uid",
            node_version, "Reservation/default/a", 600, 600, "cluster", &lease, 12,
        )
        .unwrap();
    assert_eq!(revision, snapshot + 1);
    let events = store.watch_after(snapshot, "").unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| event.revision == revision));

    let before = store.revision();
    let b = store.get("Workload/default/b").unwrap().0;
    assert!(matches!(
        store.reserve(
            "Workload/default/b", b, "uid-b", "Node/system/n1", "node-uid",
            node_version, "Reservation/default/b", 600, 600, "cluster", &lease, 12,
        ),
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(store.revision(), before);
    assert_eq!(store.get("Reservation/default/b"), None);

    store
        .reserve(
            "Workload/default/b", b, "uid-b", "Node/system/n1", "node-uid",
            node_version, "Reservation/default/b", 400, 400, "cluster", &lease, 12,
        )
        .unwrap();
    assert!(matches!(
        store.reserve(
            "Workload/default/a", revision, "uid-a", "Node/system/n1", "node-uid",
            node_version, "Reservation/default/a", 1, 1, "cluster", &lease, 12,
        ),
        Err(StoreError::Conflict(_))
    ));
}

#[test]
fn reservation_rejects_invalid_identity_version_capacity_and_stale_leadership() {
    let (store, lease) = setup();
    let node_v = put(
        &store,
        &lease,
        "Node/system/a",
        json!({
            "metadata":{"uid":"node-1"},
            "spec":{"capacity":{"cpuMillis":100,"memoryBytes":100}},
            "status":{"phase":"Ready"}
        }),
        None,
        10,
    )
    .unwrap();
    let work_v = put(
        &store,
        &lease,
        "Workload/default/a",
        json!({"metadata":{"uid":"work-1"},"spec":{},"status":{"phase":"Pending"}}),
        None,
        10,
    )
    .unwrap();
    let before = store.revision();
    assert!(matches!(
        store.reserve(
            "Workload/default/a", 0, "work-1", "Node/system/a", "node-1", node_v,
            "Reservation/default/a", 25, 25, "cluster", &lease, 11,
        ),
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store.reserve(
            "Workload/default/a", work_v, "work-1", "Node/system/a", "node-1", node_v,
            "hidden-reservation", 25, 25, "cluster", &lease, 11,
        ),
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store.reserve(
            "Workload/default/a", work_v, "work-1", "Node/system/a", "wrong", node_v,
            "Reservation/default/a", 25, 25, "cluster", &lease, 11,
        ),
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.reserve(
            "Workload/default/a", work_v, "work-1", "Node/system/a", "node-1", node_v + 1,
            "Reservation/default/a", 25, 25, "cluster", &lease, 11,
        ),
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.reserve(
            "Workload/default/a", work_v, "work-1", "Node/system/a", "node-1", node_v,
            "Reservation/default/a", 25, 25, "cluster", &lease, 30,
        ),
        Err(StoreError::Fenced(_))
    ));
    assert_eq!(store.revision(), before);

    let bad_node = put(
        &store,
        &lease,
        "Node/system/bad",
        json!({
            "metadata":{"uid":"node-bad"},
            "spec":{"capacity":{"cpuMillis":"100","memoryBytes":100}},
            "status":{"phase":"Ready"}
        }),
        None,
        11,
    )
    .unwrap();
    let bad_work = put(
        &store,
        &lease,
        "Workload/default/bad",
        json!({"metadata":{"uid":"work-bad"},"spec":{},"status":{"phase":"Pending"}}),
        None,
        11,
    )
    .unwrap();
    let error = store
        .reserve(
            "Workload/default/bad", bad_work, "work-bad", "Node/system/bad", "node-bad",
            bad_node, "Reservation/default/bad", 1, 1, "cluster", &lease, 11,
        )
        .unwrap_err();
    assert!(error.to_string().contains("invalid capacity"));
}

#[test]
fn reused_node_uid_is_not_allowed_to_hide_old_reservations() {
    let (store, lease) = setup();
    let node_key = "Node/system/n";
    let node_v = put(
        &store,
        &lease,
        node_key,
        json!({
            "metadata":{"uid":"n1"},
            "spec":{"capacity":{"cpuMillis":100,"memoryBytes":100}},
            "status":{"phase":"Ready"}
        }),
        None,
        10,
    )
    .unwrap();
    let a_v = put(
        &store,
        &lease,
        "Workload/default/a",
        json!({"metadata":{"uid":"a"},"spec":{},"status":{"phase":"Pending"}}),
        None,
        10,
    )
    .unwrap();
    store
        .reserve(
            "Workload/default/a", a_v, "a", node_key, "n1", node_v,
            "Reservation/default/a", 70, 70, "cluster", &lease, 11,
        )
        .unwrap();

    let node_next = put(
        &store,
        &lease,
        node_key,
        json!({
            "metadata":{"uid":"n2"},
            "spec":{"capacity":{"cpuMillis":100,"memoryBytes":100}},
            "status":{"phase":"Ready"}
        }),
        Some(node_v),
        12,
    )
    .unwrap();
    let b_v = put(
        &store,
        &lease,
        "Workload/default/b",
        json!({"metadata":{"uid":"b"},"spec":{},"status":{"phase":"Pending"}}),
        None,
        12,
    )
    .unwrap();
    let error = store
        .reserve(
            "Workload/default/b", b_v, "b", node_key, "n2", node_next,
            "Reservation/default/b", 20, 20, "cluster", &lease, 12,
        )
        .unwrap_err();
    assert!(error.to_string().contains("old node incarnation"));
}

#[test]
fn resume_mid_transaction_keeps_remaining_event() {
    let (store, lease) = setup();
    let node_v = put(
        &store,
        &lease,
        "Node/system/n",
        json!({
            "metadata":{"uid":"node"},
            "spec":{"capacity":{"cpuMillis":100,"memoryBytes":100}},
            "status":{"phase":"Ready"}
        }),
        None,
        10,
    )
    .unwrap();
    let work_v = put(
        &store,
        &lease,
        "Workload/default/w",
        json!({"metadata":{"uid":"w"},"spec":{},"status":{"phase":"Pending"}}),
        None,
        10,
    )
    .unwrap();
    let snapshot = store.revision();
    store
        .reserve(
            "Workload/default/w", work_v, "w", "Node/system/n", "node", node_v,
            "Reservation/default/w", 50, 50, "cluster", &lease, 12,
        )
        .unwrap();
    let events = store.watch_after(snapshot, "").unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].revision, events[1].revision);
    assert_ne!(events[0].cursor(), events[1].cursor());
    assert_eq!(
        store.watch_resume(&events[0].cursor(), "").unwrap()[0].key,
        "Reservation/default/w"
    );
    assert!(store.watch_resume(&events[1].cursor(), "").unwrap().is_empty());
    assert!(matches!(store.watch_resume("bogus", ""), Err(StoreError::ResetRequired(_))));
    assert!(matches!(
        store.watch_resume(&format!("{}:99", events[0].revision), ""),
        Err(StoreError::ResetRequired(_))
    ));
    store.compact_through(events[0].revision).unwrap();
    assert!(matches!(
        store.watch_resume(&events[0].cursor(), ""),
        Err(StoreError::ResetRequired(_))
    ));
}

#[test]
fn concurrent_reservations_serialize_at_authority() {
    let store = Arc::new(MemoryStore::new());
    let lease = store.elect("cluster", "controller-a", 10, 20).unwrap();
    let node_v = put(
        &store,
        &lease,
        "Node/system/n",
        json!({
            "metadata":{"uid":"n"},
            "spec":{"capacity":{"cpuMillis":1000,"memoryBytes":1000}},
            "status":{"phase":"Ready"}
        }),
        None,
        10,
    )
    .unwrap();
    let mut versions = Vec::new();
    for i in 0..8 {
        versions.push(
            put(
                &store,
                &lease,
                &format!("Workload/default/w{i}"),
                json!({"metadata":{"uid":format!("w{i}")},"spec":{},"status":{"phase":"Pending"}}),
                None,
                10,
            )
            .unwrap(),
        );
    }

    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let lease = lease.clone();
            let version = versions[i];
            thread::spawn(move || {
                barrier.wait();
                store
                    .reserve(
                        &format!("Workload/default/w{i}"), version, &format!("w{i}"),
                        "Node/system/n", "n", node_v, &format!("Reservation/default/w{i}"),
                        400, 400, "cluster", &lease, 12,
                    )
                    .is_ok()
            })
        })
        .collect();

    let accepted = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(|accepted| *accepted)
        .count();
    assert_eq!(accepted, 2);
    let (_, reservations) = store.list("Reservation/");
    assert_eq!(reservations.len(), 2);
    assert_eq!(
        reservations.iter().map(|(_, _, v)| v["spec"]["cpuMillis"].as_i64().unwrap()).sum::<i64>(),
        800
    );
}

#[test]
fn lease_renewal_never_rewinds_expiry_and_overflow_fails_closed() {
    let store = MemoryStore::new();
    let first = store.elect("cluster", "controller-a", 10, 100).unwrap();
    assert_eq!(first.expires_tick, 110);

    let renewed = store.elect("cluster", "controller-a", 20, 5).unwrap();
    assert_eq!(renewed.epoch, first.epoch);
    assert_eq!(renewed.expires_tick, 110);

    assert!(matches!(
        store.elect("overflow", "controller-a", i64::MAX, 1),
        Err(StoreError::Invalid(_))
    ));
}
