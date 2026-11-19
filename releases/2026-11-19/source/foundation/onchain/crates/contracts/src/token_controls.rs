//! Explicit owner-accepted issuer controls for transparent stock-token custody.
//!
//! A control digest is an asset-risk commitment, not an issuer guarantee. Raw
//! token atoms remain the only units used by financial books and allowances.
use anchor_lang::prelude::*;
use crate::funded::financial_digest;

pub const STOCK_CONTROL_VERSION: u16 = 1;
pub const TRANSPARENT_ISSUER_STOCK: u8 = 1;

/// Canonical Borsh encoding. Optional authorities retain their presence bits;
/// neither a missing authority nor a different authority is silently accepted.
/// Extension order is normalized into a bitmap; duplicate TLVs are rejected by
/// the program parser. Supply and opaque display metadata are intentionally out
/// of the commitment. Both current and scheduled scale values are committed as
/// their IEEE-754 bits, without using floating-point arithmetic for money.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct StockControlTerms {
    pub version: u16,
    pub profile: u8,
    pub mint: Pubkey,
    pub token_program: Pubkey,
    pub decimals: u8,
    pub extension_bitmap: u64,
    pub mint_authority: Option<Pubkey>,
    pub freeze_authority: Option<Pubkey>,
    pub permanent_delegate: Option<Pubkey>,
    pub pause_authority: Option<Pubkey>,
    pub paused: bool,
    pub default_account_state: u8,
    pub hook_authority: Option<Pubkey>,
    pub hook_program: Option<Pubkey>,
    pub confidential_authority: Option<Pubkey>,
    pub confidential_auto_approve: bool,
    pub confidential_auditor: [u8; 32],
    pub scale_authority: Option<Pubkey>,
    pub multiplier_bits: u64,
    pub new_multiplier_effective_timestamp: i64,
    pub new_multiplier_bits: u64,
    pub metadata_pointer_authority: Option<Pubkey>,
    pub metadata_address: Option<Pubkey>,
}

pub fn stock_control_digest(terms: &StockControlTerms) -> Result<[u8; 32]> {
    financial_digest(b"DEXTER:STOCK-TOKEN-CONTROLS:1", terms)
}

/// Positive finite f64 values, checked without floating-point computation.
pub fn valid_multiplier_bits(bits: u64) -> bool {
    bits >> 63 == 0 && bits != 0 && ((bits >> 52) & 0x7ff) != 0x7ff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_scaled_values_never_enter_the_profile() {
        for value in [0, 1_u64 << 63, f64::NAN.to_bits(), f64::INFINITY.to_bits(),
            f64::NEG_INFINITY.to_bits(), (-1_f64).to_bits()] {
            assert!(!valid_multiplier_bits(value));
        }
        assert!(valid_multiplier_bits(1_f64.to_bits()));
        assert!(valid_multiplier_bits(1.0032690125398187_f64.to_bits()));
        assert!(valid_multiplier_bits(1));
    }
}
