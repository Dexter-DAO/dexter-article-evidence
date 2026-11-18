//! Authorization of immutable bill records by a continuing principal.
//!
//! The existing report/claim digest continues to identify the accepted facts.
//! This separate wrapper identifies who authorized a *fresh admission*. Core
//! must authenticate the principal, its current owner and authority generation,
//! and the native signature before retaining this attribution. An admitted
//! record remains valid after later authority changes; changing its wrapper
//! cannot replace its immutable body. These helpers define bytes, not recovery
//! eligibility, current ownership, source independence or truthful work.
use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;

use crate::CORE_ID;

/// The explicit wire tag is used below, rather than the Borsh enum index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BillRecordRole {
    SourceReport = 1,
    SellerClaim = 2,
}

/// SHA-256 of the fixed label, domain, Core program, role byte, bill,
/// continuing principal, actual signing key, little-endian generation, and
/// canonical record digest, in that exact order. All public keys and hashes are
/// 32 raw bytes. No fee payer or transaction identity enters the message.
pub fn principal_bill_record_digest(
    role: BillRecordRole,
    domain: &[u8; 32],
    bill: &Pubkey,
    principal: &Pubkey,
    signer: &Pubkey,
    authority_generation: u64,
    body_digest: &[u8; 32],
) -> [u8; 32] {
    hashv(&[
        b"DEXTER:PRINCIPAL-BILL-RECORD:1",
        domain,
        CORE_ID.as_ref(),
        &[role as u8],
        bill.as_ref(),
        principal.as_ref(),
        signer.as_ref(),
        &authority_generation.to_le_bytes(),
        body_digest,
    ]).to_bytes()
}

pub fn source_report_authority_digest(
    domain: &[u8; 32], bill: &Pubkey, principal: &Pubkey, signer: &Pubkey,
    authority_generation: u64, body_digest: &[u8; 32],
) -> [u8; 32] {
    principal_bill_record_digest(BillRecordRole::SourceReport, domain, bill,
        principal, signer, authority_generation, body_digest)
}

pub fn seller_claim_authority_digest(
    domain: &[u8; 32], bill: &Pubkey, principal: &Pubkey, signer: &Pubkey,
    authority_generation: u64, body_digest: &[u8; 32],
) -> [u8; 32] {
    principal_bill_record_digest(BillRecordRole::SellerClaim, domain, bill,
        principal, signer, authority_generation, body_digest)
}
