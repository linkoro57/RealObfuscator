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

## Configuration and cost

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
