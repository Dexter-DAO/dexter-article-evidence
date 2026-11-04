//! Immutable wallet-spending identity and the two mint-specific financial legs.
//! Sends and swaps are explicit operations, separate from debt repayment.
use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;
use crate::{ContractError, CORE_ID};
use crate::financing::custody_reserve_address;
use crate::funded::{financial_digest, receipt_address};
use crate::spending::{SPENDING_SEND, SPENDING_SWAP};

pub const WALLET_SPEND_VERSION: u16 = 1;
pub const FINANCE_WALLET_SEND: u8 = 24;
pub const FINANCE_WALLET_SWAP_INPUT: u8 = 25;
pub const FINANCE_WALLET_SWAP_OUTPUT: u8 = 26;
pub const SPL_TOKEN_SWAP_ID: Pubkey = pubkey!("SwapsVeCiPHMUAtzQWZw7RjsKjgCjhwU55QGu4U1Szw");
const TOKEN_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ATA_ID: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct WalletSpendRequest {
    pub request_id: [u8; 32],
    pub kind: u8,
    pub principal: Pubkey,
    pub binding: Pubkey,
    pub wallet_authority: Pubkey,
    pub source_custody: Pubkey,
    pub source: Pubkey,
    pub input_mint: Pubkey,
    pub destination: Pubkey,
    pub destination_authority: Pubkey,
    pub output_mint: Pubkey,
    pub output_custody: Pubkey,
    pub adapter_program: Pubkey,
    pub adapter_pool: Pubkey,
    pub amount: u64,
    pub minimum_output: u64,
    pub authority_generation: u64,
}

impl WalletSpendRequest {
    pub fn validate(&self) -> Result<()> {
        let zero=Pubkey::default();
        require!(self.principal!=zero && self.binding!=zero && self.wallet_authority!=zero
            && !self.wallet_authority.is_on_curve() && self.input_mint!=zero
            && self.destination_authority!=zero && self.amount>0 && self.authority_generation>0,
            ContractError::WrongAccountType);
        let ata=|authority:&Pubkey,mint:&Pubkey| Pubkey::find_program_address(
            &[authority.as_ref(),TOKEN_ID.as_ref(),mint.as_ref()],&ATA_ID).0;
        require!(self.source==ata(&self.wallet_authority,&self.input_mint)
            && self.source_custody==custody_reserve_address(&self.source)
            && self.destination==ata(&self.destination_authority,&self.output_mint)
            && self.source!=self.destination,ContractError::WrongAccountType);
        match self.kind {
            SPENDING_SEND => require!(self.output_mint==self.input_mint && self.output_custody==zero
                && self.adapter_program==zero && self.adapter_pool==zero && self.minimum_output==0,
                ContractError::WrongAccountType),
            SPENDING_SWAP => require!(self.output_mint!=self.input_mint && self.output_mint!=zero
                && self.destination_authority==self.wallet_authority
                && self.output_custody==custody_reserve_address(&self.destination)
                && self.adapter_program==SPL_TOKEN_SWAP_ID && self.adapter_pool!=zero
                && self.minimum_output>0,ContractError::WrongAccountType),
            _ => return err!(ContractError::WrongAccountType),
        }
        Ok(())
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct WalletSpendReceiptData {
    pub version: u16,
    pub request: WalletSpendRequest,
    pub request_digest: [u8; 32],
    pub executor: Pubkey,
    pub permission: Pubkey,
    pub amount_out: u64,
    pub input_receipt: Pubkey,
    pub input_effects_hash: [u8; 32],
    pub output_receipt: Pubkey,
    pub output_effects_hash: [u8; 32],
    pub receipt_hash: [u8; 32],
}

pub fn wallet_operation_id(principal: &Pubkey, request_id: &[u8;32]) -> [u8;32] {
    hashv(&[b"DEXTER:WALLET-SPEND-ID:1",CORE_ID.as_ref(),principal.as_ref(),request_id]).to_bytes()
}
pub fn wallet_operation_address(principal: &Pubkey, request_id: &[u8;32]) -> Pubkey {
    Pubkey::find_program_address(&[b"wallet-operation",principal.as_ref(),
        &wallet_operation_id(principal,request_id)],&CORE_ID).0
}
pub fn wallet_leg_id(operation: &[u8;32], leg: u8) -> [u8;32] {
    hashv(&[b"DEXTER:WALLET-SPEND-LEG-ID:1",CORE_ID.as_ref(),operation,&[leg]]).to_bytes()
}
pub fn wallet_request_digest(request: &WalletSpendRequest) -> Result<[u8;32]> {
    financial_digest(b"DEXTER:WALLET-SPEND:1",request)
}
pub fn wallet_leg_digest(request_digest: &[u8;32], leg: u8) -> Result<[u8;32]> {
    financial_digest(b"DEXTER:WALLET-SPEND-LEG:1",&(request_digest,leg))
}
pub fn wallet_receipt_hash(data: &WalletSpendReceiptData) -> Result<[u8;32]> {
    let mut value=data.clone();value.receipt_hash=[0;32];
    financial_digest(b"DEXTER:WALLET-SPEND-RESULT:1",&value)
}
pub fn read_wallet_spend_receipt(info: &AccountInfo) -> Result<WalletSpendReceiptData> {
    require_keys_eq!(*info.owner,CORE_ID,ContractError::WrongOwner);
    require!(!info.executable && info.data_len()==8+WalletSpendReceiptData::INIT_SPACE,
        ContractError::WrongAccountType);
    let raw=info.try_borrow_data()?;
    let discriminator=hashv(&[b"account:WalletSpendReceipt"]).to_bytes();
    require!(raw[..8]==discriminator[..8],ContractError::WrongAccountType);
    let data=WalletSpendReceiptData::try_from_slice(&raw[8..])
        .map_err(|_|error!(ContractError::WrongAccountType))?;
    require!(data.version==WALLET_SPEND_VERSION,ContractError::SchemaVersion);
    data.request.validate()?;
    require_keys_eq!(info.key(),wallet_operation_address(&data.request.principal,&data.request.request_id),
        ContractError::WrongAccountType);
    let operation=wallet_operation_id(&data.request.principal,&data.request.request_id);
    require!(data.request_digest==wallet_request_digest(&data.request)?
        && data.receipt_hash==wallet_receipt_hash(&data)? && data.executor.is_on_curve()
        && data.permission!=Pubkey::default() && data.input_effects_hash!=[0;32]
        && data.input_receipt==receipt_address(&data.request.principal,&wallet_leg_id(&operation,0)),
        ContractError::EffectMismatch);
    if data.request.kind==SPENDING_SEND {
        require!(data.amount_out==data.request.amount && data.output_receipt==Pubkey::default()
            && data.output_effects_hash==[0;32],ContractError::EffectMismatch);
    } else {
        require!(data.amount_out>=data.request.minimum_output && data.output_effects_hash!=[0;32]
            && data.output_receipt==receipt_address(&data.request.principal,&wallet_leg_id(&operation,1)),
            ContractError::EffectMismatch);
    }
    Ok(data)
}
