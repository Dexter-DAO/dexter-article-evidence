//! Immutable economic identity, refreshable routing and observed token effects.
//! Quote/attempt replacement never changes an economic operation's identity.
use anchor_lang::prelude::*;
use solana_sha256_hasher::hashv;
use crate::{ContractError, CORE_ID};
use crate::financing::custody_reserve_address;
use crate::funded::{financial_digest, financial_digest_fixed, receipt_address};
use crate::spending::{supported_token_program, SPENDING_SEND, SPENDING_SWAP,
    ROUTE_NONE, ROUTE_SPL_TOKEN_SWAP, ROUTE_JUPITER_V2};

use crate::trading::{policy_route, route_program};

pub const WALLET_SPEND_VERSION: u16 = 2;
pub const FINANCE_WALLET_SEND: u8 = 24;
pub const FINANCE_WALLET_SWAP_INPUT: u8 = 25;
pub const FINANCE_WALLET_SWAP_OUTPUT: u8 = 26;
pub const SPL_TOKEN_SWAP_ID: Pubkey = pubkey!("SwapsVeCiPHMUAtzQWZw7RjsKjgCjhwU55QGu4U1Szw");
pub const JUPITER_ID: Pubkey = pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");
pub const TOKEN_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const TOKEN_2022_ID: Pubkey = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const ATA_ID: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// Owner-approved economics. `amount` is the exact gross source debit and
/// `minimum_output` is spendable NET receipt, excluding Token-2022 withheld fees.
/// A refreshed Jupiter route is deliberately not part of this request.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct WalletSpendRequest {
    pub request_id: [u8; 32],
    pub kind: u8,
    pub route_profile: u8,
    pub principal: Pubkey,
    pub binding: Pubkey,
    pub wallet_authority: Pubkey,
    pub source_custody: Pubkey,
    pub source: Pubkey,
    pub input_mint: Pubkey,
    pub input_token_program: Pubkey,
    pub destination: Pubkey,
    pub destination_authority: Pubkey,
    pub output_mint: Pubkey,
    pub output_token_program: Pubkey,
    pub output_custody: Pubkey,
    pub adapter_program: Pubkey,
    pub adapter_pool: Pubkey,
    pub amount: u64,
    pub minimum_output: u64,
    pub maximum_input_fee: u64,
    pub maximum_output_fee: u64,
    pub authority_generation: u64,
}

impl WalletSpendRequest {
    pub fn validate(&self) -> Result<()> {
        let zero = Pubkey::default();
        require!(self.principal != zero && self.binding != zero && self.wallet_authority != zero
            && !self.wallet_authority.is_on_curve() && self.input_mint != zero
            && self.destination_authority != zero && self.amount > 0 && self.minimum_output > 0
            && self.authority_generation > 0 && self.maximum_input_fee <= self.amount
            && supported_token_program(self.input_token_program)
            && supported_token_program(self.output_token_program), ContractError::WrongAccountType);
        let ata = |authority: &Pubkey, mint: &Pubkey, program: &Pubkey| Pubkey::find_program_address(
            &[authority.as_ref(), program.as_ref(), mint.as_ref()], &ATA_ID).0;
        require!(self.source == ata(&self.wallet_authority, &self.input_mint, &self.input_token_program)
            && self.source_custody == custody_reserve_address(&self.source)
            && self.destination == ata(&self.destination_authority, &self.output_mint, &self.output_token_program)
            && self.source != self.destination
            && (self.input_token_program != TOKEN_ID || self.maximum_input_fee == 0)
            && (self.output_token_program != TOKEN_ID || self.maximum_output_fee == 0), ContractError::WrongAccountType);
        match self.kind {
            SPENDING_SEND => require!(self.route_profile == ROUTE_NONE
                && self.output_mint == self.input_mint && self.output_custody == zero
                && self.output_token_program == self.input_token_program
                && self.adapter_program == zero && self.adapter_pool == zero
                && self.minimum_output <= self.amount && self.maximum_output_fee == 0,
                ContractError::WrongAccountType),
            SPENDING_SWAP => {
                require!(self.output_mint != self.input_mint && self.output_mint != zero
                    && self.destination_authority == self.wallet_authority
                    && self.output_custody == custody_reserve_address(&self.destination),
                    ContractError::WrongAccountType);
                match self.route_profile {
                    ROUTE_SPL_TOKEN_SWAP => require!(self.adapter_program == SPL_TOKEN_SWAP_ID
                        && self.adapter_pool != zero && self.input_token_program == TOKEN_ID
                        && self.output_token_program == TOKEN_ID, ContractError::WrongAccountType),
                    ROUTE_JUPITER_V2 => require!(self.adapter_program == JUPITER_ID
                        && self.adapter_pool == zero, ContractError::WrongAccountType),
                    profile if policy_route(profile) => require!(route_program(profile) == Some(self.adapter_program)
                        && self.adapter_pool != zero, ContractError::WrongAccountType),
                    _ => return err!(ContractError::WrongAccountType),
                }
            },
            _ => return err!(ContractError::WrongAccountType),
        }
        Ok(())
    }
}

/// Public quote values are an untrusted proposed route, not an authority grant or
/// a Jupiter attestation. Core authenticates this commitment against actual CPI
/// bytes/accounts, then independently enforces the approved economics and deltas.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug, PartialEq, Eq, InitSpace)]
pub struct WalletRouteQuote {
    pub quoted_input: u64,
    pub quoted_output: u64,
    pub minimum_net_output: u64,
    pub expires_at: i64,
    pub payload_digest: [u8; 32],
}

impl WalletRouteQuote {
    /// Historical reads deliberately do not apply today's clock or authority.
    pub fn validate_economics(&self, request: &WalletSpendRequest) -> Result<()> {
        if request.route_profile != ROUTE_JUPITER_V2 && !policy_route(request.route_profile) {
            require!(*self == Self::default(), ContractError::WrongAccountType);
        } else {
            require!(self.quoted_input == request.amount
                && self.quoted_output >= self.minimum_net_output
                && self.minimum_net_output >= request.minimum_output && self.expires_at > 0
                && self.payload_digest != [0; 32], ContractError::WrongAccountType);
        }
        Ok(())
    }
}

/// Exact ordered CPI roles, after duplicate keys within that CPI receive their
/// canonical privilege union, before transaction-level privilege union. Core's
/// implementation constructs these roles; arbitrary supplied signer bits cannot
/// confer authority. Duplicate route accounts retain their ordered positions.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct WalletRouteAccount {
    pub address: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq, InitSpace)]
pub struct WalletSpendReceiptData {
    pub version: u16,
    pub request: WalletSpendRequest,
    pub request_digest: [u8; 32],
    pub executor: Pubkey,
    pub permission: Pubkey,
    /// Actual spendable credit in the intended destination; withheld is excluded.
    pub amount_out: u64,
    /// Send: recipient withholding. Swap: newly withheld input-mint route fees.
    pub input_fee: u64,
    /// Swap: newly withheld tokens in the intended output custody. Send: zero.
    pub output_fee: u64,
    /// Send: source gross amount. Swap: net destination credit plus output_fee.
    pub gross_output: u64,
    pub quote: WalletRouteQuote,
    pub quote_digest: [u8; 32],
    pub input_receipt: Pubkey,
    pub input_effects_hash: [u8; 32],
    pub output_receipt: Pubkey,
    pub output_effects_hash: [u8; 32],
    pub receipt_hash: [u8; 32],
}

/// Economic namespaces stay stable across wire revisions. An unsupported old
/// result fails its strict reader; changing schema cannot create another debit.
pub fn wallet_operation_id(principal: &Pubkey, request_id: &[u8; 32]) -> [u8; 32] {
    hashv(&[b"DEXTER:WALLET-SPEND-ID:1", CORE_ID.as_ref(), principal.as_ref(), request_id]).to_bytes()
}
pub fn wallet_operation_address(principal: &Pubkey, request_id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[b"wallet-operation", principal.as_ref(),
        &wallet_operation_id(principal, request_id)], &CORE_ID).0
}
pub fn wallet_leg_id(operation: &[u8; 32], leg: u8) -> [u8; 32] {
    hashv(&[b"DEXTER:WALLET-SPEND-LEG-ID:1", CORE_ID.as_ref(), operation, &[leg]]).to_bytes()
}
pub fn wallet_request_digest(request: &WalletSpendRequest) -> Result<[u8; 32]> {
    financial_digest_fixed::<{ WalletSpendRequest::INIT_SPACE }, _>(b"DEXTER:WALLET-SPEND:2", request)
}
pub fn wallet_leg_digest(request_digest: &[u8; 32], leg: u8) -> Result<[u8; 32]> {
    financial_digest_fixed::<33, _>(b"DEXTER:WALLET-SPEND-LEG:2", &(request_digest, leg))
}
pub fn wallet_route_payload_digest(program: &Pubkey, accounts: &[WalletRouteAccount], data: &[u8]) -> Result<[u8; 32]> {
    financial_digest(b"DEXTER:WALLET-ROUTE-PAYLOAD:2", &(program, accounts, data))
}
pub fn wallet_quote_digest(request_digest: &[u8; 32], quote: &WalletRouteQuote) -> Result<[u8; 32]> {
    if *quote == WalletRouteQuote::default() { return Ok([0; 32]); }
    financial_digest_fixed::<{ 32 + WalletRouteQuote::INIT_SPACE }, _>(b"DEXTER:WALLET-ROUTE-QUOTE:2", &(request_digest, quote))
}
#[inline(never)]
pub fn wallet_receipt_hash(data: &WalletSpendReceiptData) -> Result<[u8; 32]> {
    let mut value = data.clone(); value.receipt_hash = [0; 32];
    financial_digest_fixed::<{ WalletSpendReceiptData::INIT_SPACE }, _>(b"DEXTER:WALLET-SPEND-RESULT:2", &value)
}
pub fn read_wallet_spend_receipt(info: &AccountInfo) -> Result<WalletSpendReceiptData> {
    require_keys_eq!(*info.owner, CORE_ID, ContractError::WrongOwner);
    require!(!info.executable && info.data_len() == 8 + WalletSpendReceiptData::INIT_SPACE,
        ContractError::WrongAccountType);
    let raw = info.try_borrow_data()?;
    let discriminator = hashv(&[b"account:WalletSpendReceipt"]).to_bytes();
    require!(raw[..8] == discriminator[..8], ContractError::WrongAccountType);
    let data = WalletSpendReceiptData::try_from_slice(&raw[8..])
        .map_err(|_| error!(ContractError::WrongAccountType))?;
    require!(data.version == WALLET_SPEND_VERSION, ContractError::SchemaVersion);
    data.request.validate()?;
    data.quote.validate_economics(&data.request)?;
    require_keys_eq!(info.key(), wallet_operation_address(&data.request.principal, &data.request.request_id),
        ContractError::WrongAccountType);
    let operation = wallet_operation_id(&data.request.principal, &data.request.request_id);
    require!(data.request_digest == wallet_request_digest(&data.request)?
        && data.receipt_hash == wallet_receipt_hash(&data)? && data.executor.is_on_curve()
        && data.permission != Pubkey::default() && data.input_effects_hash != [0; 32]
        && data.input_receipt == receipt_address(&data.request.principal, &wallet_leg_id(&operation, 0))
        && data.input_fee <= data.request.maximum_input_fee && data.input_fee <= data.request.amount
        && data.output_fee <= data.request.maximum_output_fee
        && data.amount_out >= data.request.minimum_output
        && data.quote_digest == wallet_quote_digest(&data.request_digest, &data.quote)?, ContractError::EffectMismatch);
    if data.request.kind == SPENDING_SEND {
        require!(data.gross_output == data.request.amount && data.output_fee == 0
            && data.amount_out.checked_add(data.input_fee) == Some(data.request.amount)
            && data.output_receipt == Pubkey::default() && data.output_effects_hash == [0; 32],
            ContractError::EffectMismatch);
    } else {
        require!(data.amount_out.checked_add(data.output_fee) == Some(data.gross_output)
            && data.output_effects_hash != [0; 32]
            && data.output_receipt == receipt_address(&data.request.principal, &wallet_leg_id(&operation, 1))
            && ((data.request.route_profile != ROUTE_JUPITER_V2 && !policy_route(data.request.route_profile))
                || data.amount_out >= data.quote.minimum_net_output),
            ContractError::EffectMismatch);
    }
    Ok(data)
}
