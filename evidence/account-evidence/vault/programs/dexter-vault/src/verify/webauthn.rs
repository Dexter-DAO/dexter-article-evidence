//! WebAuthn-aware SIMD-0075 introspection.
//!
//! When a browser passkey signs an operation, the authenticator does NOT
//! sign the raw operation bytes. Per the WebAuthn spec, it signs:
//!
//! ```text
//!     authenticatorData || sha256(clientDataJSON)
//! ```
//!
//! where `clientDataJSON` is a JSON object the browser produces, of the form:
//!
//! ```text
//!     {"type":"webauthn.get","challenge":"<base64url>","origin":"https://dexter.cash",...}
//! ```
//!
//! and `challenge` is the value the relying party (dexter-api) passed to
//! `navigator.credentials.get({ challenge })`. For our vault, the challenge
//! is a fixed-width authorization structure that binds the program, vault,
//! replay nonce, operation digest, and a fresh server ceremony nonce.
//!
//! This module's job:
//!   1. Pin `clientDataJSON.type`, `clientDataJSON.origin`,
//!      `clientDataJSON.crossOrigin`, and `authenticatorData.rpIdHash` to
//!      Dexter's hosted WebAuthn surface, and require user presence
//!   2. Confirm SIMD-0075 ran with `message = authenticatorData || sha256(clientDataJSON)`
//!   3. Parse `clientDataJSON` to extract the base64url challenge
//!   4. Decode the challenge and confirm every structured field exactly
//!
//! The result: the vault knows the user's passkey signed *this specific
//! operation*, with cryptographic guarantees from the SIMD-0075 precompile.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar::instructions::{
    load_current_index_checked, load_instruction_at_checked,
};
use solana_sha256_hasher::hashv;

use crate::state::{PasskeyAuthorization, VaultError, PASSKEY_AUTHORIZATION_VERSION_V1};

/// SIMD-0075 precompile program ID.
pub const SECP256R1_VERIFY_PROGRAM_ID: Pubkey =
    anchor_lang::pubkey!("Secp256r1SigVerify1111111111111111111111111");

/// `sha256("dexter.cash")`, the WebAuthn relying-party ID used by Dexter's
/// signing surface. This pins the credential ceremony, not the website using
/// Connect: arbitrary third-party sites can still integrate while the passkey
/// assertion itself remains scoped to Dexter's RP.
pub const DEXTER_WEBAUTHN_RP_ID_HASH: [u8; 32] = [
    0x56, 0xcc, 0xdf, 0x9a, 0x8a, 0x85, 0xb8, 0x88, 0xfe, 0x38, 0x4f, 0xda, 0x42, 0x4d, 0x40, 0x51,
    0xa0, 0x78, 0x42, 0x5a, 0xed, 0xd1, 0x42, 0x00, 0xa6, 0x49, 0x6e, 0xd6, 0xef, 0x91, 0x77, 0x7c,
];

/// The only signed browser origin accepted by the vault. Arbitrary merchant
/// websites remain supported by opening or embedding this Dexter-hosted
/// signing surface; merchant origins never become WebAuthn trust anchors.
pub const DEXTER_WEBAUTHN_ORIGIN: &[u8] = b"https://dexter.cash";

const WEBAUTHN_GET_TYPE: &[u8] = b"webauthn.get";

/// Fixed-width authorization domain. The NUL padding is part of the signed
/// wire contract; callers must include all 32 bytes verbatim.
pub const PASSKEY_AUTHORIZATION_DOMAIN_V1: &[u8; 32] =
    b"DEXTER_PASSKEY_AUTH_V1\0\0\0\0\0\0\0\0\0\0";

/// One-time release epoch generated for the caller-controlled signing-server
/// hardening. It is public, not secret: its purpose is to make every assertion
/// captured before this program revision cryptographically stale.
pub const PASSKEY_AUTHORIZATION_EPOCH_V1: [u8; 32] = [
    0x9e, 0xc9, 0x60, 0x51, 0x41, 0xd8, 0x75, 0x2e, 0xec, 0x18, 0xff, 0xe5, 0x6a, 0x5a, 0xcc, 0x3e,
    0x12, 0xc4, 0xa8, 0x3a, 0xb3, 0xbd, 0xed, 0x86, 0x6f, 0x45, 0xb2, 0x00, 0x57, 0x88, 0x44, 0xf1,
];

pub const PASSKEY_AUTHORIZATION_CHALLENGE_LEN: usize = 32 + 32 + 32 + 32 + 8 + 32 + 32;

const SIGNATURE_SERIALIZED_SIZE: usize = 64;
const COMPRESSED_PUBKEY_SERIALIZED_SIZE: usize = 33;
const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 14;
const DATA_START: usize = 2;

/// WebAuthn `authenticatorData` minimum length: 32 (rpIdHash) + 1 (flags) + 4 (signCount).
const AUTH_DATA_MIN_LEN: usize = 37;
const AUTH_DATA_FLAGS_OFFSET: usize = 32;
const AUTH_DATA_FLAG_USER_PRESENT: u8 = 0x01;
const AUTH_DATA_FLAG_USER_VERIFIED: u8 = 0x04;

/// Build the canonical fixed-width WebAuthn challenge:
///
/// ```text
/// domain[32] || epoch[32] || program_id[32] || vault[32] || nonce_u64_le
///   || sha256(operation)[32] || ceremony_nonce[32]
/// ```
///
/// These 200 bytes are passed directly to `navigator.credentials.get()` as the
/// challenge; callers MUST NOT add another domain or hash layer. The release
/// epoch invalidates pre-fix captures; program/vault bind scope; the guard nonce
/// makes a successful assertion usable exactly once; the random ceremony nonce
/// gives each issuance a unique server-side consumption record without burning
/// the on-chain nonce until submission succeeds.
pub fn build_passkey_authorization_challenge(
    vault: &Pubkey,
    nonce: u64,
    operation_message: &[u8],
    ceremony_nonce: &[u8; 32],
) -> [u8; PASSKEY_AUTHORIZATION_CHALLENGE_LEN] {
    let operation_hash = hashv(&[operation_message]);
    let mut challenge = [0u8; PASSKEY_AUTHORIZATION_CHALLENGE_LEN];
    challenge[0..32].copy_from_slice(PASSKEY_AUTHORIZATION_DOMAIN_V1);
    challenge[32..64].copy_from_slice(&PASSKEY_AUTHORIZATION_EPOCH_V1);
    challenge[64..96].copy_from_slice(crate::ID.as_ref());
    challenge[96..128].copy_from_slice(vault.as_ref());
    challenge[128..136].copy_from_slice(&nonce.to_le_bytes());
    challenge[136..168].copy_from_slice(operation_hash.as_ref());
    challenge[168..200].copy_from_slice(ceremony_nonce);
    challenge
}

/// Verify a passkey assertion against the current per-vault nonce and consume
/// that nonce atomically. Callers invoke this inside the state-changing
/// instruction; Solana transaction rollback restores the nonce if any later
/// check in the instruction or transaction fails.
pub fn verify_and_consume_passkey_authorization(
    instructions_sysvar: &AccountInfo,
    expected_pubkey: &[u8; COMPRESSED_PUBKEY_SERIALIZED_SIZE],
    passkey_authorization: &mut PasskeyAuthorization,
    expected_vault: &Pubkey,
    client_data_json: &[u8],
    authenticator_data: &[u8],
    operation_message: &[u8],
) -> Result<()> {
    verify_and_consume_passkey_authorization_inner(
        instructions_sysvar,
        expected_pubkey,
        passkey_authorization,
        expected_vault,
        client_data_json,
        authenticator_data,
        operation_message,
        false,
    )
}

/// Strict one-ceremony variant used by live session rotation. In addition to
/// adjacency, authorship, and nonce consumption, this rejects a SIMD-0075
/// instruction carrying any count other than one assertion.
pub fn verify_and_consume_passkey_authorization_one_assertion(
    instructions_sysvar: &AccountInfo,
    expected_pubkey: &[u8; COMPRESSED_PUBKEY_SERIALIZED_SIZE],
    passkey_authorization: &mut PasskeyAuthorization,
    expected_vault: &Pubkey,
    client_data_json: &[u8],
    authenticator_data: &[u8],
    operation_message: &[u8],
) -> Result<()> {
    verify_and_consume_passkey_authorization_inner(
        instructions_sysvar,
        expected_pubkey,
        passkey_authorization,
        expected_vault,
        client_data_json,
        authenticator_data,
        operation_message,
        true,
    )
}

/// Packet-compact one-assertion variant. The authenticator bytes are already
/// present in the immediately preceding SIMD-0075 instruction's signed
/// message, so callers need not duplicate them inside their own instruction
/// data. The local-offset and exact-pubkey checks are identical to the regular
/// path; the final 32 bytes must equal sha256(clientDataJSON).
pub fn verify_and_consume_passkey_authorization_embedded_authenticator(
    instructions_sysvar: &AccountInfo,
    expected_pubkey: &[u8; COMPRESSED_PUBKEY_SERIALIZED_SIZE],
    passkey_authorization: &mut PasskeyAuthorization,
    expected_vault: &Pubkey,
    client_data_json: &[u8],
    operation_message: &[u8],
) -> Result<()> {
    require!(
        passkey_authorization.version == PASSKEY_AUTHORIZATION_VERSION_V1
            && passkey_authorization.vault == *expected_vault,
        VaultError::PasskeyAuthorizationMismatch
    );
    let nonce = passkey_authorization.nonce;
    let authorization_challenge = validate_passkey_authorization_challenge(
        client_data_json,
        expected_vault,
        nonce,
        operation_message,
    )?;
    let signed_message = load_simd_0075_local_message(instructions_sysvar, expected_pubkey, true)?;
    require!(
        signed_message.len() >= AUTH_DATA_MIN_LEN + 32,
        VaultError::PasskeyVerificationFailed
    );
    let split = signed_message.len() - 32;
    let (authenticator_data, signed_client_hash) = signed_message.split_at(split);
    require!(
        signed_client_hash == hashv(&[client_data_json]).as_ref(),
        VaultError::PasskeyVerificationFailed
    );
    validate_webauthn_context(
        client_data_json,
        authenticator_data,
        &authorization_challenge,
    )?;
    consume_passkey_authorization_nonce(passkey_authorization, nonce)
}

fn verify_and_consume_passkey_authorization_inner(
    instructions_sysvar: &AccountInfo,
    expected_pubkey: &[u8; COMPRESSED_PUBKEY_SERIALIZED_SIZE],
    passkey_authorization: &mut PasskeyAuthorization,
    expected_vault: &Pubkey,
    client_data_json: &[u8],
    authenticator_data: &[u8],
    operation_message: &[u8],
    require_exactly_one_assertion: bool,
) -> Result<()> {
    require!(
        passkey_authorization.version == PASSKEY_AUTHORIZATION_VERSION_V1
            && passkey_authorization.vault == *expected_vault,
        VaultError::PasskeyAuthorizationMismatch
    );

    let nonce = passkey_authorization.nonce;
    let authorization_challenge = validate_passkey_authorization_challenge(
        client_data_json,
        expected_vault,
        nonce,
        operation_message,
    )?;

    verify_passkey_signed(
        instructions_sysvar,
        expected_pubkey,
        client_data_json,
        authenticator_data,
        &authorization_challenge,
        require_exactly_one_assertion,
    )?;

    consume_passkey_authorization_nonce(passkey_authorization, nonce)
}

/// Parse the signed 200-byte challenge and bind every program-controlled field.
/// The final 32 bytes are deliberately accepted as opaque entropy: dexter-api
/// issues and atomically consumes that exact random ceremony nonce, while the
/// on-chain guard enforces one successful authorization per vault nonce.
fn validate_passkey_authorization_challenge(
    client_data_json: &[u8],
    expected_vault: &Pubkey,
    expected_nonce: u64,
    operation_message: &[u8],
) -> Result<Vec<u8>> {
    let challenge = extract_challenge_from_client_data(client_data_json)?;
    require!(
        challenge.len() == PASSKEY_AUTHORIZATION_CHALLENGE_LEN,
        VaultError::PasskeyVerificationFailed
    );
    require!(
        challenge[0..32] == PASSKEY_AUTHORIZATION_DOMAIN_V1[..]
            && challenge[32..64] == PASSKEY_AUTHORIZATION_EPOCH_V1
            && challenge[64..96] == crate::ID.to_bytes()
            && challenge[96..128] == expected_vault.to_bytes()
            && challenge[128..136] == expected_nonce.to_le_bytes()
            && challenge[136..168] == hashv(&[operation_message]).to_bytes(),
        VaultError::PasskeyVerificationFailed
    );
    Ok(challenge)
}

fn consume_passkey_authorization_nonce(
    passkey_authorization: &mut PasskeyAuthorization,
    expected_nonce: u64,
) -> Result<()> {
    require!(
        passkey_authorization.nonce == expected_nonce,
        VaultError::PasskeyAuthorizationNonceMismatch
    );
    passkey_authorization.nonce = expected_nonce
        .checked_add(1)
        .ok_or_else(|| error!(VaultError::PasskeyAuthorizationNonceMismatch))?;
    Ok(())
}

/// Verifies that the user's passkey signed the given operation message.
///
/// Inputs:
///   - `instructions_sysvar`: the sysvar account, address-constrained by the caller
///   - `expected_pubkey`: the vault's stored 33-byte SEC1 compressed P-256 pubkey
///   - `client_data_json`: the JSON blob the browser produced during the ceremony
///   - `authenticator_data`: the WebAuthn authenticator output (37+ bytes)
///   - `expected_challenge`: the exact structured bytes that must equal the
///     base64url-decoded `challenge` field inside `client_data_json`
///
/// Returns Ok(()) iff every check passes. The precompile already validated
/// the signature — this helper proves authorship over a specific operation.
fn verify_passkey_signed(
    instructions_sysvar: &AccountInfo,
    expected_pubkey: &[u8; COMPRESSED_PUBKEY_SERIALIZED_SIZE],
    client_data_json: &[u8],
    authenticator_data: &[u8],
    expected_challenge: &[u8],
    require_exactly_one_assertion: bool,
) -> Result<()> {
    validate_webauthn_context(client_data_json, authenticator_data, expected_challenge)?;

    // The message SIMD-0075 verified: authenticatorData || sha256(clientDataJSON).
    let client_data_hash = hashv(&[client_data_json]);
    let mut composed = Vec::with_capacity(authenticator_data.len() + 32);
    composed.extend_from_slice(authenticator_data);
    composed.extend_from_slice(client_data_hash.as_ref());

    introspect_simd_0075(
        instructions_sysvar,
        expected_pubkey,
        &composed,
        require_exactly_one_assertion,
    )?;

    Ok(())
}

/// Validate the WebAuthn ceremony fields that the SIMD-0075 signature covers.
///
/// Merchant websites do not sign directly in their own origin. Connect opens
/// or embeds the hosted Dexter signing surface, so the signed browser origin
/// is always exactly `https://dexter.cash` while arbitrary merchant origins
/// remain supported outside this trust boundary.
fn validate_webauthn_context(
    client_data_json: &[u8],
    authenticator_data: &[u8],
    expected_challenge: &[u8],
) -> Result<()> {
    require!(
        authenticator_data.len() >= AUTH_DATA_MIN_LEN,
        VaultError::PasskeyVerificationFailed
    );
    require!(
        authenticator_data[..32] == DEXTER_WEBAUTHN_RP_ID_HASH,
        VaultError::PasskeyVerificationFailed
    );
    require!(
        authenticator_data[AUTH_DATA_FLAGS_OFFSET] & AUTH_DATA_FLAG_USER_PRESENT != 0,
        VaultError::PasskeyVerificationFailed
    );
    require!(
        authenticator_data[AUTH_DATA_FLAGS_OFFSET] & AUTH_DATA_FLAG_USER_VERIFIED != 0,
        VaultError::PasskeyVerificationFailed
    );

    // Strictly parse the signed CollectedClientData. The parser rejects
    // duplicate required keys and string-substring decoys in unknown values.
    let challenge_bytes = extract_challenge_from_client_data(client_data_json)?;
    require!(
        challenge_bytes.as_slice() == expected_challenge,
        VaultError::PasskeyVerificationFailed
    );

    Ok(())
}

/// Confirms that the previous instruction was a SIMD-0075 sigverify call
/// with exactly the given pubkey and message. The precompile already checked
/// the signature; this function only checks authorship.
fn simd_0075_offsets_are_local(
    signature_instruction_index: u16,
    public_key_instruction_index: u16,
    message_instruction_index: u16,
) -> bool {
    signature_instruction_index == u16::MAX
        && public_key_instruction_index == u16::MAX
        && message_instruction_index == u16::MAX
}

fn simd_0075_signature_count_is_allowed(count: u8, require_exactly_one: bool) -> bool {
    if require_exactly_one {
        count == 1
    } else {
        count >= 1
    }
}

fn introspect_simd_0075(
    instructions_sysvar: &AccountInfo,
    expected_pubkey: &[u8; COMPRESSED_PUBKEY_SERIALIZED_SIZE],
    expected_message: &[u8],
    require_exactly_one_assertion: bool,
) -> Result<()> {
    let actual_message = load_simd_0075_local_message(
        instructions_sysvar,
        expected_pubkey,
        require_exactly_one_assertion,
    )?;
    require!(
        actual_message.as_slice() == expected_message,
        VaultError::PasskeyVerificationFailed
    );
    Ok(())
}

fn load_simd_0075_local_message(
    instructions_sysvar: &AccountInfo,
    expected_pubkey: &[u8; COMPRESSED_PUBKEY_SERIALIZED_SIZE],
    require_exactly_one_assertion: bool,
) -> Result<Vec<u8>> {
    let current_index = load_current_index_checked(instructions_sysvar)?;
    require!(current_index > 0, VaultError::PasskeyVerificationFailed);

    let prev_ix = load_instruction_at_checked((current_index as usize) - 1, instructions_sysvar)?;

    require!(
        prev_ix.program_id == SECP256R1_VERIFY_PROGRAM_ID,
        VaultError::PasskeyVerificationFailed
    );

    let data = prev_ix.data;
    require!(
        data.len() >= DATA_START + SIGNATURE_OFFSETS_SERIALIZED_SIZE,
        VaultError::PasskeyVerificationFailed
    );

    let num_signatures = data[0];
    require!(
        simd_0075_signature_count_is_allowed(num_signatures, require_exactly_one_assertion),
        VaultError::PasskeyVerificationFailed
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
        // The bytes compared below come from this precompile instruction.
        // Requiring SIMD-0075 to verify the same local bytes prevents an
        // attacker from pointing all three offsets at another instruction.
        simd_0075_offsets_are_local(
            signature_instruction_index,
            public_key_instruction_index,
            message_instruction_index,
        ),
        VaultError::PasskeyVerificationFailed
    );

    require!(
        public_key_offset + COMPRESSED_PUBKEY_SERIALIZED_SIZE <= data.len(),
        VaultError::PasskeyVerificationFailed
    );
    require!(
        message_data_offset + message_data_size <= data.len(),
        VaultError::PasskeyVerificationFailed
    );

    let actual_pubkey =
        &data[public_key_offset..public_key_offset + COMPRESSED_PUBKEY_SERIALIZED_SIZE];
    let actual_message = &data[message_data_offset..message_data_offset + message_data_size];

    require!(
        actual_pubkey == expected_pubkey,
        VaultError::PasskeyVerificationFailed
    );
    let _ = SIGNATURE_SERIALIZED_SIZE;
    let _ = signature_offset;
    Ok(actual_message.to_vec())
}

/// Strictly parses the top-level CollectedClientData object and returns its
/// base64url-decoded challenge. We keep a small no-allocation parser on chain
/// instead of pulling in serde_json. Required properties may appear in any
/// order, but must appear exactly once with unescaped canonical values:
///
/// ```text
///     {"type":"webauthn.get","challenge":"<base64url>",
///      "origin":"https://dexter.cash","crossOrigin":false,...}
/// ```
fn extract_challenge_from_client_data(client_data_json: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = 0usize;
    skip_json_whitespace(client_data_json, &mut cursor);
    require_json_byte(client_data_json, &mut cursor, b'{')?;

    let mut type_seen = false;
    let mut challenge: Option<&[u8]> = None;
    let mut origin_seen = false;
    let mut cross_origin_seen = false;

    loop {
        skip_json_whitespace(client_data_json, &mut cursor);
        if consume_json_byte(client_data_json, &mut cursor, b'}') {
            break;
        }

        let (key, key_escaped) = parse_json_string(client_data_json, &mut cursor)?;
        require!(!key_escaped, VaultError::PasskeyVerificationFailed);
        skip_json_whitespace(client_data_json, &mut cursor);
        require_json_byte(client_data_json, &mut cursor, b':')?;
        skip_json_whitespace(client_data_json, &mut cursor);

        match key {
            b"type" => {
                require!(!type_seen, VaultError::PasskeyVerificationFailed);
                let (value, escaped) = parse_json_string(client_data_json, &mut cursor)?;
                require!(
                    !escaped && value == WEBAUTHN_GET_TYPE,
                    VaultError::PasskeyVerificationFailed
                );
                type_seen = true;
            }
            b"challenge" => {
                require!(challenge.is_none(), VaultError::PasskeyVerificationFailed);
                let (value, escaped) = parse_json_string(client_data_json, &mut cursor)?;
                require!(!escaped, VaultError::PasskeyVerificationFailed);
                challenge = Some(value);
            }
            b"origin" => {
                require!(!origin_seen, VaultError::PasskeyVerificationFailed);
                let (value, escaped) = parse_json_string(client_data_json, &mut cursor)?;
                require!(
                    !escaped && value == DEXTER_WEBAUTHN_ORIGIN,
                    VaultError::PasskeyVerificationFailed
                );
                origin_seen = true;
            }
            b"crossOrigin" => {
                require!(
                    !cross_origin_seen
                        && consume_json_literal(client_data_json, &mut cursor, b"false"),
                    VaultError::PasskeyVerificationFailed
                );
                cross_origin_seen = true;
            }
            _ => skip_json_value(client_data_json, &mut cursor, 0)?,
        }

        skip_json_whitespace(client_data_json, &mut cursor);
        if consume_json_byte(client_data_json, &mut cursor, b',') {
            continue;
        }
        require_json_byte(client_data_json, &mut cursor, b'}')?;
        break;
    }

    skip_json_whitespace(client_data_json, &mut cursor);
    require!(
        cursor == client_data_json.len()
            && type_seen
            && origin_seen
            && cross_origin_seen
            && challenge.is_some(),
        VaultError::PasskeyVerificationFailed
    );

    base64url_decode(challenge.unwrap())
}

fn skip_json_whitespace(input: &[u8], cursor: &mut usize) {
    while *cursor < input.len() && matches!(input[*cursor], b' ' | b'\n' | b'\r' | b'\t') {
        *cursor += 1;
    }
}

fn consume_json_byte(input: &[u8], cursor: &mut usize, expected: u8) -> bool {
    if input.get(*cursor) == Some(&expected) {
        *cursor += 1;
        true
    } else {
        false
    }
}

fn require_json_byte(input: &[u8], cursor: &mut usize, expected: u8) -> Result<()> {
    require!(
        consume_json_byte(input, cursor, expected),
        VaultError::PasskeyVerificationFailed
    );
    Ok(())
}

/// Returns the raw UTF-8 bytes between quotes and whether an escape occurred.
/// Required WebAuthn keys/values reject escapes, avoiding alternate encodings
/// and keeping exact string comparison unambiguous.
fn parse_json_string<'a>(input: &'a [u8], cursor: &mut usize) -> Result<(&'a [u8], bool)> {
    require_json_byte(input, cursor, b'"')?;
    let start = *cursor;
    let mut escaped = false;

    while *cursor < input.len() {
        match input[*cursor] {
            b'"' => {
                let end = *cursor;
                *cursor += 1;
                return Ok((&input[start..end], escaped));
            }
            b'\\' => {
                escaped = true;
                *cursor += 1;
                let escape = *input
                    .get(*cursor)
                    .ok_or_else(|| error!(VaultError::PasskeyVerificationFailed))?;
                match escape {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {
                        *cursor += 1;
                    }
                    b'u' => {
                        *cursor += 1;
                        for _ in 0..4 {
                            let hex = *input
                                .get(*cursor)
                                .ok_or_else(|| error!(VaultError::PasskeyVerificationFailed))?;
                            require!(
                                hex.is_ascii_hexdigit(),
                                VaultError::PasskeyVerificationFailed
                            );
                            *cursor += 1;
                        }
                    }
                    _ => return err!(VaultError::PasskeyVerificationFailed),
                }
            }
            0x00..=0x1f => return err!(VaultError::PasskeyVerificationFailed),
            _ => *cursor += 1,
        }
    }

    err!(VaultError::PasskeyVerificationFailed)
}

fn consume_json_literal(input: &[u8], cursor: &mut usize, literal: &[u8]) -> bool {
    if input.get(*cursor..*cursor + literal.len()) == Some(literal) {
        *cursor += literal.len();
        true
    } else {
        false
    }
}

/// Skip a syntactically-valid JSON value for forward-compatible unknown
/// CollectedClientData properties. Nesting is capped to keep adversarial input
/// from consuming the BPF stack.
fn skip_json_value(input: &[u8], cursor: &mut usize, depth: u8) -> Result<()> {
    const MAX_JSON_DEPTH: u8 = 8;
    require!(
        depth <= MAX_JSON_DEPTH,
        VaultError::PasskeyVerificationFailed
    );
    skip_json_whitespace(input, cursor);

    match input.get(*cursor).copied() {
        Some(b'"') => {
            parse_json_string(input, cursor)?;
        }
        Some(b'{') => {
            *cursor += 1;
            skip_json_whitespace(input, cursor);
            if consume_json_byte(input, cursor, b'}') {
                return Ok(());
            }
            loop {
                parse_json_string(input, cursor)?;
                skip_json_whitespace(input, cursor);
                require_json_byte(input, cursor, b':')?;
                skip_json_value(input, cursor, depth + 1)?;
                skip_json_whitespace(input, cursor);
                if consume_json_byte(input, cursor, b',') {
                    skip_json_whitespace(input, cursor);
                    continue;
                }
                require_json_byte(input, cursor, b'}')?;
                break;
            }
        }
        Some(b'[') => {
            *cursor += 1;
            skip_json_whitespace(input, cursor);
            if consume_json_byte(input, cursor, b']') {
                return Ok(());
            }
            loop {
                skip_json_value(input, cursor, depth + 1)?;
                skip_json_whitespace(input, cursor);
                if consume_json_byte(input, cursor, b',') {
                    skip_json_whitespace(input, cursor);
                    continue;
                }
                require_json_byte(input, cursor, b']')?;
                break;
            }
        }
        Some(b't') => {
            require!(
                consume_json_literal(input, cursor, b"true"),
                VaultError::PasskeyVerificationFailed
            );
        }
        Some(b'f') => {
            require!(
                consume_json_literal(input, cursor, b"false"),
                VaultError::PasskeyVerificationFailed
            );
        }
        Some(b'n') => {
            require!(
                consume_json_literal(input, cursor, b"null"),
                VaultError::PasskeyVerificationFailed
            );
        }
        Some(b'-' | b'0'..=b'9') => skip_json_number(input, cursor)?,
        _ => return err!(VaultError::PasskeyVerificationFailed),
    }

    Ok(())
}

fn skip_json_number(input: &[u8], cursor: &mut usize) -> Result<()> {
    consume_json_byte(input, cursor, b'-');
    match input.get(*cursor).copied() {
        Some(b'0') => *cursor += 1,
        Some(b'1'..=b'9') => {
            *cursor += 1;
            while matches!(input.get(*cursor), Some(b'0'..=b'9')) {
                *cursor += 1;
            }
        }
        _ => return err!(VaultError::PasskeyVerificationFailed),
    }

    if consume_json_byte(input, cursor, b'.') {
        let fraction_start = *cursor;
        while matches!(input.get(*cursor), Some(b'0'..=b'9')) {
            *cursor += 1;
        }
        require!(
            *cursor > fraction_start,
            VaultError::PasskeyVerificationFailed
        );
    }

    if matches!(input.get(*cursor), Some(b'e' | b'E')) {
        *cursor += 1;
        if matches!(input.get(*cursor), Some(b'+' | b'-')) {
            *cursor += 1;
        }
        let exponent_start = *cursor;
        while matches!(input.get(*cursor), Some(b'0'..=b'9')) {
            *cursor += 1;
        }
        require!(
            *cursor > exponent_start,
            VaultError::PasskeyVerificationFailed
        );
    }

    Ok(())
}

/// Minimal canonical unpadded base64url decoder. WebAuthn serializes challenge
/// bytes without `=` padding; rejecting alternate encodings keeps API and
/// program consumption keyed to one exact representation.
fn base64url_decode(input: &[u8]) -> Result<Vec<u8>> {
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    let mut out = Vec::with_capacity((input.len() * 3) / 4 + 2);
    for &b in input {
        let v: u32 = match b {
            b'A'..=b'Z' => (b - b'A') as u32,
            b'a'..=b'z' => (b - b'a' + 26) as u32,
            b'0'..=b'9' => (b - b'0' + 52) as u32,
            b'-' => 62,
            b'_' => 63,
            _ => return err!(VaultError::PasskeyVerificationFailed),
        };
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    require!(
        input.len() % 4 != 1 && (bits == 0 || (buf & ((1u32 << bits) - 1)) == 0),
        VaultError::PasskeyVerificationFailed
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base64url_encode(input: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
            let word = (b0 << 16) | (b1 << 8) | b2;
            out.push(ALPHABET[((word >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((word >> 12) & 0x3f) as usize] as char);
            if chunk.len() > 1 {
                out.push(ALPHABET[((word >> 6) & 0x3f) as usize] as char);
            }
            if chunk.len() > 2 {
                out.push(ALPHABET[(word & 0x3f) as usize] as char);
            }
        }
        out
    }

    fn client_data_with_context(
        challenge: &[u8],
        ceremony_type: &str,
        origin: &str,
        cross_origin: bool,
    ) -> Vec<u8> {
        format!(
            r#"{{"type":"{}","challenge":"{}","origin":"{}","crossOrigin":{}}}"#,
            ceremony_type,
            base64url_encode(challenge),
            origin,
            cross_origin,
        )
        .into_bytes()
    }

    fn client_data_for(challenge: &[u8]) -> Vec<u8> {
        client_data_with_context(challenge, "webauthn.get", "https://dexter.cash", false)
    }

    fn authenticator_data_for(rp_id: &[u8], flags: u8) -> Vec<u8> {
        let mut data = vec![0u8; AUTH_DATA_MIN_LEN];
        data[..32].copy_from_slice(hashv(&[rp_id]).as_ref());
        data[AUTH_DATA_FLAGS_OFFSET] = flags;
        data
    }

    #[test]
    fn accepts_dexter_rp_and_exact_operation_challenge() {
        let vault = Pubkey::new_unique();
        let operation = b"request_withdrawal:exact-operation-fixture";
        let ceremony_nonce = [0x31; 32];
        let authorization =
            build_passkey_authorization_challenge(&vault, 3, operation, &ceremony_nonce);
        let client_data = client_data_for(&authorization);
        let authenticator_data =
            authenticator_data_for(b"dexter.cash", AUTH_DATA_FLAG_USER_PRESENT | 0x04);

        assert!(
            validate_webauthn_context(&client_data, &authenticator_data, &authorization).is_ok()
        );
        assert_eq!(
            validate_passkey_authorization_challenge(&client_data, &vault, 3, operation).unwrap(),
            authorization
        );
    }

    #[test]
    fn rejects_merchant_origin_even_with_dexter_rp() {
        let vault = Pubkey::new_unique();
        let authorization =
            build_passkey_authorization_challenge(&vault, 0, b"attach_node", &[0x41; 32]);
        let client_data = client_data_with_context(
            &authorization,
            "webauthn.get",
            "https://merchant.example",
            false,
        );
        let authenticator_data =
            authenticator_data_for(b"dexter.cash", AUTH_DATA_FLAG_USER_PRESENT | 0x04);

        assert!(
            validate_webauthn_context(&client_data, &authenticator_data, &authorization).is_err()
        );
        assert!(
            validate_passkey_authorization_challenge(&client_data, &vault, 0, b"attach_node")
                .is_err()
        );
    }

    #[test]
    fn rejects_origin_suffix_subdomain_wrong_type_and_cross_origin() {
        let vault = Pubkey::new_unique();
        let operation = b"attach_node";
        let authorization =
            build_passkey_authorization_challenge(&vault, 0, operation, &[0x42; 32]);
        let authenticator_data =
            authenticator_data_for(b"dexter.cash", AUTH_DATA_FLAG_USER_PRESENT | 0x04);

        for client_data in [
            client_data_with_context(
                &authorization,
                "webauthn.get",
                "https://dexter.cash.attacker.example",
                false,
            ),
            client_data_with_context(
                &authorization,
                "webauthn.get",
                "https://signin.dexter.cash",
                false,
            ),
            client_data_with_context(
                &authorization,
                "webauthn.create",
                "https://dexter.cash",
                false,
            ),
            client_data_with_context(&authorization, "webauthn.get", "https://dexter.cash", true),
        ] {
            assert!(
                validate_webauthn_context(&client_data, &authenticator_data, &authorization)
                    .is_err()
            );
        }
    }

    #[test]
    fn required_client_data_fields_are_structural_and_unique() {
        let vault = Pubkey::new_unique();
        let operation = b"rotate_passkey";
        let authorization =
            build_passkey_authorization_challenge(&vault, 4, operation, &[0x43; 32]);
        let encoded = base64url_encode(&authorization);

        // A key-looking substring inside an unknown string is ignored because
        // required fields are parsed structurally, not found by byte scan.
        let with_decoy = format!(
            r#"{{"decoy":"\"origin\":\"https://merchant.example\"","challenge":"{}","crossOrigin":false,"origin":"https://dexter.cash","type":"webauthn.get"}}"#,
            encoded,
        );
        assert!(validate_passkey_authorization_challenge(
            with_decoy.as_bytes(),
            &vault,
            4,
            operation,
        )
        .is_ok());

        let duplicate_origin = format!(
            r#"{{"type":"webauthn.get","challenge":"{}","origin":"https://dexter.cash","crossOrigin":false,"origin":"https://dexter.cash"}}"#,
            encoded,
        );
        assert!(validate_passkey_authorization_challenge(
            duplicate_origin.as_bytes(),
            &vault,
            4,
            operation,
        )
        .is_err());
    }

    #[test]
    fn rejects_hostile_rp_even_when_operation_challenge_matches() {
        let vault = Pubkey::new_unique();
        let authorization =
            build_passkey_authorization_challenge(&vault, 0, b"request_withdrawal", &[0x51; 32]);
        let client_data = client_data_for(&authorization);
        let authenticator_data =
            authenticator_data_for(b"attacker.example", AUTH_DATA_FLAG_USER_PRESENT | 0x04);

        assert!(
            validate_webauthn_context(&client_data, &authenticator_data, &authorization).is_err()
        );
    }

    #[test]
    fn rejects_replay_for_a_different_operation_domain() {
        let vault = Pubkey::new_unique();
        let ceremony_nonce = [0x61; 32];
        let signed = build_passkey_authorization_challenge(
            &vault,
            7,
            b"request_withdrawal",
            &ceremony_nonce,
        );
        let replayed_as =
            build_passkey_authorization_challenge(&vault, 7, b"rotate_passkey", &ceremony_nonce);
        let client_data = client_data_for(&signed);
        let authenticator_data =
            authenticator_data_for(b"dexter.cash", AUTH_DATA_FLAG_USER_PRESENT | 0x04);

        assert!(
            validate_webauthn_context(&client_data, &authenticator_data, &replayed_as).is_err()
        );
        assert!(validate_passkey_authorization_challenge(
            &client_data,
            &vault,
            7,
            b"rotate_passkey"
        )
        .is_err());
    }

    #[test]
    fn rejects_pre_hardening_raw_operation_assertion() {
        let vault = Pubkey::new_unique();
        let raw_operation = b"request_withdrawal:captured-before-server-pin";
        let expected = build_passkey_authorization_challenge(&vault, 0, raw_operation, &[0x71; 32]);
        let legacy_challenge = hashv(&[raw_operation]);
        let client_data = client_data_for(legacy_challenge.as_ref());
        let authenticator_data =
            authenticator_data_for(b"dexter.cash", AUTH_DATA_FLAG_USER_PRESENT | 0x04);

        assert!(validate_webauthn_context(&client_data, &authenticator_data, &expected).is_err());
        assert!(
            validate_passkey_authorization_challenge(&client_data, &vault, 0, raw_operation)
                .is_err()
        );
    }

    #[test]
    fn rejects_assertion_without_user_presence() {
        let vault = Pubkey::new_unique();
        let authorization =
            build_passkey_authorization_challenge(&vault, 0, b"request_withdrawal", &[0x81; 32]);
        let client_data = client_data_for(&authorization);
        let authenticator_data = authenticator_data_for(b"dexter.cash", 0x04);

        assert!(
            validate_webauthn_context(&client_data, &authenticator_data, &authorization).is_err()
        );
    }

    #[test]
    fn rejects_assertion_without_user_verification() {
        let vault = Pubkey::new_unique();
        let authorization =
            build_passkey_authorization_challenge(&vault, 0, b"request_withdrawal", &[0x82; 32]);
        let client_data = client_data_for(&authorization);
        let authenticator_data =
            authenticator_data_for(b"dexter.cash", AUTH_DATA_FLAG_USER_PRESENT);

        assert!(
            validate_webauthn_context(&client_data, &authenticator_data, &authorization).is_err()
        );
    }

    #[test]
    fn authorization_envelope_layout_is_exact() {
        let vault = Pubkey::new_unique();
        let operation = b"rotate_passkey";
        let ceremony_nonce = [0xa5; 32];
        let challenge = build_passkey_authorization_challenge(
            &vault,
            0x0807_0605_0403_0201,
            operation,
            &ceremony_nonce,
        );

        assert_eq!(&challenge[0..32], PASSKEY_AUTHORIZATION_DOMAIN_V1);
        assert_eq!(&challenge[32..64], PASSKEY_AUTHORIZATION_EPOCH_V1);
        assert_eq!(&challenge[64..96], crate::ID.as_ref());
        assert_eq!(&challenge[96..128], vault.as_ref());
        assert_eq!(&challenge[128..136], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&challenge[136..168], hashv(&[operation]).as_ref());
        assert_eq!(&challenge[168..200], &ceremony_nonce);
    }

    #[test]
    fn structured_challenge_rejects_every_fixed_field_substitution() {
        let vault = Pubkey::new_unique();
        let operation = b"register_session_key:material-fields";
        let challenge = build_passkey_authorization_challenge(&vault, 42, operation, &[0xb5; 32]);

        for offset in [0usize, 32, 64, 96, 128, 136] {
            let mut hostile = challenge;
            hostile[offset] ^= 0x01;
            let client_data = client_data_for(&hostile);
            assert!(
                validate_passkey_authorization_challenge(&client_data, &vault, 42, operation)
                    .is_err()
            );
        }
    }

    #[test]
    fn ceremony_random_is_opaque_on_chain_but_changes_the_signed_challenge() {
        let vault = Pubkey::new_unique();
        let operation = b"prove_passkey";
        let first = build_passkey_authorization_challenge(&vault, 9, operation, &[0xc5; 32]);
        let second = build_passkey_authorization_challenge(&vault, 9, operation, &[0xd5; 32]);
        assert_ne!(first, second);

        for challenge in [first, second] {
            let client_data = client_data_for(&challenge);
            assert!(
                validate_passkey_authorization_challenge(&client_data, &vault, 9, operation)
                    .is_ok()
            );
        }
    }

    #[test]
    fn nonce_changes_challenge_and_consumes_once() {
        let vault = Pubkey::new_unique();
        let operation = b"force_release";
        let ceremony_nonce = [0x91; 32];
        let nonce_zero =
            build_passkey_authorization_challenge(&vault, 0, operation, &ceremony_nonce);
        let nonce_one =
            build_passkey_authorization_challenge(&vault, 1, operation, &ceremony_nonce);
        assert_ne!(nonce_zero, nonce_one);

        let mut guard = PasskeyAuthorization {
            version: PASSKEY_AUTHORIZATION_VERSION_V1,
            bump: 254,
            vault,
            nonce: 0,
        };
        assert!(consume_passkey_authorization_nonce(&mut guard, 0).is_ok());
        assert_eq!(guard.nonce, 1);
        assert!(consume_passkey_authorization_nonce(&mut guard, 0).is_err());
        assert_eq!(guard.nonce, 1);
    }

    #[test]
    fn stale_register_authorization_fails_after_replacement_consumes_guard() {
        let vault = Pubkey::new_unique();
        let register_operation = [0x5a; 188];
        let challenge =
            build_passkey_authorization_challenge(&vault, 12, &register_operation, &[0x92; 32]);
        let client_data = client_data_for(&challenge);
        let mut guard = PasskeyAuthorization {
            version: PASSKEY_AUTHORIZATION_VERSION_V1,
            bump: 200,
            vault,
            nonce: 12,
        };

        assert!(validate_passkey_authorization_challenge(
            &client_data,
            &vault,
            guard.nonce,
            &register_operation,
        )
        .is_ok());
        consume_passkey_authorization_nonce(&mut guard, 12).unwrap();
        assert_eq!(guard.nonce, 13);
        assert!(validate_passkey_authorization_challenge(
            &client_data,
            &vault,
            guard.nonce,
            &register_operation,
        )
        .is_err());
    }

    #[test]
    fn authorization_nonce_overflow_fails_closed() {
        let vault = Pubkey::new_unique();
        let mut guard = PasskeyAuthorization {
            version: PASSKEY_AUTHORIZATION_VERSION_V1,
            bump: 254,
            vault,
            nonce: u64::MAX,
        };

        assert!(consume_passkey_authorization_nonce(&mut guard, u64::MAX).is_err());
        assert_eq!(guard.nonce, u64::MAX);
    }

    #[test]
    fn sequential_authorizations_require_n_then_n_plus_one() {
        let vault = Pubkey::new_unique();
        let mut guard = PasskeyAuthorization {
            version: PASSKEY_AUTHORIZATION_VERSION_V1,
            bump: 200,
            vault,
            nonce: 17,
        };

        // Any two guarded instructions in one transaction consume in order.
        assert!(consume_passkey_authorization_nonce(&mut guard, 17).is_ok());
        assert!(consume_passkey_authorization_nonce(&mut guard, 18).is_ok());
        assert_eq!(guard.nonce, 19);

        // Neither assertion can be replayed after the batch commits.
        assert!(consume_passkey_authorization_nonce(&mut guard, 17).is_err());
        assert!(consume_passkey_authorization_nonce(&mut guard, 18).is_err());
        assert_eq!(guard.nonce, 19);
    }

    #[test]
    fn simd_0075_offsets_must_all_reference_local_instruction() {
        assert!(simd_0075_offsets_are_local(u16::MAX, u16::MAX, u16::MAX));
        assert!(!simd_0075_offsets_are_local(0, 0, 0));
        assert!(!simd_0075_offsets_are_local(7, 7, 7));
        assert!(!simd_0075_offsets_are_local(u16::MAX, u16::MAX, 0));
    }

    #[test]
    fn live_replacement_requires_exactly_one_simd_assertion() {
        assert!(!simd_0075_signature_count_is_allowed(0, true));
        assert!(simd_0075_signature_count_is_allowed(1, true));
        assert!(!simd_0075_signature_count_is_allowed(2, true));
        assert!(simd_0075_signature_count_is_allowed(2, false));
    }
}
