# Static string extraction probe

The default extractor now reconstructs the scattered VM graph, decodes
[prototype-bound strings](hardening.md), and reassembles constant fragments. Use
`--pool-only` for the unchanged original extraction algorithm. The original source
is still optional and is read only after extraction to measure exact byte coverage.

## Adapted extraction, 2026-11-16

The original 22,643-byte source and its two historical artifacts were not available
for this rerun. With the user's agreement, the replacement corpus is the neighboring
`UniversalSynSaveInstance/Tools/NotScriptable-Related/NotScriptable Proxy Dumper/NotScriptable Proxy Dumper.luau`:
9,158 bytes and **49 distinct string-literal values**. This is a new corpus, not a
new measurement of the historical 167-literal corpus. Its source is not copied into
the repository and was never executed.

Artifacts were generated at revision `6f33d37` with seed 42 and default `native`,
`vm` and `hardened` profiles. Both source fragmentation and VM scattering were
enabled where selected by those profiles. The hardened output reports zero
unbound runtime string references.

| Profile | Artifact bytes | Pool-only recovery | Adapted recovery | Pool-only median | Adapted median |
| --- | ---: | ---: | ---: | ---: | ---: |
| native | 21,353 | 47 / 49 | **49 / 49** | 12.315 ms | 23.534 ms |
| vm | 690,666 | 47 / 49 | **49 / 49** | 296.334 ms | 624.345 ms |
| hardened | 680,852 | 0 / 49 | **49 / 49** | 391.624 ms | 627.397 ms |

Windows, release build, five fresh audit processes per cell. Timings include
reading, parsing and extraction, excluding startup, optional source comparison
and dump writing. The adapted path currently parses the artifact twice and is
not optimized for speed. All recognized pool, record and bound-literal checksums
validated; all rejection counters were zero.

Both VM artifacts yielded **10 prototypes and 1,336 decoded instructions**. The
hardened artifact yielded **113 verified bound string entries**, including
generated property names and repeated literals in different prototypes. Four
constant-concatenation operations reassembled the two split source literals.
The final sets contain 106, 116 and 229 distinct values respectively, including
intermediate fragments, generated strings and binary payloads; those totals must
not be confused with the 49 original source literals.

The adapted stages are:

1. Apply the independent pool decoder and verify its checksums, as before.
2. Identify generated decoder aliases; simplify exact numeric expressions and
   approved XOR calls needed to read generated data. No arbitrary function calls
   are evaluated.
3. Reconstruct the four shuffled graph columns, including empty nodes, with bounds,
   cycle and duplicate-key checks. Infer sparse descriptor slots from the emitted
   interpreter's AST; no seed or schema is supplied by the user.
4. Invert instruction feedback encoding and RLE, validate record checksums, and
   read the randomized operand layout and physical successor links.
5. Decode each bound literal using its actual prototype context and constant index;
   validate the index envelope and plaintext checksum.
6. Match emitted constant/move/concatenation handlers, follow the serialized
   successor order and propagate known string values. Drop tracked registers on
   branches, calls and unrecognized operations. Also fold literal-only native
   concatenations, including decoded bound factory calls in method bridges.

This is static reconstruction of known generated data and constant expressions.
It does not run the Lua VM, source functions, Roblox APIs, loaders or network calls.
It does not recover the original source/control flow or predict strings assembled
from unknown runtime input. It targets the current emitted helper shapes and
default unwrapped pipeline; unrecognized future formats are not evidence of
successful protection. Size/depth limits make the evaluator intentionally partial.

The result demonstrates that fragmentation, scattering and prototype binding add
extraction stages but **do not preserve literal confidentiality against this adapted
source-informed extractor**. It does not establish a general analyst discovery time.

Evidence SHA-256:

- Source: `d6570c0314ee1ffff0dc324a38c4f25d54d3bcc4e28c574c2ecf8736ad934afe`
- native: `e12c10c5ca2246fe5b2c2b8c9f7106a1a76f189fcac8700c9919b5dd9b1d660b`
- vm: `8e3a6ec637fee5b44c9b0e447631131959065330b215130cd33ceee1690ff9de`
- hardened: `5c5eaa33d20b5c01a4efb6ac1e98b201ed64b5d721055918ece7126fd3fc7db7`

Raw measurements and byte-safe plaintext TSV dumps remain in the ignored
`target/string-audit-20261116/` directory. In adapted mode, dump IDs are sorted-value
ordinals; with `--pool-only`, they remain the original pool IDs. Existing dump
files are never overwritten.

## Historical pool-only assessment

Measured on 2026-11-13 using two locally supplied artifacts and the release build of [examples/audit_strings.rs](../examples/audit_strings.rs) on Windows. No Lua was executed, and no network requests were made by the probe.

### Method

The probe parses the artifact with the existing Luau frontend and looks for numeric-keyed records containing a string or byte table plus four numeric parameters. It independently reproduces the string decoder from the known public implementation, decodes each matching record and checks the plaintext checksum. It does not call the encoder, generated helper functions or a Lua interpreter.

An optional original source is parsed **after** extraction, solely to compare its distinct string-literal byte sequences against the recovered values. It is not an input to decoding. Escaped/binary strings are compared as bytes. This coverage does not include comments, runtime-computed strings or remotely loaded code. Pool records also contain generated field names and binary VM payloads; recovering one of those payloads is not devirtualizing its instructions.

### Results

Both artifacts came from the same 22,643-byte source and the launcher's cached VM configuration. The source contains 167 distinct string-literal values.

| Artifact | Bytes | Valid decoded pool records | Rejected candidate records | Original distinct literals recovered | Median read / parse / decode |
| --- | ---: | ---: | ---: | ---: | ---: |
| Before 2.2.1 | 527,771 | 372 | 0 | 167 / 167 | 182.155 ms |
| 2.2.1 | 586,211 | 373 | 0 | 167 / 167 | 149.971 ms |

Artifact SHA-256 fingerprints:

- Before 2.2.1: `e80d36b78dcddd25439ea7b3c0aa99130b0d6a171c02fe7cb95f8198bd153c8f`
- 2.2.1: `e43b6201c40513f64874d07aee8388f5bb5610669c331f3a83644116719f8e32`

Each median uses five fresh audit processes. Timing includes reading the artifact, AST parsing, visiting records and decoding, but excludes process startup, the source-coverage check and optional dump writing. Tool development, compilation and learning the decoder are also excluded. Individual measurements were 184.952, 182.155, 181.831, 155.571 and 195.525 ms for the older artifact, and 147.565, 156.500, 154.014, 149.971 and 144.848 ms for 2.2.1. Differences at this scale are not evidence of stronger or weaker VM protection.

The encoded pool offers no confidentiality against this source-informed extractor in either artifact. Larger files, more function tokens, randomized helper names, checksums and a VM around callers do not prevent this extraction. The `+ delta - delta` integer-expression layers are also removable noise within the encoder's exact-integer range.

This is not an independent black-box assessment or a measurement of an analyst's discovery time. It does not reconstruct control flow, source code, local names or a running Roblox environment. No numeric security rating or minimum deobfuscation duration follows from these measurements.

## Reproduce

```powershell
cargo build --release --example audit_strings --locked

# Extraction without the original source:
.\target\release\examples\audit_strings.exe .\script.obf.lua

# Compare with the original pool-only algorithm:
.\target\release\examples\audit_strings.exe .\script.obf.lua --pool-only

# Optional coverage comparison and byte-safe TSV dump to a new file:
.\target\release\examples\audit_strings.exe .\script.obf.lua --source .\script.lua --dump .\recovered-strings.tsv

cargo test --example audit_strings --locked
```

The dump records a value ID, byte length, hexadecimal bytes and escaped bytes.
Existing dump paths are rejected. Unrecognized formats are not interpreted as
successful resistance: no recognized pool/VM data, invalid graphs or failed
recognized checksums make the command fail. An artifact may still have valid
outer records while an unsupported inner stage remains unresolved; inspect the
stage counters and source-coverage results. The tool targets the known formats
and does not claim to recognize arbitrary obfuscators.

Local plaintext dumps and raw measurements remain under the ignored `build/verification/` directory. The supplied scripts, original source and decoded strings are not included in this repository.
