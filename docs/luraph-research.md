# Public Luraph research and the RealObfuscator implementation

Research date: 2026-11-13. This is an evidence-based inventory, not an exhaustive specification of a closed product. Versions differ. No proprietary implementation or third-party deobfuscator code was copied into this project.

## Sources and their limits

- [Luraph's official historical updates](https://lura.ph/dashboard/documents/updates) describe VM encryption, virtualized constants, randomization, control-flow protection, anti-tamper/anti-dump features, platform locks, optional debug-library use, and selective-virtualization macros. The site requires JavaScript; search-indexed historical entries were available during research. They do not establish today's internals. The v13.4.4 entry says VM Compression was removed, so compression must not be presented as a universal current security feature.
- [Ferib's original technical analysis, 2022-02-24](https://ferib.dev/blog/lua-devirtualization-part-3-devirtualizing-luraph/) studies a sample with instruction translation, serialized function data, protected constants/upvalues, and an interpreter. Its unpacking-stage example illustrates why a complete decoded program in one table can become a useful capture point. These observations apply to that sample, not every version.
- [The author's Reddit discussion](https://www.reddit.com/r/ReverseEngineering/comments/t0l8pw/part_3_in_my_lua_devirtualization_journey/) links the research and states that some details were withheld. Reddit claims of universal protection or universal breaks are not used as implementation evidence.
- [PhoenixZeng/LuraphDeobfuscator](https://github.com/PhoenixZeng/LuraphDeobfuscator) documents handler detection, instruction redirection, decryption, and decompiler-oriented transformations. Its stated test range is v11.5–v11.8.1, making it historical evidence only.
- [A public v14.5.2 sample report](https://github.com/listentosmoke/deobluraph/blob/main/ANALYSIS_REPORT.md) reports opcode aliases and an LZMA/base85 layer. Its conclusions were not independently reproduced here. This cannot establish whether a packaging layer belongs to Luraph itself or to an additional wrapper, or which current configurations use it. Entropy alone does not establish encryption strength.

## Inventory and coverage

The following are independent implementations of general techniques, not claims of identical Luraph internals or equivalent resistance.

| Family | RealObfuscator 2.2 implementation | Scope / limit |
| --- | --- | --- |
| Custom instruction virtualization | AST compiler and register interpreter in `src/function_vm.rs` | Supported main chunks and function bodies; uncompiled chunks retain conservative statement dispatch. |
| Arithmetic / comparisons / branching | Register instructions, conditional jumps, short-circuit operations, numeric loops | Preserves native operators, including metamethod behavior; explicit stack/environment introspection needs native fallback. |
| Closures and upvalues | Mutable binding cells and closure factories that capture cell references | Factories return native callable closures with compiled bodies where supported. A new loop iteration creates new cells. |
| Variable argument and result counts | Explicit counted packs and multivalue instructions | Nil holes and zero-result calls are tested; parentheses and casts retain single-result semantics. |
| Opcode diversification | Two, three or four randomized opcode aliases per used operation | `basic`, `client-max`, and `max`; adds variation, not a proof against semantic matching. |
| Instruction fusion | Constant-right binary operations execute as fused instructions | Used when the operand is a literal and evaluation order is unchanged. |
| Control-flow scrambling | Shuffled physical instruction records, explicit successor addresses and relocated branch targets | A new physical layout and operand layout are generated per function. |
| Operand and descriptor layout | Permuted instruction words and a shuffled descriptor schema | Schema and decoder still exist in the artifact and can be analyzed. |
| Instruction encoding | Independent 32-bit feedback state per physical record | Supports jumps without a single global decoded instruction archive. This is not cryptographic secrecy. |
| Virtual constant access | Constant instructions call lazy getters; nil/false presence tracked separately | `max` keeps the function constant cache per invocation instead of on the shared prototype. |
| String / number protection | Per-string 32-bit feedback encoding, indirection, integer expressions | The maximal profile disables retained plaintext string caching by default. |
| Runtime integrity | Per-record checksums checked on decode, payload bounds, constant checksums | Detects tested mutations; checks and expected values can be patched. No whole-script authenticity claim. |
| Packing | Optional per-record RLE before encoding, offsets and packed byte strings | Size optimization; independently implemented, not LZMA/base85 and not a standalone security boundary. |
| Incremental decoding | Record decoding during execution or explicit cached decoding | Cached mode retains a decoded instruction table for speed; rolling mode does not. |
| Native runtime protocols | Native colon-call bridges and native generic-iteration drivers around VM bodies | Preserves NAMECALL ordering, `__iter`, and coroutine behavior. Their bridge expressions remain native and are counted separately. |
| Identifier and property hiding | Scope-aware local renaming, encoded property keys | Native method names can remain visible. |
| Predicates and dead handlers | Existing conservative dispatcher predicates and mixed decoy handlers | Compiler optimization and semantic analysis may remove them; more noise is not a time guarantee. |
| Build diversification | OS-seeded builds, source-derived streams, randomized names/layouts | Fixed seeds intentionally reproduce builds. |
| Selective coverage / performance | Native profiles, VM hardening profiles, cache controls, strict coverage flags | `--require-function-vm` and `--require-chunk-vm` reject whole-body fallbacks. Functions-only mode and native protocol counts expose the boundaries. No LPH macro compatibility claim. |
| Platform lock / debug-library-specific protection | Luau target and captured required runtime helpers | Platform-specific anti-debug behavior is not implemented or presented as equivalent protection. |
| Undocumented anti-dump / decompiler tricks | No claim of reproducing private mechanisms | Output remains valid Luau. Memory needed by executing code remains observable to an analyst controlling the client. |

## Validation and outstanding limits

Differential tests run original and generated code in the official Luau runtime, including recursive functions, shared upvalues, loop captures, generalized/custom iteration, yields, metatables, method lookup order, exact integer boundaries, binary strings, main-chunk varargs and multiple returns. Module-loader tests verify returned object identity and state retained after the main chunk returns. Corruption tests separately exercise string and instruction payload rejection. Strict-coverage and diversification reports are tested as API behavior.

The interpreter is not an implementation of the official Luau binary bytecode format. It compiles a supported AST subset into a private instruction set and retains compatibility bridges or native fallbacks for the remaining constructs. Fallback reasons are reported. It changes native stack frames, debug information, and compilation/performance characteristics. Test real Roblox workloads and device budgets.

Neither these sources nor the regression suite proves a minimum reverse-engineering time, completeness against all Luraph versions, or superiority to Luraph. Such claims need an independent, scoped adversarial evaluation.
