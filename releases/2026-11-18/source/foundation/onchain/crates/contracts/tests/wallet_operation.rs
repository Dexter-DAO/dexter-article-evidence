//! Schema-2 economic/route separation and authenticated token-effect records.
use anchor_lang::prelude::*;
use dexter_contracts::{CORE_ID, financing::custody_reserve_address, funded::receipt_address,
    spending::*, wallet_operation::*};
use solana_sha256_hasher::hashv;
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
    WalletSpendRequest{request_id:[5;32],kind:if swap{SPENDING_SWAP}else{SPENDING_SEND},
        route_profile:if swap{ROUTE_JUPITER_V2}else{ROUTE_NONE},principal:key(1),binding:key(6),
        wallet_authority:wallet,source_custody:custody_reserve_address(&source),source,
        input_mint:mint,input_token_program:TOKEN_2022_ID,destination,destination_authority:owner,
        output_mint:output,output_token_program:output_program,
        output_custody:if swap{custody_reserve_address(&destination)}else{Pubkey::default()},
        adapter_program:if swap{JUPITER_ID}else{Pubkey::default()},adapter_pool:Pubkey::default(),
        amount:100,minimum_output:90,maximum_input_fee:10,maximum_output_fee:0,authority_generation:2}
}
fn receipt(swap:bool)->WalletSpendReceiptData{
    let request=request(swap);let digest=wallet_request_digest(&request).unwrap();
    let operation=wallet_operation_id(&request.principal,&request.request_id);
    let quote=if swap{WalletRouteQuote{quoted_input:100,quoted_output:99,minimum_net_output:92,
        expires_at:1,payload_digest:[7;32]}}else{WalletRouteQuote::default()};
    let mut point=[0x66;32];point[0]=0x58;
    let mut data=WalletSpendReceiptData{version:2,request:request.clone(),request_digest:digest,
        executor:Pubkey::new_from_array(point),permission:key(8),amount_out:95,input_fee:5,output_fee:0,
        gross_output:if swap{95}else{100},quote:quote.clone(),quote_digest:wallet_quote_digest(&digest,&quote).unwrap(),
        input_receipt:receipt_address(&request.principal,&wallet_leg_id(&operation,0)),input_effects_hash:[9;32],
        output_receipt:if swap{receipt_address(&request.principal,&wallet_leg_id(&operation,1))}else{Pubkey::default()},
        output_effects_hash:if swap{[10;32]}else{[0;32]},receipt_hash:[0;32]};
    data.receipt_hash=wallet_receipt_hash(&data).unwrap();data
}
fn bytes(data:&WalletSpendReceiptData)->Vec<u8>{
    let mut raw=hashv(&[b"account:WalletSpendReceipt"]).to_bytes()[..8].to_vec();
    data.serialize(&mut raw).unwrap();raw
}
fn read(data:&WalletSpendReceiptData,owner:Pubkey,extra:bool)->bool{
    let address=wallet_operation_address(&data.request.principal,&data.request.request_id);
    let mut raw=bytes(data);if extra{raw.push(0);}
    let mut lamports=1;
    let info=AccountInfo::new(&address,false,false,&mut lamports,&mut raw,&owner,false);
    read_wallet_spend_receipt(&info).is_ok()
}
#[test]
fn refreshing_quote_changes_execution_commitment_but_not_economic_identity(){
    let r=request(true);r.validate().unwrap();
    let digest=wallet_request_digest(&r).unwrap();let id=wallet_operation_id(&r.principal,&r.request_id);
    let a=WalletRouteQuote{quoted_input:100,quoted_output:99,minimum_net_output:92,expires_at:100,payload_digest:[1;32]};
    let mut b=a.clone();b.quoted_output=105;b.minimum_net_output=96;b.payload_digest=[2;32];b.expires_at=120;
    a.validate_economics(&r).unwrap();b.validate_economics(&r).unwrap();
    assert_ne!(wallet_quote_digest(&digest,&a).unwrap(),wallet_quote_digest(&digest,&b).unwrap());
    assert_eq!(wallet_request_digest(&r).unwrap(),digest);
    assert_eq!(wallet_operation_id(&r.principal,&r.request_id),id);
    let mut changed=r.clone();changed.minimum_output=89;
    assert_ne!(wallet_request_digest(&changed).unwrap(),digest);
    assert_eq!(wallet_operation_id(&changed.principal,&changed.request_id),id);
    b.minimum_net_output=89;assert!(b.validate_economics(&r).is_err());
    assert_eq!(WalletSpendRequest::INIT_SPACE,522);
    assert_eq!(SpendingPermissionTerms::INIT_SPACE,514);
    assert_eq!(8+WalletSpendReceiptData::INIT_SPACE,916);
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
#[test]
fn result_reader_authenticates_current_schema_fees_and_historical_quote(){
    for swap in [false,true]{
        let data=receipt(swap);assert!(read(&data,CORE_ID,false)); // expiry=1 remains historical success
        assert_eq!(bytes(&data).len(),916);
        assert!(!read(&data,key(99),false));assert!(!read(&data,CORE_ID,true));
        for mutation in 0..6{
            let mut changed=data.clone();
            match mutation{
                0=>changed.input_fee=11,
                1=>changed.gross_output+=1,
                2=>changed.output_fee=1,
                3=>changed.amount_out=89,
                4=>changed.version=1,
                _=>changed.quote.payload_digest=[99;32],
            }
            changed.receipt_hash=wallet_receipt_hash(&changed).unwrap();
            assert!(!read(&changed,CORE_ID,false),"mutation {mutation} swap {swap}");
        }
    }
}
