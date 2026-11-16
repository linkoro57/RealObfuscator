# Prototype-bound string hardening

No mode guarantees that automatic deobfuscation is impossible. Client-side code,
keys, handlers and decoded values remain available to an analyst controlling the
runtime. OWASP describes obfuscation as raising reverse-engineering cost rather
than preventing it: [code-obfuscation guidance](https://mas.owasp.org/MASWE/MASVS-RESILIENCE/MASWE-0059/).

## Use

```sh
realobfuscator script.luau --mode hardened --stats

# Incremental adoption: report uncovered strings without rejecting the artifact
realobfuscator script.luau --mode vm --vm-bound-strings --stats

# Reject uncovered runtime strings explicitly
realobfuscator script.luau --mode vm --vm-bound-strings --require-bound-strings
```

The `hardened` preset uses maximal VM hardening, rolling record decoding,
invocation-local constant caching, disabled retained shared-pool string caching,
and strict chunk/function compilation coverage. It enables prototype-bound
strings, rejects remaining runtime string references and requires integrity checks.
Native method and iteration protocol bridges remain native and are reported as
such; strict string coverage does not claim those bridges are private bytecode.

The library equivalent is `ObfuscationConfig::hardened()`. Existing profiles keep
their earlier configuration and performance tradeoffs. Set `vm_bind_strings` for
optional binding and `require_bound_strings` for strict literal coverage.
`--vm-unbound-strings` selects legacy pool-only protection in ordinary VM mode;
disabling binding or integrity while requiring bound strings is an error.

## What changed

Compiled string factories contain an additional encoded envelope instead of the
original literal. The decoding stream depends on that string's constant index and
its prototype's seed, salt, entry position and feedback step. The outer shared
pool can still be decoded independently, but its result is another ciphertext
rather than the original literal. A one-byte index guard and plaintext checksum
detect mismatched/corrupted envelopes when integrity checks are enabled.

Literal arguments inside native method, iteration and closure factories are
rewritten to the owning prototype's string factories too. The decoder is passed
through an explicit generated interface; factory bodies do not depend on a later
local declaration. Colon calls remain native and preserve method lookup and
argument evaluation order.

Native interpolated expressions are currently left native as complete
expressions. Strict mode rejects them rather than silently emitting pool-only
protection for their literal fragments. Interpolated expressions compiled by the
VM use bound format constants normally. Unsupported function/chunk constructs
also fail strict coverage checks before replacing an existing output.

The report exposes `vm_bound_strings` (unique protected constants per prototype)
and `vm_unbound_string_references` (remaining runtime references outside those
factories). These counters are collected when binding is enabled; zero values in
a legacy profile do not establish binding coverage. References can include generated field keys; this is conservative
coverage accounting, not a count of confidential application secrets. Type syntax
and analyzer-only type functions are not runtime string references. Global names,
native method names, the watermark and some structural information remain visible.

## Evidence and limits

The existing pool-only audit remains unchanged. Its new regression fixture checks
two original strings in both a pure VM chunk and native method arguments, across
three seeds and packed/table outer pools: twelve artifacts. The outer pool must
still be recovered successfully, with valid checksums, while none of those two
original targets may occur among its recovered values.

That result identifies an added extraction stage, not resistance to all automatic
analysis. The unit test `bound_literals_are_recoverable_with_their_public_prototype_context`
independently inverts the envelope using the public context parameters. It includes
empty strings, every byte value and UTF-8 data. An adapted extractor can reconstruct
that context; an instrumented runtime can observe strings when they are used. The
encoding is build diversification, not cryptographic encryption or authentication.
An analyst can also modify the client-side checks themselves.

Runtime tests cover both cache/payload strategies, binary strings, interpolation,
varargs, captures and native protocol order. Targeted tests modify an inner
ciphertext or constant index and require rejection before the value reaches the
source program. CLI checks preserve an existing output on strict-coverage failure.
See [the legacy audit](string-audit.md) for the original extractor's scope and data.

## Performance

Binding adds a second decoding pass to source strings and additional factory
calls. Native bridges may decode their argument literals on each use. The maximal
profile keeps decoded values only for the current invocation; it does not promise
that a value returned to the application disappears from memory. Short, frequently
used strings can make these costs noticeable.

The benchmark matrix includes `vm-bound-cached` and `vm-bound-rolling`, alongside
their legacy `vm-max-cached-packed` and `vm-max-rolling-packed` counterparts. Use
the same workload, seed and iteration count to assess this tradeoff:

```sh
cargo bench --bench obfuscation --locked -- --suite full --workload unique_strings,method_calls,closures --profile =vm-max-cached-packed,=vm-max-rolling-packed,=vm-bound-cached,=vm-bound-rolling --scales 16 --seeds 42 --runtime
```

Do not interpret timing, code size, or a failed format-specific extractor as a
numeric security rating. Valuable authoritative logic and secrets require a
trust boundary outside the client, rather than a stronger obfuscation promise.

### Local cost probe (2026-11-16)

Three workloads at scale 16, seed 42; seven timed Rust samples with a 3 ms target; three fresh Luau processes with seven warm samples each, one work unit per invocation. Cached and rolling pairs use maximal hardening in both variants. These are local observations, not statistical or security guarantees.

| Workload | Records | Legacy warm CPU us | Bound warm CPU us | Change | Bound constants |
| --- | --- | ---: | ---: | ---: | ---: |
| `unique_strings` | cached | 226.4 | 362.8 | +60.2% | 16 |
| `unique_strings` | rolling | 1273.0 | 1635.6 | +28.5% | 16 |
| `method_calls` | cached | 198.6 | 235.2 | +18.4% | 3 |
| `method_calls` | rolling | 5268.2 | 4888.4 | -7.2% | 3 |
| `closures` | cached | 226.0 | 201.7 | -10.8% | 0 |
| `closures` | rolling | 6721.0 | 7297.8 | +8.6% | 0 |

`closures` has no string constants here and acts as a configuration/diversification control; its timing differences are not a string-hardening benefit. Different configuration streams and machine noise can also affect the other small timings. The string-heavy cached case increases from 31,786 to 34,723 generated bytes. Raw reports are under the ignored `target/benchmarks/bound-strings-20261116/` directory.

An additional `bound-strings-validation-20261116` run checks all 24 workloads in both bound profiles at scale 4, seed 7 (48 cases), against the original checksums. This is a correctness sweep with one process/sample per case, not precision timing.

The standalone demonstration under `target/hardening-20261116/` uses native method arguments. It executes correctly and yields 5 valid outer records to `audit_strings`, with 0 of its 2 original target strings recovered by that pool-only probe. The context-based inverse test still passes, demonstrating that an adapted automatic recovery stage remains possible.
