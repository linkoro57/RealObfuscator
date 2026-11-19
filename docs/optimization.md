# Opt-in source optimization

Enable `--optimize` in the CLI, `optimize_code = true` in `ObfuscationConfig`, or
**Optimize code** under the website's Advanced settings. It is disabled by
default in every preset. `--no-optimize` disables it; the last CLI flag wins.
Optimization and pretty/minified formatting are independent settings.

The pass runs before obfuscation and register-VM compilation. It folds literal
integer arithmetic with operands/results inside the exact-double integer range,
primitive comparisons, literal string lengths and concatenations up to 4096
bytes, known short-circuit expressions and known Luau if expressions. It removes
unreachable branches, false/nil while loops and empty blocks. Retained branches
keep their scopes; a selected call or vararg stays single-valued, including nil
when it returns no values. No user code is executed during optimization.

The pass leaves uncertain cases alone: fractional arithmetic, arithmetic on
negative zero, division by zero, modulo, exponentiation, global/property reads,
calls, unknown conditions and operations involving possible metamethods. For
example, `object * 1` remains an operation. It does not cache Roblox calls,
change yielding behavior, inline user functions or propagate mutable bindings.
Type functions and types are skipped. The original runtime AST still determines
the environment/stack-introspection guard before optimization.

The JSON/CLI report exposes `optimization_enabled`, `optimized_expressions`,
`removed_branches`, `removed_loops` and `removed_empty_blocks`. Counts describe
transformations applied during traversal, including child expressions inside a
branch that is subsequently removed; they are not a security score or a count of
executed operations. More general optimizations require additional semantic work.

## Local numeric-loop probe, 2026-11-17

This is a diagnostic workload, not a representative application benchmark or a
Centurion comparison. Its loop deliberately contains optimizable literal work:

```lua
local started=os.clock()
local total=0
for i=1,20000 do
    total+=(19+23)*(7-2)
    if 3*7==22 then total+=98765 elseif false then total*=91 else total+=11+13 end
    while false do total-=17 end
end
print(total,os.clock()-started)
```

Windows, official Luau 0.741, `--mode vm --vm-cached --seed 42`, seven fresh
processes per variant; optimized adds `--optimize`. The median is the script's
`os.clock` region, excluding process launch, parsing and most VM initialization.
The baseline runs preceded the optimized runs; machine load/frequency were not
controlled. Both variants returned the same checksum, **4,680,000**.

| Metric | Baseline | Optimized |
| --- | ---: | ---: |
| Serialized VM instructions | 79 | 51 |
| Output bytes | 64,810 | 45,377 |
| Median timed loop | 78.178 ms | 32.402 ms |

This fixture yields about 30% less output and a 2.41x faster timed loop. Other
scripts may gain little or nothing, and generation adds an AST traversal.
Native compilation already performs many optimizations; protecting constants
and interpreting private bytecode can change which source simplifications help.
No FPS improvement, stronger protection or superiority to another obfuscator is
established. Raw samples and the probe script are in the ignored local directory
`target/optimization-review/`.

`tests/optimization.rs` compares original and optimized execution across native,
VM, strict and compatible profiles. Cases cover scope/captures, side effects,
conditional branches, continue, zero/multiple returns, varargs, metamethods,
numeric boundaries, binary strings, protected type functions and strict guards.
CLI and real-WASM browser tests check defaults, overrides, applied statistics and
reduced VM instruction counts. These tests establish the checked behaviors,
not compatibility with every possible script.
