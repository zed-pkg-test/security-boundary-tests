#[cfg(not(test))]
use std::env;
use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Finding {
    path: PathBuf,
    kind: &'static str,
    detail: String,
}

fn lower_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn python_artifact_reason(path: &Path) -> Option<&'static str> {
    let name = lower_name(path);
    if name.ends_with(".py") || name.ends_with(".pyi") {
        return Some("Python source/stub");
    }
    if name.ends_with(".ipynb") {
        return Some("Python-capable notebook");
    }
    if [
        "pipfile",
        "pipfile.lock",
        "poetry.lock",
        "pytest.ini",
        "tox.ini",
        "setup.py",
        "pyproject.toml",
        "uv.lock",
        ".python-version",
    ]
    .contains(&name.as_str())
    {
        return Some("Python package/tool manifest");
    }
    if name == "requirements.txt"
        || (name.starts_with("requirements-") && name.ends_with(".txt"))
        || (name.starts_with("requirements.") && name.ends_with(".txt"))
    {
        return Some("Python requirements manifest");
    }
    if name == "__pycache__" || name.ends_with(".egg-info") {
        return Some("Python generated/package artifact");
    }
    None
}

fn is_documentation(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("md" | "mdx" | "rst" | "adoc")
    )
}

fn is_scanner_source(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        == "tools/check_no_python.rs"
}

fn should_scan_text(path: &Path) -> bool {
    let name = lower_name(path);
    if [
        "makefile",
        "gnumakefile",
        "cmakelists.txt",
        "dockerfile",
        "justfile",
        "taskfile",
    ]
    .contains(&name.as_str())
    {
        return true;
    }
    /* Extensionless tracked scripts are a common interpreter entry point.
     * They must not bypass the policy just because they omit ".py". */
    if path.extension().is_none() {
        return true;
    }
    matches!(
        path.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
        Some(
            "sh" | "bash" | "zsh" | "fish" | "yml" | "yaml" | "toml" | "json" | "jsonc"
                | "xml" | "gradle" | "kts" | "rs" | "java" | "kt" | "c" | "cc" | "cpp"
                | "h" | "hpp" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "cs"
                | "fs" | "fsx" | "ps1" | "cmd" | "bat" | "cmake" | "mk" | "go" | "swift"
                | "scala"
        )
    )
}

fn command_tokens(line: &str) -> Vec<String> {
    line.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '|' | '&' | ';' | '(' | ')' | '{' | '}' | '[' | ']' | ':' | ',' | '"' | '\''
            )
    })
    .filter(|s| !s.is_empty())
    .map(|s| s.trim_matches('$').to_ascii_lowercase())
    .filter(|s| !s.is_empty())
    .collect()
}

fn executable_basename(token: &str) -> &str {
    token.rsplit('/').next().unwrap_or(token).trim_end_matches(".exe")
}

fn is_python_exec_name(base: &str) -> bool {
    if matches!(base, "python" | "python2" | "python3") {
        return true;
    }
    if let Some(rest) = base.strip_prefix("python") {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '.');
    }
    false
}

fn is_pip_exec_name(base: &str) -> bool {
    if matches!(base, "pip" | "pip2" | "pip3" | "pipenv") {
        return true;
    }
    if let Some(rest) = base.strip_prefix("pip") {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '.');
    }
    false
}

fn python_invocation_reason(line: &str) -> Option<&'static str> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if (trimmed.starts_with('#') && !trimmed.starts_with("#!")) || trimmed.starts_with("//") {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("actions/setup-python@") || lower.contains("python-version:") {
        return Some("GitHub Actions Python setup/configuration");
    }
    if lower.starts_with("#!") && lower.contains("python") {
        return Some("Python shebang");
    }
    if lower.contains("find_package(python")
        || lower.contains("python3_add_")
        || lower.contains("python_add_")
    {
        return Some("build-system Python dependency");
    }
    if lower.contains("pythonpath") || lower.contains("pythonhome") {
        return Some("Python runtime environment dependency");
    }

    let process_api = [
        "command::new(",
        "std::process::command",
        "processbuilder(",
        "runtime.getruntime().exec(",
        "process.start(",
        "child_process",
        "spawn(",
        "execfile(",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    if process_api
        && [
            "\"python\"",
            "\"python3\"",
            "'python'",
            "'python3'",
            "\"pip\"",
            "\"pytest\"",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return Some("host source launches Python tooling");
    }

    let tokens = command_tokens(trimmed);
    let wrapper = |s: &str| {
        matches!(
            s,
            "run" | "shell" | "command" | "exec" | "spawn" | "env" | "sudo" | "time"
                | "xargs" | "sh" | "bash" | "dash" | "zsh"
        )
    };
    let env_assignment = |s: &str| {
        let Some((name, _value)) = s.split_once('=') else {
            return false;
        };
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
    };

    for (i, token) in tokens.iter().enumerate() {
        let base = executable_basename(token);
        let mut command_start = i;
        while command_start > 0 {
            let prior = &tokens[command_start - 1];
            if env_assignment(prior) || prior.starts_with('-') {
                command_start -= 1;
            } else {
                break;
            }
        }
        let command_position = i == 0
            || (command_start > 0
                && wrapper(executable_basename(&tokens[command_start - 1])));

        if command_position && is_python_exec_name(base) {
            return Some("Python interpreter invocation/reference");
        }
        if command_position && is_pip_exec_name(base) {
            return Some("Python package manager invocation/reference");
        }
        if command_position && matches!(base, "pytest" | "poetry") {
            return Some("Python tool invocation/reference");
        }
        if command_position && base == "uv" {
            let next = tokens.get(i + 1).map(String::as_str).unwrap_or_default();
            if matches!(next, "run" | "pip" | "sync" | "lock" | "venv") {
                return Some("Python uv tool invocation/reference");
            }
        }
        if command_position
            && (base.ends_with(".py") || base.ends_with(".pyi"))
            && !token.starts_with("http://")
            && !token.starts_with("https://")
        {
            return Some("Python script reference from executable/build configuration");
        }
    }
    None
}

fn scan_markdown_commands(content: &str) -> Option<String> {
    let mut fenced = false;
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        let bytes = trimmed.as_bytes();
        if bytes.starts_with(&[96, 96, 96]) || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }

        if fenced {
            if let Some(reason) = python_invocation_reason(line) {
                return Some(format!("line {}: {reason}", idx + 1));
            }
        } else {
            let lower = line.to_ascii_lowercase();
            if (lower.contains("requires")
                || lower.contains("requirement")
                || lower.contains("dependency"))
                && lower.contains("python")
            {
                return Some(format!(
                    "line {}: documentation declares a Python dependency",
                    idx + 1
                ));
            }
        }
    }
    None
}

fn scan_file_content(root: &Path, path: &Path, findings: &mut Vec<Finding>) -> io::Result<()> {
    if is_scanner_source(root, path) {
        return Ok(());
    }
    if !should_scan_text(path) && !is_documentation(path) {
        return Ok(());
    }

    let bytes = fs::read(path)?;
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(());
    };
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();

    if is_documentation(path) {
        if let Some(detail) = scan_markdown_commands(&content) {
            findings.push(Finding {
                path: rel,
                kind: "python-reference",
                detail,
            });
        }
        return Ok(());
    }

    for (idx, line) in content.lines().enumerate() {
        if let Some(reason) = python_invocation_reason(line) {
            findings.push(Finding {
                path: rel,
                kind: "python-invocation",
                detail: format!("line {}: {reason}", idx + 1),
            });
            break;
        }
    }
    Ok(())
}

fn scan(root: &Path, path: &Path, findings: &mut Vec<Finding>) -> io::Result<()> {
    let rel = path.strip_prefix(root).unwrap_or(path);
    if rel != Path::new("") {
        if let Some(reason) = python_artifact_reason(rel) {
            findings.push(Finding {
                path: rel.to_path_buf(),
                kind: "python-artifact",
                detail: reason.to_string(),
            });
        }
    }

    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        if path != root && lower_name(path) == ".git" {
            return Ok(());
        }
        for entry in fs::read_dir(path)? {
            scan(root, &entry?.path(), findings)?;
        }
    } else if meta.is_file() {
        scan_file_content(root, path, findings)?;
    }
    Ok(())
}

fn run(root: &Path) -> io::Result<Vec<Finding>> {
    let mut findings = Vec::new();
    scan(root, root, &mut findings)?;
    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.kind.cmp(b.kind))
            .then(a.detail.cmp(&b.detail))
    });
    findings.dedup();
    Ok(findings)
}

#[cfg(not(test))]
fn main() {
    let root = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    match run(&root) {
        Ok(findings) if findings.is_empty() => {
            println!("PASS: no Python artifacts, dependencies, or execution paths detected");
        }
        Ok(findings) => {
            eprintln!("Python is prohibited. Port required functionality to Rust:");
            for finding in findings {
                eprintln!(
                    "  {} [{}] {}",
                    finding.path.display(),
                    finding.kind,
                    finding.detail
                );
            }
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("FAIL-CLOSED: could not completely scan repository: {err}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_python_artifact_names() {
        for path in [
            "x.py",
            "x.pyi",
            "x.ipynb",
            "pyproject.toml",
            "requirements-dev.txt",
            ".python-version",
        ] {
            assert!(python_artifact_reason(Path::new(path)).is_some(), "{path}");
        }
        assert!(python_artifact_reason(Path::new("src/main.rs")).is_none());
    }

    #[test]
    fn rejects_python_execution_paths() {
        for line in [
            "python3 scripts/check.py",
            "uses: actions/setup-python@v5",
            "pip install something",
            "pytest -q",
            "poetry install",
            "uv run tool.py",
            "let x = Command::new(\"python\");",
            "find_package(Python3 REQUIRED)",
            "#!/usr/bin/env python3",
        ] {
            assert!(python_invocation_reason(line).is_some(), "{line}");
        }
    }

    #[test]
    fn rejects_wrapped_python_execution_paths() {
        for line in [
            "env MODE=test python3 tools/check.py",
            "run: MODE=test python3 tools/check.py",
            "bash -c python3 tools/check.py",
        ] {
            assert!(python_invocation_reason(line).is_some(), "{line}");
        }
    }

    #[test]
    fn full_scan_finds_extensionless_python_shebang() {
        let root = std::env::temp_dir().join(format!(
            "check-no-python-extensionless-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("runner"), "#!/usr/bin/env python3\nprint('legacy')\n").unwrap();
        let findings = run(&root).unwrap();
        assert!(findings.iter().any(|f| {
            f.path == Path::new("runner") && f.kind == "python-invocation"
        }));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn permits_policy_labels_and_discussion_comments() {
        assert!(python_invocation_reason("name: Rust-only / no Python").is_none());
        assert!(python_invocation_reason("# Python is prohibited here").is_none());
        assert!(python_invocation_reason("// Python migration policy").is_none());
        assert!(python_invocation_reason("echo pythonic-language").is_none());
    }

    #[test]
    fn markdown_checks_commands_and_declared_dependencies() {
        assert!(scan_markdown_commands("~~~sh\npython3 x.py\n~~~\n").is_some());
        assert!(scan_markdown_commands("Requires Python3 and LLVM.\n").is_some());
        assert!(scan_markdown_commands("Python is prohibited; replacements use Rust.\n").is_none());
    }

    #[test]
    fn full_scan_finds_python_artifact() {
        let root = std::env::temp_dir().join(format!(
            "check-no-python-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("legacy.py"), "print('legacy')\n").unwrap();
        let findings = run(&root).unwrap();
        assert!(findings.iter().any(|f| f.path == Path::new("legacy.py")));
        fs::remove_dir_all(&root).unwrap();
    }
}
