/// A small deterministic generator, so scenes replay identically in tests.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in `0.0..1.0`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// A stable pseudo-random value in `0.0..1.0` for a pair of integers.
pub fn hash01(a: i64, b: i64) -> f32 {
    let mut r = Rng::new(
        (a as u64)
            .wrapping_mul(0x1000_0001)
            .wrapping_add(b as u64)
            .wrapping_add(0x5555),
    );
    r.next_u64();
    r.next_f32()
}
