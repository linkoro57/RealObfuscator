# Compatibility regressions and validation

This update fixes observed frontend and runtime mismatches. It does not establish
complete Lua compatibility or stronger confidentiality of obfuscated strings.
The runtime target remains Luau.

## Native fallbacks with bound literals

`--mode compatible` / `ObfuscationConfig::compatible()` retain the maximal
literal/VM configuration and permit native chunk/function fallbacks. Literals
remaining in those bodies use a dedicated literal-only prototype with the same
bound envelopes or instruction-linked recipes. This addresses strict string
coverage errors caused by environment/stack introspection without compiling or
wrapping the affected functions. Native interpolations use captured `string.format`
with `%*`, preserving argument evaluation before conversion and escaping `%`.
Binary/empty strings, shorthand calls and method arguments follow the same path.
Interpolation values are adjusted to one result before formatting. A final call
with no returns or an empty vararg becomes `nil`; extra return values do not
expand into the format call. This fixes native/lightweight and VM string backends.

`vm_native_bound_strings` counts unique literals in these auxiliary prototypes;
`vm_native_literal_prototypes` reports their number. They do not increment
`vm_functions` or `vm_chunks`. Native fallback warnings remain visible, and
Hardened continues to reject uncovered source bodies. The Compatible preset is
available in the browser and exposes the native-string-binding toggle in advanced
settings. Strict coverage errors now suggest the compatible alternative.

`tests/native_literals.rs` compares original/generated Luau across recipe/envelope
and packed/cached variants. It exercises `setfenv` on functions with isolated
environments, `debug.info(1, 'f')` identity, binary literals, interpolation side
effects and mixed compiled/native method bridges. Input scripts supplied for
diagnosis are parsed/obfuscated separately; loading external game scripts is not
part of this differential suite. Native source structure remains statically
inspectable, and the independent string extractor remains applicable.

## Corrected behavior

- Windows CRLF input no longer breaks `\z` escapes or escaped interpolation
  newlines. Bare CR bytes inside long strings retain their original values.
- Surrogate escapes such as `\u{D800}` become equivalent decimal UTF-8 byte
  escapes before the Rust AST frontend can reject them. Literal backslashes in
  long strings and escaped backslashes remain untouched.
- CLI byte input preserves raw non-UTF-8 strings, long strings and nested
  interpolation. The browser worker exposes the same binary import conversion.
  Invalid bytes outside literals/comments return an error.
- Numeric VM loops validate and convert string controls once, before iteration,
  using native Luau loop behavior. They preserve signed zero and bound evaluation
  order and handle invalid controls, zero steps, NaN and infinity consistently.
- Constant helpers initialize inside one scope and export one interface binding.
  Native scripts with 195 or 199 locals no longer exhaust Luau's local limit.
  Explicit `--wrap-in-iife` isolates the application scope for 200-local scripts;
  it adds a frame and requires stripped types.
- Functions using environment/stack introspection keep native execution across
  the source, including `_G` aliases. This preserves `setfenv` changes to functions
  that do not themselves mention introspection. Shadowed local built-ins do not
  trigger this fallback. Strict coverage rejects it and reports the reason.
- With stripped types, virtualized functions are factory-created execution
  closures. An unrelated generic loop no longer gives every function an extra
  execution frame. A plain recursive function reaches depth 10,000 in the
  differential regression. Recursive `self:method()` declarations conservatively
  retain native execution, with a visible reason and strict coverage rejection.

## Checks

Seven differential regressions in `tests/compatibility.rs` exercise six
configurations: lightweight, native, rolling/packed VM, cached/unpacked VM,
preserved types without renaming, and hardened. Individual regressions select
applicable configurations; strict profiles are expected to reject native
fallbacks. Existing runtime, CLI, bridge, benchmark and static extraction tests
remain part of validation. Nine browser tests exercise the actual Wasm engine,
including binary file imports and Windows escapes.

An additional local comparison used 17 official Luau conformance fixtures from
an existing Luau source checkout and official CLI version 0.741. Each original
was executed first, then compared with lightweight/native/VM output at seed 42.
The same fixture bytes and interpreter were used before and after:

| Comparison | Passed | Failed |
| --- | ---: | ---: |
| Before these changes | 37 / 51 | 14 / 51 |
| After these changes | 50 / 51 | 1 / 51 |

The remaining failure is `calls` in VM mode, whose stress cases assert native
stack/resource exhaustion behavior. Its lightweight and native versions pass.
Passing ordinary recursion does not make stack limits or stack-sensitive error
messages transparent through a virtual machine. Native bridges, recursive
iteration, preserved typed forwarding functions, dynamic method recursion and
extreme argument counts still require application testing. The standalone CLI
does not provide Roblox services or certify Roblox native namecalls.

Reproduce the optional corpus check with a local official Luau source checkout:

```sh
python scripts/install_luau.py
cargo build --release --locked
python scripts/check_conformance.py \
  --fixtures /path/to/luau/tests/conformance \
  --luau tests/runtime/0.741/luau \
  --obfuscator target/release/realobfuscator \
  --output target/conformance
```

Use `.exe` executable names on Windows. The script records baseline exclusions,
generation failures, runtime failures and stdout mismatches separately, writes
JSON results, and returns failure when a generated case fails. It does not skip
the remaining VM failure. The Pages workflow separately requires the repository's
regression suite with the pinned interpreter on Windows and Linux before deploy.

The independent string extractor was adapted to the helper interface and VM
factory layout. A format change alone must not be reported as extraction
resistance; the [string audit limitations](string-audit.md) still apply.
