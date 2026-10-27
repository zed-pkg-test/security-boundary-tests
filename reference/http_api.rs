//! Authenticated loopback API reference used by executable protocol tests.
//! This is not the production Oreslang/mTLS server.

use crate::validate::validate_document;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::io::Read;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

type HmacSha256 = Hmac<Sha256>;

const MAX_QUERY_BYTES: usize = 8192;
const MAX_QUERY_PARAMS: usize = 32;
const MAX_ROUTE_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    BadRequest(String),
    Conflict(&'static str),
    Fenced(&'static str),
    ResetRequired(&'static str),
    Unauthorized(&'static str),
    Forbidden(&'static str),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(message) => write!(f, "{message}"),
            Self::Conflict(message)
            | Self::Fenced(message)
            | Self::ResetRequired(message)
            | Self::Unauthorized(message)
            | Self::Forbidden(message) => write!(f, "{message}"),
        }
    }
}
impl std::error::Error for ApiError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub namespaces: BTreeSet<String>,
    pub verbs: BTreeSet<String>,
    pub subject: String,
}

impl Principal {
    pub fn new(namespaces: &[&str], verbs: &[&str]) -> Self {
        Self {
            namespaces: namespaces.iter().map(|x| (*x).to_string()).collect(),
            verbs: verbs.iter().map(|x| (*x).to_string()).collect(),
            subject: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct Event {
    revision: u64,
    ordinal: u64,
    event_type: String,
    namespace: String,
    kind: String,
    name: String,
    object: Value,
}

#[derive(Clone)]
pub struct CursorCodec {
    secret: Vec<u8>,
}

impl CursorCodec {
    pub fn new(secret: &[u8]) -> Result<Self, ApiError> {
        if secret.len() < 16 {
            return Err(ApiError::BadRequest("cursor secret too short".into()));
        }
        Ok(Self { secret: secret.to_vec() })
    }

    pub fn encode(&self, value: &Value) -> String {
        let raw = serde_json::to_vec(value).expect("serializable cursor");
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC key");
        mac.update(&raw);
        let signature = mac.finalize().into_bytes();
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(raw),
            URL_SAFE_NO_PAD.encode(signature)
        )
    }

    pub fn scope(&self, principal: &Principal) -> String {
        let raw = serde_json::to_vec(&json!({
            "subject": principal.subject,
            "namespaces": principal.namespaces.iter().collect::<Vec<_>>(),
            "verbs": principal.verbs.iter().collect::<Vec<_>>()
        }))
        .expect("serializable principal");
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC key");
        mac.update(b"scope:");
        mac.update(&raw);
        hex(mac.finalize().into_bytes().as_slice())
    }

    pub fn decode(&self, token: &str) -> Result<Value, ApiError> {
        if token.len() > 4096 || token.matches('.').count() != 1 {
            return Err(ApiError::ResetRequired("invalid cursor size or encoding"));
        }
        let (a, b) = token
            .split_once('.')
            .ok_or(ApiError::ResetRequired("invalid cursor"))?;
        let raw = URL_SAFE_NO_PAD
            .decode(a)
            .map_err(|_| ApiError::ResetRequired("invalid cursor"))?;
        let signature = URL_SAFE_NO_PAD
            .decode(b)
            .map_err(|_| ApiError::ResetRequired("invalid cursor"))?;
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC key");
        mac.update(&raw);
        mac.verify_slice(&signature)
            .map_err(|_| ApiError::ResetRequired("invalid cursor"))?;
        let value: Value = serde_json::from_slice(&raw)
            .map_err(|_| ApiError::ResetRequired("invalid cursor"))?;
        if !value.is_object() {
            return Err(ApiError::ResetRequired("invalid cursor"));
        }
        Ok(value)
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

struct StoreState {
    objects: BTreeMap<(String, String, String), (u64, Value)>,
    revision: u64,
    events: Vec<Event>,
    compacted_revision: u64,
    max_events: usize,
    leader_epoch: u64,
}

pub struct ApiStore {
    state: Mutex<StoreState>,
    pub codec: CursorCodec,
}

impl ApiStore {
    pub fn new(leader_epoch: u64, cursor_secret: &[u8], max_events: usize) -> Result<Self, ApiError> {
        if !(1..=10_000).contains(&max_events) {
            return Err(ApiError::BadRequest("max_events must be 1..10000".into()));
        }
        if leader_epoch == 0 || leader_epoch >= (1u64 << 63) {
            return Err(ApiError::BadRequest("invalid initial leadership epoch".into()));
        }
        Ok(Self {
            state: Mutex::new(StoreState {
                objects: BTreeMap::new(),
                revision: 0,
                events: Vec::new(),
                compacted_revision: 0,
                max_events,
                leader_epoch,
            }),
            codec: CursorCodec::new(cursor_secret)?,
        })
    }

    pub fn test_default(leader_epoch: u64) -> Self {
        Self::new(leader_epoch, b"orestra-test-cursor-secret", 256).unwrap()
    }

    pub fn leader_epoch(&self) -> u64 {
        self.state.lock().unwrap().leader_epoch
    }

    pub fn revision(&self) -> u64 {
        self.state.lock().unwrap().revision
    }

    pub fn set_max_events(&self, max_events: usize) -> Result<(), ApiError> {
        if !(1..=10_000).contains(&max_events) {
            return Err(ApiError::BadRequest("max_events must be 1..10000".into()));
        }
        let mut state = self.state.lock().unwrap();
        state.max_events = max_events;
        trim_events(&mut state);
        Ok(())
    }

    pub fn advance_leader_epoch(&self, epoch: u64) -> Result<(), ApiError> {
        let mut state = self.state.lock().unwrap();
        if epoch <= state.leader_epoch || epoch >= (1u64 << 63) {
            return Err(ApiError::BadRequest(
                "leadership epochs must increase monotonically".into(),
            ));
        }
        state.leader_epoch = epoch;
        Ok(())
    }

    pub fn get(&self, namespace: &str, kind: &str, name: &str) -> Option<Value> {
        self.state
            .lock()
            .unwrap()
            .objects
            .get(&(namespace.to_string(), kind.to_ascii_lowercase(), name.to_string()))
            .map(|(_, value)| value.clone())
    }

    pub fn put(
        &self,
        namespace: &str,
        kind: &str,
        name: &str,
        document: Value,
        expected: Option<u64>,
        leader_epoch: u64,
    ) -> Result<(u64, Value, bool), ApiError> {
        validate_document(&document).map_err(ApiError::BadRequest)?;
        let metadata = document["metadata"]
            .as_object()
            .ok_or_else(|| ApiError::BadRequest("missing metadata".into()))?;
        if metadata.get("namespace").and_then(Value::as_str) != Some(namespace)
            || metadata.get("name").and_then(Value::as_str) != Some(name)
            || document["kind"]
                .as_str()
                .map(str::to_ascii_lowercase)
                .as_deref()
                != Some(kind.to_ascii_lowercase().as_str())
        {
            return Err(ApiError::BadRequest("route/object identity mismatch".into()));
        }

        let mut state = self.state.lock().unwrap();
        if leader_epoch != state.leader_epoch || leader_epoch == 0 {
            return Err(ApiError::Fenced("stale leader epoch"));
        }
        let key = (namespace.to_string(), kind.to_ascii_lowercase(), name.to_string());
        let old = state.objects.get(&key).cloned();
        match (expected, &old) {
            (None, Some(_)) => return Err(ApiError::Conflict("create requires absent object")),
            (Some(expected), Some((revision, _))) if expected == *revision => {}
            (Some(_), _) => return Err(ApiError::Conflict("resource version mismatch")),
            (None, None) => {}
        }

        state.revision += 1;
        let revision = state.revision;
        let mut value = document;
        value["metadata"]["resourceVersion"] = Value::String(revision.to_string());
        let created = old.is_none();
        state.objects.insert(key, (revision, value.clone()));
        state.events.push(Event {
            revision,
            ordinal: 0,
            event_type: if created { "ADDED" } else { "MODIFIED" }.to_string(),
            namespace: namespace.to_string(),
            kind: kind.to_ascii_lowercase(),
            name: name.to_string(),
            object: value.clone(),
        });
        trim_events(&mut state);
        Ok((revision, value, created))
    }

    pub fn delete(
        &self,
        namespace: &str,
        kind: &str,
        name: &str,
        expected: Option<u64>,
        leader_epoch: u64,
    ) -> Result<u64, ApiError> {
        let mut state = self.state.lock().unwrap();
        if leader_epoch != state.leader_epoch || leader_epoch == 0 {
            return Err(ApiError::Fenced("stale leader epoch"));
        }
        let key = (namespace.to_string(), kind.to_ascii_lowercase(), name.to_string());
        let Some((revision, old)) = state.objects.get(&key).cloned() else {
            return Err(ApiError::Conflict("resource version mismatch"));
        };
        if expected != Some(revision) {
            return Err(ApiError::Conflict("resource version mismatch"));
        }
        state.revision += 1;
        let next = state.revision;
        state.objects.remove(&key);
        state.events.push(Event {
            revision: next,
            ordinal: 0,
            event_type: "DELETED".into(),
            namespace: namespace.to_string(),
            kind: kind.to_ascii_lowercase(),
            name: name.to_string(),
            object: old,
        });
        trim_events(&mut state);
        Ok(next)
    }

    pub fn list_page(
        &self,
        namespace: &str,
        kind: &str,
        limit: usize,
        cursor: Option<&str>,
        scope: &str,
    ) -> Result<(u64, Vec<Value>, Option<String>), ApiError> {
        if !(1..=100).contains(&limit) {
            return Err(ApiError::BadRequest("limit must be 1..100".into()));
        }
        let state = self.state.lock().unwrap();
        let mut start_after = String::new();
        let mut snapshot = state.revision;
        if let Some(cursor) = cursor {
            let current = self.codec.decode(cursor)?;
            if current["purpose"].as_str() != Some("list")
                || current["namespace"].as_str() != Some(namespace)
                || current["kind"].as_str() != Some(kind.to_ascii_lowercase().as_str())
                || current["scope"].as_str() != Some(scope)
            {
                return Err(ApiError::ResetRequired("cursor scope mismatch"));
            }
            snapshot = current["revision"]
                .as_u64()
                .ok_or(ApiError::ResetRequired("cursor scope mismatch"))?;
            start_after = current["after"]
                .as_str()
                .ok_or(ApiError::ResetRequired("cursor scope mismatch"))?
                .to_string();
            if snapshot != state.revision {
                return Err(ApiError::ResetRequired("list snapshot changed; relist"));
            }
        }

        let kind_lower = kind.to_ascii_lowercase();
        let rows: Vec<(String, Value)> = state
            .objects
            .iter()
            .filter(|((ns, object_kind, name), _)| {
                ns == namespace && object_kind == &kind_lower && name > &start_after
            })
            .map(|((_, _, name), (_, value))| (name.clone(), value.clone()))
            .collect();

        let page: Vec<_> = rows.iter().take(limit).cloned().collect();
        let next = if rows.len() > limit {
            Some(self.codec.encode(&json!({
                "purpose":"list",
                "namespace":namespace,
                "kind":kind_lower,
                "revision":snapshot,
                "after":page.last().unwrap().0,
                "scope":scope
            })))
        } else {
            None
        };
        Ok((snapshot, page.into_iter().map(|(_, value)| value).collect(), next))
    }

    pub fn watch(
        &self,
        namespace: &str,
        kind: &str,
        after: u64,
        cursor: Option<&str>,
        limit: usize,
        scope: &str,
    ) -> Result<Vec<Value>, ApiError> {
        if !(1..=100).contains(&limit) {
            return Err(ApiError::BadRequest("watch limit must be 1..100".into()));
        }
        if cursor.is_some() && after != 0 {
            return Err(ApiError::ResetRequired("cursor and after are mutually exclusive"));
        }
        let state = self.state.lock().unwrap();
        if after > state.revision {
            return Err(ApiError::ResetRequired("watch revision ahead of state"));
        }
        if after < state.compacted_revision {
            return Err(ApiError::ResetRequired("watch history compacted; relist"));
        }

        let mut boundary = (after, u64::MAX);
        if let Some(cursor) = cursor {
            let current = self.codec.decode(cursor)?;
            if current["purpose"].as_str() != Some("watch")
                || current["namespace"].as_str() != Some(namespace)
                || current["kind"].as_str() != Some(kind.to_ascii_lowercase().as_str())
                || current["scope"].as_str() != Some(scope)
            {
                return Err(ApiError::ResetRequired("cursor scope mismatch"));
            }
            let revision = current["revision"]
                .as_u64()
                .ok_or(ApiError::ResetRequired("cursor scope mismatch"))?;
            let ordinal = current["ordinal"]
                .as_u64()
                .ok_or(ApiError::ResetRequired("cursor scope mismatch"))?;
            if revision <= state.compacted_revision || revision > state.revision {
                return Err(ApiError::ResetRequired("cursor scope mismatch"));
            }
            boundary = (revision, ordinal);
        }

        let kind_lower = kind.to_ascii_lowercase();
        let mut result = Vec::new();
        for event in &state.events {
            if event.namespace != namespace || event.kind != kind_lower {
                continue;
            }
            if (event.revision, event.ordinal) <= boundary {
                continue;
            }
            if result.len() >= limit {
                break;
            }
            let token = self.codec.encode(&json!({
                "purpose":"watch",
                "namespace":namespace,
                "kind":kind_lower,
                "revision":event.revision,
                "ordinal":event.ordinal,
                "scope":scope
            }));
            result.push(json!({
                "type":event.event_type,
                "revision":event.revision.to_string(),
                "resumeCursor":token,
                "object":event.object
            }));
        }
        Ok(result)
    }
}

fn trim_events(state: &mut StoreState) {
    while state.events.len() > state.max_events {
        let discarded = state.events.remove(0);
        state.compacted_revision = discarded.revision;
    }
}

pub struct Authorizer {
    tokens: RwLock<HashMap<String, Principal>>,
}

impl Authorizer {
    pub fn new(tokens: HashMap<String, Principal>) -> Self {
        Self { tokens: RwLock::new(tokens) }
    }

    pub fn set_principal(&self, token: &str, principal: Principal) {
        self.tokens.write().unwrap().insert(token.to_string(), principal);
    }

    pub fn authenticate(&self, header: Option<&str>) -> Result<Principal, ApiError> {
        let header = header.ok_or(ApiError::Unauthorized("missing bearer identity"))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized("missing bearer identity"))?;
        let mut principal = self
            .tokens
            .read()
            .unwrap()
            .get(token)
            .cloned()
            .ok_or(ApiError::Unauthorized("unknown identity"))?;
        principal.subject = token.to_string();
        Ok(principal)
    }

    pub fn authorize(
        &self,
        principal: &Principal,
        verb: &str,
        namespace: &str,
    ) -> Result<(), ApiError> {
        if !principal.namespaces.contains(namespace) || !principal.verbs.contains(verb) {
            return Err(ApiError::Forbidden("operation not authorized"));
        }
        Ok(())
    }
}

pub struct ReferenceServer {
    pub port: u16,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ReferenceServer {
    pub fn start(
        store: Arc<ApiStore>,
        authorizer: Arc<Authorizer>,
        host: &str,
        port: u16,
    ) -> Result<Self, ApiError> {
        if host != "127.0.0.1" {
            return Err(ApiError::BadRequest(
                "insecure reference HTTP service must bind loopback only".into(),
            ));
        }
        let serving_epoch = store.leader_epoch();
        if serving_epoch == 0 {
            return Err(ApiError::BadRequest(
                "cannot serve writes without established leadership".into(),
            ));
        }

        let chosen_port = if port == 0 {
            let probe = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .map_err(|e| ApiError::BadRequest(e.to_string()))?;
            let port = probe.local_addr().map_err(|e| ApiError::BadRequest(e.to_string()))?.port();
            drop(probe);
            port
        } else {
            port
        };
        let server = Arc::new(
            Server::http((Ipv4Addr::LOCALHOST, chosen_port))
                .map_err(|e| ApiError::BadRequest(e.to_string()))?,
        );
        let running = Arc::new(AtomicBool::new(true));
        let server_for_thread = Arc::clone(&server);
        let running_for_thread = Arc::clone(&running);
        let thread = thread::spawn(move || {
            while running_for_thread.load(Ordering::SeqCst) {
                match server_for_thread.recv_timeout(Duration::from_millis(100)) {
                    Ok(Some(request)) => {
                        handle_request(request, &store, &authorizer, serving_epoch);
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });

        Ok(Self { port: chosen_port, running, thread: Some(thread) })
    }

    pub fn shutdown(mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for ReferenceServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn handle_request(
    mut request: Request,
    store: &ApiStore,
    authorizer: &Authorizer,
    serving_epoch: u64,
) {
    let result = dispatch(&mut request, store, authorizer, serving_epoch);
    let (status, body) = match result {
        Ok(value) => value,
        Err(error) => (
            match error {
                ApiError::Unauthorized(_) => 401,
                ApiError::Forbidden(_) => 403,
                ApiError::ResetRequired(_) => 410,
                ApiError::Conflict(_) | ApiError::Fenced(_) => 409,
                ApiError::BadRequest(_) => 400,
            },
            json!({"error":error.to_string()}),
        ),
    };

    let encoded = serde_json::to_string(&body).unwrap();
    let response = Response::from_string(encoded)
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
    let _ = request.respond(response);
}

fn dispatch(
    request: &mut Request,
    store: &ApiStore,
    authorizer: &Authorizer,
    serving_epoch: u64,
) -> Result<(u16, Value), ApiError> {
    let url = request.url().to_string();
    let (path, query_text) = url.split_once('?').unwrap_or((&url, ""));
    if path.len() > MAX_ROUTE_BYTES {
        return Err(ApiError::BadRequest("route path too large".into()));
    }
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() < 3 || parts[0] != "v1alpha1" {
        return Err(ApiError::BadRequest("invalid route".into()));
    }
    let namespace = parts[1];
    let kind = parts[2];
    let query = parse_query(query_text)?;

    match request.method() {
        Method::Get => {
            if parts.len() == 4 && parts[3] == "_watch" {
                let principal = principal(request, authorizer, "watch", namespace)?;
                if query.contains_key("after") && query.contains_key("cursor") {
                    return Err(ApiError::ResetRequired("cursor and after are mutually exclusive"));
                }
                let after_text = query.get("after").map(String::as_str).unwrap_or("0");
                let after = after_text
                    .parse::<i64>()
                    .map_err(|_| ApiError::ResetRequired("invalid watch revision"))?;
                if after < 0 {
                    return Err(ApiError::ResetRequired("invalid watch revision"));
                }
                let limit = query
                    .get("limit")
                    .map(String::as_str)
                    .unwrap_or("100")
                    .parse::<usize>()
                    .map_err(|_| ApiError::BadRequest("watch limit must be 1..100".into()))?;
                let cursor = query.get("cursor").map(String::as_str);
                let scope = store.codec.scope(&principal);
                let events = store.watch(namespace, kind, after as u64, cursor, limit, &scope)?;
                return Ok((200, json!({"events":events})));
            }
            if parts.len() == 3 {
                let principal = principal(request, authorizer, "list", namespace)?;
                let limit = query
                    .get("limit")
                    .map(String::as_str)
                    .unwrap_or("100")
                    .parse::<usize>()
                    .map_err(|_| ApiError::BadRequest("limit must be 1..100".into()))?;
                let cursor = query.get("cursor").map(String::as_str);
                let scope = store.codec.scope(&principal);
                let (revision, items, next) =
                    store.list_page(namespace, kind, limit, cursor, &scope)?;
                return Ok((200, json!({
                    "revision":revision.to_string(),
                    "items":items,
                    "continueCursor":next
                })));
            }
            if parts.len() == 4 {
                principal(request, authorizer, "get", namespace)?;
                return match store.get(namespace, kind, parts[3]) {
                    Some(value) => Ok((200, value)),
                    None => Ok((404, json!({"error":"not found"}))),
                };
            }
            Err(ApiError::BadRequest("invalid route".into()))
        }
        Method::Put => {
            if parts.len() != 4 {
                return Err(ApiError::BadRequest("invalid route".into()));
            }
            principal(request, authorizer, "put", namespace)?;
            reject_legacy_epoch_header(request)?;
            let length = header(request, "Content-Length")
                .unwrap_or_else(|| "0".into())
                .parse::<usize>()
                .map_err(|_| ApiError::BadRequest("invalid body size".into()))?;
            if length == 0 || length > 1024 * 1024 {
                return Err(ApiError::BadRequest("invalid body size".into()));
            }
            let mut body = String::new();
            request
                .as_reader()
                .take((1024 * 1024 + 1) as u64)
                .read_to_string(&mut body)
                .map_err(|e| ApiError::BadRequest(e.to_string()))?;
            let document: Value =
                serde_json::from_str(&body).map_err(|e| ApiError::BadRequest(e.to_string()))?;
            let expected = match header(request, "If-Match") {
                Some(value) => Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| ApiError::BadRequest("invalid If-Match".into()))?,
                ),
                None => None,
            };
            let (revision, object, created) =
                store.put(namespace, kind, parts[3], document, expected, serving_epoch)?;
            Ok((
                if created { 201 } else { 200 },
                json!({"revision":revision.to_string(),"object":object}),
            ))
        }
        Method::Delete => {
            if parts.len() != 4 {
                return Err(ApiError::BadRequest("invalid route".into()));
            }
            principal(request, authorizer, "delete", namespace)?;
            reject_legacy_epoch_header(request)?;
            let expected = header(request, "If-Match")
                .unwrap_or_else(|| "0".into())
                .parse::<u64>()
                .map_err(|_| ApiError::BadRequest("invalid If-Match".into()))?;
            let revision =
                store.delete(namespace, kind, parts[3], Some(expected), serving_epoch)?;
            Ok((200, json!({"deleted":true,"revision":revision.to_string()})))
        }
        _ => Err(ApiError::BadRequest("unsupported method".into())),
    }
}

fn principal(
    request: &Request,
    authorizer: &Authorizer,
    verb: &str,
    namespace: &str,
) -> Result<Principal, ApiError> {
    let auth = header(request, "Authorization");
    let principal = authorizer.authenticate(auth.as_deref())?;
    authorizer.authorize(&principal, verb, namespace)?;
    Ok(principal)
}

fn reject_legacy_epoch_header(request: &Request) -> Result<(), ApiError> {
    if header(request, "X-Leader-Epoch").is_some() {
        Err(ApiError::BadRequest(
            "clients cannot supply X-Leader-Epoch".into(),
        ))
    } else {
        Ok(())
    }
}

fn header(request: &Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str().to_string())
}

fn parse_query(query: &str) -> Result<HashMap<String, String>, ApiError> {
    let mut result = HashMap::new();
    if query.is_empty() {
        return Ok(result);
    }
    if query.len() > MAX_QUERY_BYTES {
        return Err(ApiError::BadRequest("query string too large".into()));
    }
    for (index, part) in query.split('&').enumerate() {
        if index >= MAX_QUERY_PARAMS {
            return Err(ApiError::BadRequest("too many query parameters".into()));
        }
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        let key = percent_decode(key)?;
        let value = percent_decode(value)?;
        if result.insert(key, value).is_some() {
            return Err(ApiError::BadRequest("duplicate query parameter".into()));
        }
    }
    Ok(result)
}

fn percent_decode(input: &str) -> Result<String, ApiError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let high = from_hex(bytes[i + 1])
                    .ok_or_else(|| ApiError::BadRequest("invalid URL encoding".into()))?;
                let low = from_hex(bytes[i + 2])
                    .ok_or_else(|| ApiError::BadRequest("invalid URL encoding".into()))?;
                out.push((high << 4) | low);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ApiError::BadRequest("invalid URL encoding".into()))
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
