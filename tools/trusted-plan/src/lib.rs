use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_COMMANDS: usize = 16;
const MAX_ARGS: usize = 64;
const MAX_ARG_LENGTH: usize = 4096;
const MAX_TIMEOUT: u64 = 1800;
const MAX_LOG_BYTES: usize = 64 * 1024;
const MAX_RECEIPT_BYTES: u64 = 64 * 1024;
const MAX_PLAN_BYTES: u64 = 256 * 1024;
const SAFE_ENV_KEYS: [&str; 6] = [
    "PATH",
    "JAVA_HOME",
    "GRAALVM_HOME",
    "MAVEN_OPTS",
    "LANG",
    "LC_ALL",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied(pub String);

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Denied {}

fn deny(msg: impl Into<String>) -> Denied {
    Denied(msg.into())
}

fn object<'a>(v: &'a Value, msg: &str) -> Result<&'a Map<String, Value>, Denied> {
    v.as_object().ok_or_else(|| deny(msg))
}

fn exact_keys(map: &Map<String, Value>, expected: &[&str]) -> bool {
    map.len() == expected.len() && expected.iter().all(|k| map.contains_key(*k))
}

fn safe_plan_path(v: Option<&Value>) -> Result<&str, Denied> {
    let value = v
        .and_then(Value::as_str)
        .ok_or_else(|| deny("test plan path"))?;
    let path = Path::new(value);
    if !value.starts_with("ci/test-plans/")
        || !value.ends_with(".json")
        || value.contains('\\')
        || path.is_absolute()
        || path.components().any(|c| matches!(c, Component::ParentDir))
        || path.components().any(|c| c.as_os_str() == ".git")
    {
        return Err(deny("unsafe test plan path"));
    }
    Ok(value)
}

fn read_regular_bounded(path: &Path, max_bytes: u64, label: &str) -> Result<Vec<u8>, Denied> {
    let meta = fs::symlink_metadata(path).map_err(|_| deny(format!("{label} missing or unsafe")))?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > max_bytes {
        return Err(deny(format!("{label} missing or unsafe")));
    }
    fs::read(path).map_err(|_| deny(format!("{label} unreadable")))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

#[derive(Debug, Clone)]
pub struct Context {
    pub source_repo: String,
    pub source_pr: i64,
    pub plan: Value,
}

pub fn load_context_in(workspace: &Path, receipt_path: &Path) -> Result<Context, Denied> {
    let receipt_bytes = read_regular_bounded(receipt_path, MAX_RECEIPT_BYTES, "invalid receipt")?;
    let receipt: Value =
        serde_json::from_slice(&receipt_bytes).map_err(|_| deny("invalid receipt"))?;
    let receipt = object(&receipt, "receipt schema")?;
    let schema = receipt
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| deny("receipt schema"))?;
    if !matches!(
        schema,
        "ores-private-ci-receipt-v3" | "ores-private-ci-receipt-v4"
    ) {
        return Err(deny("receipt schema"));
    }
    if schema == "ores-private-ci-receipt-v4" {
        let auth = receipt
            .get("authorization")
            .and_then(Value::as_object)
            .ok_or_else(|| deny("v4 authorization receipt"))?;
        if !exact_keys(
            auth,
            &[
                "repository",
                "repository_id",
                "branch",
                "pr",
                "merge_sha",
                "manifest_path",
                "manifest_blob_sha",
                "manifest_sha256",
            ],
        ) {
            return Err(deny("v4 authorization receipt"));
        }
    }
    let source = receipt
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| deny("receipt source/test plan"))?;
    let test_plan = receipt
        .get("test_plan")
        .and_then(Value::as_object)
        .ok_or_else(|| deny("receipt source/test plan"))?;
    let plan_rel = safe_plan_path(test_plan.get("path"))?;
    let plan_file = workspace.join(plan_rel);
    let plan_bytes = read_regular_bounded(&plan_file, MAX_PLAN_BYTES, "test plan")?;
    let expected_digest = test_plan.get("sha256").and_then(Value::as_str);
    let actual_digest = sha256_hex(&plan_bytes);
    if expected_digest != Some(actual_digest.as_str()) {
        return Err(deny("test plan digest mismatch"));
    }
    let plan: Value = serde_json::from_slice(&plan_bytes).map_err(|_| deny("invalid test plan"))?;
    let plan_obj = object(&plan, "test plan schema")?;
    if plan_obj.get("schema").and_then(Value::as_str) != Some("ores-private-ci-test-plan-v1") {
        return Err(deny("test plan schema"));
    }
    if !exact_keys(
        plan_obj,
        &["schema", "source_repo", "allowed_prs", "commands"],
    ) {
        return Err(deny("unknown or missing test plan keys"));
    }
    let source_repo = source
        .get("repo")
        .and_then(Value::as_str)
        .ok_or_else(|| deny("test plan source repository mismatch"))?;
    if plan_obj.get("source_repo").and_then(Value::as_str) != Some(source_repo) {
        return Err(deny("test plan source repository mismatch"));
    }
    let source_pr = source
        .get("pr")
        .and_then(Value::as_i64)
        .ok_or_else(|| deny("source PR not authorized by test plan"))?;
    let allowed = plan_obj
        .get("allowed_prs")
        .and_then(Value::as_array)
        .ok_or_else(|| deny("source PR not authorized by test plan"))?;
    if !allowed.iter().any(|v| v.as_i64() == Some(source_pr))
        || allowed
            .iter()
            .any(|v| v.is_boolean() || v.as_i64().is_none_or(|n| n <= 0))
    {
        return Err(deny("source PR not authorized by test plan"));
    }
    Ok(Context {
        source_repo: source_repo.into(),
        source_pr,
        plan,
    })
}

fn path_has_unsafe_parts(value: &str) -> bool {
    let path = Path::new(value);
    path.is_absolute()
        || value.contains('\\')
        || path.components().any(|c| matches!(c, Component::ParentDir))
        || path.components().any(|c| c.as_os_str() == ".git")
}

pub fn confined_cwd(root: &Path, relative: &str) -> Result<PathBuf, Denied> {
    if relative.is_empty() {
        return Err(deny("command cwd"));
    }
    if relative != "." && path_has_unsafe_parts(relative) {
        return Err(deny("unsafe command cwd"));
    }
    let root_real =
        fs::canonicalize(root).map_err(|_| deny("command cwd escapes mirrored source"))?;
    let candidate = if relative == "." {
        root.to_path_buf()
    } else {
        root.join(relative)
    };
    let candidate_real =
        fs::canonicalize(&candidate).map_err(|_| deny("command cwd escapes mirrored source"))?;
    if candidate_real.strip_prefix(&root_real).is_err() {
        return Err(deny("command cwd escapes mirrored source"));
    }
    if !candidate_real.is_dir() {
        return Err(deny("command cwd is not a directory"));
    }
    Ok(candidate_real)
}

fn safe_executable(s: &str) -> bool {
    if matches!(
        s,
        "python3"
            | "mvn"
            | "cmake"
            | "ctest"
            | "g++"
            | "clang++"
            | "./target/ores-aot"
            | "./target/ores-hybrid"
    ) {
        return true;
    }
    let Some(rest) = s.strip_prefix("/tmp/ores-") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

#[derive(Debug, Clone)]
pub struct ValidatedCommand {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: u64,
    pub expected: Option<String>,
}

pub fn validate_command(command: &Value, root: &Path) -> Result<ValidatedCommand, Denied> {
    let c = object(command, "command entry")?;
    let allowed = ["argv", "cwd", "timeout_seconds", "stdout_contains"];
    if c.keys().any(|k| !allowed.contains(&k.as_str()))
        || !c.contains_key("argv")
        || !c.contains_key("timeout_seconds")
    {
        return Err(deny("unknown or missing command keys"));
    }
    let argv_v = c
        .get("argv")
        .and_then(Value::as_array)
        .ok_or_else(|| deny("command argv"))?;
    if argv_v.is_empty() || argv_v.len() > MAX_ARGS {
        return Err(deny("command argv"));
    }
    let mut argv = Vec::with_capacity(argv_v.len());
    for arg in argv_v {
        let Some(s) = arg.as_str() else {
            return Err(deny("command argv"));
        };
        if s.is_empty() || s.chars().count() > MAX_ARG_LENGTH || s.contains('\0') {
            return Err(deny("command argv"));
        }
        argv.push(s.to_string());
    }
    if !safe_executable(&argv[0]) {
        return Err(deny(format!("unapproved executable: {}", argv[0])));
    }
    if argv[0] == "python3" {
        if argv.len() < 2 || matches!(argv[1].as_str(), "-c" | "-m" | "-") {
            return Err(deny("inline/module Python execution is not allowed"));
        }
        if path_has_unsafe_parts(&argv[1])
            || Path::new(&argv[1]).extension().and_then(|s| s.to_str()) != Some("py")
        {
            return Err(deny("unsafe Python script path"));
        }
    }
    let timeout_v = c
        .get("timeout_seconds")
        .ok_or_else(|| deny("command timeout"))?;
    if timeout_v.is_boolean() {
        return Err(deny("command timeout"));
    }
    let timeout = timeout_v.as_u64().ok_or_else(|| deny("command timeout"))?;
    if !(1..=MAX_TIMEOUT).contains(&timeout) {
        return Err(deny("command timeout"));
    }
    let cwd = confined_cwd(root, c.get("cwd").and_then(Value::as_str).unwrap_or("."))?;
    if argv[0] == "python3" {
        let script_path = cwd.join(&argv[1]);
        let script_real = fs::canonicalize(&script_path)
            .map_err(|_| deny("Python script escapes mirrored source"))?;
        let root_real =
            fs::canonicalize(root).map_err(|_| deny("Python script escapes mirrored source"))?;
        if script_real.strip_prefix(root_real).is_err() {
            return Err(deny("Python script escapes mirrored source"));
        }
        let meta = fs::symlink_metadata(&script_path)
            .map_err(|_| deny("Python script is not a regular source file"))?;
        if meta.file_type().is_symlink() || !script_real.is_file() {
            return Err(deny("Python script is not a regular source file"));
        }
    }
    let expected = match c.get("stdout_contains") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let Some(s) = v.as_str() else {
                return Err(deny("stdout assertion"));
            };
            if s.is_empty() || s.chars().count() > 4096 {
                return Err(deny("stdout assertion"));
            }
            Some(s.to_string())
        }
    };
    Ok(ValidatedCommand {
        argv,
        cwd,
        timeout,
        expected,
    })
}

pub fn safe_env() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for key in SAFE_ENV_KEYS {
        if let Ok(value) = env::var(key) {
            out.insert(key.into(), value);
        }
    }
    out.insert("CI".into(), "true".into());
    out.insert("NO_COLOR".into(), "1".into());
    out
}

fn read_tail<R: Read>(mut reader: R, needle: Option<Vec<u8>>) -> (Vec<u8>, bool) {
    let mut tail = Vec::new();
    let mut carry = Vec::new();
    let mut found = needle.is_none();
    let mut buf = [0u8; 8192];
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        if !found {
            if let Some(ref needle) = needle {
                let mut hay = Vec::with_capacity(carry.len() + n);
                hay.extend_from_slice(&carry);
                hay.extend_from_slice(&buf[..n]);
                found = hay.windows(needle.len()).any(|w| w == needle);
                let keep = needle.len().saturating_sub(1).min(hay.len());
                carry.clear();
                carry.extend_from_slice(&hay[hay.len() - keep..]);
            }
        }
        tail.extend_from_slice(&buf[..n]);
        if tail.len() > MAX_LOG_BYTES {
            let excess = tail.len() - MAX_LOG_BYTES;
            tail.drain(..excess);
        }
    }
    (tail, found)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub index: usize,
    pub argv: Vec<String>,
    pub returncode: i32,
}

fn execute(index: usize, cmd: &ValidatedCommand) -> Result<CommandResult, Denied> {
    println!(
        "PLAN_COMMAND_{index}_START executable={} cwd={}",
        cmd.argv[0],
        cmd.cwd.file_name().and_then(|s| s.to_str()).unwrap_or(".")
    );
    let mut child = Command::new(&cmd.argv[0])
        .args(&cmd.argv[1..])
        .current_dir(&cmd.cwd)
        .env_clear()
        .envs(safe_env())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| deny(format!("command {index} unavailable or timed out")))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let needle = cmd.expected.as_ref().map(|s| s.as_bytes().to_vec());
    let out_thread = thread::spawn(move || read_tail(stdout, needle));
    let err_thread = thread::spawn(move || read_tail(stderr, None));
    let deadline = Instant::now() + Duration::from_secs(cmd.timeout);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| deny(format!("command {index} unavailable or timed out")))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out_thread.join();
            let _ = err_thread.join();
            return Err(deny(format!("command {index} unavailable or timed out")));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let (stdout_tail, found) = out_thread
        .join()
        .map_err(|_| deny(format!("command {index} unavailable or timed out")))?;
    let (stderr_tail, _) = err_thread
        .join()
        .map_err(|_| deny(format!("command {index} unavailable or timed out")))?;
    if !stdout_tail.is_empty() {
        let text = String::from_utf8_lossy(&stdout_tail);
        print!("{text}");
        if !text.ends_with('\n') {
            println!();
        }
    }
    if !stderr_tail.is_empty() {
        let text = String::from_utf8_lossy(&stderr_tail);
        eprint!("{text}");
        if !text.ends_with('\n') {
            eprintln!();
        }
    }
    let code = status.code().unwrap_or(-1);
    if !status.success() {
        return Err(deny(format!("command {index} failed with exit {code}")));
    }
    if cmd.expected.is_some() && !found {
        return Err(deny(format!(
            "command {index} missing required stdout evidence"
        )));
    }
    println!("PLAN_COMMAND_{index}_PASS");
    Ok(CommandResult {
        index,
        argv: cmd.argv.clone(),
        returncode: code,
    })
}

pub fn run_plan_in(
    workspace: &Path,
    receipt_path: &Path,
    source_dir: &Path,
) -> Result<Vec<CommandResult>, Denied> {
    let context = load_context_in(workspace, receipt_path)?;
    let commands = context
        .plan
        .get("commands")
        .and_then(Value::as_array)
        .ok_or_else(|| deny("test plan command count"))?;
    if commands.is_empty() || commands.len() > MAX_COMMANDS {
        return Err(deny("test plan command count"));
    }
    let source_meta = fs::symlink_metadata(source_dir)
        .map_err(|_| deny("mirrored source directory missing or unsafe"))?;
    if source_meta.file_type().is_symlink() || !source_meta.is_dir() {
        return Err(deny("mirrored source directory missing or unsafe"));
    }
    let source_root = fs::canonicalize(source_dir)
        .map_err(|_| deny("mirrored source directory missing or unsafe"))?;
    let mut results = Vec::with_capacity(commands.len());
    for (offset, command) in commands.iter().enumerate() {
        let validated = validate_command(command, &source_root)?;
        results.push(execute(offset + 1, &validated)?);
    }
    println!(
        "TEST_PLAN_PASS commands={} source={} pr={}",
        results.len(),
        context.source_repo,
        context.source_pr
    );
    Ok(results)
}

pub fn run_plan(receipt_path: &Path, source_dir: &Path) -> Result<Vec<CommandResult>, Denied> {
    let workspace = env::current_dir().map_err(|_| deny("invalid receipt"))?;
    run_plan_in(&workspace, receipt_path, source_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let p = env::temp_dir().join(format!(
                "orestra-plan-test-{}-{nonce}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_fixture(
        root: &Path,
        command: Value,
        source_repo: &str,
        pr: i64,
        schema: &str,
    ) -> PathBuf {
        fs::create_dir_all(root.join("private-oreslang-source/formal")).unwrap();
        fs::create_dir_all(root.join("ci/test-plans")).unwrap();
        fs::write(
            root.join("private-oreslang-source/formal/input.txt"),
            "synthetic
",
        )
        .unwrap();
        let plan = serde_json::json!({
            "schema":"ores-private-ci-test-plan-v1",
            "source_repo":source_repo,
            "allowed_prs":[pr],
            "commands":[command]
        });
        let mut plan_bytes = serde_json::to_vec(&plan).unwrap();
        plan_bytes.push(b'\n');
        let plan_path = root.join("ci/test-plans/synthetic.json");
        fs::write(&plan_path, &plan_bytes).unwrap();
        let mut receipt = serde_json::json!({
            "schema":schema,
            "source":{"repo":"synthetic/private","pr":7},
            "test_plan":{"path":"ci/test-plans/synthetic.json","sha256":sha256_hex(&plan_bytes)}
        });
        if schema == "ores-private-ci-receipt-v4" {
            receipt["authorization"] = serde_json::json!({
                "repository":"ores-truffle-oreslang/ores-truffle-oreslang.github.io",
                "repository_id":1409246164u64,
                "branch":"main",
                "pr":52,
                "merge_sha":"a".repeat(40),
                "manifest_path":"ci/private-ci/authorizations/java-pr1668.json",
                "manifest_blob_sha":"b".repeat(40),
                "manifest_sha256":"c".repeat(64)
            });
        }
        let receipt_path = root.join("ci/oreslang-receipt.json");
        fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        receipt_path
    }

    fn happy_command() -> Value {
        serde_json::json!({
            "argv":["cmake","-E","echo","TRUSTED_PLAN_SYNTHETIC_PASS"],
            "cwd":"formal",
            "timeout_seconds":30,
            "stdout_contains":"TRUSTED_PLAN_SYNTHETIC_PASS"
        })
    }

    #[test]
    fn happy_path_and_v4_execute_without_shell() {
        for schema in ["ores-private-ci-receipt-v3", "ores-private-ci-receipt-v4"] {
            let t = Temp::new();
            let receipt = write_fixture(&t.0, happy_command(), "synthetic/private", 7, schema);
            let result = run_plan_in(&t.0, &receipt, &t.0.join("private-oreslang-source")).unwrap();
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].argv[0], "cmake");
        }
    }

    #[test]
    fn receipt_and_test_plan_inputs_are_bounded() {
        let t = Temp::new();
        let receipt = write_fixture(
            &t.0,
            happy_command(),
            "synthetic/private",
            7,
            "ores-private-ci-receipt-v3",
        );

        fs::write(&receipt, vec![b' '; (MAX_RECEIPT_BYTES + 1) as usize]).unwrap();
        assert!(load_context_in(&t.0, &receipt).is_err());

        let receipt = write_fixture(
            &t.0,
            happy_command(),
            "synthetic/private",
            7,
            "ores-private-ci-receipt-v3",
        );
        fs::write(
            t.0.join("ci/test-plans/synthetic.json"),
            vec![b' '; (MAX_PLAN_BYTES + 1) as usize],
        )
        .unwrap();
        assert!(load_context_in(&t.0, &receipt).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_receipt_is_rejected() {
        use std::os::unix::fs::symlink;

        let t = Temp::new();
        let receipt = write_fixture(
            &t.0,
            happy_command(),
            "synthetic/private",
            7,
            "ores-private-ci-receipt-v3",
        );
        let linked = t.0.join("ci/linked-receipt.json");
        symlink(&receipt, &linked).unwrap();
        assert!(load_context_in(&t.0, &linked).is_err());
    }

    #[test]
    fn safe_environment_never_forwards_home() {
        let env = safe_env();
        assert!(!env.contains_key("HOME"));
        assert_eq!(env.get("CI").map(String::as_str), Some("true"));
        assert_eq!(env.get("NO_COLOR").map(String::as_str), Some("1"));
    }

    #[test]
    fn rejects_inline_module_and_parent_python_escapes() {
        let t = Temp::new();
        fs::create_dir_all(t.0.join("private-oreslang-source/formal")).unwrap();
        let root = fs::canonicalize(t.0.join("private-oreslang-source")).unwrap();
        for argv in [
            serde_json::json!(["python3", "-c", "print('x')"]),
            serde_json::json!(["python3", "-m", "http.server"]),
            serde_json::json!(["python3", "../outside.py"]),
        ] {
            let cmd = serde_json::json!({"argv":argv,"cwd":"formal","timeout_seconds":30});
            assert!(validate_command(&cmd, &root).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_python_symlink_escape() {
        use std::os::unix::fs::symlink;
        let t = Temp::new();
        fs::create_dir_all(t.0.join("private-oreslang-source/formal")).unwrap();
        fs::write(
            t.0.join("outside.py"),
            "print('outside')
",
        )
        .unwrap();
        symlink(
            t.0.join("outside.py"),
            t.0.join("private-oreslang-source/formal/link.py"),
        )
        .unwrap();
        let root = fs::canonicalize(t.0.join("private-oreslang-source")).unwrap();
        let cmd = serde_json::json!({
            "argv":["python3","link.py"],"cwd":"formal","timeout_seconds":30
        });
        assert!(validate_command(&cmd, &root).is_err());
    }

    #[test]
    fn rejects_unknown_command_key_shell_and_cwd_escape() {
        let t = Temp::new();
        fs::create_dir_all(t.0.join("private-oreslang-source/formal")).unwrap();
        let root = fs::canonicalize(t.0.join("private-oreslang-source")).unwrap();
        let unknown = serde_json::json!({
            "argv":["cmake","-E","echo","ok"],"cwd":"formal","timeout_seconds":30,
            "future_shell":"bash -lc whoami"
        });
        assert!(validate_command(&unknown, &root).is_err());
        let shell =
            serde_json::json!({"argv":["bash","-lc","echo x"],"cwd":".","timeout_seconds":30});
        assert!(validate_command(&shell, &root).is_err());
        let escape =
            serde_json::json!({"argv":["cmake","-E","echo","x"],"cwd":"../","timeout_seconds":30});
        assert!(validate_command(&escape, &root).is_err());
    }

    #[test]
    fn rejects_plan_digest_mismatch_and_different_source_repo() {
        let t = Temp::new();
        let receipt = write_fixture(
            &t.0,
            happy_command(),
            "synthetic/private",
            7,
            "ores-private-ci-receipt-v3",
        );
        fs::OpenOptions::new()
            .append(true)
            .open(t.0.join("ci/test-plans/synthetic.json"))
            .unwrap()
            .write_all(b" ")
            .unwrap();
        assert!(load_context_in(&t.0, &receipt)
            .unwrap_err()
            .0
            .contains("digest mismatch"));

        let t = Temp::new();
        let receipt = write_fixture(
            &t.0,
            happy_command(),
            "other/private",
            7,
            "ores-private-ci-receipt-v3",
        );
        assert!(load_context_in(&t.0, &receipt)
            .unwrap_err()
            .0
            .contains("source repository mismatch"));
    }

    #[test]
    fn stdout_evidence_is_required() {
        let t = Temp::new();
        let cmd = serde_json::json!({
            "argv":["cmake","-E","echo","actual"],
            "cwd":"formal",
            "timeout_seconds":30,
            "stdout_contains":"expected"
        });
        let receipt = write_fixture(
            &t.0,
            cmd,
            "synthetic/private",
            7,
            "ores-private-ci-receipt-v3",
        );
        let e = run_plan_in(&t.0, &receipt, &t.0.join("private-oreslang-source")).unwrap_err();
        assert!(e.0.contains("missing required stdout"));
    }

    #[test]
    fn validates_command_bounds_and_plan_count() {
        let t = Temp::new();
        fs::create_dir_all(t.0.join("private-oreslang-source/formal")).unwrap();
        let root = fs::canonicalize(t.0.join("private-oreslang-source")).unwrap();
        let bad_timeout = serde_json::json!({
            "argv":["cmake","-E","echo","x"],"cwd":"formal","timeout_seconds":0
        });
        assert!(validate_command(&bad_timeout, &root).is_err());
        let bool_timeout = serde_json::json!({
            "argv":["cmake","-E","echo","x"],"cwd":"formal","timeout_seconds":true
        });
        assert!(validate_command(&bool_timeout, &root).is_err());
    }
}
