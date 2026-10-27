//! Executable process-lifecycle oracle for node-agent semantics.
//!
//! This is a test harness, not an OCI implementation. It starts exactly one
//! allowlisted executable directly (never through a shell) so CI can exercise
//! real process creation, fencing, idempotency and physically confirmed stop.
use std::{
    collections::HashMap,
    fmt,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::{
    fs::{MetadataExt, PermissionsExt},
    process::CommandExt,
};

const MAX_IDENTITY_BYTES: usize = 256;
const HANDLE_HEX_BYTES: usize = 32;
const MAX_TRACKED_REQUESTS: usize = 4096;

fn valid_handle(handle: &str) -> bool {
    handle.len() == HANDLE_HEX_BYTES
        && handle
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

#[cfg(unix)]
fn group_alive(pgid: u32) -> bool {
    let result = unsafe { libc::kill(-(pgid as libc::pid_t), 0) };
    if result == 0 {
        return true;
    }
    !matches!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH))
}

#[cfg(unix)]
fn signal_group(pgid: u32, signal: libc::c_int) -> Result<(), RuntimeError> {
    let result = unsafe { libc::kill(-(pgid as libc::pid_t), signal) };
    if result == 0 {
        return Ok(());
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(()),
        _ => Err(RuntimeError::Io(std::io::Error::last_os_error().to_string())),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    Invalid(String),
    Fenced,
    Conflict(String),
    UnknownSandbox,
    Io(String),
    StopUnconfirmed,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(m) | Self::Conflict(m) | Self::Io(m) => f.write_str(m),
            Self::Fenced => f.write_str("stale node-agent epoch"),
            Self::UnknownSandbox => f.write_str("unknown sandbox"),
            Self::StopUnconfirmed => f.write_str("runtime stop not physically confirmed"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sandbox {
    pub handle: String,
    pub uid: String,
    pub generation: u64,
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    uid: String,
    generation: u64,
    duration_seconds: u64,
}

struct RunningSandbox {
    info: Sandbox,
    process: Child,
}

pub struct ProcessRuntime {
    executable: PathBuf,
    #[cfg(unix)]
    executable_dev: u64,
    #[cfg(unix)]
    executable_ino: u64,
    agent_epoch: u64,
    sandboxes: HashMap<String, RunningSandbox>,
    by_workload: HashMap<(String, u64), String>,
    idempotency: HashMap<String, (Identity, String)>,
}

impl ProcessRuntime {
    pub fn new(allowed_executable: impl AsRef<Path>, agent_epoch: u64) -> Result<Self, RuntimeError> {
        let executable = allowed_executable.as_ref();
        let metadata = fs::symlink_metadata(executable)
            .map_err(|_| RuntimeError::Invalid("invalid runtime configuration".into()))?;
        if !executable.is_absolute()
            || metadata.file_type().is_symlink()
            || !metadata.is_file()
            || agent_epoch == 0
        {
            return Err(RuntimeError::Invalid("invalid runtime configuration".into()));
        }
        #[cfg(unix)]
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(RuntimeError::Invalid("runtime executable is not executable".into()));
        }
        Ok(Self {
            executable: executable.to_owned(),
            #[cfg(unix)]
            executable_dev: metadata.dev(),
            #[cfg(unix)]
            executable_ino: metadata.ino(),
            agent_epoch,
            sandboxes: HashMap::new(),
            by_workload: HashMap::new(),
            idempotency: HashMap::new(),
        })
    }

    pub fn agent_epoch(&self) -> u64 {
        self.agent_epoch
    }

    fn validate_executable_identity(&self) -> Result<(), RuntimeError> {
        let metadata = fs::symlink_metadata(&self.executable)
            .map_err(|_| RuntimeError::Invalid("runtime executable changed".into()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(RuntimeError::Invalid("runtime executable changed".into()));
        }
        #[cfg(unix)]
        {
            if metadata.dev() != self.executable_dev
                || metadata.ino() != self.executable_ino
                || metadata.permissions().mode() & 0o111 == 0
            {
                return Err(RuntimeError::Invalid("runtime executable changed".into()));
            }
        }
        Ok(())
    }

    pub fn advance_epoch(&mut self, new_epoch: u64) -> Result<(), RuntimeError> {
        if new_epoch <= self.agent_epoch {
            return Err(RuntimeError::Invalid("agent epoch must advance".into()));
        }
        self.agent_epoch = new_epoch;
        Ok(())
    }

    fn check_epoch(&self, claimed: u64) -> Result<(), RuntimeError> {
        if claimed != self.agent_epoch {
            Err(RuntimeError::Fenced)
        } else {
            Ok(())
        }
    }

    fn new_handle() -> Result<String, RuntimeError> {
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes).map_err(|e| RuntimeError::Io(e.to_string()))?;
        let mut out = String::with_capacity(32);
        for b in bytes {
            use std::fmt::Write as _;
            write!(&mut out, "{b:02x}").expect("writing into String cannot fail");
        }
        Ok(out)
    }

    fn running(entry: &mut RunningSandbox) -> Result<bool, RuntimeError> {
        let leader_alive = entry
            .process
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|e| RuntimeError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            Ok(leader_alive || group_alive(entry.info.pid))
        }
        #[cfg(not(unix))]
        {
            Ok(leader_alive)
        }
    }

    pub fn start(
        &mut self,
        uid: &str,
        generation: u64,
        agent_epoch: u64,
        idempotency_key: &str,
        duration_seconds: u64,
    ) -> Result<Sandbox, RuntimeError> {
        self.check_epoch(agent_epoch)?;
        if uid.is_empty()
            || uid.len() > MAX_IDENTITY_BYTES
            || uid.contains('\0')
            || generation == 0
            || idempotency_key.is_empty()
            || idempotency_key.len() > MAX_IDENTITY_BYTES
            || idempotency_key.contains('\0')
            || !(1..=300).contains(&duration_seconds)
        {
            return Err(RuntimeError::Invalid("invalid start request".into()));
        }

        let identity = Identity {
            uid: uid.to_owned(),
            generation,
            duration_seconds,
        };
        if let Some((prior_identity, handle)) = self.idempotency.get(idempotency_key) {
            if prior_identity != &identity {
                return Err(RuntimeError::Conflict(
                    "idempotency key reused for different request".into(),
                ));
            }
            return self
                .sandboxes
                .get(handle)
                .map(|s| s.info.clone())
                .ok_or(RuntimeError::UnknownSandbox);
        }
        if self.idempotency.len() >= MAX_TRACKED_REQUESTS
            || self.sandboxes.len() >= MAX_TRACKED_REQUESTS
        {
            return Err(RuntimeError::Invalid("runtime request ledger full".into()));
        }

        let relevant: Vec<(u64, String)> = self
            .by_workload
            .iter()
            .filter_map(|((existing_uid, existing_generation), handle)| {
                (existing_uid == uid).then(|| (*existing_generation, handle.clone()))
            })
            .collect();
        for (existing_generation, handle) in relevant {
            let sandbox = self
                .sandboxes
                .get_mut(&handle)
                .ok_or(RuntimeError::UnknownSandbox)?;
            if Self::running(sandbox)? {
                let message = if existing_generation != generation {
                    "older workload generation is still running"
                } else {
                    "running workload requires original idempotency key"
                };
                return Err(RuntimeError::Conflict(message.into()));
            }
        }

        self.validate_executable_identity()?;
        let mut command = Command::new(&self.executable);
        command
            .arg(duration_seconds.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env_clear();
        #[cfg(unix)]
        command.process_group(0);

        let child = command
            .spawn()
            .map_err(|e| RuntimeError::Io(format!("process spawn failed: {e}")))?;
        let handle = Self::new_handle()?;
        let info = Sandbox {
            handle: handle.clone(),
            uid: uid.to_owned(),
            generation,
            pid: child.id(),
        };
        self.sandboxes.insert(
            handle.clone(),
            RunningSandbox {
                info: info.clone(),
                process: child,
            },
        );
        self.by_workload
            .insert((uid.to_owned(), generation), handle.clone());
        self.idempotency
            .insert(idempotency_key.to_owned(), (identity, handle));
        Ok(info)
    }

    pub fn status(&mut self, handle: &str) -> Result<&'static str, RuntimeError> {
        if !valid_handle(handle) {
            return Err(RuntimeError::Invalid("invalid sandbox handle".into()));
        }
        let sandbox = self
            .sandboxes
            .get_mut(handle)
            .ok_or(RuntimeError::UnknownSandbox)?;
        Ok(if Self::running(sandbox)? {
            "Running"
        } else {
            "Stopped"
        })
    }

    pub fn stop(&mut self, handle: &str, agent_epoch: u64) -> Result<&'static str, RuntimeError> {
        self.check_epoch(agent_epoch)?;
        if !valid_handle(handle) {
            return Err(RuntimeError::Invalid("invalid sandbox handle".into()));
        }
        let sandbox = self
            .sandboxes
            .get_mut(handle)
            .ok_or(RuntimeError::UnknownSandbox)?;
        if !Self::running(sandbox)? {
            return Ok("Stopped");
        }

        #[cfg(unix)]
        signal_group(sandbox.info.pid, libc::SIGTERM)?;
        #[cfg(not(unix))]
        sandbox
            .process
            .kill()
            .map_err(|e| RuntimeError::Io(e.to_string()))?;

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !Self::running(sandbox)? {
                return Ok("Stopped");
            }
            thread::sleep(Duration::from_millis(20));
        }

        #[cfg(unix)]
        signal_group(sandbox.info.pid, libc::SIGKILL)?;
        #[cfg(not(unix))]
        sandbox
            .process
            .kill()
            .map_err(|e| RuntimeError::Io(e.to_string()))?;

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !Self::running(sandbox)? {
                return Ok("Stopped");
            }
            thread::sleep(Duration::from_millis(20));
        }
        if Self::running(sandbox)? {
            Err(RuntimeError::StopUnconfirmed)
        } else {
            Ok("Stopped")
        }
    }

    pub fn close(&mut self) {
        for sandbox in self.sandboxes.values_mut() {
            if matches!(Self::running(sandbox), Ok(true)) {
                #[cfg(unix)]
                let _ = signal_group(sandbox.info.pid, libc::SIGKILL);
                #[cfg(not(unix))]
                let _ = sandbox.process.kill();
            }
            let _ = sandbox.process.wait();
        }
    }
}

impl Drop for ProcessRuntime {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process, time::{SystemTime, UNIX_EPOCH}};

    fn runtime() -> Option<ProcessRuntime> {
        let sleep = Path::new("/usr/bin/sleep");
        sleep.is_file().then(|| ProcessRuntime::new(sleep, 7).unwrap())
    }

    #[test]
    fn real_process_start_and_confirmed_stop() {
        let Some(mut rt) = runtime() else { return };
        let box1 = rt.start("work-1", 1, 7, "start-1", 30).unwrap();
        assert!(box1.pid > 0);
        assert_eq!(rt.status(&box1.handle).unwrap(), "Running");
        assert_eq!(rt.stop(&box1.handle, 7).unwrap(), "Stopped");
        assert_eq!(rt.status(&box1.handle).unwrap(), "Stopped");
    }

    #[test]
    fn duplicate_start_is_idempotent() {
        let Some(mut rt) = runtime() else { return };
        let a = rt.start("work-1", 1, 7, "same", 30).unwrap();
        let b = rt.start("work-1", 1, 7, "same", 30).unwrap();
        assert_eq!((a.handle, a.pid), (b.handle, b.pid));
    }

    #[test]
    fn new_generation_blocked_while_old_process_runs() {
        let Some(mut rt) = runtime() else { return };
        rt.start("work-1", 1, 7, "one", 30).unwrap();
        assert!(matches!(
            rt.start("work-1", 2, 7, "two", 30),
            Err(RuntimeError::Conflict(_))
        ));
    }

    #[test]
    fn stale_epoch_cannot_start_or_stop() {
        let Some(mut rt) = runtime() else { return };
        assert_eq!(
            rt.start("work-1", 1, 6, "x", 30).unwrap_err(),
            RuntimeError::Fenced
        );
        let box1 = rt.start("work-1", 1, 7, "y", 30).unwrap();
        rt.advance_epoch(8).unwrap();
        assert_eq!(rt.stop(&box1.handle, 7).unwrap_err(), RuntimeError::Fenced);
        assert_eq!(rt.status(&box1.handle).unwrap(), "Running");
        assert_eq!(rt.stop(&box1.handle, 8).unwrap(), "Stopped");
    }

    #[test]
    fn idempotency_key_cannot_alias_different_request() {
        let Some(mut rt) = runtime() else { return };
        rt.start("a", 1, 7, "same", 30).unwrap();
        assert!(matches!(
            rt.start("b", 1, 7, "same", 30),
            Err(RuntimeError::Conflict(_))
        ));
    }

    #[test]
    fn rejects_unbounded_or_nul_identity_inputs() {
        let Some(mut rt) = runtime() else { return };
        let oversized = "x".repeat(MAX_IDENTITY_BYTES + 1);
        assert!(matches!(
            rt.start(&oversized, 1, 7, "key", 1),
            Err(RuntimeError::Invalid(_))
        ));
        assert!(matches!(
            rt.start("uid", 1, 7, &oversized, 1),
            Err(RuntimeError::Invalid(_))
        ));
        assert!(matches!(
            rt.start("bad\0uid", 1, 7, "key", 1),
            Err(RuntimeError::Invalid(_))
        ));
        assert!(matches!(
            rt.start("uid", 1, 7, "bad\0key", 1),
            Err(RuntimeError::Invalid(_))
        ));
    }

    #[test]
    fn runtime_request_ledger_is_bounded_before_spawn() {
        let Some(mut rt) = runtime() else { return };
        let handle = "0".repeat(HANDLE_HEX_BYTES);
        for index in 0..MAX_TRACKED_REQUESTS {
            rt.idempotency.insert(
                format!("key-{index}"),
                (
                    Identity {
                        uid: format!("uid-{index}"),
                        generation: 1,
                        duration_seconds: 1,
                    },
                    handle.clone(),
                ),
            );
        }
        assert!(matches!(
            rt.start("new-work", 1, 7, "new-key", 1),
            Err(RuntimeError::Invalid(_))
        ));
        assert!(rt.sandboxes.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn runtime_executable_identity_is_revalidated_before_spawn() {
        use std::os::unix::fs::PermissionsExt;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "orestra-runtime-replace-{}-{nonce}",
            process::id()
        ));
        fs::create_dir_all(&root).unwrap();

        let executable = root.join("runner");
        fs::write(&executable, "#!/bin/sh\n/bin/sleep \"$1\"\n").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&executable, permissions).unwrap();

        let mut rt = ProcessRuntime::new(&executable, 7).unwrap();
        let old = root.join("runner-old");
        fs::rename(&executable, &old).unwrap();
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        let mut replacement_permissions = fs::metadata(&executable).unwrap().permissions();
        replacement_permissions.set_mode(0o700);
        fs::set_permissions(&executable, replacement_permissions).unwrap();

        assert!(matches!(
            rt.start("work", 1, 7, "key", 1),
            Err(RuntimeError::Invalid(_))
        ));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn untrusted_uid_is_never_shell_interpreted() {
        let Some(mut rt) = runtime() else { return };
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let marker = std::env::temp_dir().join(format!(
            "orestra-rust-shell-injection-{}-{nonce}",
            process::id()
        ));
        let _ = fs::remove_file(&marker);
        let uid = format!("x;touch {}", marker.display());
        let box1 = rt.start(&uid, 1, 7, "safe", 1).unwrap();
        assert_eq!(rt.status(&box1.handle).unwrap(), "Running");
        assert!(!marker.exists());
        rt.stop(&box1.handle, 7).unwrap();
        assert!(!marker.exists());
    }


    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_and_non_executable_runtime_binary() {
        use std::os::unix::fs::symlink;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "orestra-runtime-executable-{}-{nonce}",
            process::id()
        ));
        fs::create_dir_all(&root).unwrap();

        let real = root.join("real-runner");
        fs::write(&real, "#!/bin/sh\nexit 0\n").unwrap();
        let mut executable = fs::metadata(&real).unwrap().permissions();
        executable.set_mode(0o700);
        fs::set_permissions(&real, executable).unwrap();

        let link = root.join("runner-link");
        symlink(&real, &link).unwrap();
        assert!(matches!(
            ProcessRuntime::new(&link, 1),
            Err(RuntimeError::Invalid(_))
        ));

        let inert = root.join("not-executable");
        fs::write(&inert, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&inert).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&inert, permissions).unwrap();
        assert!(matches!(
            ProcessRuntime::new(&inert, 1),
            Err(RuntimeError::Invalid(_))
        ));

        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn exited_leader_does_not_prove_descendant_absence() {
        use std::os::unix::fs::PermissionsExt;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "orestra-process-group-{}-{nonce}",
            process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let launcher = root.join("forking-launcher");
        fs::write(&launcher, "#!/bin/sh\n/bin/sleep 30 &\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&launcher).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&launcher, permissions).unwrap();

        let mut rt = ProcessRuntime::new(&launcher, 3).unwrap();
        let sandbox = rt.start("forked", 1, 3, "forked", 30).unwrap();
        rt.sandboxes
            .get_mut(&sandbox.handle)
            .unwrap()
            .process
            .wait()
            .unwrap();

        assert_eq!(rt.status(&sandbox.handle).unwrap(), "Running");
        match rt.stop(&sandbox.handle, 3) {
            Ok("Stopped") | Err(RuntimeError::StopUnconfirmed) => {}
            other => panic!("unexpected stop result: {other:?}"),
        }
        rt.close();
        fs::remove_dir_all(&root).unwrap();
    }
}


#[cfg(test)]
mod handle_boundary_tests {
    use super::*;

    #[test]
    fn external_sandbox_handles_are_strictly_bounded_hex() {
        let sleep = Path::new("/usr/bin/sleep");
        if !sleep.is_file() {
            return;
        }
        let mut rt = ProcessRuntime::new(sleep, 7).unwrap();
        let epoch = rt.agent_epoch();
        let invalid = vec![
            String::new(),
            "a".into(),
            "A".repeat(HANDLE_HEX_BYTES),
            "g".repeat(HANDLE_HEX_BYTES),
            "x".repeat(HANDLE_HEX_BYTES + 1),
        ];
        for invalid in invalid {
            assert!(matches!(
                rt.status(&invalid),
                Err(RuntimeError::Invalid(_))
            ));
            assert!(matches!(
                rt.stop(&invalid, epoch),
                Err(RuntimeError::Invalid(_))
            ));
        }
    }
}
