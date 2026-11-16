# Benchmark suite

The dependency-free Rust driver measures the public API in an optimized build.
It exports CSV; Python's standard library is sufficient to compare two runs.

The [local coverage run](benchmark-results.md) records 1,256 completed compiler
cases, including 128 runtime cases, and selected measured results.

The [optimization comparison](performance-improvements.md) records subsequent
matched before/final measurements and their startup tradeoffs.

## Run

```sh
# 24 compiler cases plus microbenchmarks
cargo bench --bench obfuscation --locked -- --suite quick

# 128 compiler cases plus microbenchmarks
cargo bench --bench obfuscation --locked -- --suite standard

# Inspect the exhaustive matrix: 9,936 cases
cargo bench --bench obfuscation --locked -- --suite full --list

# Execute it: three sizes, three seeds, 24 workloads and 46 profiles
cargo bench --bench obfuscation --locked -- --suite full

# Also measure Luau loading and execution, checked against the original source
cargo bench --bench obfuscation --locked -- --suite standard --runtime

# Runtime only; ten work units per timed invocation
cargo bench --bench obfuscation --locked -- --runtime-only --iterations 10
```

Set `LUAU_BIN` to an executable path or use the local interpreter described in the
[test guide](../tests/README.md). The driver also searches `PATH`. An unavailable
or invalid interpreter is an error when runtime measurement is requested.

```sh
# Source-size scaling, with exact profile names
cargo bench --bench obfuscation --locked -- --suite full --workload unique_strings,large_block,many_functions --profile =lightweight,=native,=vm-basic-cached-packed --scales 8,32,128,512 --seeds 7,42 --samples 10

# Every VM variant for one workload
cargo bench --bench obfuscation --locked -- --suite full --workload =closures --profile vm- --scales 32 --seeds 42 --runtime

# Native pass ablations
cargo bench --bench obfuscation --locked -- --suite full --profile native --scales 32 --seeds 42

# Longer batches and reproducibly randomized order
cargo bench --bench obfuscation --locked -- --samples 30 --sample-ms 50 --order-seed 17 --output target/benchmarks/baseline
```

Selectors accept comma-separated substrings; prefix a selector with `=` for exact
matching. They apply within the suite: select `full` to expose every name.
Sizes are bounded to 1–2048. Use distinct sizes/seeds to keep comparison keys
unique. `--timeout-secs` sets a deadline per interpreter process. Run `--help` for
all options. Exhaustive runtime matrices can be expensive; inspect `--list` first.

## Coverage

Quick selects the first six workloads and four profiles. Standard selects the
first sixteen workloads and eight profiles. Full selects everything, with sizes
8/32/128 and seeds 7/42/1337.

| Family | Workloads |
| --- | --- |
| Arithmetic and control flow | Arithmetic, branching, nested numeric loops, while/continue, repeat |
| Objects and protocols | Tables, properties, method calls, generic iteration, custom `__iter`, metatables |
| Functions and scopes | Mutable closures, recursive Fibonacci, varargs/nil holes, shadowing, many functions |
| Strings and frontend | Interpolation, binary/Unicode strings, repeated literals, unique literals, types, comments |
| Structural and fallback behavior | Large straight-line blocks, explicit environment-introspection fallback |

`scale` changes loop lengths for dynamic workloads and generated statement/function
counts for structural workloads; it does not multiply source bytes uniformly.
Every runtime module returns a function taking a work-unit count and returning a
repeatable numeric checksum.

Profiles cover rename, lightweight and native; the complete VM cross product of
three hardening levels, packed/table records, cached/rolling instruction decoding
and retained/uncached strings; native pass ablations; eager strings; pretty output;
preserved types; density and arithmetic-depth variants; functions-only VM;
disabled VM integrity and disabled renaming. Fallback counts/reasons remain visible
because native fallback materially changes execution costs.

## Measurement boundaries

| CSV kind | Timed operation |
| --- | --- |
| `obfuscate` | Complete `obfuscate_with_report`: validation, transformations, output validation, report creation and result destruction |
| `lex` | Public lexer, including token allocation and destruction |
| `emit` | Public emitter; tokens are prepared outside the measurement |
| `rng_next`, `rng_range`, `rng_name` | One RNG operation; name lengths are 8–14 |
| `rng_shuffle` | One shuffle of 16, 128 or 1024 prepared elements |
| `module_load_cpu` | Luau `os.clock()` around `require`, including module compilation and helper/prototype initialization |
| `warm_execution_cpu` | One warmed invocation processing `--iterations` work units |
| `process_wall` | Complete fresh interpreter: startup, driver/module loading, two warmups, timed calls, output and exit |

`process_wall` is not startup latency. File writes are outside timed regions.
A fresh process does not imply a cold OS filesystem cache. Luau CPU time and Rust
wall time use different clocks.

Rust receives one calibration call. Its duration selects a fixed batch targeting
`--sample-ms`, bounded to 1–1,000,000 operations. Each sample is divided by its batch
size. Configuration/source construction is excluded; `black_box` retains measured
results. Pipeline report collection also performs an untimed invocation first.

Each of `--processes` fresh interpreters loads a module, executes two warmups, then
emits `--samples` timings. Warm timings are pooled across processes. Module-load
and whole-process rows contain `--processes` samples. Every generated checksum is
checked against the original source; every timed invocation must repeat it.
Malformed output, an interpreter error, a changed checksum or a timeout fails the
run. Both process output streams are drained concurrently.

Cases are shuffled using a separate fixed `--order-seed`. Original runtime
baselines and microbenchmarks precede profile cases. Avoid concurrent builds or
benchmarks when collecting comparable measurements.

## Output and comparison

Runs create a new directory under `target/benchmarks/`, or `--output`. Existing
directories are refused.

- `summary.csv`: sample count, batch, min/median/p95/max/mean/population standard
  deviation, input/output sizes, expansion and transformation/VM coverage. Input
  MiB/s is populated only for compiler, lexer and emitter rows.
- `samples.csv`: every duration, in milliseconds per operation.
- `metadata.txt`: arguments, OS/architecture, Rust version, Git revision/dirty
  status, interpreter path/fingerprint, clocks and batching rules.
- `complete.txt`: created only after the entire selected matrix succeeds.
- `runtime/`: last module and driver, useful for reproducing a failure.

Completed rows are flushed immediately. Partial reports lack `complete.txt` and
must not be presented as completed matrices.

```sh
python scripts/compare_benchmarks.py target/benchmarks/baseline target/benchmarks/candidate --output target/comparison.md

# Optional median regression gate, excluding baselines below 0.1 ms
python scripts/compare_benchmarks.py target/benchmarks/baseline target/benchmarks/candidate --max-regression-percent 15 --min-baseline-ms 0.1

python -m unittest discover -s scripts/tests -v
```

Comparison joins kind/workload/scale/profile/seed, reports missing cases and
compares median, p95 and output size. Positive timing changes mean slower;
zero baselines have no percentage. Duplicate keys, invalid numbers and unmatched
runs are rejected. The optional gate returns 1 for a regression; invalid input
returns 2. Invocation counts and other metadata are not automatically matched:
inspect metadata and use identical settings before comparing.

Use more samples for decisions: with fewer than twenty observations, nearest-rank
p95 is usually the maximum. Tiny native durations can hit timer resolution.
Percentages do not establish statistical significance. These workloads do not
measure Roblox services, peak memory, security strength or device frame times.
The older [performance probe](performance.md) retains different boundaries and
must not be merged into these CSVs.

## Verification

`cargo test --all-targets` checks distributions, calibration, CSV quoting,
configuration coverage, runtime checksums and timeout cleanup. Runtime checks
require Luau when `REQUIRE_LUAU_RUNTIME=1`. Cargo's test invocation skips timing the
custom benchmark driver; `cargo bench` explicitly selects measurement mode.
