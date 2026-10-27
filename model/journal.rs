use crate::store::{Event, Lease, MemoryStore, StoreError, StoreState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub trait SyncStrategy: Send + Sync {
    fn sync_file(&self, file: &File) -> io::Result<()>;
    fn sync_directory(&self, directory: &File) -> io::Result<()>;
}

#[derive(Default)]
pub struct SystemSync;

impl SyncStrategy for SystemSync {
    fn sync_file(&self, file: &File) -> io::Result<()> {
        file.sync_all()
    }

    fn sync_directory(&self, directory: &File) -> io::Result<()> {
        directory.sync_all()
    }
}

#[derive(Debug)]
pub enum JournalError {
    Store(StoreError),
    Io(io::Error),
    Corrupt(&'static str),
    UncertainCommit,
}

impl fmt::Display for JournalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Corrupt(message) => write!(f, "{message}"),
            Self::UncertainCommit => write!(
                f,
                "WAL replace completed but directory sync failed; reopen"
            ),
        }
    }
}
impl std::error::Error for JournalError {}

impl From<StoreError> for JournalError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
impl From<io::Error> for JournalError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct JournaledStore {
    path: PathBuf,
    lock_path: PathBuf,
    memory: MemoryStore,
    operation_lock: Mutex<()>,
    poisoned: AtomicBool,
    ever_persisted: AtomicBool,
    sync: Arc<dyn SyncStrategy>,
}

impl JournaledStore {
    pub const FORMAT_VERSION: u64 = 1;

    pub fn new(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        Self::with_sync_strategy(path, Arc::new(SystemSync))
    }

    pub fn with_sync_strategy(
        path: impl AsRef<Path>,
        sync: Arc<dyn SyncStrategy>,
    ) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let lock_path = path.with_file_name(format!(
            "{}.lock",
            path.file_name().and_then(|x| x.to_str()).unwrap_or("state.wal")
        ));
        let existed = path.exists();
        let store = Self {
            path,
            lock_path,
            memory: MemoryStore::new(),
            operation_lock: Mutex::new(()),
            poisoned: AtomicBool::new(false),
            ever_persisted: AtomicBool::new(existed),
            sync,
        };
        let _guard = store.operation_lock.lock().unwrap();
        let _disk = store.acquire_disk_lock()?;
        if store.path.exists() {
            store.recover_locked()?;
        }
        drop(_disk);
        drop(_guard);
        Ok(store)
    }

    fn acquire_disk_lock(&self) -> Result<DiskLock, JournalError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.lock_path)?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if result != 0 {
            return Err(JournalError::Io(io::Error::last_os_error()));
        }
        Ok(DiskLock { file })
    }

    fn frame(&self) -> Result<Value, JournalError> {
        let payload = json!({
            "version": Self::FORMAT_VERSION,
            "state": self.memory.snapshot()
        });
        let encoded = serde_json::to_vec(&payload)
            .map_err(|_| JournalError::Corrupt("cannot serialize WAL payload"))?;
        let digest = format!("{:x}", Sha256::digest(&encoded));
        Ok(json!({ "payload": payload, "sha256": digest }))
    }

    fn replace_wal_bytes(&self, contents: &[u8]) -> Result<(), JournalError> {
        let staged = self.path.with_file_name(format!(
            "{}.next",
            self.path.file_name().and_then(|x| x.to_str()).unwrap_or("state.wal")
        ));
        let result = (|| -> Result<(), JournalError> {
            let mut handle = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&staged)?;
            handle.write_all(contents)?;
            self.sync.sync_file(&handle)?;
            drop(handle);
            fs::rename(&staged, &self.path)?;

            let directory = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY)
                .open(self.path.parent().unwrap_or_else(|| Path::new(".")))?;
            if self.sync.sync_directory(&directory).is_err() {
                self.poisoned.store(true, Ordering::SeqCst);
                return Err(JournalError::UncertainCommit);
            }
            self.ever_persisted.store(true, Ordering::SeqCst);
            Ok(())
        })();

        let _ = fs::remove_file(&staged);
        result
    }

    fn persist_locked(&self) -> Result<(), JournalError> {
        let frame = self.frame()?;
        let mut encoded = serde_json::to_vec(&frame)
            .map_err(|_| JournalError::Corrupt("cannot serialize WAL frame"))?;
        encoded.push(b'\n');
        let mut prior = if self.path.exists() { fs::read(&self.path)? } else { Vec::new() };
        prior.extend_from_slice(&encoded);
        self.replace_wal_bytes(&prior)
    }

    fn recover_locked(&self) -> Result<(), JournalError> {
        let contents = fs::read(&self.path)?;
        let mut start = 0usize;
        let mut last: Option<StoreState> = None;
        let mut valid_end = 0usize;
        let mut truncated_tail = false;

        while start < contents.len() {
            let relative_newline = contents[start..].iter().position(|b| *b == b'\n');
            let Some(relative_newline) = relative_newline else {
                truncated_tail = true;
                break;
            };
            let end = start + relative_newline;
            let raw = &contents[start..end];
            let frame: Value = serde_json::from_slice(raw)
                .map_err(|_| JournalError::Corrupt("invalid complete WAL frame"))?;
            let payload = frame.get("payload")
                .ok_or(JournalError::Corrupt("invalid complete WAL frame"))?;
            let digest = frame.get("sha256").and_then(Value::as_str)
                .ok_or(JournalError::Corrupt("invalid complete WAL frame"))?;
            if payload.get("version").and_then(Value::as_u64) != Some(Self::FORMAT_VERSION) {
                return Err(JournalError::Corrupt("unsupported WAL frame version"));
            }
            let encoded = serde_json::to_vec(payload)
                .map_err(|_| JournalError::Corrupt("invalid complete WAL frame"))?;
            let actual = format!("{:x}", Sha256::digest(&encoded));
            if actual != digest {
                return Err(JournalError::Corrupt("WAL checksum mismatch"));
            }
            let state_value = payload.get("state").cloned()
                .ok_or(JournalError::Corrupt("invalid complete WAL frame"))?;
            let state: StoreState = serde_json::from_value(state_value)
                .map_err(|_| JournalError::Corrupt("invalid complete WAL frame"))?;
            if state.compacted > state.revision {
                return Err(JournalError::Corrupt("compaction exceeds revision"));
            }
            if let Some(previous) = &last {
                if state.revision != previous.revision && state.revision != previous.revision + 1 {
                    return Err(JournalError::Corrupt("non-monotonic WAL revision"));
                }
                if state.revision == previous.revision && state.objects != previous.objects {
                    return Err(JournalError::Corrupt("objects changed without revision"));
                }
                for (domain, epoch) in &previous.epoch_counters {
                    if state.epoch_counters.get(domain).copied().unwrap_or(0) < *epoch {
                        return Err(JournalError::Corrupt("leadership epoch rolled back"));
                    }
                }
            }
            last = Some(state);
            valid_end = end + 1;
            start = end + 1;
        }

        if let Some(state) = last {
            self.memory.restore(state);
        }
        if truncated_tail {
            self.replace_wal_bytes(&contents[..valid_end])?;
        }
        Ok(())
    }

    fn refresh_locked(&self) -> Result<(), JournalError> {
        if self.ever_persisted.load(Ordering::SeqCst) && !self.path.exists() {
            return Err(JournalError::Corrupt("previously persisted WAL disappeared"));
        }
        if self.path.exists() {
            self.recover_locked()?;
        }
        Ok(())
    }

    fn durable<T>(
        &self,
        operation: impl FnOnce(&MemoryStore) -> Result<T, StoreError>,
    ) -> Result<T, JournalError> {
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(JournalError::UncertainCommit);
        }
        let _local = self.operation_lock.lock().unwrap();
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(JournalError::UncertainCommit);
        }
        let _disk = self.acquire_disk_lock()?;
        self.refresh_locked()?;
        let before = self.memory.snapshot();
        let result = match operation(&self.memory) {
            Ok(value) => value,
            Err(error) => {
                self.memory.restore(before);
                return Err(JournalError::Store(error));
            }
        };
        if let Err(error) = self.persist_locked() {
            self.memory.restore(before);
            return Err(error);
        }
        Ok(result)
    }

    fn fresh_read<T>(&self, read: impl FnOnce(&MemoryStore) -> Result<T, StoreError>) -> Result<T, JournalError> {
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(JournalError::UncertainCommit);
        }
        let _local = self.operation_lock.lock().unwrap();
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(JournalError::UncertainCommit);
        }
        let _disk = self.acquire_disk_lock()?;
        self.refresh_locked()?;
        read(&self.memory).map_err(JournalError::Store)
    }

    pub fn revision(&self) -> Result<u64, JournalError> {
        self.fresh_read(|store| Ok(store.revision()))
    }

    pub fn get(&self, key: &str) -> Result<Option<(u64, Value)>, JournalError> {
        self.fresh_read(|store| Ok(store.get(key)))
    }

    pub fn list(&self, prefix: &str) -> Result<(u64, Vec<(String, u64, Value)>), JournalError> {
        self.fresh_read(|store| Ok(store.list(prefix)))
    }

    pub fn watch_after(&self, revision: u64, prefix: &str) -> Result<Vec<Event>, JournalError> {
        self.fresh_read(|store| store.watch_after(revision, prefix))
    }

    pub fn watch_resume(&self, cursor: &str, prefix: &str) -> Result<Vec<Event>, JournalError> {
        self.fresh_read(|store| store.watch_resume(cursor, prefix))
    }

    pub fn elect(&self, domain: &str, holder: &str, now: i64, ttl: i64) -> Result<Lease, JournalError> {
        self.durable(|store| store.elect(domain, holder, now, ttl))
    }

    pub fn put(
        &self,
        key: &str,
        object: Value,
        expected: Option<u64>,
        domain: &str,
        lease: &Lease,
        now: i64,
    ) -> Result<u64, JournalError> {
        self.durable(|store| store.put(key, object, expected, domain, lease, now))
    }

    pub fn delete(
        &self,
        key: &str,
        expected: u64,
        domain: &str,
        lease: &Lease,
        now: i64,
    ) -> Result<u64, JournalError> {
        self.durable(|store| store.delete(key, expected, domain, lease, now))
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
    ) -> Result<u64, JournalError> {
        self.durable(|store| {
            store.reserve(
                workload_key,
                workload_version,
                workload_uid,
                node_key,
                node_uid,
                node_version,
                reservation_key,
                cpu,
                memory,
                domain,
                lease,
                now,
            )
        })
    }

    pub fn compact_through(&self, revision: u64) -> Result<(), JournalError> {
        self.durable(|store| store.compact_through(revision))
    }
}

struct DiskLock {
    file: File,
}

impl Drop for DiskLock {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}
