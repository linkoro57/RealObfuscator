//! Contextual record words and alias-specific operand roles.
//! Public and reversible: diversification, not encryption.

/// Logical a/b/c -> physical operand slot. Each handler consumes its own roles.
pub(super) fn operands(opcode: u32) -> [usize; 3] {
    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    ORDERS[opcode as usize % ORDERS.len()]
}

/// Bind even opcode and successor words to the physical record and recipe state.
pub(super) fn encode(words: [u32; 5], mut key: u32, step: u32) -> [u32; 5] {
    let mut encoded = [0; 5];
    for (i, word) in words.into_iter().enumerate() {
        let cipher = (word ^ key).rotate_left(key % 31 + 1);
        encoded[i] = cipher;
        key = (key.rotate_left(7) ^ cipher)
            .wrapping_add(step)
            .wrapping_add(i as u32 + 1);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contextual_words_round_trip_full_width_and_wrong_context_changes_opcodes() {
        let mut rng = crate::rng::Rng::new(42);
        for index in 1..1024 {
            let words = if index == 1 {
                [0, u32::MAX, 0x80000000, 1, 0]
            } else {
                std::array::from_fn(|_| rng.next_u64() as u32)
            };
            let seed = rng.next_u64() as u32;
            let step = rng.next_u64() as u32;
            let key = seed ^ index;
            let encoded = encode(words, key, step);
            let mut state = key;
            let decoded = std::array::from_fn(|i| {
                let plain = encoded[i].rotate_right(state % 31 + 1) ^ state;
                state = (state.rotate_left(7) ^ encoded[i])
                    .wrapping_add(step)
                    .wrapping_add(i as u32 + 1);
                plain
            });
            assert_eq!(words, decoded);
            assert_ne!(encoded, encode(words, key ^ 0x80000000, step));
        }
    }

    #[test]
    fn every_alias_role_order_is_a_bijection() {
        let mut seen = std::collections::BTreeSet::new();
        for opcode in 0..6 {
            let roles = operands(opcode);
            let logical = [17, 29, u32::MAX];
            let mut physical = [0; 3];
            for i in 0..3 {
                physical[roles[i]] = logical[i];
            }
            assert_eq!(roles.map(|slot| physical[slot]), logical);
            seen.insert(roles);
        }
        assert_eq!(seen.len(), 6);
    }
}
