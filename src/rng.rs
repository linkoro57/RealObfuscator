/// A deterministic SplitMix64 stream for build diversification. This is not encryption.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// Release seeds come from the operating system, never the clock or an address.
    pub fn random() -> Self {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).expect("operating system randomness unavailable");
        Self::new(u64::from_le_bytes(bytes))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Half-open range; rejection sampling avoids modulo bias.
    pub fn range(&mut self, start: u64, end: u64) -> u64 {
        let width = end.saturating_sub(start);
        if width == 0 {
            return start;
        }
        let threshold = width.wrapping_neg() % width;
        loop {
            let value = self.next_u64();
            if value >= threshold {
                return start + value % width;
            }
        }
    }

    pub fn shuffle<T>(&mut self, values: &mut [T]) {
        for i in (1..values.len()).rev() {
            let j = self.range(0, i as u64 + 1) as usize;
            values.swap(i, j);
        }
    }

    pub fn name(&mut self, min: usize, max: usize) -> String {
        const FIRST: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_";
        const REST: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789";
        let len = self.range(
            min.max(1) as u64,
            max.max(min).max(1).saturating_add(1) as u64,
        ) as usize;
        let mut name = String::with_capacity(len);
        name.push(FIRST[self.range(0, FIRST.len() as u64) as usize] as char);
        for _ in 1..len {
            name.push(REST[self.range(0, REST.len() as u64) as usize] as char);
        }
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_seed_advances_including_the_old_absorbing_seed() {
        for seed in [0, u64::MAX, 0x9E37_79B9_7F4A_7C15] {
            let mut rng = Rng::new(seed);
            assert_ne!(rng.next_u64(), rng.next_u64());
        }
    }
}
