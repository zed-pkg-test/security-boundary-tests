#!/usr/bin/env python3
"""Source-free static admission for reviewed Oreslang carrier test plans."""
import argparse
import importlib.util
import json
from pathlib import Path, PurePosixPath
import re
import tempfile

SCRIPT_DIR = Path(__file__).resolve().parent
PLAN_NAME = re.compile(r"^(java|llvm|format)-pr([1-9][0-9]*)\.json$")
SOURCE_REPOS = {
    "java": "ores-truffle-oreslang/oreslang-source.java",
    "llvm": "ores-truffle-oreslang/oreslang-source.llvm",
    "format": "ores-truffle-oreslang/oreslang-format",
}
ALLOWED_PLAN_FIELDS = {
    "schema", "source_repo", "allowed_prs", "commands",
    "required_source_blobs", "maven_pom_blob_sha",
    "maven_prefetch_artifacts",
}
ALLOWED_COMMAND_FIELDS = {"argv", "cwd", "timeout_seconds", "stdout_contains"}
HEX40 = re.compile(r"^[0-9a-f]{40}$")
MAX_PLAN_BYTES = 64 * 1024
SHARED_FIXTURE = "contracts/v1/type-declarations-conformance-v1.tsv"
REQUIRED_HARNESS_PATHS = {
    "java-pr1947.json": {
        "pom.xml",
        SHARED_FIXTURE,
        "src/test/java/dev/oreslang/SharedTypeDeclarationConformanceTest.java",
        "src/test/java/dev/oreslang/DeclaredTypeOnlySyntaxTest.java",
        "src/test/java/dev/oreslang/AliasExtensionMethodTest.java",
        "src/test/java/dev/oreslang/TypeConstraintDescriptorTest.java",
        "src/test/java/dev/oreslang/ConstraintValidationBridgeTest.java",
    },
    "java-pr2130.json": {
        "pom.xml",
        "src/main/java/dev/oreslang/runtime/AsyncRuntime.java",
        "docs/grammar.ebnf",
        "examples/generator-v1.ores",
        "src/main/java/dev/oreslang/nodes/OresEvalRootNode.java",
        "src/main/java/dev/oreslang/parser/Parser.java",
        "src/main/java/dev/oreslang/runtime/GeneratorRuntime.java",
        "src/main/java/dev/oreslang/types/OwnershipChecker.java",
        "src/main/java/dev/oreslang/types/TypeChecker.java",
        "src/test/java/dev/oreslang/GeneratorCanonicalAdmissionTest.java",
        "src/test/java/dev/oreslang/GeneratorLanguageTest.java",
        "src/test/java/dev/oreslang/runtime/GeneratorRuntimeTest.java",
    },
    "java-pr2131.json": {
        "pom.xml",
        "src/main/java/dev/oreslang/runtime/AsyncRuntime.java",
        "docs/grammar.ebnf",
        "examples/static-generator-v1.ores",
        "src/main/java/dev/oreslang/ast/Ast.java",
        "src/main/java/dev/oreslang/compiler/IncrementalCompiler.java",
        "src/main/java/dev/oreslang/compiler/TreeShaker.java",
        "src/main/java/dev/oreslang/nodes/OresEvalRootNode.java",
        "src/main/java/dev/oreslang/parser/Parser.java",
        "src/main/java/dev/oreslang/runtime/GeneratorRuntime.java",
        "src/main/java/dev/oreslang/types/OwnershipChecker.java",
        "src/main/java/dev/oreslang/types/TypeChecker.java",
        "src/test/java/dev/oreslang/GeneratorCanonicalAdmissionTest.java",
        "src/test/java/dev/oreslang/GeneratorLanguageTest.java",
        "src/test/java/dev/oreslang/runtime/GeneratorRuntimeTest.java",
        "src/test/java/dev/oreslang/IncrementalFunctorStaticTest.java",
        "src/test/java/dev/oreslang/StaticGeneratorFunctionTest.java",
        "src/test/java/dev/oreslang/TreeShakerTest.java",
    },
    "llvm-pr1075.json": {
        "CMakeLists.txt",
        "src/main/cpp/oreslang/backend/LlvmCodegen.cpp",
        "src/main/cpp/oreslang/frontend/Parser.cpp",
        "src/main/cpp/oreslang/frontend/Parser.hpp",
        "tests/native_generator_i64_e2e.rs",
        "tests/fixtures/generator-v1-async-reject.ores",
        "tests/fixtures/generator-v1-suspend-reject.ores",
        "tests/fixtures/generator-v1-legacy-fnc-reject.ores",
        "tests/fixtures/generator-v1-legacy-routine-reject.ores",
        "tests/fixtures/generator-v1-legacy-async-fnc-reject.ores",
    },
    "llvm-pr969.json": {
        SHARED_FIXTURE,
        "CMakeLists.txt",
        "tests/shared_type_declarations_conformance.py",
        "tests/native_type_only_declarations.py",
        "tests/native_type_constraint_descriptors.py",
        "tests/native_alias_extension_methods.py",
    },
    "format-pr91.json": {
        "Cargo.toml",
        SHARED_FIXTURE,
        "tests/type_only_declarations.rs",
        "scripts/check_compiler_parity.py",
        "scripts/test_compiler_parity.py",
        "scripts/verify_private_ci_visibility.py",
        "tests/test_private_ci_visibility.py",
    },
}


def load_module(name, filename):
    spec = importlib.util.spec_from_file_location(name, SCRIPT_DIR / filename)
    if spec is None or spec.loader is None:
        raise RuntimeError("reviewed-plan validator dependency unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


runner = load_module("ores_plan_runner", "run-oreslang-test-plan.py")
prefetch = load_module("ores_plan_prefetch", "prefetch-oreslang-maven-deps.py")
Denied = runner.Denied


def safe_required_source_blobs(value):
    if value is None:
        return {}
    if not isinstance(value, dict):
        raise Denied("reviewed plan required_source_blobs must be an object")
    result = {}
    for path, sha in value.items():
        if (not isinstance(path, str) or not path
                or path.startswith("/") or "\\" in path
                or ".." in PurePosixPath(path).parts
                or ".git" in PurePosixPath(path).parts
                or not isinstance(sha, str) or not HEX40.fullmatch(sha)):
            raise Denied("reviewed plan required source blob is unsafe")
        result[path] = sha
    return result


def prepare_static_cwd(root, command):
    cwd = command.get("cwd", ".")
    if not isinstance(cwd, str) or not cwd:
        raise Denied("reviewed plan command cwd invalid")
    if cwd == ".":
        return
    parts = PurePosixPath(cwd).parts
    if (not parts or cwd.startswith("/") or "\\" in cwd
            or ".." in parts or ".git" in parts):
        raise Denied("reviewed plan command cwd invalid")
    root.joinpath(*parts).mkdir(parents=True, exist_ok=True)


def validate_plan(path):
    path = Path(path)
    match = PLAN_NAME.fullmatch(path.name)
    if match is None or path.parent.as_posix() != "ci/test-plans":
        raise Denied("reviewed plan filename/path must bind backend and PR number")
    backend, pr_text = match.groups()
    source_pr = int(pr_text)
    if path.is_symlink() or not path.is_file():
        raise Denied("reviewed plan missing or unsafe")
    raw = path.read_bytes()
    if not raw or len(raw) > MAX_PLAN_BYTES:
        raise Denied("reviewed plan missing or oversized")
    try:
        plan = runner.parse_json(raw.decode("utf-8"))
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise Denied("reviewed plan JSON invalid") from exc

    if (not isinstance(plan, dict)
            or plan.get("schema") != "ores-private-ci-test-plan-v1"
            or plan.get("source_repo") != SOURCE_REPOS[backend]):
        raise Denied("reviewed plan source/schema mismatch")
    if not set(plan).issubset(ALLOWED_PLAN_FIELDS):
        raise Denied("reviewed plan has unexpected fields")
    if plan.get("allowed_prs") != [source_pr]:
        raise Denied("reviewed plan must authorize exactly its filename PR")

    commands = plan.get("commands")
    if (not isinstance(commands, list)
            or not 1 <= len(commands) <= runner.MAX_COMMANDS):
        raise Denied("reviewed plan command count invalid")

    with tempfile.TemporaryDirectory(prefix="ores-plan-static-") as td:
        root = Path(td)
        for command in commands:
            if (not isinstance(command, dict)
                    or not {"argv", "cwd", "timeout_seconds"}.issubset(command)
                    or not set(command).issubset(ALLOWED_COMMAND_FIELDS)):
                raise Denied("reviewed plan command shape invalid")
            prepare_static_cwd(root, command)
            runner.validate_command(command, root)

    required = safe_required_source_blobs(plan.get("required_source_blobs"))
    mandatory = REQUIRED_HARNESS_PATHS.get(path.name, set())
    missing = sorted(mandatory.difference(required))
    if missing:
        raise Denied(
            "reviewed plan missing mandatory harness pins: " + ", ".join(missing)
        )
    try:
        pom_pin = prefetch.validate_maven_plan(plan)
    except prefetch.Denied as exc:
        raise Denied(str(exc)) from exc
    uses_maven = any(
        isinstance(command, dict)
        and isinstance(command.get("argv"), list)
        and command["argv"]
        and command["argv"][0] == "mvn"
        for command in commands
    )
    explicit_pom = plan.get("maven_pom_blob_sha")
    if uses_maven:
        if not isinstance(explicit_pom, str) or not HEX40.fullmatch(explicit_pom):
            raise Denied("reviewed Maven plan requires exact POM blob")
        old_pom = required.get("pom.xml")
        if old_pom is not None and old_pom != explicit_pom:
            raise Denied("reviewed Maven POM pins disagree")
        if pom_pin != explicit_pom:
            raise Denied("reviewed Maven controller POM pin mismatch")
    elif explicit_pom is not None:
        raise Denied("non-Maven reviewed plan cannot bind Maven POM")

    print(
        f"REVIEWED_PLAN_STATIC_PASS path={path.as_posix()} "
        f"source={plan['source_repo']} pr={source_pr} commands={len(commands)}"
    )
    return plan


def validate_plan_set(paths):
    validated = [(Path(path), validate_plan(path)) for path in paths]
    fixture_pins = {
        plan.get("required_source_blobs", {}).get(SHARED_FIXTURE)
        for _path, plan in validated
        if SHARED_FIXTURE in plan.get("required_source_blobs", {})
    }
    if len(fixture_pins) > 1:
        raise Denied("reviewed plans disagree on shared conformance fixture blob")
    return validated


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plans", nargs="+")
    args = parser.parse_args(argv)
    try:
        validate_plan_set(args.plans)
    except (Denied, OSError, ValueError) as exc:
        print("REVIEWED_PLAN_STATIC_DENIED: " + str(exc))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
