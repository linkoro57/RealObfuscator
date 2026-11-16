# Tests

Run from the repository root:

```sh
cargo test
```

Unit and CLI tests cover deterministic builds, random-stream progress, malformed syntax, lexical boundaries, safe output writes, directory isolation, applied transformation statistics, chunk/function-bytecode coverage, strict fallback rejection and the differences between hardening profiles.

`tests/compatibility.rs` adds differential regressions across six configurations
for numeric-loop coercion, signed zero, Windows and bare-CR source, surrogate
escapes, binary file input, nested interpolation, 185–200 local bindings,
environment mutation, and recursion to depth 10,000. The Pages workflow requires
these tests with an official interpreter on Windows and Linux before deployment.
Install the pinned interpreter with `python scripts/install_luau.py`.

`tests/runtime.rs` executes original and generated Luau and compares stdout. Fixtures cover lexical scopes, recursion, implicit `self`, shadowed built-ins, captured-variable cells, writes after a function returns, per-iteration captures, numeric and generic loops, custom `__iter`, `repeat` conditions, `continue`, `break`, coroutine yields inside iterators and VM bodies, metatables, varargs, multiple return values, nil holes, ordered method lookup, types, interpolated strings, binary strings, Unicode escapes, long strings, floor division and number boundaries. Additional cases test register bytecode across 16 short-name build seeds, string and instruction payload corruption, rolling/cached record decoding, packed/unpacked formats, and regions larger than the 128-statement limit. Each interpreter invocation has a timeout.

## Luau interpreter

The local Windows tools are at `tests/runtime/0.741/luau.exe` and `luau-compile.exe`. Runtime binaries are gitignored; a fresh checkout must install them separately from the [official Luau releases](https://github.com/luau-lang/luau/releases). The suite looks for `luau.exe` on Windows and `luau` on other systems, or accepts an explicit executable path:

```powershell
$env:LUAU_BIN = 'C:\tools\luau\luau.exe'
$env:REQUIRE_LUAU_RUNTIME = '1'
cargo test
```

```sh
LUAU_BIN=/path/to/luau REQUIRE_LUAU_RUNTIME=1 cargo test
```

Without an interpreter, runtime cases print a skip message. CI must set `REQUIRE_LUAU_RUNTIME=1` to fail rather than silently skip execution checks. An explicitly set, invalid `LUAU_BIN` always fails.

The interpreter does not provide Roblox services or validate Roblox's native NAMECALL behavior. Smoke-test generated game modules in Roblox Studio as well.

Runtime fixtures execute from their own resolved directory. This avoids Luau's
standalone `require` context errors with Windows 8.3 temporary paths and a checkout
on a different drive, including hosted Windows runners.

`tests/static_hardening.rs` exercises all 256 byte values, empty/partial words,
native bridge literals and repeated access through state-dependent literal
recipes, across eight seed/packing/cache combinations. Bound-string corruption
tests cover both legacy payloads and recipe factory indices/entry points. The
independent audit recognizes recipe operations and verifies their state-dependent
wire records before reconstructing plaintext; extraction failure is not accepted
as evidence of confidentiality.

The profile matrix includes main-chunk compilation, functions-only mode, preserved types, disabled renaming, disabled integrity checks and cache/payload variants. A module-driver test loads original and protected files through Luau `require` and checks returned object identity, method results and shared state after module initialization.

Method-argument callbacks are checked with strict chunk/function coverage to prevent a native fallback from hiding a regression. Cases include callbacks inside table arguments, loop-local capture retention, shared writes, local/parameter/implicit-self shadowing, nested returned closures, argument-side method replacement, nil holes and yielding callbacks. The unrenamed profile explicitly enables the legacy arithmetic predicates so their optional path remains covered. CLI checks verify that native fallback warnings appear without `--stats` and respect `--quiet`.

`cargo test --example audit_strings` checks the independent static extraction probe against generated packed/table pools, empty/binary strings, duplicate literals and invalid checksums. `cargo test --all-targets` includes these example tests. This deliberately demonstrates a confidentiality limit rather than claiming that runtime-equivalence tests measure resistance; see [the measured string audit](../docs/string-audit.md).

Unit checks also cover descriptor field/template agreement across shuffled schemas,
RLE escape bytes and run-length boundaries, typed/legacy configuration equivalence,
consistent validation of invalid options, and temporary-file cleanup after a failed
publication. Run `cargo fmt --check` and
`cargo clippy --all-targets --locked -- -D warnings` alongside these checks.

The benchmark harness has correctness tests in `tests/benchmarks.rs`, including
all 24 workload families, the VM configuration cross product, sample statistics,
CSV quoting and interpreter deadline cleanup. Timing runs use optimized builds
through `cargo bench --bench obfuscation`; see [the benchmark guide](../docs/benchmarks.md).
The CSV comparison tool is tested with `python -m unittest discover -s scripts/tests -v`.

Performance regression checks also verify signed-zero and binary literal indexing,
scalar-register eligibility in the presence of bridges, exact byte-literal
round-trips, and repeated nil/false/empty-vararg results across all runtime profiles.

Prototype-bound literal checks exercise strict coverage, native bridge arguments,
independent context-based inversion, corruption of payloads/indices, and failure
without replacing an existing CLI output. The pool-only audit's additional test
checks that successfully extracted outer values remain ciphertext for its source
targets; it does not establish resistance to an adapted automatic extractor.
