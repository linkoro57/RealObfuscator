//! Materialize literals through randomized instruction recipes in the VM stream.
//! All recipe inputs remain public; this raises reconstruction work, not secrecy.
use super::{
    compiler::Program,
    instruction::{Instruction, Op},
};
use crate::rng::Rng;
use darklua_core::nodes::Expression;

// Per-literal scratch state. Public deterministic inputs, not secret key material.
fn memory(state: u32) -> [u32; 8] {
    std::array::from_fn(|i| {
        (state ^ (i as u32 + 1).wrapping_mul(0x9E37_79B9)).rotate_left((i as u32 + 1) * 3)
    })
}

fn mix(state: u32, memory: &mut [u32; 8], a: u32, b: u32, c: u32) -> u32 {
    let slot = (state & 7) as usize;
    let previous = memory[slot];
    let value = state ^ previous;
    let low = value & 65535;
    // A 16-bit product keeps the corresponding Luau double arithmetic exact.
    let product = (low * ((a & 65535) | 1) + (a >> 16)) & 65535;
    let round = (low << 16) | ((value >> 16) ^ product);
    let next = round.rotate_left(b) ^ c;
    memory[slot] = previous.rotate_left(11).wrapping_add(next) ^ a;
    next
}

pub(super) fn prepare(program: &mut Program, rng: &mut Rng) -> Result<usize, &'static str> {
    let first = program.code.len();
    for (index, constant) in program.constants.iter().enumerate() {
        let Expression::String(string) = constant else {
            continue;
        };
        let bytes = string.get_value();
        let index = u32::try_from(index + 1).map_err(|_| "literal index limit")?;
        let length = u32::try_from(bytes.len()).map_err(|_| "literal byte limit")?;
        let entry = u32::try_from(program.code.len()).map_err(|_| "instruction limit")?;
        program.literal_entries.insert(index, entry);
        let mut state = rng.next_u64() as u32;
        let mut memory = memory(state);
        program.code.push(Instruction {
            op: Op::LiteralStart,
            a: state,
            b: index,
            c: length,
        });
        let mut order: Vec<_> = (0..bytes.len().div_ceil(4)).collect();
        rng.shuffle(&mut order);
        for word in order {
            // The emitted byte order differs from the execution order. Each word
            // consumes the prior state, including prior ciphertext feedback.
            for round in 0..rng.range(1, 3) {
                program.literal_keys.insert(program.code.len(), state);
                let a = rng.next_u64() as u32;
                let b = rng.range(1, 32) as u32;
                let c = rng.next_u64() as u32;
                let variant = rng.range(0, 2);
                let op = if round == 0 {
                    state = mix(state, &mut memory, a, b, c);
                    Op::LiteralMix
                } else if variant == 0 {
                    state = (state ^ a).rotate_left(b).wrapping_add(c);
                    Op::LiteralXor
                } else {
                    state = state.wrapping_add(a).rotate_left(b) ^ c;
                    Op::LiteralAdd
                };
                program.code.push(Instruction { op, a, b, c });
            }
            let offset = word * 4;
            let count = (bytes.len() - offset).min(4);
            let mut plain = (rng.next_u64() as u32).to_le_bytes();
            plain[..count].copy_from_slice(&bytes[offset..offset + count]);
            let cipher = u32::from_le_bytes(plain) ^ state;
            let position = u32::try_from(offset)
                .ok()
                .and_then(|n| n.checked_mul(8))
                .and_then(|n| n.checked_add(count as u32))
                .ok_or("literal position limit")?;
            let feedback = rng.next_u64() as u32;
            program.literal_keys.insert(program.code.len(), state);
            program.code.push(Instruction {
                op: Op::LiteralBytes,
                a: cipher,
                b: position,
                c: feedback,
            });
            state = (state.rotate_left(9) ^ cipher).wrapping_add(feedback);
        }
        let checksum = bytes.iter().fold(0u64, |hash, byte| {
            (hash * 65599 + u64::from(*byte)) % 2147483647
        }) as u32;
        program.literal_keys.insert(program.code.len(), state);
        program.code.push(Instruction {
            op: Op::LiteralReturn,
            a: length,
            b: checksum,
            c: index,
        });
    }
    Ok(program.code.len() - first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_scalar_states_require_the_selected_memory_word() {
        for state in [0, 1, 2, 3, 4, 5, 6, 7, 0x8000_0000, u32::MAX] {
            let mut first = memory(state);
            let mut second = first;
            second[(state & 7) as usize] ^= 0x8000_0000;
            assert_ne!(
                mix(state, &mut first, u32::MAX, 31, 0x8000_0000),
                mix(state, &mut second, u32::MAX, 31, 0x8000_0000)
            );
        }
    }
}
