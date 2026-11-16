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
            Op::Jump => words[1] = positions[instruction.a as usize],
            Op::JumpFalse | Op::JumpTrue => words[2] = positions[instruction.b as usize],
            Op::GenericFor => {
                words[1] = positions[instruction.a as usize];
                words[3] = positions[instruction.c as usize];
            }
            _ => {}
        }
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
        let serialized =
            crate::codec::encode(&serialized, seed ^ (physical as u32 + 1), step, multiplier);
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
    let constants = array(program.constants.iter().map(|value| {
        FunctionExpression::from_block(Block::from(ReturnStatement::one(value.clone()))).into()
    }));
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
    ]
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
    use super::rle;

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
