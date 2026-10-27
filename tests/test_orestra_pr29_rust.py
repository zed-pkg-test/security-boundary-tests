import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "fixtures" / "orestra-pr29-rust" / "Cargo.toml"

class OrestraPr29RustVerifierTests(unittest.TestCase):
    def test_exact_source_free_rust_verifiers(self):
        subprocess.run(
            ["cargo", "test", "--manifest-path", str(MANIFEST), "--all-targets", "--verbose"],
            cwd=ROOT,
            check=True,
            timeout=600,
        )

if __name__ == "__main__":
    unittest.main()
