use orestra_api_reference::http_api::{
    ApiStore, Authorizer, Principal, ReferenceServer,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

struct Harness {
    store: Arc<ApiStore>,
    auth: Arc<Authorizer>,
    server: Option<ReferenceServer>,
}

impl Harness {
    fn new() -> Self {
        let store = Arc::new(ApiStore::test_default(4));
        let auth = Arc::new(Authorizer::new(HashMap::from([
            (
                "admin".to_string(),
                Principal::new(&["default"], &["get", "list", "watch", "put", "delete"]),
            ),
            (
                "reader".to_string(),
                Principal::new(&["default"], &["get", "list", "watch"]),
            ),
            (
                "other".to_string(),
                Principal::new(&["other"], &["get", "list", "watch"]),
            ),
        ])));
        let server =
            ReferenceServer::start(Arc::clone(&store), Arc::clone(&auth), "127.0.0.1", 0)
                .unwrap();
        Self { store, auth, server: Some(server) }
    }

    fn port(&self) -> u16 {
        self.server.as_ref().unwrap().port
    }

    fn workload(&self, name: &str) -> Value {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut value: Value = serde_json::from_str(
            &fs::read_to_string(root.join("examples/workload.json")).unwrap(),
        )
        .unwrap();
        value["metadata"]["name"] = json!(name);
        value["metadata"]["uid"] = json!(format!("{name}-uid"));
        value["metadata"]["resourceVersion"] = json!("0");
        value
    }

    fn call(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&Value>,
        headers: &[(&str, &str)],
    ) -> (u16, Value) {
        call_port(self.port(), method, path, token, body, headers)
    }

    fn create(&self, name: &str) -> (u16, Value) {
        self.call(
            "PUT",
            &format!("/v1alpha1/default/workload/{name}"),
            Some("admin"),
            Some(&self.workload(name)),
            &[],
        )
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.shutdown();
        }
    }
}

fn call_port(
    port: u16,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&Value>,
    headers: &[(&str, &str)],
) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let body_text = body.map(serde_json::to_string).transpose().unwrap();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n"
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = &body_text {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", body.as_bytes().len()));
    }
    request.push_str("\r\n");
    if let Some(body) = &body_text {
        request.push_str(body);
    }
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let response = String::from_utf8(response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    let status = head
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let body: Value = serde_json::from_str(body).unwrap();
    (status, body)
}

#[test]
fn authentication_and_namespace_rbac() {
    let h = Harness::new();
    assert_eq!(h.call("GET", "/v1alpha1/default/workload", None, None, &[]).0, 401);
    assert_eq!(
        h.call("GET", "/v1alpha1/default/workload", Some("other"), None, &[]).0,
        403
    );
    assert_eq!(
        h.call("GET", "/v1alpha1/default/workload", Some("reader"), None, &[]).0,
        200
    );
    assert_eq!(
        h.call(
            "PUT",
            "/v1alpha1/default/workload/a",
            Some("reader"),
            Some(&h.workload("a")),
            &[],
        )
        .0,
        403
    );
}

#[test]
fn create_update_cas_and_server_owned_leader_fencing() {
    let h = Harness::new();
    let (code, created) = h.create("a");
    assert_eq!(code, 201);
    assert_eq!(created["revision"], "1");
    assert_eq!(h.create("a").0, 409);

    let mut update = created["object"].clone();
    update["metadata"]["generation"] = json!(2);
    assert_eq!(
        h.call(
            "PUT",
            "/v1alpha1/default/workload/a",
            Some("admin"),
            Some(&update),
            &[("If-Match", "999")],
        )
        .0,
        409
    );
    let (code, updated) = h.call(
        "PUT",
        "/v1alpha1/default/workload/a",
        Some("admin"),
        Some(&update),
        &[("If-Match", "1")],
    );
    assert_eq!(code, 200);
    assert_eq!(updated["revision"], "2");

    h.store.advance_leader_epoch(5).unwrap();
    assert_eq!(
        h.call(
            "DELETE",
            "/v1alpha1/default/workload/a",
            Some("admin"),
            None,
            &[("If-Match", "2")],
        )
        .0,
        409
    );
    assert!(h.store.get("default", "workload", "a").is_some());

    let successor =
        ReferenceServer::start(Arc::clone(&h.store), Arc::clone(&h.auth), "127.0.0.1", 0)
            .unwrap();
    let port = successor.port;
    let (code, deleted) = call_port(
        port,
        "DELETE",
        "/v1alpha1/default/workload/a",
        Some("admin"),
        None,
        &[("If-Match", "2")],
    );
    assert_eq!(code, 200);
    assert_eq!(deleted["deleted"], true);
    successor.shutdown();
}

#[test]
fn legacy_leader_header_is_rejected_and_epoch_never_rolls_back() {
    let h = Harness::new();
    assert_eq!(
        h.call(
            "PUT",
            "/v1alpha1/default/workload/a",
            Some("admin"),
            Some(&h.workload("a")),
            &[("X-Leader-Epoch", "4")],
        )
        .0,
        400
    );
    assert_eq!(h.store.revision(), 0);

    assert_eq!(h.create("a").0, 201);
    assert_eq!(
        h.call(
            "DELETE",
            "/v1alpha1/default/workload/a",
            Some("admin"),
            None,
            &[("If-Match", "1"), ("X-Leader-Epoch", "4")],
        )
        .0,
        400
    );
    assert!(h.store.get("default", "workload", "a").is_some());

    h.store.advance_leader_epoch(5).unwrap();
    for invalid in [4, 5, 0, 1u64 << 63] {
        assert!(h.store.advance_leader_epoch(invalid).is_err());
    }
    assert_eq!(h.store.leader_epoch(), 5);
    assert_eq!(
        h.call(
            "DELETE",
            "/v1alpha1/default/workload/a",
            Some("admin"),
            None,
            &[("If-Match", "1")],
        )
        .0,
        409
    );
}

#[test]
fn cursor_scope_cannot_cross_principals_or_grant_changes() {
    let h = Harness::new();
    for name in ["a", "b", "c"] {
        assert_eq!(h.create(name).0, 201);
    }
    let (code, admin_page) = h.call(
        "GET",
        "/v1alpha1/default/workload?limit=1",
        Some("admin"),
        None,
        &[],
    );
    assert_eq!(code, 200);
    let admin_cursor = admin_page["continueCursor"].as_str().unwrap();
    assert_eq!(
        h.call(
            "GET",
            &format!("/v1alpha1/default/workload?cursor={admin_cursor}"),
            Some("reader"),
            None,
            &[],
        )
        .0,
        410
    );

    let (_, watched) = h.call(
        "GET",
        "/v1alpha1/default/workload/_watch?limit=1",
        Some("reader"),
        None,
        &[],
    );
    let reader_cursor = watched["events"][0]["resumeCursor"].as_str().unwrap();
    assert_eq!(
        h.call(
            "GET",
            &format!("/v1alpha1/default/workload/_watch?cursor={reader_cursor}"),
            Some("admin"),
            None,
            &[],
        )
        .0,
        410
    );

    h.auth.set_principal(
        "reader",
        Principal::new(&["default"], &["get", "list", "watch", "delete"]),
    );
    assert_eq!(
        h.call(
            "GET",
            &format!("/v1alpha1/default/workload/_watch?cursor={reader_cursor}"),
            Some("reader"),
            None,
            &[],
        )
        .0,
        410
    );
}

#[test]
fn watch_is_bounded_and_resumes_without_gap() {
    let h = Harness::new();
    for index in 0..6 {
        assert_eq!(h.create(&format!("w{index}")).0, 201);
    }
    let mut cursor: Option<String> = None;
    let mut names = Vec::new();
    let mut cursor_count = 0;
    loop {
        let path = match &cursor {
            Some(cursor) => format!("/v1alpha1/default/workload/_watch?limit=2&cursor={cursor}"),
            None => "/v1alpha1/default/workload/_watch?limit=2".to_string(),
        };
        let (code, page) = h.call("GET", &path, Some("reader"), None, &[]);
        assert_eq!(code, 200);
        let events = page["events"].as_array().unwrap();
        assert!(events.len() <= 2);
        if events.is_empty() {
            break;
        }
        for event in events {
            names.push(event["object"]["metadata"]["name"].as_str().unwrap().to_string());
        }
        cursor = Some(events.last().unwrap()["resumeCursor"].as_str().unwrap().to_string());
        cursor_count += 1;
    }
    assert_eq!(
        names,
        (0..6).map(|index| format!("w{index}")).collect::<Vec<_>>()
    );
    assert_eq!(cursor_count, 3);

    for query in ["?limit=0", "?limit=101"] {
        assert_eq!(
            h.call(
                "GET",
                &format!("/v1alpha1/default/workload/_watch{query}"),
                Some("reader"),
                None,
                &[],
            )
            .0,
            400
        );
    }
    assert_eq!(
        h.call(
            "GET",
            "/v1alpha1/default/workload/_watch?after=-1",
            Some("reader"),
            None,
            &[],
        )
        .0,
        410
    );
    assert_eq!(
        h.call(
            "GET",
            "/v1alpha1/default/workload/_watch?after=100",
            Some("reader"),
            None,
            &[],
        )
        .0,
        410
    );
    assert_eq!(
        h.call(
            "GET",
            &format!(
                "/v1alpha1/default/workload/_watch?after=1&cursor={}",
                cursor.unwrap()
            ),
            Some("reader"),
            None,
            &[],
        )
        .0,
        410
    );
    assert_eq!(
        h.call(
            "GET",
            &format!("/v1alpha1/default/workload/_watch?cursor={}", "X".repeat(5000)),
            Some("reader"),
            None,
            &[],
        )
        .0,
        410
    );
}

#[test]
fn retention_compaction_requires_relist() {
    let h = Harness::new();
    h.store.set_max_events(2).unwrap();
    assert_eq!(h.create("a").0, 201);
    assert_eq!(h.create("b").0, 201);
    let (_, first) = h.call(
        "GET",
        "/v1alpha1/default/workload/_watch?limit=1",
        Some("reader"),
        None,
        &[],
    );
    let cursor = first["events"][0]["resumeCursor"].as_str().unwrap().to_string();
    assert_eq!(h.create("c").0, 201);
    assert_eq!(
        h.call(
            "GET",
            &format!("/v1alpha1/default/workload/_watch?cursor={cursor}"),
            Some("reader"),
            None,
            &[],
        )
        .0,
        410
    );
    assert_eq!(
        h.call(
            "GET",
            "/v1alpha1/default/workload/_watch?after=0",
            Some("reader"),
            None,
            &[],
        )
        .0,
        410
    );

    let (_, snapshot) =
        h.call("GET", "/v1alpha1/default/workload", Some("reader"), None, &[]);
    assert_eq!(snapshot["revision"], "3");
    assert_eq!(h.create("d").0, 201);
    let (_, after) = h.call(
        "GET",
        "/v1alpha1/default/workload/_watch?after=3",
        Some("reader"),
        None,
        &[],
    );
    assert_eq!(after["events"][0]["object"]["metadata"]["name"], "d");
}

#[test]
fn public_bind_schema_and_route_identity_fail_closed() {
    let h = Harness::new();
    assert!(ReferenceServer::start(
        Arc::clone(&h.store),
        Arc::clone(&h.auth),
        "0.0.0.0",
        0,
    )
    .is_err());

    let mut bad = h.workload("a");
    bad["spec"]["unknownCapability"] = json!("privileged");
    assert_eq!(
        h.call(
            "PUT",
            "/v1alpha1/default/workload/a",
            Some("admin"),
            Some(&bad),
            &[],
        )
        .0,
        400
    );

    let mismatch = h.workload("other-name");
    assert_eq!(
        h.call(
            "PUT",
            "/v1alpha1/default/workload/a",
            Some("admin"),
            Some(&mismatch),
            &[],
        )
        .0,
        400
    );
}

#[test]
fn bounded_consistent_list_cursor_rejects_tamper_and_staleness() {
    let h = Harness::new();
    for name in ["a", "b", "c"] {
        assert_eq!(h.create(name).0, 201);
    }
    let (_, first) = h.call(
        "GET",
        "/v1alpha1/default/workload?limit=2",
        Some("reader"),
        None,
        &[],
    );
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    let cursor = first["continueCursor"].as_str().unwrap().to_string();

    let (_, second) = h.call(
        "GET",
        &format!("/v1alpha1/default/workload?limit=2&cursor={cursor}"),
        Some("reader"),
        None,
        &[],
    );
    assert_eq!(second["items"].as_array().unwrap().len(), 1);

    let mut tampered = cursor.clone().into_bytes();
    let last = tampered.len() - 1;
    tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
    let tampered = String::from_utf8(tampered).unwrap();
    assert_eq!(
        h.call(
            "GET",
            &format!("/v1alpha1/default/workload?limit=2&cursor={tampered}"),
            Some("reader"),
            None,
            &[],
        )
        .0,
        410
    );

    let (_, page) = h.call(
        "GET",
        "/v1alpha1/default/workload?limit=1",
        Some("reader"),
        None,
        &[],
    );
    let stale = page["continueCursor"].as_str().unwrap().to_string();
    assert_eq!(h.create("d").0, 201);
    assert_eq!(
        h.call(
            "GET",
            &format!("/v1alpha1/default/workload?limit=1&cursor={stale}"),
            Some("reader"),
            None,
            &[],
        )
        .0,
        410
    );
}

#[test]
fn per_event_watch_cursor_resumes_after_exact_event() {
    let h = Harness::new();
    let (_, created) = h.create("a");
    let mut update = created["object"].clone();
    update["metadata"]["generation"] = json!(2);
    assert_eq!(
        h.call(
            "PUT",
            "/v1alpha1/default/workload/a",
            Some("admin"),
            Some(&update),
            &[("If-Match", "1")],
        )
        .0,
        200
    );
    let (_, watched) = h.call(
        "GET",
        "/v1alpha1/default/workload/_watch?after=0",
        Some("reader"),
        None,
        &[],
    );
    let events = watched["events"].as_array().unwrap();
    assert_eq!(events[0]["type"], "ADDED");
    assert_eq!(events[1]["type"], "MODIFIED");
    let cursor = events[0]["resumeCursor"].as_str().unwrap();
    let (_, resumed) = h.call(
        "GET",
        &format!("/v1alpha1/default/workload/_watch?cursor={cursor}"),
        Some("reader"),
        None,
        &[],
    );
    assert_eq!(resumed["events"].as_array().unwrap().len(), 1);
    assert_eq!(resumed["events"][0]["revision"], "2");
}

#[test]
fn reducing_event_retention_compacts_immediately() {
    let h = Harness::new();
    assert_eq!(h.create("a").0, 201);
    assert_eq!(h.create("b").0, 201);
    assert_eq!(h.create("c").0, 201);

    h.store.set_max_events(1).unwrap();

    assert!(h
        .store
        .watch("default", "workload", 0, None, 10, "test-scope")
        .is_err());
    let latest = h
        .store
        .watch("default", "workload", 2, None, 10, "test-scope")
        .unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0]["revision"], "3");
}

#[test]
fn duplicate_query_parameters_fail_closed_after_percent_decoding() {
    let h = Harness::new();
    assert_eq!(h.create("a").0, 201);

    for path in [
        "/v1alpha1/default/workload?limit=1&limit=2",
        "/v1alpha1/default/workload?limit=1&%6cimit=2",
        "/v1alpha1/default/workload/_watch?after=0&after=1",
    ] {
        assert_eq!(h.call("GET", path, Some("reader"), None, &[]).0, 400, "{path}");
    }
}


#[test]
fn oversized_or_excessive_query_inputs_fail_closed() {
    let h = Harness::new();

    let oversized = format!(
        "/v1alpha1/default/workload?limit=1&padding={}",
        "x".repeat(9000)
    );
    assert_eq!(h.call("GET", &oversized, Some("reader"), None, &[]).0, 400);

    let many = (0..33)
        .map(|index| format!("k{index}=v"))
        .collect::<Vec<_>>()
        .join("&");
    let path = format!("/v1alpha1/default/workload?{many}");
    assert_eq!(h.call("GET", &path, Some("reader"), None, &[]).0, 400);
}


#[test]
fn oversized_route_path_fails_closed() {
    let h = Harness::new();
    let path = format!(
        "/v1alpha1/default/workload/{}",
        "x".repeat(5000)
    );
    assert_eq!(h.call("GET", &path, Some("reader"), None, &[]).0, 400);
}
