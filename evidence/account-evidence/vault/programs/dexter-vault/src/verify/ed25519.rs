//! Ed25519 sibling-instruction introspection for session-key voucher proofs.
//!
//! When a session key signs a voucher off-chain (via tweetnacl), the buyer's
//! SDK ships a tx that includes Solana's built-in Ed25519 sigverify precompile
//! as the FIRST instruction. The precompile itself verifies the signature
//! cryptographically; this helper proves that the precompile verified the
//! SPECIFIC (pubkey, message) pair we care about — i.e. that the buyer's
//! registered session key actually signed THIS voucher, not a different one.
//!
//! Canonical layout emitted by `@dexterai/vault`:
//!   [0]      num_signatures (u8)
//!   [1]      padding (u8)
//!   [2..16]  signature_offsets {                       // 14 bytes per sig
//!              signature_offset:           u16,
//!              signature_instruction_index: u16,
//!              public_key_offset:          u16,
//!              public_key_instruction_index: u16,
//!              message_data_offset:        u16,
//!              message_data_size:          u16,
//!              message_instruction_index:  u16,
//!            }
//!   [16..48]   registered session pubkey
//!   [48..112]  signature
//!   [112..]    exact voucher message:
//!               - 44-byte grandfathered V1 payload, or
//!               - 32-byte context-bound V2 preimage digest
//!
//! The instruction carries no account metas.
//! Every instruction-index marker is `u16::MAX`, Solana's sentinel for data
//! stored locally in this same precompile instruction.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar::instructions::{
    load_current_index_checked, load_instruction_at_checked,
};
use solana_curve25519::edwards::{add_edwards, PodEdwardsPoint};

use crate::state::VaultError;
use crate::verify::voucher::{VOUCHER_V1_MESSAGE_LEN, VOUCHER_V2_MESSAGE_LEN};

/// Solana Ed25519 sigverify precompile program ID.
pub const ED25519_VERIFY_PROGRAM_ID: Pubkey =
    anchor_lang::pubkey!("Ed25519SigVerify111111111111111111111111111");

const ED25519_PUBKEY_LEN: usize = 32;
const ED25519_SIG_LEN: usize = 64;
const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 14;
const DATA_START: usize = 2;
#[cfg(test)]
const VOUCHER_MESSAGE_LEN: usize = VOUCHER_V1_MESSAGE_LEN;
const CANONICAL_PUBKEY_OFFSET: usize = DATA_START + SIGNATURE_OFFSETS_SERIALIZED_SIZE;
const CANONICAL_SIGNATURE_OFFSET: usize = CANONICAL_PUBKEY_OFFSET + ED25519_PUBKEY_LEN;
const CANONICAL_MESSAGE_OFFSET: usize = CANONICAL_SIGNATURE_OFFSET + ED25519_SIG_LEN;
#[cfg(test)]
const CANONICAL_DATA_LEN: usize = CANONICAL_MESSAGE_OFFSET + VOUCHER_MESSAGE_LEN;
const LOCAL_INSTRUCTION_INDEX: u16 = u16::MAX;
const EDWARDS_IDENTITY: PodEdwardsPoint = PodEdwardsPoint([
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
]);

/// Reject byte strings that Solana's strict Ed25519 verifier cannot use as a
/// session public key.
///
/// Three Edwards doublings compute `[8]P`, exactly the predicate Dalek uses
/// for `is_small_order()`. Each operation also decompresses its input, so an
/// invalid encoding returns `None` and fails closed. On SBF these are native
/// curve syscalls: no software curve implementation is linked into the Vault.
pub fn require_nonweak_ed25519_public_key(public_key: &[u8; 32]) -> Result<()> {
    let point = PodEdwardsPoint(*public_key);
    let doubled = add_edwards(&point, &point)
        .ok_or_else(|| error!(VaultError::InvalidOrWeakEd25519PublicKey))?;
    let quadrupled = add_edwards(&doubled, &doubled)
        .ok_or_else(|| error!(VaultError::InvalidOrWeakEd25519PublicKey))?;
    let multiplied_by_cofactor = add_edwards(&quadrupled, &quadrupled)
        .ok_or_else(|| error!(VaultError::InvalidOrWeakEd25519PublicKey))?;

    require!(
        multiplied_by_cofactor != EDWARDS_IDENTITY,
        VaultError::InvalidOrWeakEd25519PublicKey
    );
    Ok(())
}

/// Confirms the preceding instruction was an Ed25519 sigverify call with
/// EXACTLY the given pubkey and message. The precompile already validated the
/// signature; this introspection just proves the (pubkey, message) match what
/// the vault expects.
///
/// Returns Ok(()) iff every offsets/value check passes.
pub fn verify_session_signed(
    instructions_sysvar: &AccountInfo,
    expected_pubkey: &[u8; ED25519_PUBKEY_LEN],
    expected_message: &[u8],
) -> Result<()> {
    require!(
        expected_message.len() == VOUCHER_V1_MESSAGE_LEN
            || expected_message.len() == VOUCHER_V2_MESSAGE_LEN,
        VaultError::InvalidVoucherSignature
    );
    verify_preceding_canonical_ed25519(instructions_sysvar, expected_pubkey, expected_message)
}

/// Generic exact-one, inline-data Ed25519 precompile verifier. The caller
/// supplies the domain-specific key and message and maps this legacy parser's
/// error at its boundary. Session vouchers retain their historical 32/44-byte
/// restriction in `verify_session_signed`; quote attestations use a 32-byte
/// SHA-256 digest of their canonical preimage.
pub fn verify_preceding_canonical_ed25519(
    instructions_sysvar: &AccountInfo,
    expected_pubkey: &[u8; ED25519_PUBKEY_LEN],
    expected_message: &[u8],
) -> Result<()> {
    let current_index = load_current_index_checked(instructions_sysvar)?;
    require!(current_index > 0, VaultError::InvalidVoucherSignature);

    let prev_ix = load_instruction_at_checked((current_index as usize) - 1, instructions_sysvar)?;

    require!(
        prev_ix.program_id == ED25519_VERIFY_PROGRAM_ID,
        VaultError::InvalidVoucherSignature
    );
    require!(
        prev_ix.accounts.is_empty(),
        VaultError::InvalidVoucherSignature
    );

    verify_canonical_ed25519_data(&prev_ix.data, expected_pubkey, expected_message)
}

fn verify_canonical_ed25519_data(
    data: &[u8],
    expected_pubkey: &[u8; ED25519_PUBKEY_LEN],
    expected_message: &[u8],
) -> Result<()> {
    let expected_message_len = expected_message.len();
    require!(
        expected_message_len > 0 && expected_message_len <= u16::MAX as usize,
        VaultError::InvalidVoucherSignature
    );
    let expected_data_len = CANONICAL_MESSAGE_OFFSET
        .checked_add(expected_message_len)
        .ok_or_else(|| error!(VaultError::InvalidVoucherSignature))?;
    require!(
        data.len() == expected_data_len,
        VaultError::InvalidVoucherSignature
    );
    require!(
        data[0] == 1 && data[1] == 0,
        VaultError::InvalidVoucherSignature
    );

    let off = &data[DATA_START..DATA_START + SIGNATURE_OFFSETS_SERIALIZED_SIZE];
    let signature_offset = u16::from_le_bytes([off[0], off[1]]) as usize;
    let signature_instruction_index = u16::from_le_bytes([off[2], off[3]]);
    let public_key_offset = u16::from_le_bytes([off[4], off[5]]) as usize;
    let public_key_instruction_index = u16::from_le_bytes([off[6], off[7]]);
    let message_data_offset = u16::from_le_bytes([off[8], off[9]]) as usize;
    let message_data_size = u16::from_le_bytes([off[10], off[11]]) as usize;
    let message_instruction_index = u16::from_le_bytes([off[12], off[13]]);

    require!(
        signature_offset == CANONICAL_SIGNATURE_OFFSET
            && public_key_offset == CANONICAL_PUBKEY_OFFSET
            && message_data_offset == CANONICAL_MESSAGE_OFFSET
            && message_data_size == expected_message_len,
        VaultError::InvalidVoucherSignature
    );
    require!(
        signature_instruction_index == LOCAL_INSTRUCTION_INDEX
            && public_key_instruction_index == LOCAL_INSTRUCTION_INDEX
            && message_instruction_index == LOCAL_INSTRUCTION_INDEX,
        VaultError::InvalidVoucherSignature
    );

    let actual_pubkey =
        &data[CANONICAL_PUBKEY_OFFSET..CANONICAL_PUBKEY_OFFSET + ED25519_PUBKEY_LEN];
    let actual_message = &data[CANONICAL_MESSAGE_OFFSET..expected_data_len];

    require!(
        actual_pubkey == expected_pubkey,
        VaultError::InvalidVoucherSignature
    );
    require!(
        actual_message == expected_message,
        VaultError::InvalidVoucherSignature
    );

    Ok(())
}

#[cfg(test)]
mod session_signature_parser_tests {
    use super::*;
    use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
    use anchor_lang::solana_program::sysvar::instructions::{
        construct_instructions_data, store_current_index_checked, BorrowedAccountMeta,
        BorrowedInstruction,
    };

    const FIXTURE_PUBKEY: [u8; ED25519_PUBKEY_LEN] = [0x31; ED25519_PUBKEY_LEN];
    const FIXTURE_SIGNATURE: [u8; ED25519_SIG_LEN] = [0x5a; ED25519_SIG_LEN];
    // Pinned independently from the parser constants: exact bytes emitted by
    // @dexterai/vault's buildEd25519VerifyInstruction for a 44-byte voucher.
    const SDK_CANONICAL_HEADER: [u8; 16] = [
        1, 0, 48, 0, 0xff, 0xff, 16, 0, 0xff, 0xff, 112, 0, 44, 0, 0xff, 0xff,
    ];

    fn fixture_voucher_message() -> [u8; VOUCHER_MESSAGE_LEN] {
        let mut message = [0u8; VOUCHER_MESSAGE_LEN];
        for (index, byte) in message[..32].iter_mut().enumerate() {
            *byte = index as u8;
        }
        message[32..40].copy_from_slice(&42u64.to_le_bytes());
        message[40..44].copy_from_slice(&7u32.to_le_bytes());
        message
    }

    fn write_u16(data: &mut [u8], offset: usize, value: usize) {
        data[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes());
    }

    fn canonical_sdk_fixture() -> Vec<u8> {
        let message = fixture_voucher_message();
        let mut data = Vec::with_capacity(CANONICAL_DATA_LEN);
        data.extend_from_slice(&SDK_CANONICAL_HEADER);
        data.extend_from_slice(&FIXTURE_PUBKEY);
        data.extend_from_slice(&FIXTURE_SIGNATURE);
        data.extend_from_slice(&message);
        assert_eq!(data.len(), 156);
        data
    }

    fn canonical_fixture_for_message(message: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; CANONICAL_MESSAGE_OFFSET + message.len()];
        data[0] = 1;
        write_u16(&mut data, 2, CANONICAL_SIGNATURE_OFFSET);
        write_u16(&mut data, 4, LOCAL_INSTRUCTION_INDEX as usize);
        write_u16(&mut data, 6, CANONICAL_PUBKEY_OFFSET);
        write_u16(&mut data, 8, LOCAL_INSTRUCTION_INDEX as usize);
        write_u16(&mut data, 10, CANONICAL_MESSAGE_OFFSET);
        write_u16(&mut data, 12, message.len());
        write_u16(&mut data, 14, LOCAL_INSTRUCTION_INDEX as usize);
        data[CANONICAL_PUBKEY_OFFSET..CANONICAL_SIGNATURE_OFFSET].copy_from_slice(&FIXTURE_PUBKEY);
        data[CANONICAL_SIGNATURE_OFFSET..CANONICAL_MESSAGE_OFFSET]
            .copy_from_slice(&FIXTURE_SIGNATURE);
        data[CANONICAL_MESSAGE_OFFSET..].copy_from_slice(message);
        data
    }

    fn with_instruction_sysvar<T>(
        instructions: &[Instruction],
        current_index: u16,
        f: impl FnOnce(&AccountInfo) -> T,
    ) -> T {
        let borrowed_instructions: Vec<BorrowedInstruction<'_>> = instructions
            .iter()
            .map(|ix| BorrowedInstruction {
                program_id: &ix.program_id,
                accounts: ix
                    .accounts
                    .iter()
                    .map(|meta| BorrowedAccountMeta {
                        pubkey: &meta.pubkey,
                        is_signer: meta.is_signer,
                        is_writable: meta.is_writable,
                    })
                    .collect(),
                data: ix.data.as_slice(),
            })
            .collect();
        let mut data = construct_instructions_data(&borrowed_instructions);
        store_current_index_checked(&mut data, current_index).unwrap();
        let key = sysvar::instructions::ID;
        let owner = sysvar::ID;
        let mut lamports = 0;
        let account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            &mut data,
            &owner,
            false,
            0,
        );
        f(&account)
    }

    fn verify_fixture_with_program(
        data: &[u8],
        previous_program_id: Pubkey,
        current_index: u16,
        expected_pubkey: &[u8; ED25519_PUBKEY_LEN],
        expected_message: &[u8],
    ) -> Result<()> {
        let instructions = vec![
            Instruction {
                program_id: previous_program_id,
                accounts: vec![],
                data: data.to_vec(),
            },
            Instruction {
                program_id: crate::ID,
                accounts: vec![],
                data: vec![],
            },
        ];
        with_instruction_sysvar(&instructions, current_index, |instructions_sysvar| {
            verify_session_signed(instructions_sysvar, expected_pubkey, expected_message)
        })
    }

    fn verify_fixture_with_account_meta(data: &[u8]) -> Result<()> {
        let instructions = vec![
            Instruction {
                program_id: ED25519_VERIFY_PROGRAM_ID,
                accounts: vec![AccountMeta::new_readonly(Pubkey::new_unique(), false)],
                data: data.to_vec(),
            },
            Instruction {
                program_id: crate::ID,
                accounts: vec![],
                data: vec![],
            },
        ];
        with_instruction_sysvar(&instructions, 1, |instructions_sysvar| {
            verify_session_signed(
                instructions_sysvar,
                &FIXTURE_PUBKEY,
                &fixture_voucher_message(),
            )
        })
    }

    fn verify_fixture(data: &[u8]) -> Result<()> {
        verify_fixture_with_program(
            data,
            ED25519_VERIFY_PROGRAM_ID,
            1,
            &FIXTURE_PUBKEY,
            &fixture_voucher_message(),
        )
    }

    fn assert_invalid(result: Result<()>, case: &str) {
        let err = result.expect_err(case);
        assert_eq!(
            err,
            Error::from(VaultError::InvalidVoucherSignature),
            "{case}"
        );
    }

    #[test]
    fn canonical_sdk_layout_is_accepted_through_instruction_sysvar() {
        assert!(verify_fixture(&canonical_sdk_fixture()).is_ok());
    }

    #[test]
    fn canonical_context_bound_v2_layout_is_accepted_but_v1_header_is_not() {
        let message = vec![0x71; VOUCHER_V2_MESSAGE_LEN];
        let data = canonical_fixture_for_message(&message);
        assert!(verify_fixture_with_program(
            &data,
            ED25519_VERIFY_PROGRAM_ID,
            1,
            &FIXTURE_PUBKEY,
            &message,
        )
        .is_ok());

        assert_invalid(
            verify_fixture_with_program(
                &canonical_sdk_fixture(),
                ED25519_VERIFY_PROGRAM_ID,
                1,
                &FIXTURE_PUBKEY,
                &message,
            ),
            "v1 precompile cannot satisfy v2 session",
        );
    }

    #[test]
    fn exact_header_offsets_markers_and_length_are_required() {
        let canonical = canonical_sdk_fixture();
        let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();

        let mut num_signatures = canonical.clone();
        num_signatures[0] = 2;
        cases.push(("multiple signatures", num_signatures));

        let mut padding = canonical.clone();
        padding[1] = 1;
        cases.push(("nonzero padding", padding));

        for (name, offset, value) in [
            ("signature offset", 2, CANONICAL_SIGNATURE_OFFSET + 1),
            ("signature instruction index", 4, 0),
            ("public-key offset", 6, CANONICAL_PUBKEY_OFFSET + 1),
            ("public-key instruction index", 8, 0),
            ("message offset", 10, CANONICAL_MESSAGE_OFFSET - 1),
            ("message size", 12, VOUCHER_MESSAGE_LEN - 1),
            ("message instruction index", 14, 0),
        ] {
            let mut mutated = canonical.clone();
            write_u16(&mut mutated, offset, value);
            cases.push((name, mutated));
        }

        let mut trailing_data = canonical.clone();
        trailing_data.push(0);
        cases.push(("trailing data", trailing_data));

        let mut truncated_data = canonical;
        truncated_data.pop();
        cases.push(("truncated data", truncated_data));

        for (case, data) in cases {
            assert_invalid(verify_fixture(&data), case);
        }
    }

    #[test]
    fn previous_permissive_alternate_layout_is_rejected() {
        // The precompile supports arbitrary offsets. The old Vault parser did
        // too, so this message || signature || pubkey permutation was accepted.
        // Dexter's SDK has never emitted it; only its fixed layout is canonical.
        const ALTERNATE_MESSAGE_OFFSET: usize = 16;
        const ALTERNATE_SIGNATURE_OFFSET: usize = ALTERNATE_MESSAGE_OFFSET + VOUCHER_MESSAGE_LEN;
        const ALTERNATE_PUBKEY_OFFSET: usize = ALTERNATE_SIGNATURE_OFFSET + ED25519_SIG_LEN;

        let message = fixture_voucher_message();
        let mut data = vec![0u8; CANONICAL_DATA_LEN];
        data[0] = 1;
        write_u16(&mut data, 2, ALTERNATE_SIGNATURE_OFFSET);
        write_u16(&mut data, 4, LOCAL_INSTRUCTION_INDEX as usize);
        write_u16(&mut data, 6, ALTERNATE_PUBKEY_OFFSET);
        write_u16(&mut data, 8, LOCAL_INSTRUCTION_INDEX as usize);
        write_u16(&mut data, 10, ALTERNATE_MESSAGE_OFFSET);
        write_u16(&mut data, 12, VOUCHER_MESSAGE_LEN);
        write_u16(&mut data, 14, LOCAL_INSTRUCTION_INDEX as usize);
        data[ALTERNATE_MESSAGE_OFFSET..ALTERNATE_SIGNATURE_OFFSET].copy_from_slice(&message);
        data[ALTERNATE_SIGNATURE_OFFSET..ALTERNATE_PUBKEY_OFFSET]
            .copy_from_slice(&FIXTURE_SIGNATURE);
        data[ALTERNATE_PUBKEY_OFFSET..CANONICAL_DATA_LEN].copy_from_slice(&FIXTURE_PUBKEY);

        assert_invalid(verify_fixture(&data), "alternate offsets");
    }

    #[test]
    fn exact_registered_key_and_voucher_message_are_required() {
        let data = canonical_sdk_fixture();
        let wrong_pubkey = [0x32; ED25519_PUBKEY_LEN];
        assert_invalid(
            verify_fixture_with_program(
                &data,
                ED25519_VERIFY_PROGRAM_ID,
                1,
                &wrong_pubkey,
                &fixture_voucher_message(),
            ),
            "wrong registered session key",
        );

        let mut wrong_message = fixture_voucher_message();
        wrong_message[0] ^= 1;
        assert_invalid(
            verify_fixture_with_program(
                &data,
                ED25519_VERIFY_PROGRAM_ID,
                1,
                &FIXTURE_PUBKEY,
                &wrong_message,
            ),
            "wrong voucher message",
        );

        assert_invalid(
            verify_fixture_with_program(
                &data,
                ED25519_VERIFY_PROGRAM_ID,
                1,
                &FIXTURE_PUBKEY,
                &fixture_voucher_message()[..VOUCHER_MESSAGE_LEN - 1],
            ),
            "noncanonical expected voucher length",
        );
    }

    #[test]
    fn preceding_instruction_must_be_the_ed25519_precompile() {
        let data = canonical_sdk_fixture();
        assert_invalid(
            verify_fixture_with_program(
                &data,
                crate::ID,
                1,
                &FIXTURE_PUBKEY,
                &fixture_voucher_message(),
            ),
            "wrong preceding program",
        );
        assert_invalid(
            verify_fixture_with_program(
                &data,
                ED25519_VERIFY_PROGRAM_ID,
                0,
                &FIXTURE_PUBKEY,
                &fixture_voucher_message(),
            ),
            "no preceding instruction",
        );
        assert_invalid(
            verify_fixture_with_account_meta(&data),
            "noncanonical precompile account meta",
        );
    }
}

#[cfg(test)]
mod public_key_admission_tests {
    use super::*;

    // All eight canonical encodings in Edwards25519's torsion subgroup.
    const SMALL_ORDER_POINTS: [[u8; 32]; 8] = [
        [
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ],
        [
            199, 23, 106, 112, 61, 77, 216, 79, 186, 60, 11, 118, 13, 16, 103, 15, 42, 32, 83, 250,
            44, 57, 204, 198, 78, 199, 253, 119, 146, 172, 3, 122,
        ],
        [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ],
        [
            38, 232, 149, 143, 194, 178, 39, 176, 69, 195, 244, 137, 242, 239, 152, 240, 213, 223,
            172, 5, 211, 198, 51, 57, 177, 56, 2, 136, 109, 83, 252, 5,
        ],
        [
            236, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 127,
        ],
        [
            38, 232, 149, 143, 194, 178, 39, 176, 69, 195, 244, 137, 242, 239, 152, 240, 213, 223,
            172, 5, 211, 198, 51, 57, 177, 56, 2, 136, 109, 83, 252, 133,
        ],
        [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 128,
        ],
        [
            199, 23, 106, 112, 61, 77, 216, 79, 186, 60, 11, 118, 13, 16, 103, 15, 42, 32, 83, 250,
            44, 57, 204, 198, 78, 199, 253, 119, 146, 172, 3, 250,
        ],
    ];

    #[test]
    fn basepoint_is_accepted() {
        let basepoint = [
            88, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102,
            102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102,
        ];
        assert!(require_nonweak_ed25519_public_key(&basepoint).is_ok());
    }

    #[test]
    fn all_canonical_small_order_points_are_rejected() {
        for point in SMALL_ORDER_POINTS {
            assert!(
                require_nonweak_ed25519_public_key(&point).is_err(),
                "accepted small-order encoding {}",
                point
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
        }
    }

    #[test]
    fn malformed_point_is_rejected() {
        let invalid = [
            120, 140, 152, 233, 41, 227, 203, 27, 87, 115, 25, 251, 219, 5, 84, 148, 117, 38, 84,
            60, 87, 144, 161, 146, 42, 34, 91, 155, 158, 189, 121, 79,
        ];
        assert!(require_nonweak_ed25519_public_key(&invalid).is_err());
    }

    #[test]
    fn noncanonical_small_order_encoding_is_rejected() {
        let noncanonical_identity = [
            238, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 127,
        ];
        assert!(require_nonweak_ed25519_public_key(&noncanonical_identity).is_err());
    }
}
