//! Portable 32-bit feedback encoding shared with the generated Luau decoders.
//! This is build diversification, not a cryptographic confidentiality primitive.
pub(crate) fn encode(bytes: &[u8], seed: u32, step: u32, salt: u32) -> Vec<u8> {
    let mut state = seed ^ salt;
    bytes
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let cipher = byte ^ state as u8;
            state = state
                .wrapping_add(step)
                .wrapping_add(u32::from(cipher))
                .wrapping_add(index as u32 + 1);
            cipher
        })
        .collect()
}
