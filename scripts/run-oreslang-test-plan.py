#!/usr/bin/env python3
"""Execute a reviewed Oreslang private-CI test plan without shell evaluation."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import tempfile
import sys

MAX_COMMANDS = 16
MAX_ARGS = 64
MAX_ARG_LENGTH = 4096
MAX_TIMEOUT = 1800
MAX_LOG_CHARS = 64 * 1024
HEX40 = re.compile(r"^[0-9a-f]{40}$")
SAFE_ENV_KEYS = ("PATH", "JAVA_HOME", "GRAALVM_HOME", "DOTNET_ROOT", "LANG", "LC_ALL", "HOME")
SAFE_EXECUTABLE = re.compile(r"^(?:python3|mvn|cmake|ctest|cargo|node|dotnet|g\+\+|clang\+\+|/tmp/ores-[A-Za-z0-9_.-]+)$")


class Denied(RuntimeError):
    pass


def unique_json_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise Denied("duplicate JSON object member")
        result[key] = value
    return result


def reject_non_json_constant(value):
    raise Denied("invalid JSON number")


def parse_json(raw):
    return json.loads(
        raw,
        object_pairs_hook=unique_json_object,
        parse_constant=reject_non_json_constant,
    )


def git_blob_sha(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode("ascii") + b"\0" + data).hexdigest()


def safe_plan_path(value):
    if not isinstance(value, str):
        raise Denied("test plan path")
    parts = PurePosixPath(value).parts
    if (not value.startswith("ci/test-plans/")
            or not value.endswith(".json")
            or not parts
            or ".." in parts
            or ".git" in parts
            or "\\" in value):
        raise Denied("unsafe test plan path")
    return value


def load_context(receipt_path):
    try:
        receipt = parse_json(Path(receipt_path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise Denied("invalid receipt") from exc
    if not isinstance(receipt, dict) or receipt.get("schema") != "ores-private-ci-receipt-v3":
        raise Denied("receipt schema")
    source = receipt.get("source")
    test_plan = receipt.get("test_plan")
    if not isinstance(source, dict) or not isinstance(test_plan, dict):
        raise Denied("receipt source/test plan")
    plan_path = safe_plan_path(test_plan.get("path"))
    plan_file = Path(plan_path)
    if plan_file.is_symlink() or not plan_file.is_file():
        raise Denied("test plan missing or unsafe")
    try:
        plan_bytes = plan_file.read_bytes()
    except OSError as exc:
        raise Denied("test plan unreadable") from exc
    expected_digest = test_plan.get("sha256")
    actual_digest = hashlib.sha256(plan_bytes).hexdigest()
    if expected_digest != actual_digest:
        raise Denied("test plan digest mismatch")
    try:
        plan = parse_json(plan_bytes.decode("utf-8"))
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise Denied("invalid test plan") from exc
    if not isinstance(plan, dict) or plan.get("schema") != "ores-private-ci-test-plan-v1":
        raise Denied("test plan schema")
    if plan.get("source_repo") != source.get("repo"):
        raise Denied("test plan source repository mismatch")
    allowed = plan.get("allowed_prs")
    if (not isinstance(allowed, list)
            or source.get("pr") not in allowed
            or any(not isinstance(value, int) or isinstance(value, bool) or value <= 0
                   for value in allowed)):
        raise Denied("source PR not authorized by test plan")
    return receipt, plan


def confined_cwd(root, relative):
    if not isinstance(relative, str) or not relative:
        raise Denied("command cwd")
    if relative == ".":
        candidate = root
    else:
        parts = PurePosixPath(relative).parts
        if not parts or ".." in parts or ".git" in parts or "\\" in relative:
            raise Denied("unsafe command cwd")
        candidate = root.joinpath(*parts)
    try:
        root_real = root.resolve(strict=True)
        candidate_real = candidate.resolve(strict=True)
        candidate_real.relative_to(root_real)
    except (OSError, RuntimeError, ValueError) as exc:
        raise Denied("command cwd escapes mirrored source") from exc
    if not candidate_real.is_dir():
        raise Denied("command cwd is not a directory")
    return candidate_real


def validate_maven_argv(argv):
    if not any(arg in ("-o", "--offline") for arg in argv[1:]):
        raise Denied("Maven requires an explicit offline argument")
    if not any(arg in ("-C", "--strict-checksums") for arg in argv[1:]):
        raise Denied("Maven requires strict checksums")
    short_with_values = ("-s", "-gs", "-f")
    long_with_values = ("--settings", "--global-settings", "--file")
    dangerous_props = (
        "-Dmaven.repo.local", "-DskipTests", "-Dmaven.test.skip", "-DskipITs",
    )
    for arg in argv[1:]:
        if (any(arg == flag or (arg.startswith(flag) and arg != flag)
                for flag in short_with_values)
                or any(arg == flag or arg.startswith(flag + "=")
                       for flag in long_with_values)
                or any(arg == prop or arg.startswith(prop + "=")
                       for prop in dangerous_props)):
            raise Denied("Maven command overrides trusted build controls")


NODE_TEST_PATH = re.compile(r"^tests/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*\.test\.mjs$")


def validate_node_argv(argv):
    if len(argv) < 2 or argv[1] != "--test":
        raise Denied("Node command must use the built-in test runner")
    for arg in argv[2:]:
        if arg.startswith("-"):
            raise Denied("Node test options are not approved")
        path = PurePosixPath(arg)
        if (not NODE_TEST_PATH.fullmatch(arg)
                or path.is_absolute()
                or any(part in ("", ".", "..") for part in path.parts)):
            raise Denied("Node test path is not approved")


DOTNET_GENERATOR_COMMAND = [
    "dotnet", "run",
    "--project", "verification/Ores.Net.Generator.Tests/Ores.Net.Generator.Tests.csproj",
    "--configuration", "Release",
    "--disable-build-servers",
    "-p:RestoreIgnoreFailedSources=true",
    "-p:NuGetAudit=false",
]


def validate_dotnet_argv(argv):
    if argv != DOTNET_GENERATOR_COMMAND:
        raise Denied("dotnet command is not the exact reviewed generator verifier")


CARGO_ALLOWED_SUBCOMMANDS = {"fmt", "test", "clippy", "rustdoc", "build"}
CARGO_OFFLINE_SUBCOMMANDS = {"test", "clippy", "rustdoc", "build"}


def validate_cargo_argv(argv):
    if len(argv) < 2 or argv[1] not in CARGO_ALLOWED_SUBCOMMANDS:
        raise Denied("Cargo subcommand is not approved")
    if (argv[1] in CARGO_OFFLINE_SUBCOMMANDS
            and "--offline" not in argv[2:]):
        raise Denied("Cargo source command requires offline mode")
    forbidden = (
        "--manifest-path", "--config", "--target-dir", "--registry",
        "--index", "--root", "--git", "--path",
    )
    for arg in argv[2:]:
        if any(arg == flag or arg.startswith(flag + "=") for flag in forbidden):
            raise Denied("Cargo command overrides trusted project controls")


def validate_command(command, root):
    if not isinstance(command, dict):
        raise Denied("command entry")
    argv = command.get("argv")
    if (not isinstance(argv, list) or not argv or len(argv) > MAX_ARGS
            or any(not isinstance(arg, str) or not arg or len(arg) > MAX_ARG_LENGTH
                   or "\x00" in arg for arg in argv)):
        raise Denied("command argv")
    if not SAFE_EXECUTABLE.fullmatch(argv[0]):
        raise Denied("unapproved executable: " + argv[0])
    if argv[0] == "mvn":
        validate_maven_argv(argv)
    if argv[0] == "cargo":
        validate_cargo_argv(argv)
    if argv[0] == "node":
        validate_node_argv(argv)
    if argv[0] == "dotnet":
        validate_dotnet_argv(argv)
    timeout = command.get("timeout_seconds")
    if (not isinstance(timeout, int) or isinstance(timeout, bool)
            or timeout < 1 or timeout > MAX_TIMEOUT):
        raise Denied("command timeout")
    cwd = confined_cwd(root, command.get("cwd", "."))
    expected = command.get("stdout_contains")
    if expected is not None and (not isinstance(expected, str)
                                 or not expected
                                 or len(expected) > 4096):
        raise Denied("stdout assertion")
    return list(argv), cwd, timeout, expected


def validate_gitless_source(source_root):
    if source_root.is_symlink() or not source_root.is_dir():
        raise Denied("mirrored source directory missing or unsafe")
    source_root = source_root.resolve(strict=True)
    for candidate in source_root.rglob(".git"):
        raise Denied("source sandbox contains Git metadata: " + str(candidate))
    return source_root


def validate_home(source_root):
    home_text = os.environ.get("HOME")
    if not home_text:
        raise Denied("clean HOME is required")
    home = Path(home_text)
    if home.is_symlink() or not home.is_dir():
        raise Denied("HOME is missing or unsafe")
    home = home.resolve(strict=True)
    try:
        home.relative_to(source_root)
    except ValueError:
        return home
    raise Denied("HOME must be outside source sandbox")


def validate_cargo_project(plan, source_root):
    cargo = any(isinstance(command, dict)
                and isinstance(command.get("argv"), list)
                and command["argv"] and command["argv"][0] == "cargo"
                for command in plan.get("commands", []))
    if cargo and (source_root / ".cargo").exists():
        raise Denied("project .cargo configuration is not allowed in reviewed Rust carrier")
    return cargo


def validate_maven_repository(plan, source_root, dependency_repo):
    maven = any(isinstance(command, dict)
                and isinstance(command.get("argv"), list)
                and command["argv"] and command["argv"][0] == "mvn"
                for command in plan.get("commands", []))
    if not maven:
        return None
    pin = plan.get("maven_pom_blob_sha")
    if not isinstance(pin, str) or not HEX40.fullmatch(pin):
        raise Denied("Maven plan requires exact maven_pom_blob_sha")
    if (source_root / ".mvn").exists():
        raise Denied("project .mvn configuration is not allowed in reviewed Maven carrier")
    pom = source_root / "pom.xml"
    if pom.is_symlink() or not pom.is_file():
        raise Denied("source Maven pom missing or unsafe")
    if git_blob_sha(pom.read_bytes()) != pin:
        raise Denied("source Maven pom differs from reviewed blob")

    if dependency_repo is None:
        raise Denied("offline Maven repository is required")
    repo = Path(dependency_repo)
    if repo.is_symlink() or not repo.is_dir():
        raise Denied("offline Maven repository missing or unsafe")
    repo = repo.resolve(strict=True)
    try:
        repo.relative_to(source_root)
    except ValueError:
        pass
    else:
        raise Denied("offline Maven repository must be outside source sandbox")
    marker = repo / ".oreslang-prefetch-complete"
    if not marker.is_file() or marker.read_text(encoding="utf-8").strip() != pin:
        raise Denied("offline Maven repository is not bound to reviewed pom")
    return repo


def clean_environment(home):
    env = {key: os.environ[key] for key in SAFE_ENV_KEYS if key in os.environ}
    env.update({
        "CI": "true",
        "NO_COLOR": "1",
        "HOME": str(home),
        "XDG_CONFIG_HOME": str(home / "config"),
        "XDG_CACHE_HOME": str(home / "cache"),
        "TMPDIR": str(home / "tmp"),
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_CONFIG_NOSYSTEM": "1",
        "CARGO_NET_OFFLINE": "true",
        "CARGO_HOME": str(home / "cargo"),
        "DOTNET_CLI_HOME": str(home / "dotnet"),
        "DOTNET_CLI_TELEMETRY_OPTOUT": "1",
        "DOTNET_SKIP_FIRST_TIME_EXPERIENCE": "1",
        "NUGET_XMLDOC_MODE": "skip",
    })
    for path in (
            home / "config", home / "cache", home / "tmp",
            home / "cargo", home / "dotnet"):
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
    return env


def fresh_command_home(base_home, index):
    root = base_home / "tmp"
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        return Path(tempfile.mkdtemp(prefix=f"ores-home-{index}-", dir=root))
    except OSError as exc:
        raise Denied(f"cannot prepare isolated HOME for command {index}") from exc


def fresh_command_source(source_root, index, home):
    for candidate in source_root.rglob("*"):
        if candidate.is_symlink():
            raise Denied("mirrored source contains symlink")
    base = home / "tmp"
    base.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        command_root = Path(tempfile.mkdtemp(prefix=f"ores-plan-{index}-", dir=base))
        shutil.copytree(source_root, command_root, dirs_exist_ok=True, symlinks=False)
        for candidate in [command_root, *command_root.rglob("*")]:
            mode = candidate.stat().st_mode
            if candidate.is_dir():
                candidate.chmod(mode | stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
            elif candidate.is_file():
                candidate.chmod(mode | stat.S_IRUSR | stat.S_IWUSR)
        return command_root
    except (OSError, shutil.Error) as exc:
        raise Denied(f"cannot prepare isolated source for command {index}") from exc


def fresh_command_dependency_repo(repository, index, home):
    if repository is None:
        return None
    for candidate in repository.rglob("*"):
        if candidate.is_symlink():
            raise Denied("reviewed Maven repository contains symlink")
    base = home / "tmp"
    base.mkdir(mode=0o700, parents=True, exist_ok=True)
    try:
        target = Path(tempfile.mkdtemp(prefix=f"ores-m2-{index}-", dir=base))
        shutil.copytree(repository, target, dirs_exist_ok=True, symlinks=False)
        for candidate in [target, *target.rglob("*")]:
            mode = candidate.stat().st_mode
            if candidate.is_dir():
                candidate.chmod(mode | stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
            elif candidate.is_file():
                candidate.chmod(mode | stat.S_IRUSR | stat.S_IWUSR)
        return target
    except (OSError, shutil.Error) as exc:
        raise Denied(f"cannot prepare isolated dependency repository for command {index}") from exc


def run_plan(receipt_path="ci/oreslang-receipt.json",
             source_dir="private-oreslang-source",
             dependency_repo=None):
    receipt, plan = load_context(receipt_path)
    commands = plan.get("commands")
    if not isinstance(commands, list) or not commands or len(commands) > MAX_COMMANDS:
        raise Denied("test plan command count")
    source_root = validate_gitless_source(Path(source_dir))
    home = validate_home(source_root)
    validate_cargo_project(plan, source_root)
    maven_repo = validate_maven_repository(plan, source_root, dependency_repo)

    results = []
    for index, command in enumerate(commands, start=1):
        argv, admitted_cwd, timeout, expected = validate_command(command, source_root)
        relative_cwd = admitted_cwd.relative_to(source_root)
        command_home = fresh_command_home(home, index)
        env = clean_environment(command_home)
        command_root = fresh_command_source(source_root, index, command_home)
        command_cwd = command_root.joinpath(relative_cwd)
        command_maven_repo = None
        effective_argv = argv
        if argv[0] == "mvn":
            command_maven_repo = fresh_command_dependency_repo(
                maven_repo, index, command_home
            )
            effective_argv = [
                "mvn",
                "-Dmaven.repo.local=" + str(command_maven_repo),
                "-Duser.home=" + str(command_home),
                *argv[1:],
            ]
        print(f"PLAN_COMMAND_{index}_START executable={argv[0]} cwd={relative_cwd}")
        try:
            completed = subprocess.run(
                effective_argv,
                cwd=command_cwd,
                env=env,
                text=True,
                capture_output=True,
                timeout=timeout,
                check=False,
                stdin=subprocess.DEVNULL,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise Denied(f"command {index} unavailable or timed out") from exc
        finally:
            shutil.rmtree(command_root, ignore_errors=True)
            if command_maven_repo is not None:
                shutil.rmtree(command_maven_repo, ignore_errors=True)
            shutil.rmtree(command_home, ignore_errors=True)
        stdout = completed.stdout[-MAX_LOG_CHARS:]
        stderr = completed.stderr[-MAX_LOG_CHARS:]
        if stdout:
            print(stdout, end="" if stdout.endswith("\n") else "\n")
        if stderr:
            print(stderr, file=sys.stderr, end="" if stderr.endswith("\n") else "\n")
        if completed.returncode != 0:
            raise Denied(f"command {index} failed with exit {completed.returncode}")
        if expected is not None and expected not in completed.stdout:
            raise Denied(f"command {index} missing required stdout evidence")
        print(f"PLAN_COMMAND_{index}_PASS")
        results.append({"index": index, "argv": argv, "returncode": completed.returncode})
    print(f"TEST_PLAN_PASS commands={len(results)} source={receipt['source']['repo']} pr={receipt['source']['pr']}")
    return results


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", default="ci/oreslang-receipt.json")
    parser.add_argument("--source-dir", default="private-oreslang-source")
    parser.add_argument("--dependency-repo")
    args = parser.parse_args(argv)
    try:
        run_plan(args.receipt, args.source_dir, args.dependency_repo)
    except (Denied, OSError) as exc:
        print("TEST_PLAN_DENIED: " + str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
