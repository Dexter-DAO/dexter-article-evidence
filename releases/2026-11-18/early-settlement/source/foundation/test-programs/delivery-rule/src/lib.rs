//! Test fixture: the smallest program whose address
//! ["delivery-source", source_identity] can sign for an evidence order.
//!
//! Instruction data is `source_identity (32 bytes) || ClaimsCore instruction data`.
//! Accounts are `[ClaimsCore, then that instruction's accounts in order]`. The
//! program calls ClaimsCore with them, signing as its own delivery-source address.
//! It has no rules of its own: any caller can make it sign any report.
use anchor_lang::solana_program::{account_info::AccountInfo, entrypoint, entrypoint::ProgramResult,
    instruction::{AccountMeta, Instruction}, program::invoke_signed, program_error::ProgramError, pubkey::Pubkey};

entrypoint!(process);

fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    if data.len() < 32 || accounts.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }
    let (identity, inner) = data.split_at(32);
    let (source, bump) = Pubkey::find_program_address(&[b"delivery-source", identity], program_id);
    let metas = accounts[1..].iter().map(|account| AccountMeta { pubkey: *account.key,
        is_signer: account.is_signer || *account.key == source, is_writable: account.is_writable }).collect();
    invoke_signed(&Instruction { program_id: *accounts[0].key, accounts: metas, data: inner.to_vec() },
        accounts, &[&[b"delivery-source", identity, &[bump]]])
}
