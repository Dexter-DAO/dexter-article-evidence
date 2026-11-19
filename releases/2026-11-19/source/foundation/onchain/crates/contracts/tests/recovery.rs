//! Independent wire vectors for the continuing-principal authorization layer.
//! Native Core tests remain responsible for authenticating the owner/epoch,
//! signature, immutable admission and post-recovery receipt behavior.
use anchor_lang::prelude::*;
use dexter_contracts::recovery::*;

fn key(n: u8) -> Pubkey { Pubkey::new_from_array([n; 32]) }
fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn digest(role: BillRecordRole) -> [u8; 32] {
    principal_bill_record_digest(role, &[1; 32], &key(2), &key(3), &key(4), 7, &[5; 32])
}

#[test]
fn authorization_matches_independent_sha256_wire_vectors() {
    // Python hashlib over independently concatenated 231 bytes, decoding the
    // declared Core address to raw bytes rather than importing these helpers.
    assert_eq!(hex(digest(BillRecordRole::SourceReport)),
        "3aedd57a8b32f6fab1c857c872da42e22505aedd86bad40783ed814d7bf11924");
    assert_eq!(hex(digest(BillRecordRole::SellerClaim)),
        "192a5db5aafd43cba85683e15e25ea94c49dce595186fd22011146fd9168ffcb");
    assert_eq!(source_report_authority_digest(&[1; 32], &key(2), &key(3), &key(4), 7, &[5; 32]),
        digest(BillRecordRole::SourceReport));
    assert_eq!(seller_claim_authority_digest(&[1; 32], &key(2), &key(3), &key(4), 7, &[5; 32]),
        digest(BillRecordRole::SellerClaim));
}

#[test]
fn roles_and_every_authority_context_field_are_separated() {
    let baseline = digest(BillRecordRole::SourceReport);
    let variants = [
        digest(BillRecordRole::SellerClaim),
        source_report_authority_digest(&[9; 32], &key(2), &key(3), &key(4), 7, &[5; 32]),
        source_report_authority_digest(&[1; 32], &key(9), &key(3), &key(4), 7, &[5; 32]),
        source_report_authority_digest(&[1; 32], &key(2), &key(9), &key(4), 7, &[5; 32]),
        source_report_authority_digest(&[1; 32], &key(2), &key(3), &key(9), 7, &[5; 32]),
        source_report_authority_digest(&[1; 32], &key(2), &key(3), &key(4), 8, &[5; 32]),
        source_report_authority_digest(&[1; 32], &key(2), &key(3), &key(4), 7, &[9; 32]),
    ];
    for changed in variants { assert_ne!(baseline, changed); }
}

#[test]
fn returning_to_the_same_key_does_not_revive_its_old_generation_proof() {
    let original = seller_claim_authority_digest(&[1; 32], &key(2), &key(3), &key(4), 7, &[5; 32]);
    let successor = seller_claim_authority_digest(&[1; 32], &key(2), &key(3), &key(6), 8, &[5; 32]);
    let returned = seller_claim_authority_digest(&[1; 32], &key(2), &key(3), &key(4), 9, &[5; 32]);
    assert_ne!(original, successor);
    assert_ne!(original, returned);
    assert_ne!(successor, returned);
}
