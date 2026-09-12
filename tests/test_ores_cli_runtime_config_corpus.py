import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "fixtures" / "ores-cli-runtime-config"
LOADER_TOKENS = (
    "include_str!",
    "fs::read",
    "read_to_string",
    "toml::from_str",
    "load_repo_root",
    "load_optional",
    "admit_server_stack",
    "admit_runtime",
    "with_optional_sidecar_file",
)


class OresCliRuntimeConfigCorpusTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = json.loads((CORPUS / "expected.json").read_text())
        cls.cases = {case["name"]: case for case in cls.manifest["cases"]}

    def test_manifest_has_unique_six_case_acceptance_matrix(self) -> None:
        self.assertEqual(self.manifest["schemaVersion"], 1)
        self.assertEqual(self.manifest["consumer"], "ORESoftware/ores-cli")
        self.assertEqual(self.manifest["audit"], "runtime-config-consumption")
        self.assertEqual(len(self.cases), 6)
        self.assertEqual(
            set(self.cases),
            {
                "runtime-consumed",
                "production-unproven",
                "ci-only",
                "declared-only",
                "strict-redis-mismatch",
                "strict-redis-bound",
            },
        )
        for case in self.manifest["cases"]:
            self.assertTrue(case["expectedClass"])
            self.assertTrue(case["expectedCode"].startswith("runtime-config-"))

    def test_every_case_has_exactly_one_root_runtime_toml(self) -> None:
        for name in self.cases:
            with self.subTest(case=name):
                root = CORPUS / name
                configs = [
                    path
                    for path in root.iterdir()
                    if path.is_file()
                    and (
                        (path.name.startswith(".ores-") and path.suffix == ".toml")
                        or path.name
                        in {
                            ".auth-shared.toml",
                            ".shared-auth.toml",
                            ".opto-sync.toml",
                            ".fanwaave-cfg.toml",
                            ".indiebuild.toml",
                        }
                    )
                ]
                self.assertEqual(len(configs), 1, (name, configs))

    def test_runtime_consumed_fixture_has_production_loader_evidence(self) -> None:
        root = CORPUS / "runtime-consumed"
        config_name = next(root.glob(".ores-*.toml")).name
        source = (root / "src" / "runtime.rs").read_text()
        self.assertIn(config_name, source)
        self.assertTrue(any(token in source for token in LOADER_TOKENS))

    def test_production_unproven_fixture_mentions_config_without_loader(self) -> None:
        root = CORPUS / "production-unproven"
        config_name = next(root.glob(".ores-*.toml")).name
        source = (root / "src" / "main.rs").read_text()
        self.assertIn(config_name, source)
        self.assertFalse(any(token in source for token in LOADER_TOKENS))

    def test_ci_only_fixture_cannot_impersonate_production_consumption(self) -> None:
        root = CORPUS / "ci-only"
        workflow = (root / ".github" / "workflows" / "check.yml").read_text()
        self.assertIn(".ores-otel.toml", workflow)
        self.assertFalse((root / "src").exists())

    def test_declared_only_fixture_has_no_source_or_ci_reference(self) -> None:
        root = CORPUS / "declared-only"
        self.assertTrue((root / ".ores-chat.toml").is_file())
        self.assertFalse((root / "src").exists())
        self.assertFalse((root / ".github").exists())
        self.assertFalse((root / "scripts").exists())

    def test_strict_redis_pair_distinguishes_local_and_bound_runtime(self) -> None:
        mismatch = CORPUS / "strict-redis-mismatch"
        bound = CORPUS / "strict-redis-bound"
        for root in (mismatch, bound):
            config = (root / ".ores-rl.toml").read_text().lower()
            self.assertIn("redis", config)
            self.assertTrue("strict" in config or "fail_closed" in config)

        local_source = (mismatch / "src" / "limiter.rs").read_text()
        self.assertIn("Mutex<HashMap", local_source)
        self.assertIn("ZED_RATE_LIMIT_", local_source)
        self.assertNotIn("REDIS_URL", local_source)
        self.assertFalse(any(token in local_source for token in LOADER_TOKENS))

        bound_source = (bound / "src" / "limiter.rs").read_text()
        self.assertIn(".ores-rl.toml", bound_source)
        self.assertIn("REDIS_URL", bound_source)
        self.assertIn("ores_rl", bound_source)
        self.assertTrue(any(token in bound_source for token in LOADER_TOKENS))

    def test_fixture_corpus_contains_no_credential_values(self) -> None:
        # Construct detector-shaped prefixes at runtime so this regression does
        # not itself violate the repository-wide credential-pattern scanner.
        forbidden = (
            "g" + "hp" + "_",
            "github" + "_pat_",
            "lin" + "_api_",
            "BEGIN " + "PRIVATE KEY",
        )
        for path in CORPUS.rglob("*"):
            if path.is_file():
                text = path.read_text()
                for marker in forbidden:
                    self.assertNotIn(marker, text, f"credential marker {marker!r} in {path}")


if __name__ == "__main__":
    unittest.main()
