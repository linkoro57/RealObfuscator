# Tests

Run from the repository root:

```sh
cargo test
```

Unit and CLI tests cover deterministic builds, random-stream progress, malformed syntax, lexical boundaries, safe output writes, directory isolation, applied transformation statistics, chunk/function-bytecode coverage, strict fallback rejection and the differences between hardening profiles.

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

The profile matrix includes main-chunk compilation, functions-only mode, preserved types, disabled renaming, disabled integrity checks and cache/payload variants. A module-driver test loads original and protected files through Luau `require` and checks returned object identity, method results and shared state after module initialization.
