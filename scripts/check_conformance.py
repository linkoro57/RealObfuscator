"""Compare runnable official Luau fixtures with three obfuscation profiles.

Provide a Luau source checkout, the official CLI, and this project's release CLI.
Fixtures requiring the upstream C++ harness are recorded as baseline exclusions.
"""
import argparse
import json
from pathlib import Path
import subprocess

FIXTURES = (
    "assert attrib bitwise buffers calls clear constructs ifelseexpr literals locals "
    "math move strconv stringinterp strings tpack utf8"
).split()


def run(args, timeout=60):
    try:
        result = subprocess.run(args, capture_output=True, timeout=timeout)
        return result.returncode, result.stdout, result.stderr
    except subprocess.TimeoutExpired:
        return -1, b"", b"execution deadline exceeded"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", type=Path, required=True)
    parser.add_argument("--luau", type=Path, required=True)
    parser.add_argument("--obfuscator", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    results = []
    for name in FIXTURES:
        source = (args.fixtures / f"{name}.luau").resolve()
        status, expected, error = run([str(args.luau.resolve()), str(source)])
        if status:
            results.append(dict(fixture=name, mode="original", status="baseline excluded",
                                error=error.decode(errors="replace")[:2000]))
            continue
        for mode in ("lightweight", "native", "vm"):
            target = (args.output / f"{name}-{mode}.luau").resolve()
            status, _, error = run([str(args.obfuscator.resolve()), str(source), "-o",
                                    str(target), "--mode", mode, "--seed", "42", "--quiet"])
            outcome = "generation error"
            if not status:
                status, actual, error = run([str(args.luau.resolve()), str(target)])
                outcome = "runtime error" if status else (
                    "pass" if actual == expected else "stdout mismatch")
            results.append(dict(fixture=name, mode=mode, status=outcome,
                                error=error.decode(errors="replace")[:2000]))
            print(f"{name}/{mode}: {outcome}", flush=True)
    (args.output / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
    failures = [r for r in results if r["status"] not in ("pass", "baseline excluded")]
    print(f"{sum(r['status'] == 'pass' for r in results)} passed; {len(failures)} failed")
    return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
