//! The order window: each mandate (and each standing grant) admits each order
//! number at most once, without a record per number.
//!
//! The window keeps the highest admitted number and a 1,024-bit map of the
//! numbers just below it, as the anti-replay window of RFC 4303, section 3.4.3.
//! Bit `i` (byte `i / 8`, mask `1 << (i % 8)`) stands for number `highest - i`.
//! A number above the highest moves the window up; a number inside it is
//! admitted once; a number below it can never be admitted, whether or not it
//! was admitted before. One commit may move the window up by at most
//! `ORDER_JUMP_CAP`, so no single number exhausts the mandate's numbers.
use anchor_lang::prelude::*;

pub const ORDER_WINDOW_BITS: u64 = 1024;
pub const ORDER_WINDOW_BYTES: usize = 128;
pub const ORDER_JUMP_CAP: u64 = 65_536;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq, InitSpace)]
pub struct OrderWindow {
    pub highest: u64,
    pub bits: [u8; ORDER_WINDOW_BYTES],
}

impl Default for OrderWindow {
    fn default() -> Self { Self { highest: 0, bits: [0; ORDER_WINDOW_BYTES] } }
}

/// Why a number is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowRefusal {
    /// Zero is never an order number.
    Zero,
    /// More than `ORDER_JUMP_CAP` above the highest admitted number.
    TooFar,
    /// 1,024 or more below the highest admitted number: it can never be admitted.
    BelowWindow,
    /// Already admitted.
    Used,
}

/// What the window says about a number without admitting it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowState {
    /// Admitted.
    Used,
    /// Not admitted, and admissible now.
    Open,
    /// Can never be admitted.
    Below,
    /// Not admissible now: zero, or above the jump cap.
    Refused,
}

impl OrderWindow {
    fn bit(&self, index: u64) -> bool {
        self.bits[(index / 8) as usize] & (1u8 << (index % 8)) != 0
    }

    fn set(&mut self, index: u64) {
        self.bits[(index / 8) as usize] |= 1u8 << (index % 8);
    }

    /// Move every bit `shift` places up (toward older numbers). Bits shifted
    /// past the last place are dropped; the lowest `shift` places become zero.
    fn shift_up(&mut self, shift: u64) {
        if shift >= ORDER_WINDOW_BITS {
            self.bits = [0; ORDER_WINDOW_BYTES];
            return;
        }
        let bytes = (shift / 8) as usize;
        let bits = (shift % 8) as u32;
        let old = self.bits;
        for k in (0..ORDER_WINDOW_BYTES).rev() {
            let high = if k >= bytes { old[k - bytes] } else { 0 };
            let low = if bits > 0 && k > bytes { old[k - bytes - 1] } else { 0 };
            self.bits[k] = if bits == 0 { high } else { (high << bits) | (low >> (8 - bits)) };
        }
    }

    /// Where `n` stands. Never overflows: each distance is computed only in the
    /// direction where it is non-negative.
    pub fn state(&self, n: u64) -> WindowState {
        if n == 0 { return WindowState::Refused; }
        if n > self.highest {
            return if n - self.highest > ORDER_JUMP_CAP { WindowState::Refused } else { WindowState::Open };
        }
        let distance = self.highest - n;
        if distance >= ORDER_WINDOW_BITS { return WindowState::Below; }
        if self.bit(distance) { WindowState::Used } else { WindowState::Open }
    }

    /// Admit `n` once, or say why not. A refusal leaves the window unchanged.
    pub fn admit(&mut self, n: u64) -> core::result::Result<(), WindowRefusal> {
        if n == 0 { return Err(WindowRefusal::Zero); }
        if n > self.highest {
            let shift = n - self.highest;
            if shift > ORDER_JUMP_CAP { return Err(WindowRefusal::TooFar); }
            self.shift_up(shift);
            self.set(0);
            self.highest = n;
            return Ok(());
        }
        let distance = self.highest - n;
        if distance >= ORDER_WINDOW_BITS { return Err(WindowRefusal::BelowWindow); }
        if self.bit(distance) { return Err(WindowRefusal::Used); }
        self.set(distance);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference: the set of admitted numbers, checked rule by rule.
    struct Model { highest: u64, used: std::collections::BTreeSet<u64> }
    impl Model {
        fn admit(&mut self, n: u64) -> core::result::Result<(), WindowRefusal> {
            if n == 0 { return Err(WindowRefusal::Zero); }
            if n > self.highest {
                if n - self.highest > ORDER_JUMP_CAP { return Err(WindowRefusal::TooFar); }
                self.highest = n; self.used.insert(n); return Ok(());
            }
            if self.highest - n >= ORDER_WINDOW_BITS { return Err(WindowRefusal::BelowWindow); }
            if !self.used.insert(n) { return Err(WindowRefusal::Used); }
            Ok(())
        }
    }

    fn check_against_model(start: u64, numbers: &[u64]) {
        let mut window = OrderWindow { highest: start, bits: [0; ORDER_WINDOW_BYTES] };
        let mut model = Model { highest: start, used: Default::default() };
        for &n in numbers {
            let before = window;
            let got = window.admit(n);
            assert_eq!(got, model.admit(n), "number {n} from {start}");
            if got.is_err() { assert_eq!(window, before, "a refusal changed the window"); }
            assert_eq!(window.highest, model.highest);
            for distance in 0..ORDER_WINDOW_BITS.min(window.highest) {
                let m = window.highest - distance;
                assert_eq!(window.bit(distance), model.used.contains(&m), "bit for {m}");
            }
        }
    }

    #[test]
    fn edges_at_zero() {
        let mut w = OrderWindow::default();
        assert_eq!(w.admit(0), Err(WindowRefusal::Zero));
        assert_eq!(w.admit(ORDER_JUMP_CAP + 1), Err(WindowRefusal::TooFar));
        assert_eq!(w.admit(ORDER_JUMP_CAP), Ok(()));
        assert_eq!(w.admit(ORDER_JUMP_CAP), Err(WindowRefusal::Used));
        assert_eq!(w.state(ORDER_JUMP_CAP - 1023), WindowState::Open);
        assert_eq!(w.state(ORDER_JUMP_CAP - 1024), WindowState::Below);
        assert_eq!(w.admit(ORDER_JUMP_CAP - 1023), Ok(()));
        assert_eq!(w.admit(ORDER_JUMP_CAP - 1024), Err(WindowRefusal::BelowWindow));
        assert_eq!(w.state(ORDER_JUMP_CAP - 1023), WindowState::Used);
    }

    #[test]
    fn edges_at_u64_max() {
        // Within the jump cap of u64::MAX, `highest + cap` would overflow; the
        // window never computes it.
        let start = u64::MAX - 10;
        let mut w = OrderWindow { highest: start, bits: [0; ORDER_WINDOW_BYTES] };
        assert_eq!(w.state(u64::MAX), WindowState::Open);
        assert_eq!(w.admit(u64::MAX), Ok(()));
        assert_eq!(w.admit(u64::MAX), Err(WindowRefusal::Used));
        assert_eq!(w.admit(start), Ok(()));
        assert_eq!(w.admit(u64::MAX - 1023), Ok(()));
        assert_eq!(w.admit(u64::MAX - 1024), Err(WindowRefusal::BelowWindow));
        let near = u64::MAX - ORDER_JUMP_CAP;
        let mut w = OrderWindow { highest: near, bits: [0; ORDER_WINDOW_BYTES] };
        assert_eq!(w.admit(u64::MAX), Ok(()));
        let mut w = OrderWindow { highest: near - 1, bits: [0; ORDER_WINDOW_BYTES] };
        assert_eq!(w.admit(u64::MAX), Err(WindowRefusal::TooFar));
        check_against_model(u64::MAX - 5000, &[u64::MAX - 4000, u64::MAX, u64::MAX - 1, u64::MAX - 1023,
            u64::MAX - 1024, u64::MAX - 3999, u64::MAX]);
    }

    #[test]
    fn shifts_of_every_size_keep_the_right_bits() {
        for shift in [1u64, 2, 7, 8, 9, 15, 16, 17, 63, 64, 65, 511, 512, 1000, 1023, 1024, 1025, 4096, ORDER_JUMP_CAP] {
            let mut numbers = vec![5000, 4999, 4990, 4100, 3977];
            numbers.push(5000 + shift);
            numbers.extend([4999, 4990, 5000 + shift - 1, 5000 + shift - 1023, 5000 + shift - 1024, 5000]);
            check_against_model(4000, &numbers);
        }
    }

    #[test]
    fn out_of_order_landing_within_the_window() {
        let mut numbers: Vec<u64> = (1..=1500).rev().collect();
        numbers.extend(1..=1500);
        check_against_model(0, &numbers);
        // A pseudo-random walk with jumps, repeats and stale numbers.
        let mut x: u64 = 0x9e3779b97f4a7c15;
        let mut walk = Vec::new();
        let mut top = 1u64;
        for _ in 0..4000 {
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            let n = match x % 5 {
                0 => top + 1 + (x >> 8) % 3000,
                1 => top.saturating_sub((x >> 8) % 1100).max(1),
                2 => top.saturating_sub((x >> 8) % 40).max(1),
                3 => top + 1,
                _ => top.saturating_sub(1023 + (x >> 8) % 3).max(1),
            };
            top = top.max(n);
            walk.push(n);
        }
        check_against_model(0, &walk);
    }
}
