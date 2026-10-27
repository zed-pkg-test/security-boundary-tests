#!/usr/bin/env python3
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
from unittest import mock
import unittest

SCRIPT = Path(__file__).with_name("prefetch-oreslang-maven-deps.py")
spec = importlib.util.spec_from_file_location("prefetch", SCRIPT)
prefetch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prefetch)


def blob_sha(raw):
    return hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()


class MavenPrefetchTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.pom = self.root / "pom.xml"
        self.pom.write_text("""<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>test</groupId><artifactId>x</artifactId><version>1</version>
</project>
""", encoding="utf-8")

    def plan(self, argv=None, **extra):
        raw = self.pom.read_bytes()
        plan = {
            "schema": "ores-private-ci-test-plan-v1",
            "source_repo": "synthetic/private",
            "allowed_prs": [7],
            "maven_pom_blob_sha": blob_sha(raw),
            "commands": [{
                "argv": argv or ["mvn", "-o", "-C", "-B", "test"],
                "cwd": ".",
                "timeout_seconds": 30,
            }],
        }
        plan.update(extra)
        path = self.root / "plan.json"
        path.write_text(json.dumps(plan), encoding="utf-8")
        return path

    def test_exact_pom_blob_and_offline_plan_pass(self):
        plan = prefetch.load_plan(self.plan())
        pin = prefetch.validate_maven_plan(plan)
        self.assertEqual(pin, blob_sha(self.pom.read_bytes()))
        self.assertEqual(prefetch.validate_pom(self.pom, pin), self.pom.read_bytes())

    def test_online_maven_plan_denied(self):
        plan = prefetch.load_plan(self.plan(["mvn", "-C", "-B", "test"]))
        with self.assertRaisesRegex(prefetch.Denied, "offline"):
            prefetch.validate_maven_plan(plan)

    def test_missing_strict_checksum_mode_denied(self):
        plan = prefetch.load_plan(self.plan(["mvn", "-o", "-B", "test"]))
        with self.assertRaisesRegex(prefetch.Denied, "strict checksums"):
            prefetch.validate_maven_plan(plan)

    def test_settings_repo_and_skip_overrides_denied(self):
        for arg in (
            "-s/tmp/settings.xml",
            "--settings=/tmp/settings.xml",
            "-Dmaven.repo.local=/tmp/m2",
            "-DskipTests",
            "-Dmaven.test.skip=true",
            "-DskipITs=true",
        ):
            with self.subTest(arg=arg):
                plan = prefetch.load_plan(self.plan(["mvn", "-o", "-C", arg, "test"]))
                with self.assertRaisesRegex(prefetch.Denied, "overrides"):
                    prefetch.validate_maven_plan(plan)

    def test_wrong_pom_blob_denied(self):
        plan = json.loads(self.plan().read_text())
        plan["maven_pom_blob_sha"] = "0" * 40
        path = self.root / "wrong.json"
        path.write_text(json.dumps(plan))
        pin = prefetch.validate_maven_plan(prefetch.load_plan(path))
        with self.assertRaisesRegex(prefetch.Denied, "blob mismatch"):
            prefetch.validate_pom(self.pom, pin)

    def test_project_repositories_and_extensions_denied(self):
        cases = [
            "<repositories><repository><id>x</id></repository></repositories>",
            "<pluginRepositories><pluginRepository><id>x</id></pluginRepository></pluginRepositories>",
            "<distributionManagement/>",
            "<build><extensions><extension/></extensions></build>",
            "<build><plugins><plugin><extensions>true</extensions></plugin></plugins></build>",
            "<modules><module>child</module></modules>",
            "<dependencies><dependency><groupId>x</groupId><artifactId>y</artifactId><version>1</version><scope>system</scope><systemPath>/tmp/x</systemPath></dependency></dependencies>",
            "<parent><groupId>x</groupId><artifactId>p</artifactId><version>1</version></parent>",
        ]
        for fragment in cases:
            with self.subTest(fragment=fragment):
                raw = ("""<project xmlns="http://maven.apache.org/POM/4.0.0">
<modelVersion>4.0.0</modelVersion>%s</project>""" % fragment).encode()
                self.pom.write_bytes(raw)
                with self.assertRaisesRegex(prefetch.Denied, "repository/build-extension|build extension"):
                    prefetch.validate_pom(self.pom, blob_sha(raw))

    def test_project_cannot_configure_trusted_dependency_plugin(self):
        raw = b"""<project xmlns="http://maven.apache.org/POM/4.0.0">
<modelVersion>4.0.0</modelVersion>
<build><plugins><plugin>
<groupId>org.apache.maven.plugins</groupId>
<artifactId>maven-dependency-plugin</artifactId>
<version>3.8.1</version>
<configuration><outputDirectory>/tmp/escape</outputDirectory></configuration>
</plugin></plugins></build>
</project>"""
        self.pom.write_bytes(raw)
        with self.assertRaisesRegex(prefetch.Denied, "trusted dependency prefetch plugin"):
            prefetch.validate_pom(self.pom, blob_sha(raw))

    def test_clean_maven_environment_strips_credentials_and_option_injection(self):
        home = self.root / "clean-home"
        tmp = self.root / "clean-tmp"
        with mock.patch.dict(os.environ, {
            "PATH": "/trusted/bin:/usr/bin",
            "JAVA_HOME": "/trusted/java",
            "GITHUB_TOKEN": "secret",
            "GH_TOKEN": "secret",
            "AWS_SECRET_ACCESS_KEY": "secret",
            "MAVEN_OPTS": "-Dinject=true",
            "MAVEN_ARGS": "-s /tmp/evil.xml",
            "JAVA_TOOL_OPTIONS": "-javaagent:/tmp/evil.jar",
            "JDK_JAVA_OPTIONS": "-javaagent:/tmp/evil.jar",
        }, clear=True):
            env = prefetch.clean_maven_environment(home, tmp)
        self.assertEqual(env["PATH"], "/trusted/bin:/usr/bin")
        self.assertEqual(env["JAVA_HOME"], "/trusted/java")
        self.assertEqual(env["HOME"], str(home))
        self.assertEqual(env["TMPDIR"], str(tmp))
        for forbidden in (
            "GITHUB_TOKEN", "GH_TOKEN", "AWS_SECRET_ACCESS_KEY",
            "MAVEN_OPTS", "MAVEN_ARGS", "JAVA_TOOL_OPTIONS", "JDK_JAVA_OPTIONS",
        ):
            self.assertNotIn(forbidden, env)

    def test_duplicate_and_nonfinite_json_denied(self):
        path = self.root / "bad.json"
        path.write_text('{"schema":"ores-private-ci-test-plan-v1","schema":"x"}')
        with self.assertRaises(prefetch.Denied):
            prefetch.load_plan(path)
        path.write_text('{"schema":"ores-private-ci-test-plan-v1","commands":[NaN]}')
        with self.assertRaises(prefetch.Denied):
            prefetch.load_plan(path)

    def test_explicit_runtime_artifacts_are_bounded_and_exact(self):
        plan = prefetch.load_plan(self.plan(maven_prefetch_artifacts=[
            "org.apache.maven.surefire:surefire-junit-platform:3.5.2"
        ]))
        self.assertEqual(
            prefetch.prefetch_artifacts(plan),
            ["org.apache.maven.surefire:surefire-junit-platform:3.5.2"],
        )
        for bad in (
            ["bad"],
            ["g:a:v:jar:classifier:extra"],
            ["g:a:v", "g:a:v"],
            ["g:a:v"] * (prefetch.MAX_PREFETCH_ARTIFACTS + 1),
        ):
            with self.subTest(bad=bad):
                plan["maven_prefetch_artifacts"] = bad
                with self.assertRaises(prefetch.Denied):
                    prefetch.prefetch_artifacts(plan)

    @mock.patch.object(prefetch.subprocess, "run")
    def test_prefetch_uses_isolated_exact_plugin_environment(self, run):
        run.return_value.returncode = 0
        repo = self.root / "repo"
        plan = self.plan(maven_prefetch_artifacts=[
            "org.apache.maven.surefire:surefire-junit-platform:3.5.2"
        ])
        with mock.patch.dict(os.environ, {
            "PATH": "/trusted/bin:/usr/bin",
            "JAVA_HOME": "/trusted/java",
            "GITHUB_TOKEN": "secret",
            "MAVEN_OPTS": "-Dinject=true",
            "JAVA_TOOL_OPTIONS": "-javaagent:/tmp/evil.jar",
        }, clear=True):
            used = prefetch.prefetch(plan, self.pom, repo)
        self.assertTrue(used)
        self.assertEqual(run.call_count, 2)

        first = run.call_args_list[0]
        second = run.call_args_list[1]
        go_offline = first.args[0]
        get_runtime = second.args[0]
        self.assertIn("-C", go_offline)
        self.assertIn("-N", go_offline)
        self.assertIn("-s", go_offline)
        self.assertIn("-gs", go_offline)
        self.assertIn(prefetch.MAVEN_PLUGIN, go_offline)
        self.assertIn(prefetch.MAVEN_GET_PLUGIN, get_runtime)
        self.assertIn(
            "-Dartifact=org.apache.maven.surefire:surefire-junit-platform:3.5.2",
            get_runtime,
        )
        self.assertTrue(any(arg.startswith("-Dmaven.repo.local=") for arg in go_offline))
        self.assertTrue(any(arg.startswith("-Duser.home=") for arg in go_offline))
        self.assertTrue(any(arg.startswith("-Djava.io.tmpdir=") for arg in go_offline))

        pom_arg = go_offline[go_offline.index("-f") + 1]
        self.assertNotEqual(Path(pom_arg), self.pom)
        self.assertEqual(Path(pom_arg).name, "pom.xml")
        self.assertEqual(first.kwargs["cwd"], Path(pom_arg).parent)
        self.assertTrue(first.kwargs["start_new_session"])
        self.assertIs(first.kwargs["stdin"], prefetch.subprocess.DEVNULL)

        env = first.kwargs["env"]
        self.assertEqual(env["PATH"], "/trusted/bin:/usr/bin")
        self.assertEqual(env["JAVA_HOME"], "/trusted/java")
        for forbidden in ("GITHUB_TOKEN", "MAVEN_OPTS", "JAVA_TOOL_OPTIONS"):
            self.assertNotIn(forbidden, env)
        self.assertEqual(second.kwargs["env"], env)
        self.assertTrue((repo / ".oreslang-prefetch-complete").is_file())


if __name__ == "__main__":
    unittest.main(verbosity=2)
