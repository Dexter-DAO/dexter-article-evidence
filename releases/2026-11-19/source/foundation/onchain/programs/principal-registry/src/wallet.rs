//! Shared checks for governed Swig custody: the one admitted ProgramExec graph
//! and the canonical encoding of an authorized instruction. Funding wallets
//! (`wallet_funding.rs`), spending and repayment permissions build on these.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::Instruction;
use solana_sha256_hasher::hashv;
use crate::RegistryError;

pub const SWIG_ID: Pubkey = pubkey!("swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB");
const CONFIG_LEN: usize = 152;

pub(crate) fn validate_graph_for(config: &AccountInfo, wallet: &AccountInfo, binding: &Pubkey,
    wallet_id: &[u8; 32], instruction_name: &[u8]) -> Result<[u8; 32]> {
    require_keys_eq!(*config.owner, SWIG_ID, RegistryError::WalletGraph);
    let (expected_config, bump) = Pubkey::find_program_address(&[b"swig", wallet_id], &SWIG_ID);
    let (expected_wallet, wallet_bump) = Pubkey::find_program_address(&[b"swig-wallet-address", config.key.as_ref()], &SWIG_ID);
    require_keys_eq!(*config.key, expected_config, RegistryError::WalletGraph);
    require_keys_eq!(*wallet.key, expected_wallet, RegistryError::WalletGraph);
    require!(wallet.owner == &System::id() && wallet.data_is_empty() && !wallet.executable && !config.executable,
        RegistryError::WalletGraph);
    // Compare the entire admitted graph, including counters, padding, position
    // boundaries and the absence of tail entries. There is no independent owner,
    // manager, session or subaccount route around the Registry endpoint.
    let mut expected = [0u8; CONFIG_LEN];
    expected[0] = 1; expected[1] = bump; expected[2..34].copy_from_slice(wallet_id);
    expected[34..36].copy_from_slice(&1u16.to_le_bytes());
    expected[36..40].copy_from_slice(&1u32.to_le_bytes()); expected[40] = wallet_bump;
    expected[48..50].copy_from_slice(&7u16.to_le_bytes());
    expected[50..52].copy_from_slice(&80u16.to_le_bytes());
    expected[52..54].copy_from_slice(&1u16.to_le_bytes());
    expected[60..64].copy_from_slice(&104u32.to_le_bytes());
    expected[64..96].copy_from_slice(crate::ID.as_ref()); expected[96] = 40;
    expected[104..112].copy_from_slice(&hashv(&[instruction_name]).to_bytes()[..8]);
    expected[112..144].copy_from_slice(binding.as_ref());
    expected[144..146].copy_from_slice(&7u16.to_le_bytes());
    expected[148..152].copy_from_slice(&8u32.to_le_bytes());
    let data = config.try_borrow_data()?;
    require!(data.as_ref() == expected.as_slice(), RegistryError::WalletGraph);
    Ok(hashv(&[&data]).to_bytes())
}

pub(crate) fn canonical_instruction(ix: &Instruction) -> Result<Vec<u8>> {
    require!(ix.accounts.len() <= u16::MAX as usize && ix.data.len() <= u16::MAX as usize, RegistryError::WalletEnvelope);
    let mut raw = Vec::with_capacity(36 + ix.accounts.len() * 33 + ix.data.len());
    raw.extend_from_slice(&(ix.accounts.len() as u16).to_le_bytes());
    for meta in &ix.accounts {
        raw.push(u8::from(meta.is_signer) | (u8::from(meta.is_writable) << 1));
        raw.extend_from_slice(meta.pubkey.as_ref());
    }
    raw.extend_from_slice(ix.program_id.as_ref()); raw.extend_from_slice(&(ix.data.len() as u16).to_le_bytes());
    raw.extend_from_slice(&ix.data); Ok(raw)
}
