# RealObfuscator - Free Lua/Luau Obfuscator for Roblox

**RealObfuscator is a free, open-source Luau obfuscator for Roblox, written in Rust.**
It transforms compatible `.lua` and `.luau` source files locally through a command-line
interface or Rust library. No account, paid plan, subscription or API key is required
for the obfuscation workflow.

The runtime target is **Luau**. Lua source must be compatible with Luau; accepting a
`.lua` extension does not imply that generated code runs in a standard Lua 5.x
interpreter. See [compatibility and behavior](#compatibility-and-behavior).

[Quick start](#build-and-run) | [Profiles](#profiles) | [FAQ](#frequently-asked-questions) | [Benchmarks](docs/benchmarks.md)

| Project fact | Details |
| --- | --- |
| Source repository | [linkoro57/RealObfuscator](https://github.com/linkoro57/RealObfuscator) |
| Price and license | Free to build and use; [GPL-3.0-only](LICENSE) source license |
| Implementation | Rust command-line tool and library |
| Input | `.lua` and `.luau` files using supported Luau syntax; single files or directories |
| Output runtime | Luau, including Roblox projects |
| Processing | Local files; no hosted processing service required |
| Default profile | Lightweight local renaming, encoded strings and minification |
| Optional layers | Integer expressions, property indirection, control-flow dispatch and register-bytecode virtualization |

## What it does

- Rename local bindings while preserving global and member names where required.
- Encode string literals and apply optional number and property transformations.
- Compile supported chunks and functions to a private register instruction set.
- Process project directories while preserving their relative file paths.
- Report applied transformations, VM coverage and native fallbacks with `--stats`.
- Reproduce development builds with `--seed`, or diversify builds with OS randomness.

Obfuscation increases analysis work; it does not make client-side code impossible
to reverse engineer. Performance and coverage depend on the selected profile and
source. The [hardening guide](docs/hardening.md), [measured optimization report](docs/performance-improvements.md)
and [limits](#limits) explain the tradeoffs.

## Build and run

Install a stable Rust toolchain with Cargo and Git. Clone the source and run the
included example; the first build may download Rust dependencies:

```sh
git clone https://github.com/linkoro57/RealObfuscator.git
cd RealObfuscator
cargo run --release --locked -- examples/score.luau --stats
```

This creates `examples/score.obf.luau`. To install the command into Cargo's binary
directory, which must be on your `PATH`:

```sh
cargo install --path . --locked
realobfuscator --help
```

After installation, process your own scripts:

```sh
# Default: rename locals, encode strings, minify
realobfuscator script.luau

# Native transformations, with a report of applied work
realobfuscator script.luau --mode native --stats

# Function bytecode with maximum diversification and incremental decoding
realobfuscator script.luau --mode vm --vm-hardening max --vm-rolling --vm-packed --stats

# Process a project into a separate directory
realobfuscator src --output build --mode native
```

You can also build without installing using `cargo build --release --locked`.
On Windows the executable is `target/release/realobfuscator.exe`; run it as
`.\target\release\realobfuscator.exe`. On Unix-like shells, a local build is
`./target/release/realobfuscator`. Default file output is `script.obf.luau`. Directory builds preserve relative paths, skip `.obf.lua` / `.obf.luau` files, and do not follow symlinks. Input and output directories must not overlap.

## Frequently asked questions

### Is RealObfuscator a free Lua obfuscator?

RealObfuscator is free and open source under GPL-3.0-only. It obfuscates Lua source
that is compatible with Luau, as well as Luau scripts for Roblox. Its generated
helpers and VM target Luau. No paid license or remote obfuscation API is required.
Read [LICENSE](LICENSE) for the source license terms.

### Does it support every Lua version?

The frontend and generated runtime target Luau. Standard Lua 5.1, 5.2, 5.3, 5.4
and LuaJIT compatibility is not promised. Check the [compatibility section](#compatibility-and-behavior)
and validate the output in your actual runtime.

### Can I use it for Roblox scripts?

Roblox Luau is the project's target. It handles Luau-specific syntax, scopes,
closures and native method bridges as documented below. The standalone interpreter
suite checks execution equivalence; Roblox services and native userdata behavior
still need testing in Roblox Studio.

### Do scripts leave my computer?

The CLI reads and writes local files. The obfuscation pipeline does not call a
hosted processing API. Installing the Rust toolchain and downloading build
dependencies are separate network operations.

### Which profile should I start with?

`lightweight` is the default. `native` adds source transformations; `vm` adds
register-bytecode virtualization for supported constructs. `hardened` additionally
binds string constants to VM prototypes and rejects uncovered cases. Compare
[profile behavior](#profiles) and [benchmark results](docs/benchmark-results.md)
against your workload before choosing more expensive layers.

### Does obfuscation make scripts impossible to deobfuscate?

Keys and decoders ship with client-side code. An adapted extractor or instrumented
runtime can still recover information. [The string audit](docs/string-audit.md)
and [hardening limitations](docs/hardening.md) document tested boundaries. Keep
valuable authoritative logic on the server.

## Profiles

| Mode | Applied transformations |
| --- | --- |
| `rename` | Rename local bindings and parameters, preserve global/member names, minify. |
| `lightweight` (default) | Rename plus a shuffled, deduplicated string pool and eager cached decoding. |
| `native` | Rename, lazy string decoding with integrity checks, runtime integer expressions, property indirection, randomized control-flow dispatch and decoy handlers. |
| `vm` | Register-bytecode compilation for supported chunks/functions; captured-variable cells; randomized physical records, operand layouts and opcode aliases; constant fusion; incremental decoding and record integrity. Remaining compatible statement regions use conservative dispatch. |
| `hardened` | Maximal VM settings with prototype-bound string constants and strict chunk/function/string coverage; uncovered runtime strings fail before output is replaced. |

The register-bytecode backend supports main chunks and functions, captured-variable
cells, shuffled instruction records, opcode aliases, fused operations, lazy
constants and per-record integrity checks.

Native and stronger profiles also fragment long strings before encoding. VM and
hardened profiles scatter the VM table graph into shuffled columns and use sparse
prototype keys. These layers increase size and startup work; use
`--no-split-strings` and `--no-vm-scatter` to compare their cost. See the
[protection review](docs/protection-review.md) for the technique inventory,
implementation decisions and extraction limits.

The function compiler covers local bindings, assignments, native arithmetic/comparison operators, tables, indexing, short-circuit logic, interpolated strings, calls, recursive functions, closures, varargs, multiple returns, branches, loops, `break` and `continue`. Bindings observable by closures or native bridges use mutable cells; pure VM bodies keep their bindings directly in registers. Closure factories retain the cells present at creation, preserving captures across loop iterations and writes after a function returns. Method calls and generalized iteration use native protocol bridges with VM registers/body execution to preserve lookup order, `__iter` and yielding behavior.

Functions needing unsupported constructs retain their native bodies. Direct environment/stack introspection, preserved type declarations in a candidate function, and closures embedded in iterator expressions can require fallback. Child functions may still be compiled independently. Fallback warnings appear even without `--stats`, unless `--quiet` is used. `--stats` reports compiled functions, encoded instructions, native functions and fallback reasons, and counts method, iteration and closure bridges separately. `--require-function-vm` rejects whole-function fallbacks before writing output; it still permits the reported native protocol bridges.

Callbacks nested in method arguments, including table constructors, use scope-aware capture rewriting. Each bridge invocation snapshots the required cell references so callbacks retained by a receiver keep the correct loop iteration and still share later writes. The complete method call remains native to preserve lookup and argument evaluation behavior; these bridges are not private bytecode.

The main chunk is compiled after its child functions. A native fallback reason is reported when it cannot compile, and `--require-chunk-vm` can reject such an output. `--vm-functions-only` explicitly keeps main-chunk control flow native. Uncompiled chunks retain the earlier conservative backend: assignment, compound-assignment and call handlers contain transformed native Luau, while declarations and control transfers keep their scopes. A chunk containing only `return 1` can now compile to register bytecode. The register interpreter is **not the official Luau binary bytecode format**, and this is not a claim of complete virtualization for every Luau construct.

Native control-flow dispatch requires at least three consecutive compatible statements. Regions are capped at 128 statements to bound interpreter size; dispatch uses a diversified binary decision tree instead of scanning every handler. Dead-code density controls unreachable handlers inside these regions. Profiles describe enabled passes, and the statistics describe transformations actually applied.

Arithmetic predicates such as `(x*(x+1))%2==0` are algebraic identities, not a meaningful security boundary. Presets disable them. The library's legacy `opaque_predicates` field remains available for compatibility and experimentation; setting it to `true` adds removable arithmetic noise.

## Options

[Prototype-bound hardening](docs/hardening.md) adds an extraction stage for strings
in VM constants and native bridge arguments. It does not guarantee resistance to
an adapted extractor or runtime instrumentation:

```sh
realobfuscator script.luau --mode hardened --stats
realobfuscator script.luau --mode vm --vm-bound-strings --stats
```

```sh
# Reproducible development build
realobfuscator script.luau --mode native --seed 42 --stats

# VM payload and decoding variants
realobfuscator script.luau --mode vm --vm-cached --vm-packed
realobfuscator script.luau --mode vm --vm-rolling --vm-unpacked

# Require chunk and function compilation, with no whole-body fallbacks
realobfuscator script.luau --mode vm --require-function-vm --require-chunk-vm --stats

# Compile functions while retaining the main chunk's native control flow
realobfuscator script.luau --mode vm --vm-functions-only --stats

# Explicit speed / plaintext-cache tradeoff (VM mode disables string caching by default)
realobfuscator script.luau --mode vm --string-cache --vm-cached
realobfuscator script.luau --mode native --no-string-cache

# Preserve type syntax or inspect formatted output
realobfuscator script.luau --keep-types --pretty

# Decode strings on demand instead of at startup
realobfuscator script.luau --decode lazy

realobfuscator --help-all
```

Release builds use operating-system randomness. An explicit seed produces identical output across processes for the same input and configuration, and different source modules derive different random streams from that seed. Do not use fixed development seeds for diversified releases.

`basic`, `client-max` and `max` emit respectively two, three and four opcode aliases per used register operation. Each compiled chunk/function receives a new physical record order, operand-word layout and encoding parameters. Packed mode uses per-record RLE when it reduces size, followed by 32-bit feedback encoding. Rolling mode decodes records as needed; cached mode decodes and retains the records for faster execution. Maximum hardening keeps constants in an invocation-local cache rather than a shared plaintext prototype cache. These choices have significant size and execution costs; lightweight remains the default profile.

## Compatibility and behavior

- The [darklua](https://github.com/seaofvoices/darklua) / [full_moon](https://github.com/Kampfkarren/full-moon) AST frontend handles supported Luau syntax including interpolated strings, type declarations, generics, casts, compound assignments, floor division, and generalized iteration. Unsupported or malformed syntax returns an error before output is replaced.
- Renaming follows lexical scopes, including recursive local functions, loop bindings, implicit method `self`, shadowing and `repeat ... until` conditions. Local assignment initializers are resolved before their new bindings are introduced.
- Quoted, long-bracket and binary strings share the same encoding pipeline. Luau escapes are decoded by the parser rather than approximated. Interpolation keeps the order of argument evaluation and uses Luau's `%*` formatting.
- Property reads, writes and table keys can be redirected through encoded string constants. Colon method calls remain intact to preserve Roblox method behavior and single receiver evaluation. Method names and named function declarations may remain visible.
- Integer transformations are restricted to exact unsigned 32-bit values. Fractional values and larger doubles retain their original values. Numeric keys and decoder functions are captured before source locals can shadow built-ins.
- Type syntax is removed using AST nodes by default. `--keep-types` retains syntax outside replaced VM bodies and function signatures; type declarations in a candidate body can cause native fallback. Runtime transformations skip type functions. `--!native` and optimization directives in the original leading comments are retained. Output uses `--!nocheck` and `--!nolint`.
- The optional library `wrap_in_iife` setting defaults to `false`. Explicitly enabling it adds a function frame and requires stripped types. VM calls and native bridges themselves add interpreter frames; stack/environment introspection is not transparent through virtualization.
- Generated code is parsed again before writing. Files are written through unique, exclusively created temporary files; invalid input leaves existing output intact.

## Library API

```rust
use realobfuscator::{ObfuscationConfig, Obfuscator, ProtectionLevel};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ObfuscationConfig::default()
        .with_seed(42)
        .with_level(ProtectionLevel::Maximum);
    let output = Obfuscator::new(config)
        .obfuscate_with_report("local n=17; n+=2; n*=3; print(n, 'hello')")?;
    println!("{:#?}", output.report);
    std::fs::write("script.obf.luau", output.code)?;
    Ok(())
}
```

`obfuscate()` continues to return just the generated source. `obfuscate_with_report()` returns source and counts for original bindings, unique encoded strings, string references, numbers, hidden fields, dispatch regions, VM statements, decoys, predicates and integrity checks. It also reports `vm_chunks`, `vm_chunk_fallback`, `vm_functions`, `vm_instructions`, `vm_opcode_variants`, `vm_fused_instructions`, native protocol/factory bridges, `vm_native_functions`, and a reason/count map in `vm_fallback_reasons`.

Protection levels map to these passes: `Minimal` enables renaming, strings and numbers; `Standard` adds native dispatch; `Maximum` adds property indirection and integrity checks; `Paranoid` enables the function-bytecode backend with maximal diversification, incremental decoding and disabled retained string caching.

New callers can use typed configuration builders while existing string fields remain compatible:

```rust
use realobfuscator::{ObfuscationConfig, StringDecodeMode, VmHardening};

let config = ObfuscationConfig::default()
    .with_string_decode_mode(StringDecodeMode::Lazy)
    .with_vm_hardening(VmHardening::Max);
```

The [architecture guide](docs/architecture.md) describes pass ordering, VM contracts,
capture semantics and file publication invariants.

## Verification

The [performance optimization report](docs/performance-improvements.md) documents
the current implementation changes, matched timings and cold-load tradeoffs.

The [benchmark suite](docs/benchmarks.md) covers 24 workload families and 46
profiles, with a 9,936-case exhaustive matrix, Rust microbenchmarks, Luau runtime
measurements, CSV exports and a comparison tool:

```sh
cargo bench --bench obfuscation --locked -- --suite quick
cargo bench --bench obfuscation --locked -- --suite standard --runtime
cargo bench --bench obfuscation --locked -- --suite full --list
```

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The runtime suite compares original and generated scripts in the official Luau interpreter across profiles, payload formats, decoder strategies and seeds. It tests arithmetic, aliasing, recursive/shared closures and loop captures, nil holes, method lookup order, native/custom iteration, yields, strict coverage, corruption rejection and large regions. See [tests/README.md](tests/README.md) to require execution tests on another machine.

## Limits

Obfuscation increases analysis effort; keys, decoders and handlers still ship to the client. The feedback encoding is not cryptographic encryption. Integrity checks detect some payload modifications on decoding, and can themselves be patched; they do not authenticate the entire script. Cached strings are checked when first decoded. There are no anti-debugging or exploit-specific hooks.

The string pool uses one shared decoder, which an analyst can identify, emulate or instrument. Repeated calls and cached plaintext are useful analysis targets. A `loadstring` / HTTP loader still exposes its behavior, and its destination is observable when used; encoding the URL does not provide confidentiality. The visible watermark intentionally identifies RealObfuscator and its GitHub repository. Successful parsing or runtime-equivalence tests establish compatibility, not resistance to deobfuscation.

A [source-informed static extraction probe](docs/string-audit.md) recovered all 167 original distinct string literals from each of two supplied artifacts, including 2.2.1, without executing Lua. The known pool decoder can be reproduced independently; the local median read/parse/decode times were below 0.2 seconds. This does not reconstruct the VM or measure discovery time, but it demonstrates that this pool format does not keep literals confidential against that extractor. Arithmetic cancellation layers such as `value + delta - delta` are removable noise as well.

More dispatch, constants and decoys increase output size, memory use, compile time and execution overhead. Generated helpers consume Luau registers/locals. Error line numbers, local names and introspection of transformed code can change. Test actual Roblox workloads and target devices before deployment. Keep valuable authoritative logic on the server.

A [local performance probe](docs/performance.md) records the size and execution costs of native, cached VM and maximal rolling VM profiles on the same small numeric workload. The maximal profile is much slower on that loop; it is not a default recommendation for frame-sensitive code.

The [public research inventory](docs/luraph-research.md) records consulted sources,
implemented techniques, native bridges and unknown/private features.

There is no benchmark demonstrating superiority to Luraph, minimum reverse-engineering time, or resistance to dedicated devirtualizers. The research inventory explicitly records private or platform-specific mechanisms that have not been reproduced. Independent adversarial evaluation and Roblox compatibility/performance testing remain necessary.

## License

[GNU General Public License v3.0](LICENSE). Third-party Rust dependencies retain their respective licenses.
