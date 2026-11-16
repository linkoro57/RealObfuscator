#!/usr/bin/env python3
"""Compare matching benchmark cases using only Python's standard library."""
import argparse
import csv
import math
from pathlib import Path

KEY = ("kind", "workload", "scale", "profile", "seed")


def load(path):
    if path.is_dir():
        path = path / "summary.csv"
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        required = (*KEY, "median_ms", "p95_ms", "output_bytes", "samples")
        if not set(required).issubset(reader.fieldnames or []):
            raise ValueError(f"{path}: missing benchmark columns")
        rows = {}
        for row in reader:
            key = tuple(row[column] for column in KEY)
            if key in rows:
                raise ValueError(f"{path}: duplicate benchmark case {key}")
            for column in ("median_ms", "p95_ms", "output_bytes"):
                value = float(row[column])
                if not math.isfinite(value) or value < 0:
                    raise ValueError(f"{path}: invalid {column} for {key}")
                row[column] = value
            rows[key] = row
    return rows


def delta(before, after):
    return None if before == 0 else (after / before - 1) * 100


def compare(before, after):
    results = []
    for key in sorted(before.keys() & after.keys()):
        old, new = before[key], after[key]
        results.append({
            "key": key,
            "before_ms": old["median_ms"],
            "after_ms": new["median_ms"],
            "median_change": delta(old["median_ms"], new["median_ms"]),
            "p95_change": delta(old["p95_ms"], new["p95_ms"]),
            "size_change": delta(old["output_bytes"], new["output_bytes"]),
        })
    return results


def percentage(value):
    return "n/a" if value is None else f"{value:+.2f}%"


def render(results, before, after, limit):
    lines = [
        "# Benchmark comparison", "",
        f"Matched cases: {len(results)}. Only in baseline: {len(before.keys() - after.keys())}. "
        f"Only in candidate: {len(after.keys() - before.keys())}.", "",
        "Positive timing changes mean slower execution. Percentages are undefined for a zero baseline. "
        "Compare runs from the same machine, toolchain, interpreter, workload parameters and clock conditions. "
        "This report does not calculate statistical significance.", "",
        "| Case | Baseline ms | Candidate ms | Median change | p95 change | Size change |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    ordered = sorted(results, key=lambda row: row["median_change"] if row["median_change"] is not None else -math.inf, reverse=True)
    for row in ordered[:limit]:
        key = "/".join(row["key"]).replace("|", "\\|")
        lines.append(f"| {key} | {row['before_ms']:.6f} | {row['after_ms']:.6f} | "
                     f"{percentage(row['median_change'])} | {percentage(row['p95_change'])} | {percentage(row['size_change'])} |")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--output", type=Path, help="Write Markdown instead of stdout")
    parser.add_argument("--limit", type=int, default=30)
    parser.add_argument("--max-regression-percent", type=float, help="Optional exit-code gate on median time")
    parser.add_argument("--min-baseline-ms", type=float, default=0.1, help="Exclude tiny baselines from the optional gate")
    args = parser.parse_args()
    if args.limit < 1 or args.min_baseline_ms < 0:
        parser.error("limit must be positive and minimum baseline must be nonnegative")
    try:
        before, after = load(args.baseline), load(args.candidate)
        results = compare(before, after)
        if not results:
            raise ValueError("no matching benchmark cases")
        markdown = render(results, before, after, args.limit)
        if args.output:
            args.output.write_text(markdown, encoding="utf-8")
        else:
            print(markdown, end="")
    except (OSError, ValueError) as error:
        parser.exit(2, f"comparison error: {error}\n")
    if args.max_regression_percent is not None:
        if any(row["median_change"] is not None and row["before_ms"] >= args.min_baseline_ms
               and row["median_change"] > args.max_regression_percent for row in results):
            raise SystemExit(1)


if __name__ == "__main__":
    main()
