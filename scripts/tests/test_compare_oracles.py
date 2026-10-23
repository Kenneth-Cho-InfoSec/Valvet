import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "compare_oracles.py"

spec = importlib.util.spec_from_file_location("compare_oracles", SCRIPT)
co = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(co)


def write(path: Path, records: list[tuple[str, str]]) -> None:
    path.write_text("".join(f"{label}\n=\n{value}\n" for label, value in records))


class CompareOraclesTest(unittest.TestCase):
    def run_cli(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            text=True,
            capture_output=True,
            check=False,
        )

    def test_exact(self) -> None:
        expected = {"a": "1", "b": "2.5"}
        self.assertEqual(co.compare(expected, expected, False)["exact"], 2)
        self.assertEqual(co.compare(expected, expected, False)["status"], "ok")

    def test_one_ulp(self) -> None:
        expected = {"a": "1.0"}
        actual = {"a": "1.0000000000000002"}
        self.assertEqual(co.compare(expected, actual, False)["status"], "fail")
        result = co.compare(expected, actual, True)
        self.assertEqual(result["one_ulp"], 1)
        self.assertEqual(result["status"], "ok")

    def test_missing_and_extra(self) -> None:
        result = co.compare({"a": "1"}, {"b": "2"}, True)
        self.assertEqual(result["missing"], 1)
        self.assertEqual(result["extra"], 1)
        self.assertEqual(result["status"], "fail")

    def test_malformed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "bad.txt"
            path.write_text("label\nnot-equals\n1\n")
            records, malformed = co.parse_records(path)
            self.assertEqual(records, {})
            self.assertEqual(malformed, 1)

    def test_cli_absent_root_fails_cleanly(self) -> None:
        proc = self.run_cli("--all", "--oracles-root", "/definitely/not/here")
        self.assertEqual(proc.returncode, 2)
        self.assertIn("error:", proc.stderr)

    def test_cli_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            write(root / "expected.txt", [("a", "1")])
            write(root / "actual.txt", [("a", "1")])
            (root / "manifest.json").write_text(
                json.dumps(
                    {
                        "schema": "valvet-oracle-manifest-v1",
                        "ports": {"p": {"expected": "expected.txt", "actual": "actual.txt"}},
                    }
                )
            )
            proc = self.run_cli("--all", "--oracles-root", str(root))
            self.assertEqual(proc.returncode, 0, proc.stderr)
            payload = json.loads(proc.stdout)
            self.assertEqual(payload["ports"]["p"]["status"], "ok")


if __name__ == "__main__":
    unittest.main()
