#!/usr/bin/env python3
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
from unittest import mock
import unittest

SCRIPT = Path(__file__).with_name("run-oreslang-test-plan.py")
spec = importlib.util.spec_from_file_location("plan_runner", SCRIPT)
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class TrustedPlanRunnerTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.old_cwd = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(lambda: os.chdir(self.old_cwd))
        (self.root/"private-oreslang-source/formal").mkdir(parents=True)
        (self.root/"private-oreslang-source/formal/input.txt").write_text("synthetic\n")
        (self.root/"ci/test-plans").mkdir(parents=True)
        self.home = self.root/"guest-home"
        self.home.mkdir()
        self.env_patch = mock.patch.dict(os.environ,{
            "HOME":str(self.home),
            "PATH":os.environ.get("PATH","/usr/bin:/bin"),
        },clear=False)
        self.env_patch.start()
        self.addCleanup(self.env_patch.stop)

    def write(self, command=None, source_repo="synthetic/private", pr=7, extra_plan=None):
        command = command or {
            "argv":["python3","-c","print('TRUSTED_PLAN_SYNTHETIC_PASS')"],
            "cwd":"formal",
            "timeout_seconds":30,
            "stdout_contains":"TRUSTED_PLAN_SYNTHETIC_PASS",
        }
        commands = command if isinstance(command, list) else [command]
        plan = {
            "schema":"ores-private-ci-test-plan-v1",
            "source_repo":source_repo,
            "allowed_prs":[pr],
            "commands":commands,
        }
        if extra_plan:
            plan.update(extra_plan)
        plan_bytes=(json.dumps(plan,sort_keys=True,separators=(",",":"))+"\n").encode()
        plan_path=self.root/"ci/test-plans/synthetic.json"
        plan_path.write_bytes(plan_bytes)
        receipt={
            "schema":"ores-private-ci-receipt-v3",
            "source":{"repo":"synthetic/private","pr":7},
            "test_plan":{
                "path":"ci/test-plans/synthetic.json",
                "sha256":hashlib.sha256(plan_bytes).hexdigest(),
            },
        }
        receipt_path=self.root/"ci/oreslang-receipt.json"
        receipt_path.write_text(json.dumps(receipt))
        return receipt_path

    def test_happy_path_executes_without_shell_in_clean_home(self):
        receipt=self.write()
        result=runner.run_plan(str(receipt),"private-oreslang-source")
        self.assertEqual(len(result),1)
        self.assertEqual(result[0]["argv"][0],"python3")
        self.assertTrue((self.home/"tmp").is_dir())

    def test_plan_digest_mismatch_fails(self):
        receipt=self.write()
        plan=self.root/"ci/test-plans/synthetic.json"
        plan.write_text(plan.read_text()+" ")
        with self.assertRaisesRegex(runner.Denied,"digest mismatch"):
            runner.run_plan(str(receipt),"private-oreslang-source")

    def test_shell_executable_rejected(self):
        receipt=self.write({
            "argv":["bash","-lc","echo should-not-run"],
            "cwd":".",
            "timeout_seconds":30,
        })
        with self.assertRaisesRegex(runner.Denied,"unapproved executable"):
            runner.run_plan(str(receipt),"private-oreslang-source")

    def test_cwd_escape_rejected(self):
        receipt=self.write({
            "argv":["python3","-c","print('x')"],
            "cwd":"../",
            "timeout_seconds":30,
        })
        with self.assertRaisesRegex(runner.Denied,"unsafe command cwd|escapes"):
            runner.run_plan(str(receipt),"private-oreslang-source")

    def test_stdout_evidence_required(self):
        receipt=self.write({
            "argv":["python3","-c","print('actual')"],
            "cwd":".",
            "timeout_seconds":30,
            "stdout_contains":"expected",
        })
        with self.assertRaisesRegex(runner.Denied,"missing required stdout"):
            runner.run_plan(str(receipt),"private-oreslang-source")

    def test_plan_cannot_authorize_different_source(self):
        receipt=self.write(source_repo="other/private")
        with self.assertRaisesRegex(runner.Denied,"source repository mismatch"):
            runner.run_plan(str(receipt),"private-oreslang-source")

    def test_duplicate_and_nonfinite_authorization_json_rejected(self):
        receipt=self.write()
        receipt.write_text('{"schema":"ores-private-ci-receipt-v3","schema":"x"}')
        with self.assertRaises(runner.Denied):
            runner.load_context(str(receipt))
        receipt.write_text('{"schema":"ores-private-ci-receipt-v3","source":{"pr":NaN}}')
        with self.assertRaises(runner.Denied):
            runner.load_context(str(receipt))

    def test_git_metadata_inside_guest_source_is_denied(self):
        receipt=self.write()
        (self.root/"private-oreslang-source/.git").mkdir()
        with self.assertRaisesRegex(runner.Denied,"Git metadata"):
            runner.run_plan(str(receipt),"private-oreslang-source")

    def test_home_inside_source_is_denied(self):
        receipt=self.write()
        nested=self.root/"private-oreslang-source/home"
        nested.mkdir()
        with mock.patch.dict(os.environ,{"HOME":str(nested)}):
            with self.assertRaisesRegex(runner.Denied,"HOME must be outside"):
                runner.run_plan(str(receipt),"private-oreslang-source")

    def test_subprocess_does_not_inherit_action_credentials_or_runner_home(self):
        source=(
            "import os; "
            "assert os.environ['HOME'].startswith(" + repr(str(self.home / "tmp")) + "); "
            "assert os.environ.get('GITHUB_TOKEN') is None; "
            "assert os.environ.get('GH_TOKEN') is None; "
            "assert os.environ.get('MAVEN_OPTS') is None; "
            "assert os.environ['XDG_CONFIG_HOME'].startswith(os.environ['HOME']); "
            "assert os.environ['GIT_TERMINAL_PROMPT'] == '0'; "
            "print('CLEAN_ENV_PASS')"
        )
        receipt=self.write({
            "argv":["python3","-c",source],
            "cwd":".",
            "timeout_seconds":30,
            "stdout_contains":"CLEAN_ENV_PASS",
        })
        with mock.patch.dict(os.environ,{
            "GITHUB_TOKEN":"synthetic-secret",
            "GH_TOKEN":"synthetic-secret",
            "MAVEN_OPTS":"-Dleak=yes",
        }):
            result=runner.run_plan(str(receipt),"private-oreslang-source")
        self.assertEqual(result[0]["returncode"],0)

    def _maven_fixture(self):
        pom=(self.root/"private-oreslang-source/pom.xml")
        pom.write_text("<project/>\n")
        pin=runner.git_blob_sha(pom.read_bytes())
        dep=self.root/"deps"
        dep.mkdir()
        (dep/".oreslang-prefetch-complete").write_text(pin+"\n")
        command={"argv":["mvn","-o","-C","-B","test"],"cwd":".","timeout_seconds":30}
        receipt=self.write(command,extra_plan={"maven_pom_blob_sha":pin})
        return receipt,dep,pin

    def test_maven_requires_offline_mode_and_prefetched_repo(self):
        pom=(self.root/"private-oreslang-source/pom.xml")
        pom.write_text("<project/>\n")
        pin=runner.git_blob_sha(pom.read_bytes())
        receipt=self.write(
            {"argv":["mvn","-C","-B","test"],"cwd":".","timeout_seconds":30},
            extra_plan={"maven_pom_blob_sha":pin},
        )
        with self.assertRaisesRegex(runner.Denied,"offline"):
            runner.run_plan(str(receipt),"private-oreslang-source",self.root/"missing")

    def test_maven_rejects_project_dot_mvn_and_wrong_dependency_marker(self):
        receipt,dep,pin=self._maven_fixture()
        (self.root/"private-oreslang-source/.mvn").mkdir()
        with self.assertRaisesRegex(runner.Denied,"project .mvn"):
            runner.validate_maven_repository(
                runner.load_context(str(receipt))[1],
                (self.root/"private-oreslang-source").resolve(),
                dep,
            )
        (self.root/"private-oreslang-source/.mvn").rmdir()
        (dep/".oreslang-prefetch-complete").write_text("0"*40+"\n")
        with self.assertRaisesRegex(runner.Denied,"not bound"):
            runner.validate_maven_repository(
                runner.load_context(str(receipt))[1],
                (self.root/"private-oreslang-source").resolve(),
                dep,
            )

    def test_maven_plan_cannot_override_settings_repo_or_skip_tests(self):
        for arg in (
            "-s/tmp/x", "--settings=/tmp/x", "-fother.xml",
            "-Dmaven.repo.local=/tmp/m2", "-DskipTests", "-Dmaven.test.skip=true",
        ):
            with self.subTest(arg=arg), self.assertRaisesRegex(runner.Denied,"overrides"):
                runner.validate_maven_argv(["mvn","-o","-C",arg,"test"])


    def test_maven_requires_strict_checksums(self):
        with self.assertRaisesRegex(runner.Denied, "strict checksums"):
            runner.validate_maven_argv(["mvn", "-o", "-B", "test"])

    def test_each_plan_command_gets_fresh_source_copy(self):
        marker = self.root/"private-oreslang-source/marker.txt"
        marker.write_text("pristine\n")
        commands = [
            {
                "argv":["python3","-c",
                        "from pathlib import Path; Path('marker.txt').write_text('mutated\\n'); print('MUTATED_COPY')"],
                "cwd":".","timeout_seconds":30,"stdout_contains":"MUTATED_COPY",
            },
            {
                "argv":["python3","-c",
                        "from pathlib import Path; assert Path('marker.txt').read_text() == 'pristine\\n'; print('FRESH_COPY_PASS')"],
                "cwd":".","timeout_seconds":30,"stdout_contains":"FRESH_COPY_PASS",
            },
        ]
        receipt = self.write(commands)
        result = runner.run_plan(str(receipt),"private-oreslang-source")
        self.assertEqual(len(result), 2)
        self.assertEqual(marker.read_text(), "pristine\n")


    def test_each_maven_command_gets_fresh_dependency_copy(self):
        canonical = self.root/"canonical-m2"
        canonical.mkdir()
        (canonical/"artifact.jar").write_text("pristine\n")
        first = runner.fresh_command_dependency_repo(canonical, 1, self.home)
        (first/"artifact.jar").write_text("poisoned\n")
        second = runner.fresh_command_dependency_repo(canonical, 2, self.home)
        self.assertEqual((second/"artifact.jar").read_text(), "pristine\n")
        self.assertEqual((canonical/"artifact.jar").read_text(), "pristine\n")


    def test_cargo_commands_are_offline_and_narrow(self):
        runner.validate_cargo_argv(["cargo", "fmt", "--all", "--", "--check"])
        runner.validate_cargo_argv(["cargo", "test", "--offline", "--all-targets"])
        runner.validate_cargo_argv(["cargo", "clippy", "--offline", "--all-targets", "--", "-D", "warnings"])
        for argv in (
            ["cargo", "test", "--all-targets"],
            ["cargo", "publish"],
            ["cargo", "test", "--offline", "--manifest-path", "../other/Cargo.toml"],
            ["cargo", "build", "--offline", "--config=net.git-fetch-with-cli=true"],
        ):
            with self.subTest(argv=argv), self.assertRaises(runner.Denied):
                runner.validate_cargo_argv(argv)

    def test_reviewed_rust_plan_rejects_project_cargo_config(self):
        plan = {"commands":[{"argv":["cargo","test","--offline"],"cwd":".","timeout_seconds":30}]}
        root = self.root/"private-oreslang-source"
        (root/".cargo").mkdir()
        with self.assertRaisesRegex(runner.Denied, "project .cargo"):
            runner.validate_cargo_project(plan, root)

    def test_each_plan_command_gets_fresh_home(self):
        first = runner.fresh_command_home(self.home, 1)
        (first/"poison").write_text("x")
        second = runner.fresh_command_home(self.home, 2)
        self.assertNotEqual(first, second)
        self.assertFalse((second/"poison").exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
