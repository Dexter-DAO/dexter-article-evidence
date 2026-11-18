//! THE SPREAD ENGINE — on-chain interest accrual (the cash register).
//!
//! One pure formula, applied at every touch of `PrincipalNode.borrowed`:
//! `interest = borrowed × rate_bps × elapsed_secs / (10_000 × 31_536_000)`.
//!
//! `rate_bps` is an APR in basis points over a 365-day year, floor division,
//! u128 intermediates (max ≈ 3e32 « 2^127 — no overflow possible before the
//! checked u64 narrowing). The SAME formula ships in @dexterai/vault (BigInt),
//! so a client building settlement money legs computes byte-identical numbers.
//!
//! ── THE LEDGER SPLIT (design D2, docs/superpowers/specs/2026-07-04) ────────
//! Interest lives in `accrued_fee`, a LEAF-LOCAL ledger that NEVER enters
//! `borrowed`/`subtree_draw`. The traverse's lockstep invariant (every ancestor
//! incremented on draw is decremented by the identical amount on release) is a
//! PRINCIPAL-only contract; ancestors pre-authorized the draw, not its
//! earnings. The cascade therefore covers principal only, and settlement paths
//! decrement ancestors by the PRINCIPAL portion of a payment alone.
//!
//! ── DETERMINISM (design D6) ────────────────────────────────────────────────
//! Settlement money legs are client-built but interest is time-dependent, so
//! repay/seize/settle carry an `accrual_ts`: the timestamp the client computed
//! interest against. `require_valid_accrual_ts` bounds it (never before the
//! node's accrual clock, never in the future, never staler than
//! ACCRUAL_TS_MAX_STALENESS_SECS) and `settle_accrual` accrues to EXACTLY that
//! second — handler and client agree to the atomic unit. Seconds between
//! `accrual_ts` and landing accrue at the next touch; on a full payoff they
//! die with the zeroed principal (a payoff quote's dust tail).
//!
//! ── CLOCK RULES ────────────────────────────────────────────────────────────
//! * `shortfall > 0` stops the clock (design D4): a defaulted node's earnings
//!   claim crystallized at seizure; post-default interest could never be
//!   collected (collateral is drained, the cascade pays principal only) and
//!   would zombie-block `close_node`.
//! * A zero delta with live debt does NOT advance `last_accrual` — fractional
//!   interest keeps accumulating across touches (`refill_bucket` philosophy),
//!   so rapid small touches cannot grind the floor division to zero forever.
//! * `reprice` (rate writes) settles at the OLD rate first, then force-advances
//!   the clock: repricing is accounting-exact and never recharges old debt.

use anchor_lang::prelude::*;

use crate::constants::{
    ACCRUAL_TS_MAX_STALENESS_SECS, BPS_DENOMINATOR, RATE_BPS_CAP, SECONDS_PER_YEAR,
};
use crate::state::{PrincipalNode, VaultError};

/// Interest owed on `borrowed` at `rate_bps` APR between the two timestamps.
/// Floor division; zero when nothing is borrowed, the rate is zero, or the
/// window is empty/inverted. Errors only on the (structurally unreachable
/// with capped rates) u64 narrowing overflow.
pub fn interest_due(borrowed: u64, rate_bps: u16, from_ts: i64, to_ts: i64) -> Result<u64> {
    if borrowed == 0 || rate_bps == 0 {
        return Ok(0);
    }
    let elapsed = to_ts.saturating_sub(from_ts);
    if elapsed <= 0 {
        return Ok(0);
    }
    let numer = (borrowed as u128) * (rate_bps as u128) * (elapsed as u128);
    let denom = (BPS_DENOMINATOR as u128) * (SECONDS_PER_YEAR as u128);
    u64::try_from(numer / denom).map_err(|_| error!(VaultError::CarryMathOverflow))
}

/// Bank the interest owed up to `to_ts` into `accrued_fee` and advance the
/// accrual clock. Returns the banked delta. See the module header for the
/// clock rules (shortfall stop, zero-delta hold, idle advance).
pub fn settle_accrual(node: &mut PrincipalNode, to_ts: i64) -> Result<u64> {
    // D4: a defaulted node's clock is stopped — its earnings claim crystallized
    // (and was written off) at seizure.
    if node.shortfall > 0 {
        return Ok(0);
    }
    let delta = interest_due(node.borrowed, node.rate_bps, node.last_accrual, to_ts)?;
    if delta > 0 {
        node.accrued_fee = node
            .accrued_fee
            .checked_add(delta)
            .ok_or(error!(VaultError::CarryMathOverflow))?;
        node.last_accrual = to_ts;
    } else if (node.borrowed == 0 || node.rate_bps == 0) && to_ts > node.last_accrual {
        // Nothing can be owed for the skipped span; advance so a future draw's
        // accrual window starts at its most recent touch, not account birth.
        node.last_accrual = to_ts;
    }
    // delta == 0 with live debt and a live rate: HOLD the clock so fractional
    // interest keeps accumulating (no floor-grind).
    Ok(delta)
}

/// The node's total interest claim as of `now` WITHOUT mutating it — the
/// already-banked `accrued_fee` plus the un-banked tail since `last_accrual`.
/// Read-only callers (attest_liquidation's debt, finalize_withdrawal's pin)
/// price the claim exactly without needing a `mut` account.
pub fn pro_forma_accrued(node: &PrincipalNode, now: i64) -> Result<u64> {
    if node.shortfall > 0 {
        return Ok(node.accrued_fee);
    }
    let delta = interest_due(node.borrowed, node.rate_bps, node.last_accrual, now)?;
    node.accrued_fee
        .checked_add(delta)
        .ok_or(error!(VaultError::CarryMathOverflow))
}

/// Bound a client-supplied accrual timestamp (design D6): never before the
/// node's accrual clock, never in the future, never staler than the window.
pub fn require_valid_accrual_ts(node: &PrincipalNode, accrual_ts: i64, now: i64) -> Result<()> {
    require!(
        accrual_ts >= node.last_accrual && accrual_ts <= now,
        VaultError::AccrualTsInvalid
    );
    require!(
        now.saturating_sub(accrual_ts) <= ACCRUAL_TS_MAX_STALENESS_SECS,
        VaultError::AccrualTsStale
    );
    Ok(())
}

/// Write a new rate (design D7): settle at the OLD rate first, force-advance
/// the clock (forfeiting any sub-atomic fractional tail — the reprice anchor
/// must be exact), then set the new rate. Cap-bounded. Returns the interest
/// settled at the old rate.
pub fn reprice(node: &mut PrincipalNode, new_rate_bps: u16, now: i64) -> Result<u64> {
    require!(new_rate_bps <= RATE_BPS_CAP, VaultError::RateExceedsCap);
    let settled = settle_accrual(node, now)?;
    if now > node.last_accrual {
        node.last_accrual = now;
    }
    node.rate_bps = new_rate_bps;
    Ok(settled)
}

/// How a collected interest slice splits between the financier and the
/// protocol treasury (design D5). `take_bps` is protocol state
/// (`GraphConfig.interest_take_bps`, capped at INTEREST_TAKE_CAP_BPS). Floor —
/// rounding dust favors the financier. `treasury_cut == 0` ⇒ the settlement
/// stays a single-leg envelope.
pub fn treasury_cut(interest_paid: u64, take_bps: u16) -> u64 {
    ((interest_paid as u128) * (take_bps as u128) / (BPS_DENOMINATOR as u128)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::PRINCIPAL_NODE_VERSION_V1;

    fn node(borrowed: u64, rate_bps: u16, last_accrual: i64) -> PrincipalNode {
        PrincipalNode {
            version: PRINCIPAL_NODE_VERSION_V1,
            bump: 255,
            node_id: [7u8; 32],
            controller: Pubkey::new_unique(),
            parent: None,
            root_attestation: Some(Pubkey::new_unique()),
            cap: crate::state::RateCap {
                rate_amount: 1_000_000,
                period_secs: 60,
                bucket: 1_000_000,
                last_refill: 0,
                ceiling: None,
                burst_multiple: 1,
            },
            borrowed,
            subtree_draw: borrowed,
            borrow_recovery_at: None,
            shortfall: 0,
            frozen: false,
            child_count: 0,
            accrued_fee: 0,
            rate_bps,
            last_accrual,
            financier: Pubkey::new_unique(),
        }
    }

    // ── interest_due: the formula ────────────────────────────────────────────

    #[test]
    fn one_year_at_100pct_apr_doubles() {
        // 1,000 USDC at RATE_BPS_CAP (100% APR) for exactly one year = 1,000.
        let i = interest_due(1_000_000_000, RATE_BPS_CAP, 0, SECONDS_PER_YEAR as i64).unwrap();
        assert_eq!(i, 1_000_000_000);
    }

    #[test]
    fn known_value_60s_at_50pct() {
        // 1,000 USDC × 5_000 bps × 60s / (10_000 × 31_536_000) = 951 atomic
        // (floor of 951.29…) — the fork test's expected value derives the same way.
        let i = interest_due(1_000_000_000, 5_000, 100, 160).unwrap();
        assert_eq!(i, 951);
    }

    #[test]
    fn zero_paths() {
        assert_eq!(interest_due(0, 5_000, 0, 1000).unwrap(), 0); // no debt
        assert_eq!(interest_due(1_000_000, 0, 0, 1000).unwrap(), 0); // no rate
        assert_eq!(interest_due(1_000_000, 5_000, 1000, 1000).unwrap(), 0); // empty window
        assert_eq!(interest_due(1_000_000, 5_000, 2000, 1000).unwrap(), 0); // inverted window
    }

    #[test]
    fn u128_headroom_at_extremes() {
        // u64::MAX borrowed, max u16 rate, 100 years elapsed: numerator ≈ 3.8e33
        // fits u128; the RESULT overflows u64 and must error, not wrap.
        let r = interest_due(u64::MAX, u16::MAX, 0, 100 * SECONDS_PER_YEAR as i64);
        assert!(r.is_err());
        // The same magnitudes over one hour stay well inside u64.
        let i = interest_due(u64::MAX, u16::MAX, 0, 3_600).unwrap();
        assert!(i > 0);
    }

    // ── settle_accrual: banking + clock rules ────────────────────────────────

    #[test]
    fn settle_banks_and_advances() {
        let mut n = node(1_000_000_000, 5_000, 100);
        let d = settle_accrual(&mut n, 160).unwrap();
        assert_eq!(d, 951);
        assert_eq!(n.accrued_fee, 951);
        assert_eq!(n.last_accrual, 160);
        // Second settle over the same instant adds nothing.
        assert_eq!(settle_accrual(&mut n, 160).unwrap(), 0);
        assert_eq!(n.accrued_fee, 951);
    }

    #[test]
    fn zero_delta_with_live_debt_holds_the_clock() {
        // 100 atomic at 1 bps: ~3e-10/sec — floors to 0 for any short window.
        let mut n = node(100, 1, 1_000);
        assert_eq!(settle_accrual(&mut n, 1_010).unwrap(), 0);
        // Clock HELD: the fractional tail keeps accruing from 1_000.
        assert_eq!(n.last_accrual, 1_000);
        // After enough total elapsed the floor finally yields 1 — from the
        // ORIGINAL anchor, so nothing was ground away by the earlier touch.
        let one_unit_secs = (SECONDS_PER_YEAR as u64 * BPS_DENOMINATOR / 100) as i64; // 3.15e9 s
        assert_eq!(settle_accrual(&mut n, 1_000 + one_unit_secs).unwrap(), 1);
    }

    #[test]
    fn idle_node_advances_clock_without_banking() {
        let mut n = node(0, 5_000, 100);
        assert_eq!(settle_accrual(&mut n, 5_000).unwrap(), 0);
        assert_eq!(n.accrued_fee, 0);
        // Advanced: a future draw's window starts at the latest touch.
        assert_eq!(n.last_accrual, 5_000);
    }

    #[test]
    fn legacy_zero_rate_node_is_inert_and_self_heals() {
        // Every pre-spread-engine mainnet node: rate 0, last_accrual 0. First
        // touch decades later banks NOTHING and advances the clock.
        let mut n = node(50_000_000, 0, 0);
        assert_eq!(settle_accrual(&mut n, 1_780_000_000).unwrap(), 0);
        assert_eq!(n.accrued_fee, 0);
        assert_eq!(n.last_accrual, 1_780_000_000);
    }

    #[test]
    fn defaulted_node_clock_is_stopped() {
        let mut n = node(500, 5_000, 100);
        n.shortfall = 500;
        assert_eq!(settle_accrual(&mut n, SECONDS_PER_YEAR as i64).unwrap(), 0);
        assert_eq!(n.accrued_fee, 0);
        assert_eq!(n.last_accrual, 100); // untouched
    }

    // ── pro_forma_accrued ────────────────────────────────────────────────────

    #[test]
    fn pro_forma_matches_settle_without_mutating() {
        let mut n = node(1_000_000_000, 5_000, 100);
        n.accrued_fee = 40;
        let quoted = pro_forma_accrued(&n, 160).unwrap();
        assert_eq!(quoted, 40 + 951);
        assert_eq!(n.accrued_fee, 40); // untouched
        settle_accrual(&mut n, 160).unwrap();
        assert_eq!(n.accrued_fee, quoted);
    }

    // ── require_valid_accrual_ts ─────────────────────────────────────────────

    #[test]
    fn accrual_ts_bounds() {
        let n = node(1_000, 5_000, 1_000);
        let now = 1_100;
        assert!(require_valid_accrual_ts(&n, 1_050, now).is_ok());
        assert!(require_valid_accrual_ts(&n, now, now).is_ok()); // ts == now
        assert!(require_valid_accrual_ts(&n, 1_000, now).is_ok()); // ts == last_accrual
        assert!(require_valid_accrual_ts(&n, 999, now).is_err()); // before the clock
        assert!(require_valid_accrual_ts(&n, 1_101, now).is_err()); // future
        let far_now = 1_000 + ACCRUAL_TS_MAX_STALENESS_SECS + 10;
        assert!(require_valid_accrual_ts(&n, 1_000, far_now).is_err()); // stale
        assert!(require_valid_accrual_ts(&n, far_now - ACCRUAL_TS_MAX_STALENESS_SECS, far_now).is_ok());
    }

    // ── reprice ──────────────────────────────────────────────────────────────

    #[test]
    fn reprice_settles_old_rate_then_switches() {
        let mut n = node(1_000_000_000, 5_000, 100);
        let settled = reprice(&mut n, 1_200, 160).unwrap();
        assert_eq!(settled, 951); // 60s at the OLD 50% rate
        assert_eq!(n.accrued_fee, 951);
        assert_eq!(n.rate_bps, 1_200);
        assert_eq!(n.last_accrual, 160); // force-advanced
    }

    #[test]
    fn reprice_force_advances_even_on_zero_delta() {
        // Dusty balance whose delta floors to 0: settle would HOLD the clock,
        // but a reprice must anchor exactly at `now` (forfeit the fraction) so
        // the lagged span is never recharged at the NEW rate.
        let mut n = node(100, 1, 1_000);
        reprice(&mut n, 5_000, 2_000).unwrap();
        assert_eq!(n.accrued_fee, 0);
        assert_eq!(n.last_accrual, 2_000);
        assert_eq!(n.rate_bps, 5_000);
    }

    #[test]
    fn reprice_rejects_above_cap() {
        let mut n = node(0, 0, 0);
        assert!(reprice(&mut n, RATE_BPS_CAP + 1, 100).is_err());
        assert!(reprice(&mut n, RATE_BPS_CAP, 100).is_ok());
    }

    // ── treasury_cut ─────────────────────────────────────────────────────────

    #[test]
    fn cut_floors_toward_financier() {
        assert_eq!(treasury_cut(1_000, 2_500), 250);
        assert_eq!(treasury_cut(999, 2_500), 249); // 249.75 floors
        assert_eq!(treasury_cut(1_000, 0), 0);
        assert_eq!(treasury_cut(0, 5_000), 0);
        assert_eq!(treasury_cut(3, 2_500), 0); // dust interest → single leg
        assert_eq!(treasury_cut(u64::MAX, 5_000), u64::MAX / 2); // no overflow at the cap
    }
}
