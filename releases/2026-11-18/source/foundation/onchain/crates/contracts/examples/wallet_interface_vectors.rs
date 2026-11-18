//! Deterministic schema-2 encoding vectors, not execution or adapter evidence.
//! Run with the pinned host Cargo via scripts/anchor-workspace.py's environment.
use anchor_lang::prelude::*;
use dexter_contracts::{CORE_ID, REGISTRY_ID};
use dexter_contracts::financing::custody_reserve_address;
use dexter_contracts::funded::{book_address, projection_address, receipt_address};
use dexter_contracts::spending::*;
use dexter_contracts::wallet_operation::*;
use solana_sha256_hasher::hashv;

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|v| format!("{v:02x}")).collect() }
fn encoded<T: AnchorSerialize>(value: &T) -> Vec<u8> { let mut bytes=Vec::new(); value.serialize(&mut bytes).unwrap(); bytes }
fn key(n: u8) -> Pubkey { Pubkey::new_from_array([n; 32]) }
fn ata(owner: Pubkey, mint: Pubkey, token: Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[owner.as_ref(), token.as_ref(), mint.as_ref()],
        &pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")).0
}
fn json_string(value: impl std::fmt::Display) -> String { format!("\"{value}\"") }

fn vector(swap: bool) -> String {
    let mut point = [0x66; 32]; point[0] = 0x58;
    let executor = Pubkey::new_from_array(point);
    let wallet = Pubkey::find_program_address(&[b"encoding-vector-wallet"],
        &pubkey!("swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB")).0;
    let principal = key(1); let binding = key(2); let mint = key(3);
    let output_mint = if swap { key(4) } else { mint };
    let output_program = if swap { TOKEN_ID } else { TOKEN_2022_ID };
    let destination_authority = if swap { wallet } else { executor };
    let source = ata(wallet, mint, TOKEN_2022_ID);
    let destination = ata(destination_authority, output_mint, output_program);
    let request = WalletSpendRequest {
        request_id: [if swap { 12 } else { 11 }; 32], kind: if swap { SPENDING_SWAP } else { SPENDING_SEND },
        route_profile: if swap { ROUTE_JUPITER_V2 } else { ROUTE_NONE },
        principal, binding, wallet_authority: wallet, source_custody: custody_reserve_address(&source),
        source, input_mint: mint, input_token_program: TOKEN_2022_ID, destination,
        destination_authority, output_mint, output_token_program: output_program,
        output_custody: if swap { custody_reserve_address(&destination) } else { Pubkey::default() },
        adapter_program: if swap { JUPITER_ID } else { Pubkey::default() }, adapter_pool: Pubkey::default(),
        amount: 100_000, minimum_output: if swap { 90_000 } else { 99_000 },
        maximum_input_fee: 1_000, maximum_output_fee: 0, authority_generation: 7,
    };
    request.validate().unwrap();
    let digest = wallet_request_digest(&request).unwrap();
    let operation_id = wallet_operation_id(&principal, &request.request_id);
    let operation = wallet_operation_address(&principal, &request.request_id);
    let permission_id = [9; 32];
    let (permission, bump) = Pubkey::find_program_address(
        &[b"spending-permission", principal.as_ref(), &permission_id], &REGISTRY_ID);
    let terms = SpendingPermissionTerms {
        binding, kind: request.kind, route_profile: request.route_profile, mint,
        input_token_program: TOKEN_2022_ID, debit_from: source, output_mint,
        output_token_program: output_program, destination, destination_authority,
        adapter_program: request.adapter_program, adapter_pool: request.adapter_pool, executor,
        authority_generation: 7, not_before: 1_000, expires_at: 2_000, minimum_interval: 0,
        per_operation_amount: 100_000, total_amount: 100_000, maximum_operations: 1,
        minimum_output_numerator: if swap { 9 } else { 99 },
        minimum_output_denominator: if swap { 10 } else { 100 },
        maximum_input_fee: 1_000, maximum_output_fee: 0,
        exact_request_digest: digest, exact_receipt: operation, exact_amount: request.amount,
    };
    terms.validate().unwrap();
    let terms_bytes = encoded(&terms);
    let terms_digest = hashv(&[b"DEXTER:SPENDING-PERMISSION:2", REGISTRY_ID.as_ref(), principal.as_ref(),
        &permission_id, executor.as_ref(), &terms_bytes]).to_bytes();
    let mut permission_account = hashv(&[b"account:SpendingPermission"]).to_bytes()[..8].to_vec();
    permission_account.extend_from_slice(&encoded(&(SPENDING_PERMISSION_VERSION, bump, principal,
        permission_id, executor, terms_digest, &terms, true, SpendingConsumption::default())));
    let mut quote = WalletRouteQuote::default();
    let mut payload = Vec::new();
    let mut roles = Vec::new();
    if swap {
        // Exact RouteV2 scalar/route-plan encoding, with synthetic venue accounts.
        // This vector does not assert that those synthetic accounts form a pool.
        payload.extend_from_slice(&[187, 100, 250, 204, 49, 196, 175, 20]);
        payload.extend_from_slice(&100_000u64.to_le_bytes());
        payload.extend_from_slice(&95_000u64.to_le_bytes());
        payload.extend_from_slice(&500u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&[47, 1, 0]); // WhirlpoolSwapV2, a_to_b, no extras
        payload.extend_from_slice(&10_000u16.to_le_bytes());
        payload.extend_from_slice(&[0, 1]);
        let event = Pubkey::find_program_address(&[b"__event_authority"], &JUPITER_ID).0;
        for (address, is_signer, is_writable) in [
            (wallet,true,false),(source,false,true),(destination,false,true),(mint,false,false),
            (output_mint,false,false),(TOKEN_2022_ID,false,false),(TOKEN_ID,false,false),
            (JUPITER_ID,false,false),(event,false,false),(JUPITER_ID,false,false),
            (key(20),false,false),(key(21),false,true),
        ] { roles.push(WalletRouteAccount { address, is_signer, is_writable }); }
        quote = WalletRouteQuote { quoted_input:100_000, quoted_output:95_000,
            minimum_net_output:90_250, expires_at:1_500,
            payload_digest:wallet_route_payload_digest(&JUPITER_ID,&roles,&payload).unwrap() };
    }
    quote.validate_economics(&request).unwrap();
    let input_leg = wallet_leg_id(&operation_id,0);
    let output_leg = wallet_leg_id(&operation_id,1);
    let mut result = WalletSpendReceiptData {
        version:WALLET_SPEND_VERSION, request:request.clone(), request_digest:digest,
        executor, permission, amount_out:if swap {92_000} else {99_000}, input_fee:1_000,
        output_fee:0, gross_output:if swap {92_000} else {100_000}, quote:quote.clone(),
        quote_digest:wallet_quote_digest(&digest,&quote).unwrap(),
        input_receipt:receipt_address(&principal,&input_leg), input_effects_hash:[31;32],
        output_receipt:if swap {receipt_address(&principal,&output_leg)} else {Pubkey::default()},
        output_effects_hash:if swap {[32;32]} else {[0;32]}, receipt_hash:[0;32],
    };
    result.receipt_hash = wallet_receipt_hash(&result).unwrap();
    let mut result_account = hashv(&[b"account:WalletSpendReceipt"]).to_bytes()[..8].to_vec();
    result_account.extend_from_slice(&encoded(&result));
    let mut instruction = hashv(&[if swap {b"global:wallet_swap_jupiter".as_slice()} else {b"global:wallet_send".as_slice()}]).to_bytes()[..8].to_vec();
    if swap {
        instruction.extend_from_slice(&encoded(&(request.request_id,request.amount,request.minimum_output,
            request.maximum_input_fee,request.maximum_output_fee,request.authority_generation,&quote,&payload)));
    } else {
        instruction.extend_from_slice(&encoded(&(request.request_id,request.amount,request.minimum_output,
            request.maximum_input_fee,request.authority_generation,request.destination_authority)));
    }
    let book = book_address(&principal,&mint); let output_book=book_address(&principal,&output_mint);
    let roles_json=roles.iter().map(|a|format!("{{\"address\":\"{}\",\"isSigner\":{},\"isWritable\":{}}}",a.address,a.is_signer,a.is_writable)).collect::<Vec<_>>().join(",");
    let mut fields=vec![
        ("case",json_string(if swap {"jupiter_token2022_input_encoding"} else {"token2022_send_encoding"})),
        ("requestBorsh",json_string(hex(&encoded(&request)))),("requestDigest",json_string(hex(&digest))),
        ("termsBorsh",json_string(hex(&terms_bytes))),("termsDigest",json_string(hex(&terms_digest))),
        ("permissionAccount",json_string(hex(&permission_account))),("permission",json_string(permission)),
        ("operationId",json_string(hex(&operation_id))),("operation",json_string(operation)),
        ("inputLegId",json_string(hex(&input_leg))),("inputLegDigest",json_string(hex(&wallet_leg_digest(&digest,0).unwrap()))),
        ("inputReceipt",json_string(result.input_receipt)),("inputBook",json_string(book)),("inputProjection",json_string(projection_address(&book))),
        ("quoteBorsh",json_string(hex(&encoded(&quote)))),("quoteDigest",json_string(hex(&result.quote_digest))),
        ("routeData",json_string(hex(&payload))),("routeAccounts",format!("[{roles_json}]")),
        ("resultAccount",json_string(hex(&result_account))),("resultHash",json_string(hex(&result.receipt_hash))),
        ("coreInstructionData",json_string(hex(&instruction))),
    ];
    if swap {
        fields.extend([("outputReceipt",json_string(result.output_receipt)),("outputBook",json_string(output_book)),
            ("outputProjection",json_string(projection_address(&output_book)))]);
    }
    format!("{{{}}}",fields.into_iter().map(|(k,v)|format!("\"{k}\":{v}")).collect::<Vec<_>>().join(","))
}

fn main() {
    println!("{{\"schema\":\"dexter-wallet-schema2-encoding-vectors/v1\",\"scope\":\"Deterministic Rust serialization only; synthetic accounts and financial hashes, not execution evidence or usable Jupiter pool\",\"coreProgram\":\"{}\",\"registryProgram\":\"{}\",\"sizes\":{{\"permissionTerms\":{},\"economicRequest\":{},\"routeQuote\":{},\"resultAccount\":{}}},\"vectors\":[{},{}]}}",
        CORE_ID,REGISTRY_ID,SpendingPermissionTerms::INIT_SPACE,WalletSpendRequest::INIT_SPACE,
        WalletRouteQuote::INIT_SPACE,8+WalletSpendReceiptData::INIT_SPACE,vector(false),vector(true));
}
