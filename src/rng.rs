use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self { Self(seed ^ 0x9E37_79B9_7F4A_7C15) }
    pub fn random() -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let address = &nanos as *const _ as usize as u128;
        Self::new((nanos ^ address.rotate_left(37)) as u64)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0 = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        self.0
    }
    pub fn range(&mut self, start: u64, end: u64) -> u64 {
        start + self.next_u64() % (end.saturating_sub(start).max(1))
    }
    pub fn name(&mut self, min: usize, max: usize) -> String {
        const FIRST: &[u8] = b"lIO";
        const REST: &[u8] = b"lIO01";
        let len = self.range(min as u64, max.saturating_add(1) as u64) as usize;
        let mut value = String::with_capacity(len);
        value.push(FIRST[self.range(0, FIRST.len() as u64) as usize] as char);
        for _ in 1..len { value.push(REST[self.range(0, REST.len() as u64) as usize] as char); }
        value
    }
}

