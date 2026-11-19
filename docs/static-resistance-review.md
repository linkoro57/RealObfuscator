# Static resistance review — 2026-11-19

The autonomous Hardened preset does not prevent static recovery of its
string literals. The initial probe recovered all four original distinct string values
without executing Lua, including a synthetic credential, a URL, a long fragmented
string and a binary literal.

This is a source-informed extraction probe for the known output format. It is not
full deobfuscation of arbitrary control flow, an independent assessment, or a
measurement of analyst discovery time.

## Baseline before memory-dependent recipes

Built the CLI and audit from the working tree before the memory-dependent change with
`cargo build --locked --example audit_strings --bin realobfuscator` on Windows.
Used seed 42, default settings for each preset, and the same
[synthetic source](../tests/fixtures/static-analysis-probe.luau) in every case.

| Preset | Artifact bytes | Original literals recovered | Decoded VM instructions | Interpreted literal recipe steps |
| --- | ---: | ---: | ---: | ---: |
| Lightweight | 2,562 | 4 / 4 | 0 | 0 |
| Virtual machine | 76,146 | 4 / 4 | 94 | 0 |
| Hardened | 149,755 | 4 / 4 | 276 | 182 |

All rejected graph, VM-prototype and bound-string counters were zero. Hardened
reported 16 bound literal entries and zero unbound runtime string references;
those counters describe transformations, not confidentiality.

The audit reads the original source only after extraction to compare literal sets.
It reconstructs graph data, decodes contextual instruction words, follows the
stateful recipes and joins recognized fragments. The probe source is never
executed by the audit. These single-run debug-build measurements are not suitable
for performance or security-strength comparisons.

## Memory-dependent recipes: after the change

The same source and seed now use an eight-word scratch table local to each literal
decoder call. Every emitted word has a memory-dependent mixing transition in place
of one previous scalar-only arithmetic transition. No key or service is required.

The current audit independently replays both formats:

| Hardened artifact | Bytes | VM instructions | Recipe steps interpreted | Memory steps interpreted | Original literals recovered |
| --- | ---: | ---: | ---: | ---: | ---: |
| Before memory-dependent mixing | 149,755 | 276 | 182 | 0 | 4 / 4 |
| With memory-dependent mixing | 137,359 | 276 | 182 | 60 | 4 / 4 |

Both rows have 16 bound literal entries, zero unbound runtime references and zero
rejected graph/prototype/bound-string counters. The added requirement is replaying
the state-dependent reads and writes, not an increase in the number of recipe
records. Output sizes also depend on opcode allocation and encoded record contents;
the smaller artifact here is not a general compression claim.

The adapted extractor still recovers every source literal. This establishes a
format and reconstruction dependency change, not confidentiality or a measured
minimum analyst effort. The [format notes](static-hardening.md) describe the
arithmetic and allocation costs.

### Diagnostic runtime cost

On the local Windows Luau 0.741 interpreter, five alternating fresh processes per
artifact each ran the generated body 200 times. Printing was captured by an outer
local function; every run produced the same 30,600-byte total. Timing covers
generated-helper initialization and execution, excluding parsing and process
startup. These are cold-body repetitions of this small fixture, not a general
Roblox workload or a pure decoder microbenchmark.

| Artifact | Median seconds | Minimum–maximum seconds |
| --- | ---: | ---: |
| Before | 1.476 | 1.301–1.558 |
| With scratch memory | 1.541 | 1.391–1.673 |

The median increased about 4.4% in this diagnostic, with overlapping sample ranges.
The run is too small and uncontrolled to establish a general overhead bound or an
analyst-cost improvement. Raw values and the harness remain under ignored
`target/memory-cost-results.json` and `target/measure-literal-memory.mjs`.

## Reproduce the current probe

```powershell
cargo build --locked --example audit_strings --bin realobfuscator

./target/debug/realobfuscator.exe tests/fixtures/static-analysis-probe.luau --mode hardened --seed 42 -o target/static-probe-memory-hardened.luau --stats
./target/debug/examples/audit_strings.exe target/static-probe-memory-hardened.luau --source tests/fixtures/static-analysis-probe.luau
```

Substitute `lightweight` or `vm` and distinct output names for the other rows.
The original baseline run used an identical source copy in `target/` before this
engine change. The commands above regenerate the current format, not that prior
baseline artifact.

## What would change the security boundary?

Additional autonomous layers can require more reconstruction stages, but the
artifact still supplies the decoder and all its inputs. Breaking this particular
extractor through a format change would not establish resistance to an adapted
extractor. Any new transformation needs updated adversarial checks and runtime
equivalence tests, with its output-size and execution costs reported separately.

Protecting encrypted data against analysis of the distributed file alone would
require a secret that is absent from that file, supplied through a separately
designed runtime integration. This changes deployment, availability and key
handling; it must not be silently enabled for ordinary standalone output. If a
client receives a key or decrypted data, runtime observation remains possible.

For code and secrets that must remain unavailable to a Roblox client, keep the
sensitive work on the server and expose only the required operations. See
[Roblox's client/server architecture](https://create.roblox.com/docs/projects/client-server)
and the project's [existing static extraction audit](string-audit.md).

The new memory-dependent transition is not represented as making deobfuscation
impossible. The existing `--no-vm-literal-programs` option disables the entire
recipe mechanism when its cost is unsuitable.
