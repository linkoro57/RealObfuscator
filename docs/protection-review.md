# Protection review and structural hardening

Review date: 2026-11-16. Reference revision examined:
`a98c50165f2a53f6d765a82eee60dfea53803445`.
This is a source comparison, not a measured ranking of resistance to reverse
engineering. The additions below are original Rust implementations of general
serialization and AST transformation techniques. No third-party implementation
or runtime source has been incorporated.

## Inventory and decisions

| Reference mechanism | Existing implementation / decision |
| --- | --- |
| Lexical renaming and minification | Already implemented with scope-aware bindings; retain. |
| String feedback encoding, per-literal seeds, shared decoding cache, VM string hoisting | Existing per-constant feedback encoding and optional caching; hardened mode also binds literals to their prototype. A different shipped PRNG does not establish stronger confidentiality. Retain. |
| Constant array deduplication, shuffling, rotation, base64/base85/mixed representation and offset wrappers | Deduplication and shuffled random IDs already exist. Rotation, alternate alphabets and offset cancellation are readily reversible; do not add extra decoding stages without evidence of benefit. |
| Split strings with randomized piece lengths and reconstruction | Added bounded byte-level fragmentation before VM compilation and encoding. Literal-only concatenation preserves binary data and evaluation order. |
| Integer arithmetic expressions and representation mutation | Runtime-key XOR already exists. Constant arithmetic and textual base changes offer no demonstrated advantage. Keep exact uint32 handling and signed-zero safeguards. |
| Local proxy objects accessed through metamethods | VM captures already use cells. Wrapping all native locals adds allocation, metamethod overhead and observable behavior; no demonstrated advantage over the existing register VM. |
| Standalone control-flow flattening, randomized block IDs, declaration/independent-statement order and branch forms | Existing conservative native dispatch and whole-body VM compilation cover overlapping goals. The reference also supports flattening its own interpreter. That additional layer was examined but is not implemented here; no resistance/performance result justifies enabling a second dispatcher by default. |
| Register VM, randomized opcodes and operand positions, balanced handler dispatch | Already implemented, including multiple opcode aliases, physical instruction shuffling, fused operations and incremental record decoding. Retain. |
| Sparse prototype metadata keys | Added when data scattering is enabled. Each descriptor field receives a unique random key in `[1024, 2^24)`, shared with the emitted interpreter. |
| Scattered VM table graph, reconstructed before execution | Added using four independently shuffled columns: destination node, key, leaf value and referenced node. Node IDs and entry order are randomized. Factory functions remain opaque leaves. |
| Anti-tamper sanity state, debug hooks, traceback/error-line checks, protected-call checks and deliberate hangs | Retain existing record/literal integrity checks. Debug hooks and Lua 5.1 introspection are incompatible with normal Roblox Luau; error text and line assumptions are brittle. Startup sanity expressions do not authenticate code. Deliberate hangs are not emitted. |
| Add unused variadic parameters | No demonstrated resistance benefit; changes introspection. Not added. |
| Function wrappers | Already available as opt-in `wrap_in_iife`; retain the default to preserve module/type scope and avoid an extra unconditional frame. |
| Watermark insertion and runtime watermark comparison | Existing visible provenance comment retained. An easily removable equality check does not strengthen payload integrity and can introduce global state. |

## Enabled behavior

- `native` / level 3 fragments source literals of at least 32 bytes into two to
  eight pieces when string encoding is enabled.
- `vm`, `hardened` / level 4 additionally scatter VM data and use sparse metadata.
- `rename` and `lightweight` retain their previous pass selection.
- `--no-split-strings` and `--no-vm-scatter` disable the new layers independently.
  `--split-strings` and `--vm-scatter` enable them explicitly; scattering requires
  VM mode. Library callers use `split_strings` and `vm_scatter_data`.
- `--stats` counts split source strings, scattered tables and scattered entries.
  No eligible input or no compiled prototypes yields zero applied work.

The splitter runs after property indirection, before VM compilation. Type syntax
is skipped. String-call shorthand is normalized before expression replacement.
Fragment boundaries operate on bytes, so embedded NULs and arbitrary byte values
are retained even when a UTF-8 sequence crosses a boundary. Long strings have at
most eight pieces to bound expression depth. Interpolated string segments retain
their existing formatting path.

The graph serializer only processes compiler-owned table initializers. It leaves
functions and their scopes intact, creates all empty nodes before linking them,
and rebuilds the bank once before user code runs. The regular literal encoding
pass still protects the emitted data. Sparse schema keys are used consistently
by bound-string factories, bridges and the interpreter.

## Costs and limits

These changes conceal source layout, not secrets. A static analyzer can collect
the four columns and reproduce reconstruction; an instrumented runtime can inspect
the rebuilt bank. Fragments remain individually extractable from a native string
pool, and concatenations can be reassembled. Existing instruction and bound-string
integrity checks remain patchable and do not authenticate the reconstruction code.

Scattering increases serialized size, startup allocation and generation time,
especially for unpacked byte arrays. It does not add a dispatch step per VM
instruction, but larger data can still affect loading and memory use. Fragmentation
adds decoder/concatenation work and can increase constant counts. Disable either
layer to compare costs on a target workload. No blanket performance or security
superiority is claimed.

An [adapted static audit](string-audit.md#adapted-extraction-2026-11-16) now
reconstructs these layers automatically: all 49 original literals in its replacement
corpus were recovered from native, VM and hardened outputs. In particular, the
hardened result changed from 0/49 for the old pool-only probe to 49/49 after adapting
the extractor. The structural layers do not keep those literals confidential.

## Local cost sample

Windows, optimized Rust build, Luau 0.741, seed 42; medians of five fresh
interpreter processes per configuration. Baseline means the current implementation
with both added layers disabled. This small synthetic sample does not compare
against another engine or establish a general performance improvement.

| Profile | Added layers | Output bytes | Module load ms | Warm execution ms |
| --- | --- | ---: | ---: | ---: |
| native | off | 1,913 | 1.825 | 2.978 |
| native | on | 2,303 | 2.462 | 3.205 |
| vm | off | 26,777 | 4.592 | 4,558.283 |
| vm | on | 59,484 | 7.034 | 4,355.983 |
| hardened | off | 28,315 | 4.580 | 5,394.616 |
| hardened | on | 60,560 | 8.740 | 4,571.681 |

The module returns a function that computes a modular integer sum over 500
iterations and uses the length of an 87-byte string. Warm time measures 100 calls
after loading and a first call. All configurations returned checksum `72687`.
VM output grows roughly 2.1–2.2 times in this sample. Load time increases; the
different warm times are not evidence of a speedup, since RNG consumption also
changes instruction layouts and encoding parameters. Runtime execution remains
much more expensive than the native profile on this loop.

Benchmark source:

```lua
return function(rounds)
local text='a moderately long string used repeatedly to exercise the decoder and concatenation path'
local total=0
for i=1,rounds do total=(total+i*17+#text)%100003 end
return total
end
```

Generate with `--mode native|vm|hardened --seed 42`; toggle
`--no-split-strings` / `--split-strings` and, for VM profiles,
`--no-vm-scatter` / `--vm-scatter`. Measure `require` separately from the 100 warm
calls using `os.clock()`. Raw local results are in
`target/layer-review/results.json` (an untracked build artifact).

## Verification

The differential Luau suite covers packed/table payloads, rolling/cached decoding,
strict prototype-bound literals, native bridges, modules, closure cells, nil holes,
yields and corrupted payload rejection. A dedicated 32-case matrix compares binary,
Unicode, 31/32-byte boundary strings and shorthand calls across four seeds and both
scatter/payload/cache settings. Unit checks verify fragmentation of up to 100,000
bytes with a maximum of eight pieces, sparse key uniqueness and seeded stability.
CLI tests cover switches, applied statistics and output preservation after invalid
configuration. The pool-only extraction regression documents the narrow benefit
and explicitly retains the adapted-extractor limitation.

```powershell
$env:REQUIRE_LUAU_RUNTIME = '1'
cargo test --locked
cargo test --locked --example audit_strings
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
```
