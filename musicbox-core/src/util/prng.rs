/// Xorshift64 PRNG for cheap per-sample noise.
/// Deterministic given a seed, no allocation.
pub struct Xorshift64 {
    state: u64,
}

impl Xorshift64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    #[inline]
    pub fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// White noise sample in [-1.0, 1.0].
    #[inline]
    pub fn white(&mut self) -> f32 {
        (self.next() as f32) / (u64::MAX as f32) * 2.0 - 1.0
    }
}
