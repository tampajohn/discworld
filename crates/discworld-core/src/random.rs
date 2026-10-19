// SPDX-License-Identifier: GPL-3.0-or-later
//! Pinned ScummVM common/random.cpp generator and V1 tinlib.cpp RandomFn.
//! The host chooses a seed; the core never consults a platform clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Random {
    seed: u32,
    last: u32,
}
impl Default for Random {
    fn default() -> Self {
        Self::new(1)
    }
}
impl Random {
    pub fn new(seed: u32) -> Self {
        Self {
            seed: seed.max(1),
            last: 0,
        }
    }
    fn number(&mut self, max: u32) -> u32 {
        self.seed ^= self.seed >> 13;
        self.seed ^= self.seed << 21;
        self.seed ^= self.seed >> 11;
        let value = self.seed.wrapping_mul(0xdeadbf03);
        if max == u32::MAX {
            value
        } else {
            value % (max + 1)
        }
    }
    /// Inclusive bounds, swapped if reversed. Mode 1 retries equal values at
    /// most ten times (eleven draws), even when the range has only one value.
    pub fn range(&mut self, a: i32, b: i32, mode: i32) -> i32 {
        let (min, max) = (a.min(b), a.max(b));
        let span = (i64::from(max) - i64::from(min)) as u32;
        let mut value = 0;
        for _ in 0..11 {
            value = (min as u32).wrapping_add(self.number(span));
            if mode != 1 || value != self.last {
                break;
            }
        }
        self.last = value;
        value as i32
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_sequence_and_reversed_bounds() {
        let mut a = Random::new(1);
        let mut b = Random::new(0);
        for expected in [5, 5, 5, 5, 4, 5, 3, 3] {
            assert_eq!(a.range(2, 5, 0), expected);
            assert_eq!(b.range(5, 2, 0), expected);
        }
        assert_eq!(a, b);
    }
    #[test]
    fn no_repeat_is_bounded_and_snapshot_preserves_draws() {
        let mut a = Random::default();
        assert_eq!(a.range(2, 5, 1), 5);
        assert_eq!(a.range(2, 5, 1), 4);
        let mut b = a.clone();
        assert_eq!(a.range(4, 4, 1), 4);
        for _ in 0..11 {
            b.range(4, 4, 0);
        }
        assert_eq!(a, b);
        assert_eq!(
            a.range(i32::MIN, i32::MAX, 0),
            b.range(i32::MIN, i32::MAX, 0)
        );
        assert_eq!(a.range(-7, -7, 0), -7);
    }
}
