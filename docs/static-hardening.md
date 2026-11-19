# Autonomous instruction-linked literal programs

The Hardened preset enables `vm_literal_programs`. Its output remains autonomous:
no external secret, loader service or runtime network request is added.

## What changes

Bound source strings no longer have independent ciphertext/checksum factories.
Their factories identify an entry point inside the VM's instruction stream.
Executing that entry point reconstructs the string:

1. An initialization record binds the state to the prototype, literal index and
   relocated recipe entry point.
2. Randomized add/XOR/rotate instructions evolve a 32-bit state. Tags, operand
   layouts, physical locations and arithmetic operands vary with the build seed.
   Runtime bit operations use Luau's documented unsigned 32-bit semantics:
   [standard library](https://luau.org/library/#bit32-library).
3. Byte-emission instructions consume this state and ciphertext feedback. Words
   execute in a shuffled order and write to checked output positions.
4. Every record after initialization is itself encoded with the preceding recipe
   state. Decoding all records with only the prototype's public seed fails their
   checksums. An analyzer must follow the recipe while calculating its state.
5. The final instruction validates the literal index, length, complete coverage
   and plaintext checksum before returning the string.

Native method bridge arguments use these same factories. Empty and binary strings
follow the same protocol. Cached instruction mode skips state-dependent recipe
records; it does not predecode them using an incorrect zero state. Literal state
and reconstruction buffers belong to each decoder invocation.

The default maximum hardening profile retains its existing invocation-local
plaintext lifetime. User code can still store or expose returned values.

## Memory-dependent literal reconstruction

Literal programs now keep eight invocation-local scratch words in addition to
the scalar state. Before emitting each group of up to four bytes, a `LiteralMix`
record selects a scratch slot using the current state, reads that word, performs
a 16-bit Feistel-style mixing step with the record operands, and writes an updated
word back into that slot. The next record's encoding context depends on the result.
An interpreter of the recipe therefore needs to reproduce the evolving scratch
memory as well as the previous scalar state and ciphertext feedback.

This replaces one existing arithmetic transition per emitted word, rather than
adding padding instructions. Empty literals still work without a mixing record.
Initialization derives the eight words from the literal's initial state; these
are public values, not 256 bits of secret entropy. The odd 16-bit multiplier,
16-bit product reduction and 32-bit sums remain exact in Luau's number model.
The generated code uses captured `bit32` functions and does not add network calls,
external keys, debug hooks or script/environment checks.

The memory belongs to an individual decoder call. Cached instruction mode still
excludes state-dependent recipe records, and a subsequent or nested literal call
starts with a fresh scratch table. More arithmetic, table initialization and
state-dependent reads/writes increase literal-access work and allocation; the
instruction count alone does not express this runtime cost.

The source-informed audit independently identifies the indexed memory writes and
replays the new semantics. Its `interpreted_literal_memory_steps` counter reports
successfully reconstructed memory-mixing steps. It continues to support the prior
scalar-only format. See the [current probe](static-resistance-review.md) for the
before/after evidence. This is an added reconstruction dependency, not encryption
or a demonstrated minimum analyst recovery time.

## Configuration and cost

All VM profiles also use contextual word encoding and alias-specific operand
roles. Opcode, successor and operand words are transformed together through
rotate/XOR/add feedback keyed by prototype parameters, physical record position
and, for recipe records, the preceding literal state. Repeated operations therefore
do not have a directly reusable encoded opcode word. Each alias of one operation
uses a different permutation of its three operands, and its handler consumes that
permutation directly. Relocation happens before these transformations.

These layers require a static reader to reproduce another record transformation
and recover the operand roles of handlers. They remain public reversible
transformations. The adapted audit has been updated with an independent inverse;
its ability to extract literals is preserved, not counted as a protection failure
that can be hidden by changing a file format. Existing historical audit timings
do not measure this revised wire format.

Word feedback adds work to every cold/cached initialization or rolling decode and
can reduce the effectiveness of per-record RLE compression. Cached execution pays
this decoding cost at initialization; rolling execution repeats it on each
instruction visit. No claim of improved analyst recovery time follows from these
changes. Seeded output changes with this version; generated scripts remain
self-contained and do not depend on a separately installed decoder.

```sh
# New instruction-linked reconstruction is enabled by Hardened.
realobfuscator script.luau --mode hardened --stats

# Explicit comparison with the older prototype-bound payload format.
realobfuscator script.luau --mode hardened --no-vm-literal-programs --stats

# Enable recipes with a non-strict VM configuration.
realobfuscator script.luau --mode vm --vm-bound-strings --vm-literal-programs
```

The library field and browser setting are `vm_literal_programs`. Bound strings,
VM mode and integrity checks are required. `vm_literal_instructions` reports the
actual number of recipe instructions added, not a security score.

This increases output size, generation work and literal-access cost. Long strings
require more records; disabling retained instruction caches can increase repeated
decoding work. Use native/lightweight profiles for workloads whose performance
matters more than this added analysis layer. A fixed seed is reproducible within
the current implementation, not an artifact-compatibility promise across versions.

## Static analysis limits

The initial state and every transition remain reconstructible from the artifact.
This is state-dependent obfuscation, not cryptographic confidentiality. A static
interpreter that recognizes the recipe semantics can still recover the literals
without executing the application, contacting Roblox or possessing its original
source. Dynamic observation of returned plaintext also remains possible.

The independent audit therefore includes a bounded recipe interpreter. It infers
opcode aliases from the emitted decoder AST, reads the physical records with the
current state, verifies checksums, reconstructs shuffled output and checks the
final length/checksum. It does not call the Rust encoder or generated Luau code.
The older extractor's failure is recorded as an unsupported stage, never as proof
of resistance. See [the measured string audit](string-audit.md) for results.

Tests cover every byte, empty and partial words, repeated access, native method
bridges, build seeds, packed/table payloads, rolling/cached decoding and incorrect
factory indices/entry points. Existing runtime-equivalence and browser tests remain
required. Client-side obfuscation cannot establish a minimum analyst recovery time
or a guarantee that autonomous code is almost impossible to deobfuscate.
