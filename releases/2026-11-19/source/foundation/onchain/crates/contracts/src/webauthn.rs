//! Strict Account WebAuthn context and native P-256 proof framing.
//!
//! Parser adapted from Dexter Vault commit
//! 7dbd239ec8166d658cc6a51e9a555ddc546a88bb,
//! programs/dexter-vault/src/verify/webauthn.rs, Git blob
//! d4fd4177e3d786d131e399f943877852580bfd34. Account supplies its own challenge,
//! authority, generation, nonce, exact-operation and deployment binding.
//!
//! This helper does NOT execute a signature verifier. The caller must load an
//! already executed native Secp256r1 instruction from the real instructions
//! sysvar and enforce its position in the authorized top-level envelope.
//! Runtime verification authenticates the same local point and signature.
//!
//! Authenticator bytes come from the precompile's signed message. Signature
//! counters are observations only: synced passkeys can report zero or a lower
//! count. Account's nonce/generation and exact operation provide replay safety.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::Instruction;
use solana_sha256_hasher::hashv;

pub const SECP256R1_VERIFY_PROGRAM_ID: Pubkey =
    pubkey!("Secp256r1SigVerify1111111111111111111111111");
pub const DEXTER_WEBAUTHN_ORIGIN: &[u8] = b"https://dexter.cash";
pub const DEXTER_WEBAUTHN_RP_ID_HASH: [u8; 32] = [
    0x56, 0xcc, 0xdf, 0x9a, 0x8a, 0x85, 0xb8, 0x88, 0xfe, 0x38, 0x4f, 0xda, 0x42, 0x4d, 0x40, 0x51,
    0xa0, 0x78, 0x42, 0x5a, 0xed, 0xd1, 0x42, 0x00, 0xa6, 0x49, 0x6e, 0xd6, 0xef, 0x91, 0x77, 0x7c,
];
pub const MAX_CLIENT_DATA_JSON_LEN: usize = 2048;
pub const MAX_AUTHENTICATOR_DATA_LEN: usize = 1024;
pub const MAX_CHALLENGE_LEN: usize = 512;
const WEBAUTHN_GET_TYPE: &[u8] = b"webauthn.get";
const AUTH_DATA_MIN_LEN: usize = 37;
const AUTH_DATA_FLAG_USER_PRESENT: u8 = 0x01;
const AUTH_DATA_FLAG_USER_VERIFIED: u8 = 0x04;
const SIGNATURE_OFFSET: usize = 16;
const PUBLIC_KEY_OFFSET: usize = SIGNATURE_OFFSET + 64;
const MESSAGE_OFFSET: usize = PUBLIC_KEY_OFFSET + 33;

/// Authenticated observations, never substitutes for authority or replay checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebAuthnObservation {
    pub flags: u8,
    pub sign_count: u32,
}

/// Separate range: no existing program/shared-contract error is renumbered.
#[error_code(offset = 8000)]
pub enum WebAuthnError {
    #[msg("WebAuthn proof must be an account-free native Secp256r1 instruction")]
    Precompile,
    #[msg("WebAuthn proof must contain one canonical local signature, key and message")]
    Framing,
    #[msg("CollectedClientData must be valid strict Dexter same-origin WebAuthn JSON")]
    ClientData,
    #[msg("WebAuthn challenge does not match the exact authorized Account action")]
    Challenge,
    #[msg("Authenticator RP, user presence or user verification is invalid")]
    Authenticator,
    #[msg("WebAuthn proof exceeds the supported byte bounds")]
    Bounds,
}

/// Check an already executed proof. Account constructs expected_challenge from
/// its authenticated action; neither the assertion nor its relayer selects it.
pub fn verify_webauthn_assertion(
    precompile: &Instruction,
    expected_key: &[u8; 33],
    client_data_json: &[u8],
    expected_challenge: &[u8],
) -> Result<WebAuthnObservation> {
    require!(
        precompile.program_id == SECP256R1_VERIFY_PROGRAM_ID && precompile.accounts.is_empty(),
        WebAuthnError::Precompile
    );
    require!(
        !client_data_json.is_empty() && client_data_json.len() <= MAX_CLIENT_DATA_JSON_LEN
            && !expected_challenge.is_empty() && expected_challenge.len() <= MAX_CHALLENGE_LEN,
        WebAuthnError::Bounds
    );
    require!(core::str::from_utf8(client_data_json).is_ok(), WebAuthnError::ClientData);
    let data = &precompile.data;
    require!(
        data.len() >= MESSAGE_OFFSET + AUTH_DATA_MIN_LEN + 32
            && data.len() <= MESSAGE_OFFSET + MAX_AUTHENTICATOR_DATA_LEN + 32
            && matches!(expected_key[0], 2 | 3),
        WebAuthnError::Framing
    );
    let word = |offset: usize| u16::from_le_bytes([data[offset], data[offset + 1]]);
    let message_len = usize::from(word(12));
    require!(
        data[0] == 1 && data[1] == 0
            && usize::from(word(2)) == SIGNATURE_OFFSET && word(4) == u16::MAX
            && usize::from(word(6)) == PUBLIC_KEY_OFFSET && word(8) == u16::MAX
            && usize::from(word(10)) == MESSAGE_OFFSET && word(14) == u16::MAX
            && data.len() == MESSAGE_OFFSET + message_len,
        WebAuthnError::Framing
    );
    require!(&data[PUBLIC_KEY_OFFSET..MESSAGE_OFFSET] == expected_key, WebAuthnError::Framing);
    let message = &data[MESSAGE_OFFSET..];
    let split = message.len() - 32;
    let authenticator_data = &message[..split];
    require!(
        message[split..] == hashv(&[client_data_json]).to_bytes(),
        WebAuthnError::Framing
    );
    require!(
        authenticator_data[..32] == DEXTER_WEBAUTHN_RP_ID_HASH
            && authenticator_data[32] & AUTH_DATA_FLAG_USER_PRESENT != 0
            && authenticator_data[32] & AUTH_DATA_FLAG_USER_VERIFIED != 0,
        WebAuthnError::Authenticator
    );
    let actual_challenge = extract_challenge_from_client_data(client_data_json)?;
    require!(actual_challenge == expected_challenge, WebAuthnError::Challenge);
    Ok(WebAuthnObservation {
        flags: authenticator_data[32],
        sign_count: u32::from_be_bytes(authenticator_data[33..37].try_into().unwrap()),
    })
}

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
        let (key, key_escaped) = parse_json_string(client_data_json, &mut cursor)?;
        require!(!key_escaped, WebAuthnError::ClientData);
        skip_json_whitespace(client_data_json, &mut cursor);
        require_json_byte(client_data_json, &mut cursor, b':')?;
        skip_json_whitespace(client_data_json, &mut cursor);

        match key {
            b"type" => {
                require!(!type_seen, WebAuthnError::ClientData);
                let (value, escaped) = parse_json_string(client_data_json, &mut cursor)?;
                require!(
                    !escaped && value == WEBAUTHN_GET_TYPE,
                    WebAuthnError::ClientData
                );
                type_seen = true;
            }
            b"challenge" => {
                require!(challenge.is_none(), WebAuthnError::ClientData);
                let (value, escaped) = parse_json_string(client_data_json, &mut cursor)?;
                require!(!escaped, WebAuthnError::ClientData);
                challenge = Some(value);
            }
            b"origin" => {
                require!(!origin_seen, WebAuthnError::ClientData);
                let (value, escaped) = parse_json_string(client_data_json, &mut cursor)?;
                require!(
                    !escaped && value == DEXTER_WEBAUTHN_ORIGIN,
                    WebAuthnError::ClientData
                );
                origin_seen = true;
            }
            b"crossOrigin" => {
                require!(
                    !cross_origin_seen
                        && consume_json_literal(client_data_json, &mut cursor, b"false"),
                    WebAuthnError::ClientData
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
        WebAuthnError::ClientData
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
        WebAuthnError::ClientData
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
                    .ok_or_else(|| error!(WebAuthnError::ClientData))?;
                match escape {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {
                        *cursor += 1;
                    }
                    b'u' => {
                        *cursor += 1;
                        for _ in 0..4 {
                            let hex = *input
                                .get(*cursor)
                                .ok_or_else(|| error!(WebAuthnError::ClientData))?;
                            require!(
                                hex.is_ascii_hexdigit(),
                                WebAuthnError::ClientData
                            );
                            *cursor += 1;
                        }
                    }
                    _ => return err!(WebAuthnError::ClientData),
                }
            }
            0x00..=0x1f => return err!(WebAuthnError::ClientData),
            _ => *cursor += 1,
        }
    }

    err!(WebAuthnError::ClientData)
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
        WebAuthnError::ClientData
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
                WebAuthnError::ClientData
            );
        }
        Some(b'f') => {
            require!(
                consume_json_literal(input, cursor, b"false"),
                WebAuthnError::ClientData
            );
        }
        Some(b'n') => {
            require!(
                consume_json_literal(input, cursor, b"null"),
                WebAuthnError::ClientData
            );
        }
        Some(b'-' | b'0'..=b'9') => skip_json_number(input, cursor)?,
        _ => return err!(WebAuthnError::ClientData),
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
        _ => return err!(WebAuthnError::ClientData),
    }

    if consume_json_byte(input, cursor, b'.') {
        let fraction_start = *cursor;
        while matches!(input.get(*cursor), Some(b'0'..=b'9')) {
            *cursor += 1;
        }
        require!(
            *cursor > fraction_start,
            WebAuthnError::ClientData
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
            WebAuthnError::ClientData
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
            _ => return err!(WebAuthnError::ClientData),
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
        WebAuthnError::ClientData
    );
    Ok(out)
}

