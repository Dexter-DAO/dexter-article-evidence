//! Initial governed-custody profile: owner-approved repayment of this
//! principal's existing Core claim. This does not admit generic wallet spending,
//! role management, sessions, passkeys or a wallet-backed guarantee.
use anchor_lang::prelude::*;
use anchor_lang::InstructionData;
use anchor_lang::solana_program::{
    instruction::{get_stack_height, Instruction, TRANSACTION_LEVEL_STACK_HEIGHT},
};
use solana_instructions_sysvar::{load_current_index_checked, load_instruction_at_checked};
use dexter_contracts::{read_core_source, plus, StageArgs, TabTerms, CORE_ID, SCHEMA_VERSION, METERED_CLAIM_VERSION};
use solana_sha256_hasher::hashv;
use crate::{Principal, RegistryError};

pub const SWIG_ID: Pubkey = pubkey!("swigypWHEksbC64pWKwah1WTeh9JXwx8H1rJHLdbQMB");
const INSTRUCTIONS_ID: Pubkey = pubkey!("Sysvar1nstructions1111111111111111111111111");
const COMPUTE_ID: Pubkey = pubkey!("ComputeBudget111111111111111111111111111111");
const TOKEN_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const CONFIG_LEN: usize = 152;

#[account]
#[derive(InitSpace)]
pub struct WalletBinding {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub config: Pubkey,
    pub wallet: Pubkey,
    pub wallet_id: [u8; 32],
    pub generation: u64,
    pub authority_generation: u64,
    pub nonce: u64,
    pub graph_hash: [u8; 32],
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct BindWalletArgs {
    pub generation: u64,
    pub wallet_id: [u8; 32],
    pub authority_generation: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct AuthorizeWalletArgs {
    // Swig's 40-byte ProgramExec prefix is this instruction's 8-byte
    // discriminator followed by the exact binding account's public key.
    pub binding: Pubkey,
    pub nonce: u64,
    pub authority_generation: u64,
    pub expires_at: i64,
    pub operation_id: [u8; 32],
    pub amount: u64,
    pub bundle_hash: [u8; 32],
}

#[derive(Accounts)]
#[instruction(args: BindWalletArgs)]
pub struct BindWallet<'info> {
    /// CHECK: Full pinned profile and canonical Swig PDA checked below.
    pub config: UncheckedAccount<'info>,
    /// CHECK: Canonical empty System-owned Swig wallet PDA checked below.
    pub wallet: UncheckedAccount<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(address = principal.owner @ RegistryError::WalletOwner)]
    pub owner: Signer<'info>,
    #[account(mut)] pub payer: Signer<'info>,
    #[account(init_if_needed, payer = payer, space = 8 + WalletBinding::INIT_SPACE,
        seeds = [b"wallet", principal.key().as_ref(), &args.generation.to_le_bytes()], bump)]
    pub binding: Box<Account<'info, WalletBinding>>,
    /// CHECK: Exact admitted executable. Its upgrade governance is a separate
    /// accepted dependency; this field does not claim executable immutability.
    #[account(address = SWIG_ID, executable)] pub swig_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: AuthorizeWalletArgs)]
pub struct AuthorizeWallet<'info> {
    // ProgramExec requires these exact first two positions.
    /// CHECK: Pinned graph, ownership and canonical PDA validated below.
    pub config: UncheckedAccount<'info>,
    /// CHECK: Canonical Swig custody PDA validated below.
    pub wallet: UncheckedAccount<'info>,
    #[account(mut, has_one = principal, has_one = config, has_one = wallet,
        seeds = [b"wallet", principal.key().as_ref(), &binding.generation.to_le_bytes()], bump = binding.bump)]
    pub binding: Box<Account<'info, WalletBinding>>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(address = principal.owner @ RegistryError::WalletOwner)]
    pub owner: Signer<'info>,
    /// CHECK: Exact runtime instructions sysvar; parsed with checked helpers.
    #[account(address = INSTRUCTIONS_ID)] pub instructions: UncheckedAccount<'info>,
    /// CHECK: Core owner, schema, discriminator, principal and PDA checked below.
    pub claim: UncheckedAccount<'info>,
    /// CHECK: Shared strict Core source reader checks owner, schema and PDA.
    pub source: UncheckedAccount<'info>,
}

pub fn bind(ctx: Context<BindWallet>, args: BindWalletArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    require!(ctx.accounts.principal.version == SCHEMA_VERSION && args.generation > 0
        && args.authority_generation == ctx.accounts.principal.authority_generation,
        RegistryError::WalletGeneration);
    let graph_hash = validate_graph_for(&ctx.accounts.config, &ctx.accounts.wallet,
        &ctx.accounts.binding.key(), &args.wallet_id, b"global:authorize_wallet")?;
    let binding = &mut ctx.accounts.binding;
    if binding.version != 0 {
        require!(binding.version == SCHEMA_VERSION && binding.principal == ctx.accounts.principal.key()
            && binding.config == ctx.accounts.config.key() && binding.wallet == ctx.accounts.wallet.key()
            && binding.wallet_id == args.wallet_id && binding.generation == args.generation
            && binding.authority_generation == args.authority_generation && binding.graph_hash == graph_hash,
            RegistryError::WalletBinding);
        return Ok(()); // Never reset the consumed nonce on a binding retry.
    }
    binding.set_inner(WalletBinding { version: SCHEMA_VERSION, bump: ctx.bumps.binding,
        principal: ctx.accounts.principal.key(), config: ctx.accounts.config.key(),
        wallet: ctx.accounts.wallet.key(), wallet_id: args.wallet_id, generation: args.generation,
        authority_generation: args.authority_generation, nonce: 0, graph_hash });
    Ok(())
}

pub fn authorize(ctx: Context<AuthorizeWallet>, args: AuthorizeWalletArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let binding = &ctx.accounts.binding;
    require!(binding.version == SCHEMA_VERSION && ctx.accounts.principal.version == SCHEMA_VERSION,
        RegistryError::WalletBinding);
    require_keys_eq!(args.binding, binding.key(), RegistryError::WalletBinding);
    // Enrollment generation is provenance. A legitimate current owner retains
    // the continuing principal's wallet after a later authority rotation.
    require!(args.authority_generation == ctx.accounts.principal.authority_generation,
        RegistryError::WalletGeneration);
    require!(args.nonce == binding.nonce, RegistryError::WalletNonce);
    require!(args.expires_at > Clock::get()?.unix_timestamp && args.amount > 0, RegistryError::WalletExpired);
    require!(validate_graph_for(&ctx.accounts.config, &ctx.accounts.wallet, &binding.key(), &binding.wallet_id,
        b"global:authorize_wallet")?
        == binding.graph_hash, RegistryError::WalletGraph);
    let source = read_core_source(&ctx.accounts.source)?;
    let claim = read_claim(&ctx.accounts.claim, ctx.accounts.principal.key())?;
    require_keys_eq!(claim.terms.source, ctx.accounts.source.key(), RegistryError::WalletRepayment);
    require_keys_eq!(claim.terms.asset, source.mint, RegistryError::WalletRepayment);
    require!(claim.terms.domain == ctx.accounts.principal.domain, RegistryError::WalletRepayment);

    let ixsys = ctx.accounts.instructions.to_account_info();
    let current = load_current_index_checked(&ixsys)?;
    require!(current <= 1, RegistryError::WalletEnvelope);
    if current == 1 {
        // The local v0 fixture's one resource instruction is admitted explicitly.
        // V1 carries its resource profile in the transaction header.
        let prefix = load_instruction_at_checked(0, &ixsys)?;
        require!(prefix.program_id == COMPUTE_ID && prefix.accounts.is_empty()
            && prefix.data.len() == 5 && prefix.data[0] == 2, RegistryError::WalletEnvelope);
        let units = u32::from_le_bytes(prefix.data[1..5].try_into().unwrap());
        require!(units > 0 && units <= 1_400_000, RegistryError::WalletEnvelope);
    }
    let own = load_instruction_at_checked(current as usize, &ixsys)?;
    let expected_own = crate::instruction::AuthorizeWallet { args: args.clone() }.data();
    require!(own.program_id == crate::ID && own.data == expected_own,
        RegistryError::WalletEnvelope);
    let next = load_instruction_at_checked(current as usize + 1, &ixsys)?;
    require!(load_instruction_at_checked(current as usize + 2, &ixsys).is_err(), RegistryError::WalletEnvelope);
    validate_repayment(&next, &ctx.accounts, &args, source.treasury, source.mint)?;
    let raw = canonical_instruction(&next)?;
    let hash = hashv(&[b"DEXTER:SWIG_BUNDLE:1", crate::ID.as_ref(), CORE_ID.as_ref(), SWIG_ID.as_ref(),
        &ctx.accounts.principal.domain, ctx.accounts.principal.key().as_ref(), binding.key().as_ref(),
        &args.nonce.to_le_bytes(), &args.authority_generation.to_le_bytes(), &args.expires_at.to_le_bytes(),
        &args.operation_id, &args.amount.to_le_bytes(), &raw]).to_bytes();
    require!(hash == args.bundle_hash, RegistryError::WalletBundleHash);
    ctx.accounts.binding.nonce = plus(args.nonce, 1)?;
    Ok(())
}

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

fn validate_repayment(ix: &Instruction, accounts: &AuthorizeWallet, args: &AuthorizeWalletArgs,
    treasury: Pubkey, mint: Pubkey) -> Result<()> {
    require!(ix.program_id == SWIG_ID && ix.accounts.len() >= 4 && ix.accounts.len() <= 64,
        RegistryError::WalletRepayment);
    let meta = |index: usize| ix.accounts.get(index).ok_or_else(|| error!(RegistryError::WalletRepayment));
    require!(meta(0)?.pubkey == accounts.config.key() && meta(1)?.pubkey == accounts.wallet.key()
        && meta(2)?.pubkey == INSTRUCTIONS_ID && !meta(0)?.is_signer && !meta(1)?.is_signer
        && meta(0)?.is_writable && meta(1)?.is_writable, RegistryError::WalletRepayment);
    let d = &ix.data;
    require!(d.len() >= 12 && d[..2] == 11u16.to_le_bytes() && d[4..8] == [0; 4], RegistryError::WalletRepayment);
    let len = u16::from_le_bytes(d[2..4].try_into().unwrap()) as usize;
    require!(d.len() == 8 + len + 1 && d[8 + len] == 2, RegistryError::WalletRepayment);
    let compact = &d[8..8 + len];
    require!(compact.len() == 75 && compact[0] == 1 && compact[2] == 13, RegistryError::WalletRepayment);
    require!(meta(compact[1] as usize)?.pubkey == CORE_ID, RegistryError::WalletRepayment);
    let mut keys = Vec::with_capacity(13);
    for index in &compact[3..16] { keys.push(meta(*index as usize)?); }
    require!(u16::from_le_bytes(compact[16..18].try_into().unwrap()) == 57, RegistryError::WalletRepayment);
    let inner = &compact[18..];
    require!(inner[..8] == hashv(&[b"global:repay"]).to_bytes()[..8], RegistryError::WalletRepayment);
    let stage = StageArgs::try_from_slice(&inner[8..]).map_err(|_| error!(RegistryError::WalletRepayment))?;
    require!(stage.operation_id == args.operation_id && stage.amount == args.amount, RegistryError::WalletRepayment);
    let receipt_id = dexter_contracts::signed_request_id(accounts.wallet.key(), &stage.operation_id);
    let receipt = Pubkey::find_program_address(&[b"operation", accounts.principal.key().as_ref(), &receipt_id], &CORE_ID).0;
    require!(keys[0].is_signer && keys[0].is_writable && keys[0].pubkey != accounts.wallet.key()
        && keys[1].pubkey == accounts.wallet.key() && keys[2].pubkey == accounts.source.key()
        && keys[3].pubkey == accounts.claim.key() && keys[4].pubkey == receipt && keys[6].pubkey == treasury
        && keys[7].pubkey == mint && keys[11].pubkey == TOKEN_ID && keys[12].pubkey == System::id(),
        RegistryError::WalletRepayment);
    // No extra unused accounts: every meta must be part of the exact Swig
    // control tuple, Core program, or one of Core's thirteen required positions.
    for (index, _) in ix.accounts.iter().enumerate() {
        require!(index < 3 || index == compact[1] as usize || compact[3..16].contains(&(index as u8)),
            RegistryError::WalletRepayment);
    }
    Ok(())
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

#[derive(AnchorDeserialize)]
struct ClaimRead {
    version: u16, bump: u8, terms: TabTerms,
    _remaining: u64, _payable: u64, _debt: u64, _paid: u64, _repaid: u64, _released: u64, _revision: u64,
}

fn read_claim(info: &AccountInfo, principal: Pubkey) -> Result<ClaimRead> {
    require_keys_eq!(*info.owner, CORE_ID, RegistryError::WalletRepayment);
    let data = info.try_borrow_data()?;
    require!(data.len() >= 8 && data[..8] == hashv(&[b"account:Claim"]).to_bytes()[..8], RegistryError::WalletRepayment);
    let value = ClaimRead::try_from_slice(&data[8..]).map_err(|_| error!(RegistryError::WalletRepayment))?;
    let (expected, bump) = Pubkey::find_program_address(&[b"claim", principal.as_ref(), &value.terms.tab_id], &CORE_ID);
    require!(supported_claim_version(value.version) && value.terms.principal == principal
        && *info.key == expected && value.bump == bump, RegistryError::WalletRepayment);
    Ok(value)
}

fn supported_claim_version(version: u16) -> bool {
    matches!(version, SCHEMA_VERSION | METERED_CLAIM_VERSION)
}

#[cfg(test)]
mod claim_versions {
    use super::*;

    #[test]
    fn only_the_two_explicit_claim_versions_are_admitted() {
        assert!(supported_claim_version(SCHEMA_VERSION));
        assert!(supported_claim_version(METERED_CLAIM_VERSION));
        for version in [0, 3, u16::MAX] {
            assert!(!supported_claim_version(version));
        }
    }
}

