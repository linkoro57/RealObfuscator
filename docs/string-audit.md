# Static string-pool extraction probe

Measured on 2026-11-13 using two locally supplied artifacts and the release build of [examples/audit_strings.rs](../examples/audit_strings.rs) on Windows. No Lua was executed, and no network requests were made by the probe.

## Method

The probe parses the artifact with the existing Luau frontend and looks for numeric-keyed records containing a string or byte table plus four numeric parameters. It independently reproduces the string decoder from the known public implementation, decodes each matching record and checks the plaintext checksum. It does not call the encoder, generated helper functions or a Lua interpreter.

An optional original source is parsed **after** extraction, solely to compare its distinct string-literal byte sequences against the recovered values. It is not an input to decoding. Escaped/binary strings are compared as bytes. This coverage does not include comments, runtime-computed strings or remotely loaded code. Pool records also contain generated field names and binary VM payloads; recovering one of those payloads is not devirtualizing its instructions.

## Results

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

# Optional coverage comparison and byte-safe TSV dump to a new file:
.\target\release\examples\audit_strings.exe .\script.obf.lua --source .\script.lua --dump .\recovered-strings.tsv

cargo test --example audit_strings --locked
```

The dump records the pool ID, byte length, hexadecimal bytes and escaped bytes. Existing dump paths are rejected. Unrecognized formats are not interpreted as successful resistance: no candidate records or failed candidate checksums make the command fail. The tool is specific to the known pool schema and does not claim to recognize arbitrary obfuscators.

Local plaintext dumps and raw measurements remain under the ignored `build/verification/` directory. The supplied scripts, original source and decoded strings are not included in this repository.
