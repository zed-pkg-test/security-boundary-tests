#!/usr/bin/env python3
"""Validate a reviewed Java plan/POM and prefetch Maven dependencies source-free."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

HEX40 = re.compile(r"^[0-9a-f]{40}$")
MAVEN_ARTIFACT = re.compile(r"^[A-Za-z0-9_.-]+:[A-Za-z0-9_.-]+:[A-Za-z0-9_.-]+(?::[A-Za-z0-9_.-]+(?::[A-Za-z0-9_.-]+)?)?$")
MAX_PREFETCH_ARTIFACTS = 16
MAVEN_PLUGIN = "org.apache.maven.plugins:maven-dependency-plugin:3.8.1:go-offline"
MAVEN_GET_PLUGIN = "org.apache.maven.plugins:maven-dependency-plugin:3.8.1:get"
FORBIDDEN_XML = {
    "repositories", "pluginRepositories", "distributionManagement",
    "extensions", "modules", "systemPath", "parent",
}


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


def plan_from_receipt(receipt_path):
    try:
        receipt = parse_json(Path(receipt_path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise Denied("invalid receipt") from exc
    if not isinstance(receipt, dict) or receipt.get("schema") != "ores-private-ci-receipt-v3":
        raise Denied("receipt schema")
    test_plan = receipt.get("test_plan")
    if not isinstance(test_plan, dict):
        raise Denied("receipt test plan")
    path = test_plan.get("path")
    if (not isinstance(path, str)
            or not path.startswith("ci/test-plans/")
            or not path.endswith(".json")
            or ".." in Path(path).parts
            or ".git" in Path(path).parts
            or "\\" in path):
        raise Denied("unsafe test plan path")
    plan_file = Path(path)
    raw = plan_file.read_bytes()
    if hashlib.sha256(raw).hexdigest() != test_plan.get("sha256"):
        raise Denied("test plan digest mismatch")
    return path


def load_plan(path):
    try:
        plan = parse_json(Path(path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise Denied("invalid test plan") from exc
    if not isinstance(plan, dict) or plan.get("schema") != "ores-private-ci-test-plan-v1":
        raise Denied("test plan schema")
    commands = plan.get("commands")
    if not isinstance(commands, list) or not commands:
        raise Denied("test plan commands")
    return plan


def maven_commands(plan):
    result = []
    for command in plan["commands"]:
        if not isinstance(command, dict):
            raise Denied("command entry")
        argv = command.get("argv")
        if not isinstance(argv, list) or not argv:
            raise Denied("command argv")
        if argv[0] == "mvn":
            result.append(argv)
    return result


def validate_maven_plan(plan):
    commands = maven_commands(plan)
    if not commands:
        extras = plan.get("maven_prefetch_artifacts", [])
        if extras not in ([], None):
            raise Denied("non-Maven plan cannot request Maven prefetch artifacts")
        return None
    pin = plan.get("maven_pom_blob_sha")
    if not isinstance(pin, str) or not HEX40.fullmatch(pin):
        raise Denied("Maven plan requires exact maven_pom_blob_sha")
    for argv in commands:
        if not any(arg in ("-o", "--offline") for arg in argv[1:]):
            raise Denied("Maven source command requires offline mode")
        if not any(arg in ("-C", "--strict-checksums") for arg in argv[1:]):
            raise Denied("Maven source command requires strict checksums")
        forbidden_prefixes = (
            "-s", "-gs", "-f",
            "--settings", "--global-settings", "--file",
            "-Dmaven.repo.local", "-DskipTests",
            "-Dmaven.test.skip", "-DskipITs",
        )
        for arg in argv[1:]:
            if any(arg == flag or arg.startswith(flag + "=")
                   or (flag in ("-s", "-gs", "-f") and arg.startswith(flag) and arg != flag)
                   for flag in forbidden_prefixes):
                raise Denied("Maven source command overrides trusted build controls")
    prefetch_artifacts(plan)
    return pin


def prefetch_artifacts(plan):
    values = plan.get("maven_prefetch_artifacts", [])
    if values is None:
        values = []
    if (not isinstance(values, list)
            or len(values) > MAX_PREFETCH_ARTIFACTS
            or any(not isinstance(value, str)
                   or len(value) > 256
                   or not MAVEN_ARTIFACT.fullmatch(value)
                   for value in values)
            or len(set(values)) != len(values)):
        raise Denied("invalid Maven prefetch artifact list")
    return list(values)


def validate_pom(path, expected_blob):
    pom = Path(path)
    if pom.is_symlink() or not pom.is_file():
        raise Denied("reviewed pom missing or unsafe")
    raw = pom.read_bytes()
    if git_blob_sha(raw) != expected_blob:
        raise Denied("reviewed pom Git blob mismatch")
    try:
        root = ET.fromstring(raw)
    except ET.ParseError as exc:
        raise Denied("invalid Maven pom XML") from exc

    def local(tag):
        return tag.rsplit("}", 1)[-1]

    for element in root.iter():
        name = local(element.tag)
        if name in FORBIDDEN_XML:
            raise Denied("pom contains unreviewed repository/build-extension surface: " + name)
        if name == "plugin":
            fields = {
                local(child.tag): (child.text or "").strip()
                for child in element
                if local(child.tag) in ("groupId", "artifactId", "extensions")
            }
            if fields.get("extensions", "").lower() == "true":
                raise Denied("pom plugin build extension is not allowed")
            group = fields.get("groupId", "org.apache.maven.plugins")
            if (group == "org.apache.maven.plugins"
                    and fields.get("artifactId") == "maven-dependency-plugin"):
                raise Denied("pom may not configure trusted dependency prefetch plugin")
    return raw


def write_settings(path):
    Path(path).write_text("""<settings xmlns="http://maven.apache.org/SETTINGS/1.0.0">
  <mirrors>
    <mirror>
      <id>central-only</id>
      <mirrorOf>*</mirrorOf>
      <url>https://repo.maven.apache.org/maven2</url>
    </mirror>
  </mirrors>
  <profiles>
    <profile>
      <id>strict-central</id>
      <repositories>
        <repository>
          <id>central</id>
          <url>https://repo.maven.apache.org/maven2</url>
          <releases><enabled>true</enabled><checksumPolicy>fail</checksumPolicy></releases>
          <snapshots><enabled>false</enabled></snapshots>
        </repository>
      </repositories>
      <pluginRepositories>
        <pluginRepository>
          <id>central</id>
          <url>https://repo.maven.apache.org/maven2</url>
          <releases><enabled>true</enabled><checksumPolicy>fail</checksumPolicy></releases>
          <snapshots><enabled>false</enabled></snapshots>
        </pluginRepository>
      </pluginRepositories>
    </profile>
  </profiles>
  <activeProfiles><activeProfile>strict-central</activeProfile></activeProfiles>
</settings>
""", encoding="utf-8")


def write_empty_global_settings(path):
    Path(path).write_text(
        '<settings xmlns="http://maven.apache.org/SETTINGS/1.0.0"></settings>\n',
        encoding="utf-8",
    )


def clean_maven_environment(home, tmp):
    env = {}
    for key in ("PATH", "JAVA_HOME", "GRAALVM_HOME", "LANG", "LC_ALL"):
        value = os.environ.get(key)
        if value:
            env[key] = value
    env.update({
        "CI": "true",
        "NO_COLOR": "1",
        "HOME": str(home),
        "XDG_CONFIG_HOME": str(home / "config"),
        "XDG_CACHE_HOME": str(home / "cache"),
        "TMPDIR": str(tmp),
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_CONFIG_NOSYSTEM": "1",
    })
    for path in (home, home / "config", home / "cache", tmp):
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
    return env


def prefetch(plan_path, pom_path, repository_path):
    plan = load_plan(plan_path)
    pin = validate_maven_plan(plan)
    repo = Path(repository_path)
    repo.mkdir(parents=True, exist_ok=True)
    if repo.is_symlink() or not repo.is_dir():
        raise Denied("dependency repository path is unsafe")
    if pin is None:
        (repo / ".oreslang-no-maven").write_text("source-free\n", encoding="utf-8")
        return False

    reviewed_pom = validate_pom(pom_path, pin)
    with tempfile.TemporaryDirectory(prefix="oreslang-maven-prefetch-") as td:
        root = Path(td)
        settings = root / "settings.xml"
        global_settings = root / "global-settings.xml"
        project = root / "project"
        home = root / "home"
        tmp = root / "tmp"
        project.mkdir(mode=0o700)
        isolated_pom = project / "pom.xml"
        isolated_pom.write_bytes(reviewed_pom)
        write_settings(settings)
        write_empty_global_settings(global_settings)
        env = clean_maven_environment(home, tmp)

        common = [
            "mvn", "-B", "-ntp", "-C", "-N",
            "-s", str(settings),
            "-gs", str(global_settings),
            "-Duser.home=" + str(home),
            "-Djava.io.tmpdir=" + str(tmp),
            "-Dmaven.repo.local=" + str(repo.resolve()),
            "-f", str(isolated_pom),
        ]
        argv = [*common, MAVEN_PLUGIN]
        try:
            result = subprocess.run(
                argv,
                check=False,
                stdin=subprocess.DEVNULL,
                cwd=project,
                env=env,
                start_new_session=True,
            )
        except OSError as exc:
            raise Denied("Maven unavailable for dependency prefetch") from exc
        if result.returncode != 0:
            raise Denied("Maven dependency prefetch failed")

        for artifact in prefetch_artifacts(plan):
            argv = [
                *common,
                MAVEN_GET_PLUGIN,
                "-Dartifact=" + artifact,
                "-Dtransitive=true",
            ]
            try:
                result = subprocess.run(
                    argv,
                    check=False,
                    stdin=subprocess.DEVNULL,
                    cwd=project,
                    env=env,
                    start_new_session=True,
                )
            except OSError as exc:
                raise Denied("Maven unavailable for explicit artifact prefetch") from exc
            if result.returncode != 0:
                raise Denied("explicit Maven artifact prefetch failed: " + artifact)
    (repo / ".oreslang-prefetch-complete").write_text(pin + "\n", encoding="utf-8")
    return True


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--plan")
    source.add_argument("--receipt")
    parser.add_argument("--pom")
    parser.add_argument("--repository")
    parser.add_argument("--validate-only", action="store_true")
    parser.add_argument("--describe", action="store_true")
    args = parser.parse_args(argv)
    try:
        plan_path = args.plan or plan_from_receipt(args.receipt)
        plan = load_plan(plan_path)
        pin = validate_maven_plan(plan)
        if args.describe:
            print("PLAN_PATH=" + plan_path)
            print("MAVEN_REQUIRED=" + ("true" if pin is not None else "false"))
            print("POM_BLOB=" + (pin or ""))
            return 0
        if pin is not None:
            if not args.pom:
                raise Denied("reviewed pom is required for Maven plan")
            validate_pom(args.pom, pin)
        if args.validate_only:
            print("MAVEN_PREFETCH_VALIDATION_PASS")
            return 0
        if not args.repository:
            raise Denied("dependency repository path is required")
        used = prefetch(plan_path, args.pom or "", args.repository)
        print("MAVEN_PREFETCH_PASS used=" + ("true" if used else "false"))
        return 0
    except (Denied, OSError, json.JSONDecodeError) as exc:
        print("MAVEN_PREFETCH_DENIED: " + str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
