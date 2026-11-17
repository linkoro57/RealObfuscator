# Architecture and invariants

The library transforms AST nodes; the public lexer provides metadata and lexical
utilities. The CLI owns filesystem traversal, diagnostics and publication.

`source.rs` prepares byte input before parsing: it preserves binary literal bytes,
normalizes CRLF, and expands surrogate escapes into their Luau UTF-8 bytes. Long
strings retain bare CR bytes and literal backslash sequences.

## Pipeline

`Obfuscator::obfuscate_inner` validates configuration, parses source, removes types
when requested, renames bindings and redirects fields. Function virtualization runs
before constant protection; generated descriptors receive constant protection but
the interpreter itself does not. Conservative control-flow dispatch runs after
virtualization and skips generated VM entry bodies. Finally, helper bindings are
renamed, prefixes are joined to the source body, and the result is generated and
parsed again. The interpreter is renamed once in its own construction phase; the
constant helpers and descriptor factories are renamed separately. Large descriptor
and string-pool data are built directly as AST nodes rather than formatted and
reparsed as generated source.

Type functions execute in a separate analyzer environment. Runtime visitors must
skip them; the renamer temporarily replaces their bodies and restores them after
scope traversal. Do not inject runtime helpers into preserved type functions.

The source token stream is computed once and reused for reserved names, directives
and retained comments. Each invocation clones the configured random stream and
mixes in a stable source hash. Preserve random draw order when changing code that
must retain seeded output compatibility. Never depend on hash-map iteration order
when emitting output or allocating random identifiers.

## Register VM

| Module | Responsibility |
| --- | --- |
| `function_vm/mod.rs` | Candidate traversal, fallback accounting, opcode allocation and assembly |
| `compiler.rs` | AST lowering and logical register instructions |
| `bridge.rs` | Native protocol bridges and scope-aware captured-cell rewriting |
| `instruction.rs` | Logical instruction vocabulary |
| `literal_programs.rs` / `literal_programs.luau` | Shuffled literal recipes and state-dependent record decoding |
| `encoding.rs` | Physical placement, branch relocation, compression and record encoding |
| `wire.rs` | Alias-specific operand roles and contextual word feedback |
| `schema.rs` | Named descriptor fields and their randomized Luau table slots |
| `runtime.rs` / `runtime.luau` / `invocation.luau` | Interpreter factory and invocation generation |

Compilation borrows the input AST and builds private mutable state. It returns a
`Program` only when the whole candidate succeeds, including its final empty return.
The caller replaces the candidate body and publishes statistics only on success.
With stripped types, function declarations become factory-created VM closures,
removing a forwarding call from each invocation. Local declarations retain a
separate lexical binding before assignment so recursive references remain valid.
Only prototypes with generic iteration bridges receive nested execution frames.
Preserved signatures and the main chunk keep forwarding bodies. Environment
introspection is detected before renaming and prevents virtualization throughout
the source; recursive native namecalls have a reported conservative fallback.
Unsupported constructs discard the candidate state; previously compiled child
functions may remain virtualized inside a native parent. Literal lookup uses
indexed decoded strings, exact numeric bits, and explicit nil/boolean slots;
structural capture-list tables retain AST equality checks. Pure programs containing
no closure, method or iteration bridge replace cell operations with register moves,
without moving instructions or changing logical branch targets.

Registers and constant IDs are one-based. Logical branch targets are zero-based
instruction offsets until serialization. Physical record positions are one-based,
with zero reserved for termination. A record has five 32-bit little-endian words
and a four-byte checksum. Physical layout, relocation and encoding must agree with
the interpreter; runtime-equivalence and corruption tests cover that contract.

Before word serialization, each opcode alias assigns logical a/b/c operands to
its own physical roles. Aliases of one operation use distinct permutations. The
generated handler directly uses those roles, including branch operands and native
bridge arguments. Literal handlers normalize roles before interpreting recipes.
Next, all five words (including opcode and successor) are masked with rotating
32-bit feedback derived from prototype seed/salt, physical position and optional
recipe state. The checksum covers these transformed words. The decoder verifies
that checksum, reverses word feedback, and returns alias-specific operands.
All arithmetic uses wrapping u32 operations or exactly representable Luau sums.
The independent audit reverses both layers before its bounded interpretation.

Literal recipes append unreachable-from-main records after the compiler's final
return. Each factory points to a relocated recipe entry. The writer latches the
state before each recipe transition into that record's encoding key; the runtime
supplies the same state to `newDecoder`. A descriptor bitmask marks these records
so the optional eager instruction cache skips them. The independent audit decodes
them lazily with its own bounded interpreter rather than treating them as corrupt
keyless records. The mask identifies record roles; it is not secret key material.

`schema::Field` names descriptor fields on the serialization side and template
placeholders on the interpreter side. Add or change fields in that contract before
changing either consumer. Captured locals hold mutable cells: escaping closures
retain the cell references present at creation, not a mutable register map.

The rolling decoder returns five scalar operands and reuses bounded invocation-local
scratch buffers; it does not cache decoded instruction tables. Cached mode
materializes record tables once per prototype. Scratch lengths are checked against
the current record, including truncated RLE escapes, so a shorter record cannot
reuse old bytes. Constant caches use a private sentinel for nil; maximum hardening
keeps its plaintext cache invocation-local. Empty vararg packs are shared only as
internal read-only values, and native protocol bridges keep their cell storage.
The decoder factory is skipped entirely on warm cached calls. Constant-cache reads
are inlined into their opcode handlers. If no generic-iteration operation requires
recursive execution, the dispatch loop runs directly in the runner; otherwise the
nested execution and return-signal protocol remains in place.

## Configuration and errors

Public string configuration fields remain available for existing callers.
`StringDecodeMode` and `VmHardening` provide typed builders for new callers.
Validation parses those fields once per obfuscation invocation; transformation
passes consume the parsed values without string fallbacks. `validate()` uses the
same rules, including rejecting line breaks in the build watermark.

Malformed user input and invalid configuration return errors. Generated snippets
use an internal parser assertion: a failure there indicates a generator defect.
The library and CLI forbid unsafe Rust in their own code.

## File publication

The CLI creates a unique temporary file beside its destination with exclusive
creation, writes and syncs it, closes it, then renames it into place. A guard closes
and removes unpublished temporary files on errors or unwinding. A failed write
must leave the destination intact. This does not promise crash durability of the
parent directory or a transaction across a directory build.

## Verification

Run formatting, Clippy and all targets. Set `REQUIRE_LUAU_RUNTIME=1` for execution
checks so an absent interpreter fails the suite; see [the test guide](../tests/README.md).
Runtime tests compare original and generated execution across profiles and seeds.
They establish behavioral compatibility, not resistance to reverse engineering.

Optional prototype-bound strings are described in [the hardening guide](hardening.md).
Their factories receive a decoder and context explicitly. Native bridge literals
are moved into those same prototype factories before serialization; native
interpolations remain conservatively uncovered. Strict coverage is checked before
publishing output, and the decoder's interface name is preserved when renaming.
