import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("compare_benchmarks", Path(__file__).parents[1] / "compare_benchmarks.py")
benchmark = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(benchmark)


class ComparisonTests(unittest.TestCase):
    def test_comparison_joins_keys_and_handles_zero_baselines(self):
        key = ("obfuscate", "arithmetic", "8", "native", "42")
        before = {key: {"median_ms": 2.0, "p95_ms": 0.0, "output_bytes": 100}}
        after = {key: {"median_ms": 3.0, "p95_ms": 1.0, "output_bytes": 80}}
        result = benchmark.compare(before, after)[0]
        self.assertEqual(result["median_change"], 50.0)
        self.assertIsNone(result["p95_change"])
        self.assertAlmostEqual(result["size_change"], -20.0)
        self.assertIn("+50.00%", benchmark.render([result], before, after, 10))

    def test_csv_quotes_are_read_and_duplicate_keys_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "summary.csv"
            header = "kind,workload,scale,profile,seed,median_ms,p95_ms,output_bytes,samples\n"
            row = 'obfuscate,"a,b",8,native,42,1,2,100,3\n'
            path.write_text(header + row, encoding="utf-8")
            self.assertEqual(next(iter(benchmark.load(path)))[1], "a,b")
            path.write_text(header + row + row, encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "duplicate"):
                benchmark.load(path)

    def test_nonfinite_durations_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "summary.csv"
            path.write_text("kind,workload,scale,profile,seed,median_ms,p95_ms,output_bytes,samples\n"
                            "obfuscate,a,8,native,42,nan,2,100,3\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "invalid median_ms"):
                benchmark.load(path)


if __name__ == "__main__":
    unittest.main()
