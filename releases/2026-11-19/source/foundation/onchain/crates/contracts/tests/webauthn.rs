//! Pure framing/context tests. The signature bytes are inert fixtures:
//! actual P-256 signature execution is separately qualified by native program
//! tests loading Secp256r1 and the real instructions sysvar.
use anchor_lang::prelude::{Pubkey, Result};
use anchor_lang::error::Error;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use dexter_contracts::webauthn::*;
use solana_sha256_hasher::hashv;

const KEY: [u8; 33] = [
    0x03, 0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63,
    0xa4, 0x40, 0xf2, 0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39,
    0x45, 0xd8, 0x98, 0xc2, 0x96,
];
const CHALLENGE: &[u8] = b"Account:exact-principal:exact-generation:exact-target";

fn encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 { out.push(ALPHABET[((n >> 6) & 63) as usize] as char); }
        if chunk.len() > 2 { out.push(ALPHABET[(n & 63) as usize] as char); }
    }
    out
}

fn client(challenge: &[u8]) -> Vec<u8> {
    format!(r#"{{"type":"webauthn.get","challenge":"{}","origin":"https://dexter.cash","crossOrigin":false}}"#,
        encode(challenge)).into_bytes()
}

fn auth(flags: u8, count: u32) -> Vec<u8> {
    let mut data = DEXTER_WEBAUTHN_RP_ID_HASH.to_vec();
    data.push(flags);
    data.extend_from_slice(&count.to_be_bytes());
    data
}

fn proof(json: &[u8], auth: &[u8]) -> Instruction {
    let length = u16::try_from(auth.len() + 32).unwrap();
    let mut data = vec![1, 0];
    for n in [16, u16::MAX, 80, u16::MAX, 113, length, u16::MAX] {
        data.extend_from_slice(&n.to_le_bytes());
    }
    data.extend_from_slice(&[0x11; 64]); // Framing only; not a valid native proof.
    data.extend_from_slice(&KEY);
    data.extend_from_slice(auth);
    data.extend_from_slice(hashv(&[json]).as_ref());
    Instruction { program_id: SECP256R1_VERIFY_PROGRAM_ID, accounts: vec![], data }
}

fn check(json: &[u8]) -> Result<WebAuthnObservation> {
    verify_webauthn_assertion(&proof(json, &auth(5, 0)), &KEY, json, CHALLENGE)
}

fn code(result: Result<WebAuthnObservation>) -> u32 {
    match result.unwrap_err() {
        Error::AnchorError(error) => error.error_code_number,
        other => panic!("wrong error family: {other:?}"),
    }
}

#[test]
fn canonical_context_returns_authenticated_observations() {
    let json = client(CHALLENGE);
    assert_eq!(DEXTER_WEBAUTHN_RP_ID_HASH, hashv(&[b"dexter.cash"]).to_bytes());
    assert_eq!(verify_webauthn_assertion(&proof(&json, &auth(5, 0x01020304)), &KEY, &json, CHALLENGE).unwrap(),
        WebAuthnObservation { flags: 5, sign_count: 0x01020304 });
}

#[test]
fn synced_passkey_zero_and_decreasing_counters_are_observed_not_replay_gates() {
    let json = client(CHALLENGE);
    for count in [100, 0, 1, 0] {
        let result = verify_webauthn_assertion(&proof(&json, &auth(0x1d, count)), &KEY, &json, CHALLENGE).unwrap();
        assert_eq!(result, WebAuthnObservation { flags: 0x1d, sign_count: count });
    }
}

#[test]
fn client_hash_is_the_hash_of_the_exact_bytes_not_a_reserialized_object() {
    let json = client(CHALLENGE);
    let precompile = proof(&json, &auth(5, 0));
    let mut with_whitespace = json.clone();
    with_whitespace.push(b' ');
    assert_eq!(code(verify_webauthn_assertion(&precompile, &KEY, &with_whitespace, CHALLENGE)), 8001);
    assert!(check(&with_whitespace).is_ok()); // Same values are valid when those exact bytes were signed.
}

#[test]
fn another_action_challenge_is_not_authorization_for_this_action() {
    let json = client(b"Account:different-principal:different-target");
    assert_eq!(code(check(&json)), 8003);
}

#[test]
fn the_proof_key_is_the_exact_current_expected_key() {
    let json = client(CHALLENGE);
    let precompile = proof(&json, &auth(5, 0));
    let mut other_key = KEY;
    other_key[32] ^= 1;
    assert_eq!(code(verify_webauthn_assertion(&precompile, &other_key, &json, CHALLENGE)), 8001);
    let mut invalid_prefix = KEY;
    invalid_prefix[0] = 4;
    assert_eq!(code(verify_webauthn_assertion(&precompile, &invalid_prefix, &json, CHALLENGE)), 8001);
}

#[test]
fn exact_dexter_origin_type_and_non_cross_origin_are_required() {
    let json = String::from_utf8(client(CHALLENGE)).unwrap();
    for invalid in [
        json.replace("https://dexter.cash", "https://merchant.example"),
        json.replace("https://dexter.cash", "https://dexter.cash.attacker.example"),
        json.replace("https://dexter.cash", "https://signin.dexter.cash"),
        json.replace("https://dexter.cash", "http://dexter.cash"),
        json.replace("webauthn.get", "webauthn.create"),
        json.replace(r#""crossOrigin":false"#, r#""crossOrigin":true"#),
        json.replace(r#","crossOrigin":false"#, ""),
    ] { assert_eq!(code(check(invalid.as_bytes())), 8002, "{invalid}"); }
}

#[test]
fn rp_user_presence_and_user_verification_cannot_be_replaced_by_client_fields() {
    let json = client(CHALLENGE);
    let mut wrong_rp = auth(5, 0);
    wrong_rp[0] ^= 1;
    assert_eq!(code(verify_webauthn_assertion(&proof(&json, &wrong_rp), &KEY, &json, CHALLENGE)), 8004);
    for flags in [0, 1, 4, 0x18] {
        assert_eq!(code(verify_webauthn_assertion(&proof(&json, &auth(flags, 0)), &KEY, &json, CHALLENGE)), 8004);
    }
}

#[test]
fn every_security_field_is_unique_even_when_duplicate_values_agree() {
    let json = String::from_utf8(client(CHALLENGE)).unwrap();
    for field in [
        r#""type":"webauthn.get""#.to_owned(),
        format!(r#""challenge":"{}""#, encode(CHALLENGE)),
        r#""origin":"https://dexter.cash""#.to_owned(),
        r#""crossOrigin":false"#.to_owned(),
    ] {
        let duplicate = format!("{},{}{}", &json[..json.len()-1], field, "}");
        assert_eq!(code(check(duplicate.as_bytes())), 8002);
    }
}

#[test]
fn security_fields_must_be_structural_and_unknown_json_stays_bounded() {
    let valid = format!(r#" {{
        "decoy":"\"origin\":\"https://merchant.example\"",
        "extra":{{"records":[null,true,false,-12.5e+3,{{"escaped":"\\\"\u0061"}}]}},
        "challenge":"{}", "crossOrigin":false,
        "origin":"https://dexter.cash", "type":"webauthn.get"
    }} "#, encode(CHALLENGE));
    assert!(check(valid.as_bytes()).is_ok());
    let missing_real_origin = valid.replace(r#""origin":"https://dexter.cash","#, "");
    assert_eq!(code(check(missing_real_origin.as_bytes())), 8002);
    let depth = format!(r#"{{"extra":{}0{},"type":"webauthn.get","challenge":"{}","origin":"https://dexter.cash","crossOrigin":false}}"#,
        "[".repeat(10), "]".repeat(10), encode(CHALLENGE));
    assert_eq!(code(check(depth.as_bytes())), 8002);
}

#[test]
fn escaped_required_keys_or_values_are_not_alternate_security_encodings() {
    let json = String::from_utf8(client(CHALLENGE)).unwrap();
    for escaped in [
        json.replace(r#""origin""#, r#""or\u0069gin""#),
        json.replace("https://dexter.cash", r"https://dexter\u002ecash"),
        json.replace("webauthn.get", r"webauthn\u002eget"),
        json.replace(r#""crossOrigin""#, r#""cross\u004frigin""#),
    ] { assert_eq!(code(check(escaped.as_bytes())), 8002); }
}

#[test]
fn malformed_json_and_trailing_comma_are_rejected() {
    let json = String::from_utf8(client(CHALLENGE)).unwrap();
    for malformed in [
        "{}".to_owned(),
        format!("{},{}", &json[..json.len()-1], "}"),
        format!("{json}false"),
        json.replace(r#""crossOrigin":false"#, r#""crossOrigin":falsehood"#),
        json.replace(r#""crossOrigin":false"#, r#""crossOrigin":false,"extra":[1,]"#),
        json.replace(r#""crossOrigin":false"#, r#""crossOrigin":false,"extra":{"x":1,}"#),
        json.replace(r#""crossOrigin":false"#, r#""crossOrigin":false,"extra":01"#),
        json.replace(r#""crossOrigin":false"#, r#""crossOrigin":false,"extra":1."#),
        json.replace(r#""crossOrigin":false"#, r#""crossOrigin":false,"extra":1e"#),
        json.replace(r#""crossOrigin":false"#, "\"crossOrigin\":false,\"extra\":\"\n\""),
    ] { assert_eq!(code(check(malformed.as_bytes())), 8002, "{malformed}"); }
    let mut invalid_utf8 = json.into_bytes();
    invalid_utf8.insert(1, 0xff);
    assert_eq!(code(check(&invalid_utf8)), 8002);
}

#[test]
fn challenge_base64url_is_unpadded_canonical_and_exact() {
    let json = String::from_utf8(client(CHALLENGE)).unwrap();
    let encoded = encode(CHALLENGE);
    for bad in [format!("{encoded}="), "A".to_owned(), "AB".to_owned(), "AA+".to_owned(), "AA/".to_owned()] {
        let bad_json = json.replace(&encoded, &bad);
        assert_eq!(code(check(bad_json.as_bytes())), 8002);
    }
    // "AA" is a canonical encoding of one zero byte, but it is the wrong action.
    assert_eq!(code(check(json.replace(&encoded, "AA").as_bytes())), 8003);
}

#[test]
fn proof_program_accounts_header_and_signature_count_are_canonical() {
    let json = client(CHALLENGE);
    let original = proof(&json, &auth(5, 0));
    let mut other_program = original.clone();
    other_program.program_id = Pubkey::new_unique();
    assert_eq!(code(verify_webauthn_assertion(&other_program, &KEY, &json, CHALLENGE)), 8000);
    let mut accounts = original.clone();
    accounts.accounts.push(AccountMeta::new_readonly(Pubkey::new_unique(), false));
    assert_eq!(code(verify_webauthn_assertion(&accounts, &KEY, &json, CHALLENGE)), 8000);
    for (offset, value) in [(0, 0), (0, 2), (1, 1)] {
        let mut malformed = original.clone();
        malformed.data[offset] = value;
        assert_eq!(code(verify_webauthn_assertion(&malformed, &KEY, &json, CHALLENGE)), 8001);
    }
}

#[test]
fn cross_instruction_offsets_aliases_and_noncanonical_layouts_are_refused() {
    let json = client(CHALLENGE);
    let original = proof(&json, &auth(5, 0));
    // Each offset/index/size field is changed independently from a valid neighbor.
    for offset in [2, 4, 6, 8, 10, 12, 14] {
        let mut malformed = original.clone();
        malformed.data[offset] ^= 1;
        assert_eq!(code(verify_webauthn_assertion(&malformed, &KEY, &json, CHALLENGE)), 8001, "{offset}");
    }
    let mut trailing = original.clone();
    trailing.data.push(0);
    assert_eq!(code(verify_webauthn_assertion(&trailing, &KEY, &json, CHALLENGE)), 8001);
    let mut truncation = original.clone();
    truncation.data.pop();
    assert_eq!(code(verify_webauthn_assertion(&truncation, &KEY, &json, CHALLENGE)), 8001);
    for length in [0, 1, 15, 112, 181] {
        let mut truncated = original.clone();
        truncated.data.truncate(length);
        assert_eq!(code(verify_webauthn_assertion(&truncated, &KEY, &json, CHALLENGE)), 8001);
    }
}

#[test]
fn supported_input_bounds_are_enforced_before_parsing() {
    let json = client(CHALLENGE);
    let precompile = proof(&json, &auth(5, 0));
    assert_eq!(code(verify_webauthn_assertion(&precompile, &KEY, &[], CHALLENGE)), 8005);
    assert_eq!(code(verify_webauthn_assertion(&precompile, &KEY, &json, &[])), 8005);
    assert_eq!(code(verify_webauthn_assertion(&precompile, &KEY, &vec![b' '; MAX_CLIENT_DATA_JSON_LEN+1], CHALLENGE)), 8005);
    assert_eq!(code(verify_webauthn_assertion(&precompile, &KEY, &json, &vec![0; MAX_CHALLENGE_LEN+1])), 8005);
    let mut oversized_auth = auth(5, 0);
    oversized_auth.resize(MAX_AUTHENTICATOR_DATA_LEN+1, 0);
    assert_eq!(code(verify_webauthn_assertion(&proof(&json, &oversized_auth), &KEY, &json, CHALLENGE)), 8001);
}

#[test]
fn maximum_authenticator_extension_bytes_are_authenticated_without_duplication() {
    let json = client(CHALLENGE);
    let mut extended_auth = auth(5, 17);
    extended_auth.resize(MAX_AUTHENTICATOR_DATA_LEN, 0x42);
    let precompile = proof(&json, &extended_auth);
    assert_eq!(verify_webauthn_assertion(&precompile, &KEY, &json, CHALLENGE).unwrap(),
        WebAuthnObservation { flags: 5, sign_count: 17 });
}

