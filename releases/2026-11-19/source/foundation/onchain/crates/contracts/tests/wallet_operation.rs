//! Economic/route separation of wallet trades. Change 5: a trade names its
//! permission and number; it keeps no result record (its event carries it).
use anchor_lang::prelude::*;
use dexter_contracts::{CORE_ID, financing::custody_reserve_address,
    spending::*, wallet_operation::*};
fn key(n:u8)->Pubkey{Pubkey::new_from_array([n;32])}
fn request(swap:bool)->WalletSpendRequest{
    let wallet=Pubkey::find_program_address(&[b"test-wallet"],&CORE_ID).0;
    let mint=key(2);let output=if swap{key(3)}else{mint};
    let output_program=if swap{TOKEN_ID}else{TOKEN_2022_ID};
    let owner=if swap{wallet}else{key(4)};
    let ata=|authority:Pubkey,mint:Pubkey,token:Pubkey|Pubkey::find_program_address(
        &[authority.as_ref(),token.as_ref(),mint.as_ref()],
        &pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")).0;
    let source=ata(wallet,mint,TOKEN_2022_ID);let destination=ata(owner,output,output_program);
    WalletSpendRequest{permission:key(8),number:5,kind:if swap{SPENDING_SWAP}else{SPENDING_SEND},
        route_profile:if swap{ROUTE_JUPITER_V2}else{ROUTE_NONE},principal:key(1),binding:key(6),
        wallet_authority:wallet,source_custody:custody_reserve_address(&source),source,
        input_mint:mint,input_token_program:TOKEN_2022_ID,destination,destination_authority:owner,
        output_mint:output,output_token_program:output_program,
        output_custody:if swap{custody_reserve_address(&destination)}else{Pubkey::default()},
        adapter_program:if swap{JUPITER_ID}else{Pubkey::default()},adapter_pool:Pubkey::default(),
        amount:100,minimum_output:90,maximum_input_fee:10,maximum_output_fee:0,authority_generation:2}
}
#[test]
fn refreshing_quote_changes_execution_commitment_but_not_economic_identity(){
    let r=request(true);r.validate().unwrap();
    let digest=wallet_request_digest(&r).unwrap();let id=wallet_operation_id(&r.principal,&r.permission,r.number);
    let a=WalletRouteQuote{quoted_input:100,quoted_output:99,minimum_net_output:92,expires_at:100,payload_digest:[1;32]};
    let mut b=a.clone();b.quoted_output=105;b.minimum_net_output=96;b.payload_digest=[2;32];b.expires_at=120;
    a.validate_economics(&r).unwrap();b.validate_economics(&r).unwrap();
    assert_ne!(wallet_quote_digest(&digest,&a).unwrap(),wallet_quote_digest(&digest,&b).unwrap());
    assert_eq!(wallet_request_digest(&r).unwrap(),digest);
    assert_eq!(wallet_operation_id(&r.principal,&r.permission,r.number),id);
    let mut changed=r.clone();changed.minimum_output=89;
    assert_ne!(wallet_request_digest(&changed).unwrap(),digest);
    assert_eq!(wallet_operation_id(&changed.principal,&changed.permission,changed.number),id);
    b.minimum_net_output=89;assert!(b.validate_economics(&r).is_err());
    assert_eq!(WalletSpendRequest::INIT_SPACE,530);
    assert_eq!(SpendingPermissionTerms::INIT_SPACE,442);
}
#[test]
fn a_trade_is_its_permission_and_number(){
    let r=request(false);
    let id=wallet_operation_id(&r.principal,&r.permission,r.number);
    assert_ne!(wallet_operation_id(&r.principal,&r.permission,r.number+1),id);
    assert_ne!(wallet_operation_id(&r.principal,&key(9),r.number),id);
    let mut changed=r.clone();changed.number=6;
    assert_ne!(wallet_request_digest(&changed).unwrap(),wallet_request_digest(&r).unwrap());
    changed=r.clone();changed.permission=key(9);
    assert_ne!(wallet_request_digest(&changed).unwrap(),wallet_request_digest(&r).unwrap());
    changed=r.clone();changed.number=0;assert!(changed.validate().is_err());
    changed=r.clone();changed.permission=Pubkey::default();assert!(changed.validate().is_err());
}
#[test]
fn exact_payload_commits_order_privileges_program_and_instruction_bytes(){
    let accounts=vec![WalletRouteAccount{address:key(1),is_signer:true,is_writable:false},
        WalletRouteAccount{address:key(2),is_signer:false,is_writable:true}];
    let digest=wallet_route_payload_digest(&JUPITER_ID,&accounts,&[1,2,3]).unwrap();
    let mut changed=accounts.clone();changed.swap(0,1);
    assert_ne!(wallet_route_payload_digest(&JUPITER_ID,&changed,&[1,2,3]).unwrap(),digest);
    changed=accounts.clone();changed[1].is_writable=false;
    assert_ne!(wallet_route_payload_digest(&JUPITER_ID,&changed,&[1,2,3]).unwrap(),digest);
    assert_ne!(wallet_route_payload_digest(&key(9),&accounts,&[1,2,3]).unwrap(),digest);
    assert_ne!(wallet_route_payload_digest(&JUPITER_ID,&accounts,&[1,2,4]).unwrap(),digest);
}
