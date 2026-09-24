//! NumPy's default random generator and summation, reproduced bit for bit.
//!
//! The Python voxelizer seeded its sampling with `np.random.default_rng(seed)`:
//! PCG64 (XSL-RR 128/64) seeded through NumPy's `SeedSequence`, with
//! `Generator.random()` turning each 64-bit draw into a double as
//! `(x >> 11) * 2⁻⁵³`. Drawing the same stream in the same order lets the Rust
//! voxelizer land on the same voxels, which is what lets the parity harness
//! compare the two implementations cell by cell — and keeps conversions made
//! before the port reproducible after it.

/// PCG's 128-bit LCG multiplier.
const MULTIPLIER: u128 = 0x2360_ed05_1fc6_5da4_4385_df64_9fcc_f645;

// SeedSequence's hashing constants (numpy/random/bit_generator.pyx).
const INIT_A: u32 = 0x43b0_d7e5;
const MULT_A: u32 = 0x931e_8875;
const INIT_B: u32 = 0x8b51_f9dd;
const MULT_B: u32 = 0x58f3_8ded;
const MIX_MULT_L: u32 = 0xca01_f9dd;
const MIX_MULT_R: u32 = 0x4973_f715;
const XSHIFT: u32 = 16;
const POOL_SIZE: usize = 4;

fn hashmix(value: u32, hash_const: &mut u32) -> u32 {
    let mut value = value ^ *hash_const;
    *hash_const = hash_const.wrapping_mul(MULT_A);
    value = value.wrapping_mul(*hash_const);
    value ^ (value >> XSHIFT)
}

fn mix(x: u32, y: u32) -> u32 {
    let result = MIX_MULT_L
        .wrapping_mul(x)
        .wrapping_sub(MIX_MULT_R.wrapping_mul(y));
    result ^ (result >> XSHIFT)
}

/// `SeedSequence(seed).generate_state(4, np.uint64)`.
fn seed_state(seed: u64) -> [u64; 4] {
    // The seed as little-endian 32-bit words, with no leading zero words
    // (but at least one word, so 0 is [0]).
    let entropy: Vec<u32> = if seed >> 32 == 0 {
        vec![seed as u32]
    } else {
        vec![seed as u32, (seed >> 32) as u32]
    };

    let mut pool = [0u32; POOL_SIZE];
    let mut hash_const = INIT_A;
    for (i, slot) in pool.iter_mut().enumerate() {
        *slot = hashmix(entropy.get(i).copied().unwrap_or(0), &mut hash_const);
    }
    for src in 0..POOL_SIZE {
        for dst in 0..POOL_SIZE {
            if src != dst {
                let hashed = hashmix(pool[src], &mut hash_const);
                pool[dst] = mix(pool[dst], hashed);
            }
        }
    }
    // A u64 seed never has more words than the pool, so nothing is left over.

    let mut hash_const = INIT_B;
    let mut words = [0u32; 8];
    for (i, word) in words.iter_mut().enumerate() {
        let mut value = pool[i % POOL_SIZE] ^ hash_const;
        hash_const = hash_const.wrapping_mul(MULT_B);
        value = value.wrapping_mul(hash_const);
        *word = value ^ (value >> XSHIFT);
    }
    std::array::from_fn(|i| u64::from(words[2 * i]) | u64::from(words[2 * i + 1]) << 32)
}

/// `np.random.default_rng(seed)`: the draws [`NumpyRng::random`] returns are
/// the ones `Generator.random()` returns, in the same order.
#[derive(Debug, Clone)]
pub struct NumpyRng {
    state: u128,
    increment: u128,
}

impl NumpyRng {
    pub fn new(seed: u64) -> Self {
        let words = seed_state(seed);
        let init_state = u128::from(words[0]) << 64 | u128::from(words[1]);
        let init_seq = u128::from(words[2]) << 64 | u128::from(words[3]);
        let mut rng = NumpyRng {
            state: 0,
            increment: init_seq << 1 | 1,
        };
        rng.step();
        rng.state = rng.state.wrapping_add(init_state);
        rng.step();
        rng
    }

    fn step(&mut self) {
        self.state = self
            .state
            .wrapping_mul(MULTIPLIER)
            .wrapping_add(self.increment);
    }

    pub fn next_u64(&mut self) -> u64 {
        self.step();
        let rotation = (self.state >> 122) as u32;
        (((self.state >> 64) as u64) ^ (self.state as u64)).rotate_right(rotation)
    }

    /// A double in `[0, 1)`, as `Generator.random()` makes it.
    pub fn random(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Skip `draws` values in O(log draws), so that parallel workers can each
    /// start at their own offset into one stream.
    pub fn advance(&mut self, draws: u128) {
        let (mut acc_mult, mut acc_plus) = (1u128, 0u128);
        let (mut cur_mult, mut cur_plus) = (MULTIPLIER, self.increment);
        let mut delta = draws;
        while delta > 0 {
            if delta & 1 == 1 {
                acc_mult = acc_mult.wrapping_mul(cur_mult);
                acc_plus = acc_plus.wrapping_mul(cur_mult).wrapping_add(cur_plus);
            }
            cur_plus = cur_mult.wrapping_add(1).wrapping_mul(cur_plus);
            cur_mult = cur_mult.wrapping_mul(cur_mult);
            delta >>= 1;
        }
        self.state = acc_mult.wrapping_mul(self.state).wrapping_add(acc_plus);
    }

    /// A copy positioned `draws` values further along.
    pub fn skipped(&self, draws: u128) -> Self {
        let mut rng = self.clone();
        rng.advance(draws);
        rng
    }
}

/// `np.sum` of a contiguous float64 array: NumPy's pairwise summation, which
/// rounds differently from a running total.
pub fn pairwise_sum(values: &[f64]) -> f64 {
    const BLOCK: usize = 128;
    let n = values.len();
    if n < 8 {
        let mut sum = 0.0;
        for &v in values {
            sum += v;
        }
        sum
    } else if n <= BLOCK {
        let mut r: [f64; 8] = values[..8].try_into().expect("eight values");
        let whole = n - n % 8;
        let mut i = 8;
        while i < whole {
            for (j, acc) in r.iter_mut().enumerate() {
                *acc += values[i + j];
            }
            i += 8;
        }
        let mut sum = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        for &v in &values[whole..] {
            sum += v;
        }
        sum
    } else {
        let mut half = n / 2;
        half -= half % 8;
        pairwise_sum(&values[..half]) + pairwise_sum(&values[half..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values printed by NumPy 2.4:
    //   struct.unpack('<Q', struct.pack('<d', np.random.default_rng(s).random()))

    #[test]
    fn seeding_matches_numpy() {
        let rng = NumpyRng::new(0x5CE2);
        assert_eq!(rng.state, 0xe528_dbe1_3dcf_b74e_88ee_3210_b4d8_3fbb);
        assert_eq!(rng.increment, 0xd4a4_06e5_543d_cca9_1fbb_b562_efc7_ac0b);
    }

    #[test]
    fn draws_match_numpy() {
        let mut rng = NumpyRng::new(0x5CE2);
        let expected: [u64; 5] = [
            0x3fd4_8ea6_7a94_fafa,
            0x3fc5_a136_c8ca_abb0,
            0x3fef_30d6_a979_436a,
            0x3fe1_faa3_9c89_a860,
            0x3fc2_d93c_9674_a000,
        ];
        for bits in expected {
            assert_eq!(rng.random().to_bits(), bits);
        }
        let mut rng = NumpyRng::new(0x5CE2);
        assert_eq!(rng.next_u64(), 0x523a_99ea_53eb_eb10);
    }

    #[test]
    fn other_seeds_match_numpy() {
        for (seed, bits) in [
            (0u64, 0x3fe4_61fd_79fb_3850u64),
            (1, 0x3fe0_60d7_be6f_245c),
            (0xffff_ffff, 0x3fd0_1fd7_ea4c_363e),
            (0x1_0000_0000, 0x3fec_78bd_7c50_b582),
            (u64::MAX, 0x3fe5_c2c7_4f51_88ea),
        ] {
            assert_eq!(
                NumpyRng::new(seed).random().to_bits(),
                bits,
                "seed {seed:#x}"
            );
        }
    }

    #[test]
    fn advance_skips_exactly() {
        let mut stepped = NumpyRng::new(0x5CE2);
        for _ in 0..1000 {
            stepped.random();
        }
        let mut jumped = NumpyRng::new(0x5CE2);
        jumped.advance(1000);
        assert_eq!(jumped.random().to_bits(), 0x3fef_ee14_76a8_0be9);
        assert_eq!(stepped.random().to_bits(), 0x3fef_ee14_76a8_0be9);
        assert_eq!(
            NumpyRng::new(7).skipped(0).random(),
            NumpyRng::new(7).random()
        );
    }

    #[test]
    fn pairwise_sum_rounds_like_numpy() {
        // np.sum(np.full(n, 0.1)), where a running total gives 0x3fefffffffffffff
        // and 0x4058ffffffffff9d.
        assert_eq!(pairwise_sum(&[]), 0.0);
        assert_eq!(pairwise_sum(&[0.1; 10]).to_bits(), 0x3ff0_0000_0000_0000);
        assert_eq!(pairwise_sum(&[0.1; 1000]).to_bits(), 0x4059_0000_0000_0001);
    }
}
