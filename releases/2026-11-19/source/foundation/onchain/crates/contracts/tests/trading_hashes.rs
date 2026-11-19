//! Fixed-buffer hashes must preserve the canonical serialized commitments.
//! These are serialization fixtures, including boundary values; semantic route
//! admission remains covered by the policy and signed execution suites.
use anchor_lang::prelude::*;
use dexter_contracts::{
    funded::{financial_digest, financial_digest_fixed},
    trading::{
        trading_policy_digest, WalletTradingDetailsData,
        WalletTradingHop, WalletTradingPolicyTerms, NATIVE_MINT, ROUTE_JUPITER_PUMP,
    },
    wallet_operation::{
        wallet_leg_digest, wallet_quote_digest, wallet_request_digest,
        wallet_route_payload_digest, WalletRouteAccount, WalletRouteQuote,
        WalletSpendRequest, JUPITER_ID, TOKEN_2022_ID, TOKEN_ID,
    },
};

fn key(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

fn serialized_len<T: AnchorSerialize>(value: &T) -> usize {
    let mut bytes = Vec::new();
    value.serialize(&mut bytes).unwrap();
    bytes.len()
}

fn request() -> WalletSpendRequest {
    WalletSpendRequest {
        permission: key(1), number: 0x0a0b_0c0d_0e0f_1011, kind: 2, route_profile: ROUTE_JUPITER_PUMP,
        principal: key(2), binding: key(3), wallet_authority: key(4),
        source_custody: key(5), source: key(6), input_mint: key(7),
        input_token_program: TOKEN_2022_ID, destination: key(8),
        destination_authority: key(4), output_mint: key(9),
        output_token_program: TOKEN_2022_ID, output_custody: key(10),
        adapter_program: JUPITER_ID, adapter_pool: key(11),
        amount: 0x0102_0304_0506_0708, minimum_output: 987_654_321,
        maximum_input_fee: 1_234, maximum_output_fee: 5_678, authority_generation: 42,
    }
}

fn quote() -> WalletRouteQuote {
    let accounts = [
        WalletRouteAccount { address: key(20), is_signer: false, is_writable: true },
        WalletRouteAccount { address: key(21), is_signer: true, is_writable: false },
        WalletRouteAccount { address: key(20), is_signer: false, is_writable: true },
    ];
    let data = [0, 1, 127, 128, 255, 6, 7, 8, 9];
    let payload_digest = wallet_route_payload_digest(&JUPITER_ID, &accounts, &data).unwrap();
    assert_eq!(payload_digest, financial_digest(
        b"DEXTER:WALLET-ROUTE-PAYLOAD:2", &(&JUPITER_ID, &accounts[..], &data[..]),
    ).unwrap());
    WalletRouteQuote {
        quoted_input: request().amount, quoted_output: 1_100_000_000,
        minimum_net_output: 1_000_000_000, expires_at: 1_900_000_001, payload_digest,
    }
}

fn policy() -> WalletTradingPolicyTerms {
    WalletTradingPolicyTerms {
        binding: key(3), authority_generation: 42, route_profile: ROUTE_JUPITER_PUMP,
        input_mint: key(7), input_token_program: TOKEN_2022_ID,
        output_mint: key(9), output_token_program: TOKEN_2022_ID,
        input_control_digest: [40; 32], output_control_digest: [41; 32],
        maximum_venue_fee_bps: 999, intermediate_count: 3,
        intermediate_mints: [NATIVE_MINT, key(42), key(43)],
        intermediate_token_programs: [TOKEN_ID, TOKEN_2022_ID, TOKEN_ID],
    }
}

fn details() -> WalletTradingDetailsData {
    WalletTradingDetailsData {
        version: 1, request_digest: [51; 32], policy: key(52),
        policy_digest: [53; 32], input_control_digest: [54; 32],
        output_control_digest: [55; 32], payload_digest: [56; 32],
        venue_fee_bps_bound: 1_024, hop_count: 4,
        hops: std::array::from_fn(|i| WalletTradingHop {
            program: key(60 + i as u8), market: key(64 + i as u8),
            base_mint: key(68 + i as u8), quote_mint: key(72 + i as u8),
            base_token_program: TOKEN_2022_ID, quote_token_program: TOKEN_ID,
            base_amount: 1_000_000 + i as u64, quote_amount: 2_000_000 + i as u64,
            retained_protocol_fee: 3_000 + i as u64,
            retained_creator_fee: 4_000 + i as u64, buyback_fee: 5_000 + i as u64,
            lp_fee_bps_bound: 6 + i as u16, buy: i % 2 == 0,
        }),
        leg_payload_digests: [[80; 32], [81; 32]],
        leg_amounts: [0x0102_0304_0506_0708, 9_876_543_210],
    }
}

#[test]
fn fixed_digest_requires_exact_capacity_and_preserves_domain_separation() {
    let data = ([0xa5u8; 32], 0xffu8);
    let canonical = financial_digest(b"fixed-boundary", &data).unwrap();
    assert_eq!(financial_digest_fixed::<33, _>(b"fixed-boundary", &data).unwrap(), canonical);
    assert!(financial_digest_fixed::<32, _>(b"fixed-boundary", &data).is_err());
    assert!(financial_digest_fixed::<34, _>(b"fixed-boundary", &data).is_err());
    assert!(financial_digest_fixed::<0, _>(b"fixed-boundary", &data).is_err());
    assert_ne!(financial_digest_fixed::<33, _>(b"another-domain", &data).unwrap(), canonical);

    // No zero padding or truncation may silently change a real commitment.
    let data = request();
    assert!(financial_digest_fixed::<{ WalletSpendRequest::INIT_SPACE - 1 }, _>(
        b"DEXTER:WALLET-SPEND:3", &data).is_err());
    assert!(financial_digest_fixed::<{ WalletSpendRequest::INIT_SPACE + 1 }, _>(
        b"DEXTER:WALLET-SPEND:3", &data).is_err());
    assert_eq!(financial_digest_fixed::<0, _>(b"empty", &()).unwrap(),
        financial_digest(b"empty", &()).unwrap());
}

#[test]
fn request_and_leg_hashes_match_canonical_at_economic_boundaries() {
    let base = request();
    assert_eq!(serialized_len(&base), WalletSpendRequest::INIT_SPACE);
    let baseline = financial_digest(b"DEXTER:WALLET-SPEND:3", &base).unwrap();
    assert_eq!(wallet_request_digest(&base).unwrap(), baseline);
    let changes: &[fn(&mut WalletSpendRequest)] = &[
        |v| v.permission = key(90), |v| v.number ^= 1, |v| v.kind ^= 1, |v| v.route_profile ^= 1,
        |v| v.principal = key(90), |v| v.binding = key(90),
        |v| v.wallet_authority = key(90), |v| v.source_custody = key(90),
        |v| v.source = key(90), |v| v.input_mint = key(90),
        |v| v.input_token_program = TOKEN_ID, |v| v.destination = key(90),
        |v| v.destination_authority = key(90), |v| v.output_mint = key(90),
        |v| v.output_token_program = TOKEN_ID, |v| v.output_custody = key(90),
        |v| v.adapter_program = key(90), |v| v.adapter_pool = key(90),
        |v| v.amount = u64::MAX, |v| v.minimum_output = 0,
        |v| v.maximum_input_fee = u64::MAX, |v| v.maximum_output_fee = 0,
        |v| v.authority_generation = u64::MAX,
    ];
    for (index, change) in changes.iter().enumerate() {
        let mut value = base.clone();
        change(&mut value);
        let actual = wallet_request_digest(&value).unwrap();
        assert_eq!(actual, financial_digest(b"DEXTER:WALLET-SPEND:3", &value).unwrap(), "field {index}");
        assert_ne!(actual, baseline, "field {index}");
    }
    for digest in [[0; 32], baseline, [255; 32]] {
        for leg in [0, 1, 2, 127, 255] {
            assert_eq!(wallet_leg_digest(&digest, leg).unwrap(),
                financial_digest(b"DEXTER:WALLET-SPEND-LEG:2", &(&digest, leg)).unwrap());
        }
        assert_ne!(wallet_leg_digest(&digest, 0).unwrap(), wallet_leg_digest(&digest, 1).unwrap());
    }
}

#[test]
fn quote_hash_preserves_sentinel_and_commits_each_quote_field() {
    let request_digest = wallet_request_digest(&request()).unwrap();
    assert_eq!(wallet_quote_digest(&request_digest, &WalletRouteQuote::default()).unwrap(), [0; 32]);
    let base = quote();
    assert_eq!(serialized_len(&base), WalletRouteQuote::INIT_SPACE);
    let baseline = financial_digest(b"DEXTER:WALLET-ROUTE-QUOTE:2", &(&request_digest, &base)).unwrap();
    assert_eq!(wallet_quote_digest(&request_digest, &base).unwrap(), baseline);
    let changes: &[fn(&mut WalletRouteQuote)] = &[
        |v| v.quoted_input = u64::MAX, |v| v.quoted_output = 0,
        |v| v.minimum_net_output += 1, |v| v.expires_at = i64::MIN,
        |v| v.expires_at = i64::MAX, |v| v.payload_digest[31] ^= 1,
    ];
    for (index, change) in changes.iter().enumerate() {
        let mut value = base.clone();
        change(&mut value);
        let actual = wallet_quote_digest(&request_digest, &value).unwrap();
        assert_eq!(actual, financial_digest(b"DEXTER:WALLET-ROUTE-QUOTE:2", &(&request_digest, &value)).unwrap(), "field {index}");
        assert_ne!(actual, baseline, "field {index}");
    }
    let mut changed_request = request_digest;
    changed_request[31] ^= 1;
    assert_eq!(wallet_quote_digest(&changed_request, &base).unwrap(),
        financial_digest(b"DEXTER:WALLET-ROUTE-QUOTE:2", &(&changed_request, &base)).unwrap());
    assert_ne!(wallet_quote_digest(&changed_request, &base).unwrap(), baseline);
    assert_eq!(wallet_request_digest(&request()).unwrap(), request_digest);
}

#[test]
fn policy_hash_preserves_identity_consent_fees_and_all_intermediates() {
    let principal = key(100);
    let id = [101; 32];
    let owner = key(102);
    let base = policy();
    assert_eq!(serialized_len(&base), WalletTradingPolicyTerms::INIT_SPACE);
    let canonical = |principal: &Pubkey, id: &[u8; 32], owner: &Pubkey, terms: &WalletTradingPolicyTerms| {
        financial_digest(b"DEXTER:WALLET-TRADING-POLICY:1", &(principal, id, owner, terms)).unwrap()
    };
    let baseline = canonical(&principal, &id, &owner, &base);
    assert_eq!(trading_policy_digest(&principal, &id, &owner, &base).unwrap(), baseline);
    let changes: &[fn(&mut WalletTradingPolicyTerms)] = &[
        |v| v.binding = key(110), |v| v.authority_generation = u64::MAX,
        |v| v.route_profile ^= 1, |v| v.input_mint = key(110),
        |v| v.input_token_program = TOKEN_ID, |v| v.output_mint = key(110),
        |v| v.output_token_program = TOKEN_ID, |v| v.input_control_digest[31] ^= 1,
        |v| v.output_control_digest[31] ^= 1, |v| v.maximum_venue_fee_bps = u16::MAX,
        |v| v.intermediate_count = 0,
    ];
    for (index, change) in changes.iter().enumerate() {
        let mut value = base.clone();
        change(&mut value);
        let actual = trading_policy_digest(&principal, &id, &owner, &value).unwrap();
        assert_eq!(actual, canonical(&principal, &id, &owner, &value), "field {index}");
        assert_ne!(actual, baseline, "field {index}");
    }
    for slot in 0..3 {
        for mint in [true, false] {
            let mut value = base.clone();
            if mint { value.intermediate_mints[slot] = key(110); }
            else { value.intermediate_token_programs[slot] = key(111); }
            let actual = trading_policy_digest(&principal, &id, &owner, &value).unwrap();
            assert_eq!(actual, canonical(&principal, &id, &owner, &value));
            assert_ne!(actual, baseline, "intermediate {slot}, mint {mint}");
        }
    }
    for (principal, id, owner) in [
        (key(110), id, owner), (principal, [110; 32], owner), (principal, id, key(110)),
    ] {
        let actual = trading_policy_digest(&principal, &id, &owner, &base).unwrap();
        assert_eq!(actual, canonical(&principal, &id, &owner, &base));
        assert_ne!(actual, baseline);
    }
}

/// Change 5: no wallet result or trading-details record is hashed; the trade's
/// event carries them. The details still serialize at their fixed size.
#[test]
fn trading_details_serialize_at_their_fixed_size() {
    let base = details();
    assert_eq!(serialized_len(&base), WalletTradingDetailsData::INIT_SPACE);
    assert_eq!(serialized_len(&request()), WalletSpendRequest::INIT_SPACE);
}
