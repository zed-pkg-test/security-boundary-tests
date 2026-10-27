#!/usr/bin/env python3
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("validate-oreslang-reviewed-plans.py")
spec = importlib.util.spec_from_file_location("reviewed_plan_validator", SCRIPT)
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


class ReviewedPlanValidatorTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.old_cwd = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(lambda: os.chdir(self.old_cwd))
        (self.root / "ci/test-plans").mkdir(parents=True)

    def write(self, name, plan):
        path = Path("ci/test-plans") / name
        path.write_text(json.dumps(plan) + "\n", encoding="utf-8")
        return path

    def java_plan(self):
        return {
            "schema": "ores-private-ci-test-plan-v1",
            "source_repo": "ores-truffle-oreslang/oreslang-source.java",
            "allowed_prs": [1947],
            "commands": [{
                "argv": ["mvn", "-o", "-C", "-B", "test"],
                "cwd": ".",
                "timeout_seconds": 300,
                "stdout_contains": "BUILD SUCCESS",
            }],
            "maven_pom_blob_sha": "a" * 40,
            "required_source_blobs": {
                "pom.xml": "a" * 40,
                validator.SHARED_FIXTURE: "c" * 40,
                "src/test/java/dev/oreslang/SharedTypeDeclarationConformanceTest.java": "1" * 40,
                "src/test/java/dev/oreslang/DeclaredTypeOnlySyntaxTest.java": "2" * 40,
                "src/test/java/dev/oreslang/AliasExtensionMethodTest.java": "3" * 40,
                "src/test/java/dev/oreslang/TypeConstraintDescriptorTest.java": "4" * 40,
                "src/test/java/dev/oreslang/ConstraintValidationBridgeTest.java": "5" * 40,
            },
            "maven_prefetch_artifacts": [
                "org.apache.maven.surefire:surefire-junit-platform:3.5.2",
            ],
        }

    def llvm_plan(self):
        return {
            "schema": "ores-private-ci-test-plan-v1",
            "source_repo": "ores-truffle-oreslang/oreslang-source.llvm",
            "allowed_prs": [969],
            "commands": [{
                "argv": ["python3", "-c", "print('ok')"],
                "cwd": ".",
                "timeout_seconds": 300,
                "stdout_contains": "ok",
            }],
            "required_source_blobs": {
                validator.SHARED_FIXTURE: "c" * 40,
                "CMakeLists.txt": "6" * 40,
                "tests/shared_type_declarations_conformance.py": "7" * 40,
                "tests/native_type_only_declarations.py": "8" * 40,
                "tests/native_type_constraint_descriptors.py": "9" * 40,
                "tests/native_alias_extension_methods.py": "a" * 40,
            },
        }


    def format_plan(self):
        return {
            "schema": "ores-private-ci-test-plan-v1",
            "source_repo": "ores-truffle-oreslang/oreslang-format",
            "allowed_prs": [91],
            "commands": [{
                "argv": ["cargo", "test", "--offline", "--test", "type_only_declarations"],
                "cwd": ".",
                "timeout_seconds": 300,
            }],
            "required_source_blobs": {
                "Cargo.toml": "b" * 40,
                validator.SHARED_FIXTURE: "c" * 40,
                "tests/type_only_declarations.rs": "1" * 40,
                "scripts/check_compiler_parity.py": "2" * 40,
                "scripts/test_compiler_parity.py": "3" * 40,
                "scripts/verify_private_ci_visibility.py": "4" * 40,
                "tests/test_private_ci_visibility.py": "5" * 40,
            },
        }

    def test_valid_java_llvm_and_format_plans(self):
        validator.validate_plan(self.write("java-pr1947.json", self.java_plan()))
        validator.validate_plan(self.write("llvm-pr969.json", self.llvm_plan()))
        validator.validate_plan(self.write("format-pr91.json", self.format_plan()))

    def test_filename_binds_repo_and_exact_single_pr(self):
        cases = []
        doc = self.java_plan()
        doc["allowed_prs"] = [1947, 2000]
        cases.append(("java-pr1947.json", doc))
        doc = self.java_plan()
        doc["allowed_prs"] = [1948]
        cases.append(("java-pr1947.json", doc))
        doc = self.java_plan()
        doc["source_repo"] = "ores-truffle-oreslang/oreslang-source.llvm"
        cases.append(("java-pr1947.json", doc))
        cases.append(("other-pr1947.json", self.java_plan()))
        for index, (name, plan) in enumerate(cases):
            with self.subTest(name=name, index=index):
                path = self.write(name, plan)
                with self.assertRaises(validator.Denied):
                    validator.validate_plan(path)

    def test_unexpected_fields_and_unsafe_commands_fail(self):
        doc = self.llvm_plan()
        doc["network"] = "allow"
        with self.assertRaisesRegex(validator.Denied, "unexpected fields"):
            validator.validate_plan(self.write("llvm-pr969.json", doc))

        for argv in (
            ["bash", "-lc", "true"],
            ["sh", "-c", "true"],
            ["git", "status"],
            ["curl", "https://example.invalid"],
        ):
            with self.subTest(argv=argv):
                doc = self.llvm_plan()
                doc["commands"][0]["argv"] = argv
                with self.assertRaises(validator.Denied):
                    validator.validate_plan(self.write("llvm-pr969.json", doc))

        doc = self.llvm_plan()
        doc["commands"][0]["cwd"] = "../escape"
        with self.assertRaises(validator.Denied):
            validator.validate_plan(self.write("llvm-pr969.json", doc))

        doc = self.llvm_plan()
        doc["commands"][0]["ignored"] = True
        with self.assertRaisesRegex(validator.Denied, "command shape"):
            validator.validate_plan(self.write("llvm-pr969.json", doc))

    def test_maven_requires_offline_strict_checksums_and_pom(self):
        mutations = [
            (["mvn", "-C", "-B", "test"], "offline"),
            (["mvn", "-o", "-B", "test"], "strict checksums"),
        ]
        for argv, message in mutations:
            with self.subTest(argv=argv):
                doc = self.java_plan()
                doc["commands"][0]["argv"] = argv
                with self.assertRaisesRegex(validator.Denied, message):
                    validator.validate_plan(self.write("java-pr1947.json", doc))

        doc = self.java_plan()
        doc.pop("maven_pom_blob_sha")
        with self.assertRaisesRegex(validator.Denied, "maven_pom_blob_sha|POM"):
            validator.validate_plan(self.write("java-pr1947.json", doc))

    def test_maven_pom_pins_must_agree(self):
        doc = self.java_plan()
        doc["required_source_blobs"]["pom.xml"] = "b" * 40
        with self.assertRaisesRegex(validator.Denied, "POM pins disagree"):
            validator.validate_plan(self.write("java-pr1947.json", doc))

    def test_non_maven_plan_rejects_maven_metadata(self):
        doc = self.llvm_plan()
        doc["maven_pom_blob_sha"] = "a" * 40
        with self.assertRaisesRegex(validator.Denied, "non-Maven"):
            validator.validate_plan(self.write("llvm-pr969.json", doc))

        doc = self.llvm_plan()
        doc["maven_prefetch_artifacts"] = ["g:a:v"]
        with self.assertRaisesRegex(validator.Denied, "non-Maven"):
            validator.validate_plan(self.write("llvm-pr969.json", doc))

    def test_mandatory_harness_pins_cannot_be_removed(self):
        for name, plan in (
            ("java-pr1947.json", self.java_plan()),
            ("llvm-pr969.json", self.llvm_plan()),
            ("format-pr91.json", self.format_plan()),
        ):
            with self.subTest(name=name):
                key = next(iter(validator.REQUIRED_HARNESS_PATHS[name]))
                plan["required_source_blobs"].pop(key)
                with self.assertRaisesRegex(validator.Denied, "mandatory harness pins"):
                    validator.validate_plan(self.write(name, plan))

    def test_all_backends_must_share_fixture_blob(self):
        java = self.write("java-pr1947.json", self.java_plan())
        llvm = self.write("llvm-pr969.json", self.llvm_plan())
        fmt_doc = self.format_plan()
        fmt_doc["required_source_blobs"][validator.SHARED_FIXTURE] = "d" * 40
        fmt = self.write("format-pr91.json", fmt_doc)
        with self.assertRaisesRegex(validator.Denied, "shared conformance fixture"):
            validator.validate_plan_set([java, llvm, fmt])

    def test_duplicate_json_members_fail_closed(self):
        path = Path("ci/test-plans/java-pr1947.json")
        path.write_text(
            '{"schema":"ores-private-ci-test-plan-v1",'
            '"schema":"x","source_repo":"ores-truffle-oreslang/oreslang-source.java",'
            '"allowed_prs":[1947],"commands":[]}\n',
            encoding="utf-8",
        )
        with self.assertRaises(validator.Denied):
            validator.validate_plan(path)


if __name__ == "__main__":
    unittest.main(verbosity=2)
