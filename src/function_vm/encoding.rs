//! Serialize logical instructions into diversified physical records.
use super::{compiler::Program, instruction::Op, schema::Field};
use crate::rng::Rng;
use darklua_core::nodes::{
    Block, Expression, FunctionExpression, ReturnStatement, StringExpression, TableEntry,
    TableExpression,
};
use std::collections::BTreeMap;

fn array(values: impl IntoIterator<Item = Expression>) -> Expression {
    TableExpression::new(values.into_iter().map(TableEntry::from_value).collect()).into()
}

/// Build descriptors directly as AST nodes, without formatting and reparsing data.
/// RNG consumption stays stable: layout, keys, then one alias per physical record.
pub(super) fn descriptor(
    program: &Program,
    codes: &BTreeMap<Op, Vec<u32>>,
    packed: bool,
    bind_strings: bool,
    rng: &mut Rng,
) -> [(Field, Expression); Field::COUNT] {
    let count = program.code.len();
    let mut order: Vec<_> = (0..count).collect();
    rng.shuffle(&mut order);
    let mut positions = vec![0u32; count + 1];
    for (physical, &logical) in order.iter().enumerate() {
        positions[logical] = physical as u32 + 1;
    }
    let mut layout = [0, 1, 2, 3, 4];
    rng.shuffle(&mut layout);
    let seed = rng.range(1, 1u64 << 32) as u32;
    let step = rng.range(1, 1u64 << 32) as u32;
    let multiplier = rng.next_u64() as u32;
    let mut payload = Vec::with_capacity(if packed { count.saturating_mul(25) } else { 0 });
    let mut offsets = Vec::with_capacity(count + 1);
    let mut records = Vec::with_capacity(if packed { 0 } else { count });
    let mut payload_length = 0;
    for (physical, &logical) in order.iter().enumerate() {
        let instruction = &program.code[logical];
        let choices = &codes[&instruction.op];
        let opcode = choices[rng.range(0, choices.len() as u64) as usize];
        let mut words = [
            opcode,
            instruction.a,
            instruction.b,
            instruction.c,
            positions[logical + 1],
        ];
        match instruction.op {
            Op::LiteralStart => {
                words[1] ^=
                    seed ^ multiplier ^ instruction.b.wrapping_mul(65599) ^ positions[logical];
            }
            Op::Jump => words[1] = positions[instruction.a as usize],
            Op::JumpFalse | Op::JumpTrue => words[2] = positions[instruction.b as usize],
            Op::GenericFor => {
                words[1] = positions[instruction.a as usize];
                words[3] = positions[instruction.c as usize];
            }
            _ => {}
        }
        let roles = super::wire::operands(opcode);
        let operands = [words[1], words[2], words[3]];
        for i in 0..3 {
            words[roles[i] + 1] = operands[i];
        }
        let carry = program.literal_keys.get(&logical).copied().unwrap_or(0);
        let words = super::wire::encode(
            words,
            seed ^ multiplier ^ (physical as u32 + 1) ^ carry,
            step,
        );
        let mut record = [0u8; 24];
        for (index, word) in words.iter().enumerate() {
            record[layout[index] * 4..layout[index] * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        let checksum = record[..20].iter().fold(physical as u64 + 1, |hash, byte| {
            (hash * 257 + u64::from(*byte)) % 2147483647
        }) as u32;
        record[20..].copy_from_slice(&checksum.to_le_bytes());
        let compressed = if packed { rle(&record) } else { Vec::new() };
        let mut serialized = Vec::with_capacity(25);
        if packed && compressed.len() < record.len() {
            serialized.push(1);
            serialized.extend(compressed);
        } else {
            serialized.push(0);
            serialized.extend_from_slice(&record);
        }
        let serialized = crate::codec::encode(
            &serialized,
            seed ^ (physical as u32 + 1) ^ carry,
            step,
            multiplier,
        );
        offsets.push(((payload_length + 1) as f64).into());
        payload_length += serialized.len();
        if packed {
            payload.extend_from_slice(&serialized);
        } else {
            records.push(array(
                serialized.into_iter().map(|byte| (byte as f64).into()),
            ));
        }
    }
    offsets.push(((payload_length + 1) as f64).into());
    let payload = if packed {
        StringExpression::from_value(payload).into()
    } else {
        array(records)
    };
    let constants = array(program.constants.iter().enumerate().map(|(index, value)| {
        let value = if bind_strings {
            if let Expression::String(string) = value {
                if let Some(&entry) = program.literal_entries.get(&(index as u32 + 1)) {
                    use darklua_core::nodes::{FunctionCall, TypedIdentifier};
                    let call = FunctionCall::from_name("decode")
                        .with_argument(Expression::identifier("context"))
                        .with_argument(Expression::identifier("index"))
                        .with_argument(positions[entry as usize] as f64);
                    return FunctionExpression::new(
                        Block::from(ReturnStatement::one(call)),
                        ["decode", "context", "index"]
                            .map(TypedIdentifier::new)
                            .into(),
                        false,
                    )
                    .into();
                }
                return bound_string(
                    string.get_value(),
                    index as u32 + 1,
                    seed,
                    step,
                    multiplier,
                    positions[0],
                );
            }
            value.clone()
        } else {
            value.clone()
        };
        FunctionExpression::from_block(Block::from(ReturnStatement::one(value))).into()
    }));
    let recipe_mask = if program.literal_keys.is_empty() {
        Expression::nil()
    } else {
        let mut mask = vec![0u8; count.div_ceil(8)];
        for &logical in program.literal_keys.keys() {
            let position = positions[logical] as usize - 1;
            mask[position / 8] |= 1 << (position % 8);
        }
        StringExpression::from_value(mask).into()
    };
    [
        (Field::Payload, payload),
        (Field::Seed, (seed as f64).into()),
        (Field::Step, (step as f64).into()),
        (Field::Salt, (multiplier as f64).into()),
        (
            Field::Layout,
            array(layout.map(|slot| ((slot * 4 + 1) as f64).into())),
        ),
        (Field::Entry, (positions[0] as f64).into()),
        (Field::InstructionCount, (count as f64).into()),
        (Field::Constants, constants),
        (Field::ConstantValues, TableExpression::default().into()),
        (Field::ConstantKnown, TableExpression::default().into()),
        (Field::Records, Expression::nil()),
        (Field::Offsets, array(offsets)),
        (Field::RecipeMask, recipe_mask),
    ]
}

/// A factory receives its decoder and prototype from the running interpreter.
/// The outer pool can expose this ciphertext, but cannot decode the original
/// literal using the old self-contained five-field record alone.
fn bound_string(
    bytes: &[u8],
    index: u32,
    seed: u32,
    step: u32,
    salt: u32,
    entry: u32,
) -> Expression {
    use darklua_core::nodes::{FunctionCall, TypedIdentifier};
    let mut envelope = Vec::with_capacity(bytes.len() + 1);
    envelope.push(index as u8);
    envelope.extend_from_slice(bytes);
    let key = seed ^ index.wrapping_mul(65599) ^ entry ^ 0xa511_e9b3;
    let payload = crate::codec::encode(&envelope, key, step, salt);
    let checksum = bytes.iter().fold(0u64, |hash, byte| {
        (hash * 65599 + u64::from(*byte)) % 2147483647
    });
    let call = FunctionCall::from_name("decode")
        .with_argument(StringExpression::from_value(payload))
        .with_argument(checksum as f64)
        .with_argument(Expression::identifier("index"))
        .with_argument(Expression::identifier("context"));
    FunctionExpression::new(
        Block::from(ReturnStatement::one(call)),
        ["decode", "context", "index"]
            .map(TypedIdentifier::new)
            .into(),
        false,
    )
    .into()
}

fn rle(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let mut end = i + 1;
        while end < bytes.len() && bytes[end] == bytes[i] && end - i < 255 {
            end += 1;
        }
        let count = end - i;
        if count >= 4 || bytes[i] == 255 {
            result.extend([255, count as u8, bytes[i]]);
        } else {
            result.extend_from_slice(&bytes[i..end]);
        }
        i = end;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_literals_are_recoverable_with_their_public_prototype_context() {
        use darklua_core::nodes::{Arguments, LastStatement};
        for bytes in [
            Vec::new(),
            (0..=255).collect(),
            b"multi-byte \xf0\x9f\x98\x80".to_vec(),
        ] {
            for index in [1u32, 257, 20000] {
                let (seed, step, salt, entry) = (0x12345678, 0xf1234567, 0x87654321, 19);
                let Expression::Function(factory) =
                    bound_string(&bytes, index, seed, step, salt, entry)
                else {
                    panic!("expected factory")
                };
                let Some(LastStatement::Return(ret)) = factory.get_block().get_last_statement()
                else {
                    panic!("expected return")
                };
                let Some(Expression::Call(call)) = ret.iter_expressions().next() else {
                    panic!("expected call")
                };
                let Arguments::Tuple(args) = call.get_arguments() else {
                    panic!("expected arguments")
                };
                let Some(Expression::String(payload)) = args.iter_values().next() else {
                    panic!("expected payload")
                };
                // Independent inverse: context is sufficient to automate recovery.
                // Binding removes self-contained plaintext records, not reversibility.
                let mut state = seed ^ salt ^ entry ^ index.wrapping_mul(65599) ^ 0xa511_e9b3;
                let decoded: Vec<_> = payload
                    .get_value()
                    .iter()
                    .enumerate()
                    .map(|(offset, cipher)| {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        let value = *cipher ^ state as u8;
                        state = state
                            .wrapping_add(step)
                            .wrapping_add(u32::from(*cipher))
                            .wrapping_add(offset as u32 + 1);
                        value
                    })
                    .collect();
                assert_eq!(decoded[0], index as u8);
                assert_eq!(&decoded[1..], bytes);
            }
        }
    }

    // Independent reader of the Luau wire format: 255 introduces count/value.
    fn expand(encoded: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut input = encoded.iter().copied();
        while let Some(byte) = input.next() {
            if byte == 255 {
                let count = input.next().expect("escape needs a count");
                assert_ne!(count, 0);
                let value = input.next().expect("escape needs a value");
                bytes.extend(std::iter::repeat_n(value, usize::from(count)));
            } else {
                bytes.push(byte);
            }
        }
        bytes
    }

    #[test]
    fn rle_preserves_every_byte_and_run_boundaries() {
        assert!(expand(&rle(&[])).is_empty());
        let alphabet: Vec<_> = (0..=255).collect();
        assert_eq!(expand(&rle(&alphabet)), alphabet);
        for byte in 0u8..=255 {
            for length in [1, 2, 3, 4, 254, 255, 256, 510, 511] {
                let mut bytes = vec![byte; length];
                bytes.extend([byte.wrapping_add(1), 255, 0]);
                assert_eq!(expand(&rle(&bytes)), bytes, "{byte}: {length}");
            }
        }
    }
}
