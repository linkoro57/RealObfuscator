# RealObfuscator

A Rust command-line obfuscator for Luau and Roblox projects. Version **2.1.0** replaces token-based rewriting with a parsed Luau syntax tree, scope-aware renaming, diversified constant encoding, and conservative control-flow dispatch.

## Build and run

```sh
cargo build --release

# Default: rename locals, encode strings, minify
realobfuscator script.luau

# Native transformations, with a report of applied work
realobfuscator script.luau --mode native --stats

# Experimental encoded statement dispatch
realobfuscator script.luau --mode vm --vm-packed --stats

# Process a project into a separate directory
realobfuscator src --output build --mode native
```

On Windows the executable is `target/release/realobfuscator.exe`. Default file output is `script.obf.luau`. Directory builds preserve relative paths, skip `.obf.lua` / `.obf.luau` files, and do not follow symlinks. Input and output directories must not overlap.

## Profiles

| Mode | Applied transformations |
| --- | --- |
| `rename` | Rename local bindings and parameters, preserve global/member names, minify. |
| `lightweight` (default) | Rename plus a shuffled, deduplicated string pool and eager cached decoding. |
| `native` | Rename, lazy string decoding with integrity checks, runtime integer expressions, property indirection, randomized control-flow dispatch, opaque predicates and decoy handlers. |
| `vm` | Native constant transformations plus an experimental interpreter for compatible statement sequences, with randomized opcodes, feedback-encoded instruction tapes and balanced dispatch trees. |

The VM interprets **assignment, compound-assignment and call statements**. Their handlers contain transformed native Luau. Declarations, loops, branches, returns, `break` and `continue` retain their original scopes and control behavior; eligible sequences inside nested blocks are processed separately. It is **not a complete Luau bytecode virtualizer**. A chunk containing only `return 1` has zero virtualized statements. `--stats` reports this explicitly.

Native control-flow dispatch requires at least three consecutive compatible statements. Regions are capped at 128 statements to bound interpreter size; dispatch uses a diversified binary decision tree instead of scanning every handler. Dead-code density controls unreachable handlers inside these regions. Profiles describe enabled passes, and the statistics describe transformations actually applied.

## Options

```sh
# Reproducible development build
realobfuscator script.luau --mode native --seed 42 --stats

# VM payload and decoding variants
realobfuscator script.luau --mode vm --vm-cached --vm-packed
realobfuscator script.luau --mode vm --vm-rolling --vm-unpacked

# Preserve type syntax or inspect formatted output
realobfuscator script.luau --keep-types --pretty

# Decode strings on demand instead of at startup
realobfuscator script.luau --decode lazy

realobfuscator --help-all
```

Release builds use operating-system randomness. An explicit seed produces identical output across processes for the same input and configuration, and different source modules derive different random streams from that seed. Do not use fixed development seeds for diversified releases.

## Compatibility and behavior

- The [darklua](https://github.com/seaofvoices/darklua) / [full_moon](https://github.com/Kampfkarren/full-moon) AST frontend handles supported Luau syntax including interpolated strings, type declarations, generics, casts, compound assignments, floor division, and generalized iteration. Unsupported or malformed syntax returns an error before output is replaced.
- Renaming follows lexical scopes, including recursive local functions, loop bindings, implicit method `self`, shadowing and `repeat ... until` conditions. Local assignment initializers are resolved before their new bindings are introduced.
- Quoted, long-bracket and binary strings share the same encoding pipeline. Luau escapes are decoded by the parser rather than approximated. Interpolation keeps the order of argument evaluation and uses Luau's `%*` formatting.
- Property reads, writes and table keys can be redirected through encoded string constants. Colon method calls remain intact to preserve Roblox method behavior and single receiver evaluation. Method names and named function declarations may remain visible.
- Integer transformations are restricted to exact unsigned 32-bit values. Fractional values and larger doubles retain their original values. Numeric keys and decoder functions are captured before source locals can shadow built-ins.
- Type syntax is removed using AST nodes by default. `--keep-types` retains it; runtime transformations skip type-only code. `--!native` and optimization directives in the original leading comments are retained. Output uses `--!nocheck` and `--!nolint`.
- The optional library `wrap_in_iife` setting defaults to `false`, preserving ordinary stack/environment behavior. Explicitly enabling it adds a function frame and requires stripped types.
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

`obfuscate()` continues to return just the generated source. `obfuscate_with_report()` returns source and counts for original bindings, unique encoded strings, string references, numbers, hidden fields, dispatch regions, VM statements, decoys, predicates and integrity checks.

Protection levels map to these passes: `Minimal` enables renaming, strings and numbers; `Standard` adds native dispatch; `Maximum` adds property indirection and integrity checks; `Paranoid` enables experimental statement dispatch.

## Verification

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The runtime suite compares the stdout of original and generated scripts with the official Luau interpreter, across profiles, payload formats, decoder strategies and seeds. It also checks corruption rejection and large dispatch regions. See [tests/README.md](tests/README.md) to require execution tests on another machine.

## Limits

Obfuscation increases analysis effort; keys, decoders and handlers still ship to the client. The feedback encoding is not cryptographic encryption. Integrity checks detect some payload modifications on decoding, and can themselves be patched; they do not authenticate the entire script. Cached strings are checked when first decoded. There are no anti-debugging or exploit-specific hooks.

More dispatch, constants and decoys increase output size, memory use, compile time and execution overhead. Generated helpers consume Luau registers/locals. Error line numbers, local names and introspection of transformed code can change. Test actual Roblox workloads and target devices before deployment. Keep valuable authoritative logic on the server.

There is no benchmark demonstrating superiority to Luraph, and this project makes no such claim. Full expression/closure virtualization, resistance to dedicated devirtualizers and adversarial evaluation are separate, substantial areas of work.

## License

[GNU General Public License v3.0](LICENSE). Third-party Rust dependencies retain their respective licenses.
