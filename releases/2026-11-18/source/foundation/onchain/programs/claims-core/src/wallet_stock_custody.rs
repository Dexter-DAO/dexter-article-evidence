//! Explicit owner approval and canonical wallet-only stock custody.
//!
//! No issuer power is exercised or neutralized. These approvals record accepted
//! controls; they do not qualify the asset as supplier backing. Initial custody
//! allocates zero capital, including when the ATA was legitimately prefunded.
use anchor_lang::prelude::*;
use anchor_spl::{associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface}};
use dexter_contracts::{CORE_ID, POOL_ID};
use dexter_contracts::funded::{financial_digest, CAPITAL_ALLOCATED, INITIAL_CAPITAL,
    FUNDED_VERSION, FinanceBookData, read_finance_book, read_finance_receipt};
use principal_registry::Principal;
use crate::{CoreError, funding::operation_id, funding_state::*,
    funding_journal::{inc, journal},
    wallet_tokens::{self, WALLET_STOCK_TOKENS},
    wallet_token_lifecycle::{WalletTokenTransferReceipt, WalletTokenTransferReceiptData,
        TOKEN_LIFECYCLE_VERSION, token_receipt_address, token_receipt_hash}};

pub const STOCK_APPROVAL_VERSION: u16 = 1;
pub const TOKEN_STOCK_SETUP: u8 = 5;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct WalletStockControlArgs {
    pub request_id: [u8; 32],
    pub authority_generation: u64,
    pub controls_digest: [u8; 32],
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, InitSpace)]
pub struct StockControlApprovalData {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub binding: Pubkey,
    pub custody: Pubkey,
    pub wallet: Pubkey,
    pub mint: Pubkey,
    pub token_program: Pubkey,
    pub owner: Pubkey,
    pub authority_generation: u64,
    pub controls_digest: [u8; 32],
    pub request_id: [u8; 32],
    pub initialized_custody: bool,
    pub approved_at: i64,
    pub receipt_hash: [u8; 32],
}

#[account]
#[derive(InitSpace)]
pub struct StockControlApproval { pub data: StockControlApprovalData }

pub fn stock_approval_address(custody: Pubkey, controls_digest: &[u8; 32], generation: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"stock-control", custody.as_ref(), controls_digest,
        &generation.to_le_bytes()], &CORE_ID)
}

pub fn stock_approval_hash(data: &StockControlApprovalData) -> Result<[u8; 32]> {
    let mut value = data.clone(); value.receipt_hash = [0; 32];
    financial_digest(b"DEXTER:STOCK-CONTROL-APPROVAL:1", &value)
}

/// Historical approval readback does not consult today's issuer controls or
/// owner. A fresh trade separately checks its policy and current generation.
pub fn read_stock_approval(info: &AccountInfo) -> Result<StockControlApprovalData> {
    require_keys_eq!(*info.owner, CORE_ID, CoreError::FundingOwner);
    require!(!info.executable, CoreError::FundingSchema);
    let bytes = info.try_borrow_data()?;
    let approval = StockControlApproval::try_deserialize(&mut &bytes[..])?;
    require!(bytes.len() == 8 + StockControlApproval::INIT_SPACE, CoreError::FundingSchema);
    let data = approval.data;
    let (key, bump) = stock_approval_address(data.custody, &data.controls_digest, data.authority_generation);
    require!(data.version == STOCK_APPROVAL_VERSION && info.key() == key && data.bump == bump
        && data.controls_digest != [0; 32] && data.receipt_hash == stock_approval_hash(&data)?,
        CoreError::FundingReplay);
    Ok(data)
}

#[allow(clippy::too_many_arguments)]
fn check_approval(data: &StockControlApprovalData, principal: Pubkey, binding: Pubkey,
    custody: Pubkey, wallet: Pubkey, mint: Pubkey, token_program: Pubkey, owner_key: Pubkey,
    args: &WalletStockControlArgs, initialized: bool) -> Result<()> {
    require!(data.principal == principal && data.binding == binding && data.custody == custody
        && data.wallet == wallet && data.mint == mint && data.token_program == token_program
        && data.owner == owner_key && data.authority_generation == args.authority_generation
        && data.controls_digest == args.controls_digest && data.request_id == args.request_id
        && data.initialized_custody == initialized, CoreError::FundingReplay);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn set_approval(approval: &mut Account<StockControlApproval>, bump: u8,
    principal: Pubkey, binding: Pubkey, custody: Pubkey, wallet: Pubkey, mint: Pubkey,
    token_program: Pubkey, owner_key: Pubkey, args: &WalletStockControlArgs,
    initialized: bool, now: i64) -> Result<()> {
    require!(approval.data.version == 0 && args.controls_digest != [0; 32], CoreError::FundingReplay);
    let mut data = StockControlApprovalData { version: STOCK_APPROVAL_VERSION, bump,
        principal, binding, custody, wallet, mint, token_program, owner: owner_key,
        authority_generation: args.authority_generation, controls_digest: args.controls_digest,
        request_id: args.request_id, initialized_custody: initialized, approved_at: now,
        receipt_hash: [0; 32] };
    data.receipt_hash = stock_approval_hash(&data)?;
    approval.data = data;
    Ok(())
}

#[derive(Accounts)]
#[instruction(args: WalletStockControlArgs)]
pub struct InitializeWalletStockCustody<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Complete Registry binding and Swig graph authenticated below.
    pub binding: UncheckedAccount<'info>,
    /// CHECK: Authenticated by Registry; this does not grant a new signer.
    pub wallet_config: UncheckedAccount<'info>,
    /// CHECK: Canonical bound wallet authenticated below.
    pub wallet_authority: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = owner, associated_token::mint = mint,
        associated_token::authority = wallet_authority, associated_token::token_program = token_program)]
    pub custody_token: Box<InterfaceAccount<'info, TokenAccount>>,
    pub mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    #[account(init_if_needed, payer = owner, space = 8 + FinancierCustody::INIT_SPACE,
        seeds = [b"custody-reserve", custody_token.key().as_ref()], bump)]
    pub custody: Box<Account<'info, FinancierCustody>>,
    #[account(init_if_needed, payer = owner, space = 8 + FinanceReceipt::INIT_SPACE,
        seeds = [b"finance-operation", principal.key().as_ref(),
            &operation_id(b"wallet-stock-setup", principal.key(), &args.request_id)], bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    #[account(init_if_needed, payer = owner, space = 8 + WalletTokenTransferReceipt::INIT_SPACE,
        seeds = [b"wallet-token-operation", principal.key().as_ref(),
            &operation_id(b"wallet-stock-setup", principal.key(), &args.request_id)], bump)]
    pub token_receipt: Box<Account<'info, WalletTokenTransferReceipt>>,
    #[account(init_if_needed, payer = owner, space = 8 + StockControlApproval::INIT_SPACE,
        seeds = [b"stock-control", custody.key().as_ref(), &args.controls_digest,
            &args.authority_generation.to_le_bytes()], bump)]
    pub stock_control_approval: Box<Account<'info, StockControlApproval>>,
    /// CHECK: New setup requires exact current Core book and Pool projection.
    #[account(address = POOL_ID, executable)] pub pool_program: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = owner, space = 8 + FinanceBook::INIT_SPACE,
        seeds = [b"financial-book", principal.key().as_ref(), mint.key().as_ref()], bump)]
    pub finance_book: Box<Account<'info, FinanceBook>>,
    /// CHECK: Canonical Pool PDA, created/authenticated by Pool after explicit
    /// current owner and issuer-control consent. Historical retry skips the CPI.
    #[account(mut, seeds = [b"financial-projection", finance_book.key().as_ref()],
        bump, seeds::program = POOL_ID)]
    pub projection: UncheckedAccount<'info>,
}

#[inline(never)]
fn prepare_stock_book<'info>(a: &mut InitializeWalletStockCustody<'info>, bump: u8) -> Result<()> {
    if a.finance_book.data.version == 0 {
        a.finance_book.data = FinanceBookData { version: FUNDED_VERSION, bump,
            principal: a.principal.key(), mint: a.mint.key(), ..FinanceBookData::default() };
    } else {
        let book = read_finance_book(&a.finance_book.to_account_info())?;
        require!(book.principal == a.principal.key() && book.mint == a.mint.key(),
            CoreError::FundingBookAccounts);
    }
    // Pool reads the just-initialized Core account through its strict reader.
    // Existing positions/frontier are preserved, never reset to zero.
    a.finance_book.exit(&crate::ID)?;
    pool_issuer::cpi::initialize_financial_projection(CpiContext::new(a.pool_program.key(),
        pool_issuer::cpi::accounts::InitializeFinancialProjection {
            payer: a.owner.to_account_info(), book: a.finance_book.to_account_info(),
            projection: a.projection.to_account_info(), system_program: a.system_program.to_account_info(),
        }))
}

pub fn initialize<'info>(ctx: Context<'info, InitializeWalletStockCustody<'info>>,
    args: WalletStockControlArgs) -> Result<()> {
    let a = ctx.accounts; let principal = a.principal.key();
    require!(ctx.remaining_accounts.is_empty(), CoreError::FundingBookAccounts);
    let id = operation_id(b"wallet-stock-setup", principal, &args.request_id);
    let digest = financial_digest(b"DEXTER:WALLET-STOCK-SETUP:1", &(principal, a.owner.key(),
        a.binding.key(), a.wallet_authority.key(), a.custody_token.key(), a.mint.key(),
        a.token_program.key(), args.request_id, args.authority_generation, args.controls_digest,
        a.stock_control_approval.key()))?;
    if a.token_receipt.data.version != 0 {
        let data = &a.token_receipt.data;
        let financial = read_finance_receipt(&a.receipt.to_account_info())?;
        require!(data.version == TOKEN_LIFECYCLE_VERSION && data.kind == TOKEN_STOCK_SETUP
            && data.principal == principal && data.operation_id == id && data.request_digest == digest
            && a.token_receipt.key() == token_receipt_address(principal, &id)
            && data.finance_receipt == a.receipt.key() && data.finance_effects_hash == financial.effects_hash
            && financial.related_receipt == a.token_receipt.key() && financial.request_digest == digest
            && data.receipt_hash == token_receipt_hash(data)?, CoreError::FundingReplay);
        let approval = read_stock_approval(&a.stock_control_approval.to_account_info())?;
        check_approval(&approval, principal, a.binding.key(), a.custody.key(),
            a.wallet_authority.key(), a.mint.key(), a.token_program.key(), a.owner.key(), &args, true)?;
        return Ok(());
    }
    require!(a.receipt.data.version == 0 && a.stock_control_approval.data.version == 0,
        CoreError::FundingReplay);
    owner(&a.principal, a.owner.key(), args.authority_generation)?;
    require!(args.controls_digest != [0; 32] && a.custody.version == 0, CoreError::FundingCustody);
    let binding = principal_registry::validate_funding_wallet(&a.binding, &a.wallet_config, &a.wallet_authority)?;
    require!(binding.principal == principal && binding.wallet == a.wallet_authority.key(),
        CoreError::FundingWalletAuthority);
    let clock = Clock::get()?;
    let mint = wallet_tokens::read_mint_with_controls(&a.mint.to_account_info(), a.token_program.key(),
        clock.epoch, args.controls_digest)?;
    let snapshot = wallet_tokens::read_account(&a.custody_token.to_account_info(), &mint)?;
    require!(snapshot.owner == a.wallet_authority.key() && a.custody_token.key() == mint.ata(&snapshot.owner),
        CoreError::FundingTokenRouting);
    prepare_stock_book(a, ctx.bumps.finance_book)?;
    a.custody.set_inner(FinancierCustody { version: FUNDED_VERSION, bump: ctx.bumps.custody,
        domain: a.principal.domain, principal, binding: a.binding.key(), wallet: a.wallet_authority.key(),
        token_account: a.custody_token.key(), mint: a.mint.key(), profile: WALLET_STOCK_TOKENS,
        accounted_cash: 0, unused: 0, payable: 0, contributed_capital: 0, withdrawn_cash: 0, revision: 1 });
    let mut financial = record_base(principal, id, digest, INITIAL_CAPITAL,
        a.mint.key(), Pubkey::default(), Pubkey::default(), 0);
    financial.related_receipt = a.token_receipt.key(); financial.capital_custody = a.custody.key();
    financial.capital_authority = a.owner.key(); financial.capital_owner_generation = args.authority_generation;
    financial.capital_provenance_hash = args.controls_digest;
    journal(&mut a.receipt, financial, &[inc(principal, CAPITAL_ALLOCATED, 0)],
        &[a.finance_book.to_account_info(), a.projection.to_account_info()], &a.pool_program, false, true)?;
    // The journal writes through AccountInfo; refresh Anchor's cached account so
    // its exit serialization cannot restore the pre-journal zero frontier.
    a.finance_book.reload()?;
    let mut result = WalletTokenTransferReceiptData { version: TOKEN_LIFECYCLE_VERSION,
        principal, operation_id: id, request_digest: digest, kind: TOKEN_STOCK_SETUP,
        mint: a.mint.key(), token_program: a.token_program.key(), custody: a.custody.key(),
        destination: a.custody_token.key(), owner: a.owner.key(), authority_generation: args.authority_generation,
        provenance_hash: args.controls_digest, finance_receipt: a.receipt.key(),
        finance_effects_hash: a.receipt.data.effects_hash, ..WalletTokenTransferReceiptData::default() };
    result.receipt_hash = token_receipt_hash(&result)?; a.token_receipt.data = result;
    set_approval(&mut a.stock_control_approval, ctx.bumps.stock_control_approval, principal,
        a.binding.key(), a.custody.key(), a.wallet_authority.key(), a.mint.key(), a.token_program.key(),
        a.owner.key(), &args, true, clock.unix_timestamp)
}

#[derive(Accounts)]
#[instruction(args: WalletStockControlArgs)]
pub struct ApproveWalletStockControls<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Complete current binding and Swig graph authenticated below.
    pub binding: UncheckedAccount<'info>,
    /// CHECK: Authenticated by Registry.
    pub wallet_config: UncheckedAccount<'info>,
    /// CHECK: Authenticated by Registry.
    pub wallet_authority: UncheckedAccount<'info>,
    pub custody: Box<Account<'info, FinancierCustody>>,
    pub custody_token: Box<InterfaceAccount<'info, TokenAccount>>,
    pub mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,
    #[account(init_if_needed, payer = owner, space = 8 + StockControlApproval::INIT_SPACE,
        seeds = [b"stock-control", custody.key().as_ref(), &args.controls_digest,
            &args.authority_generation.to_le_bytes()], bump)]
    pub stock_control_approval: Box<Account<'info, StockControlApproval>>,
    pub system_program: Program<'info, System>,
}

pub fn approve<'info>(ctx: Context<'info, ApproveWalletStockControls<'info>>,
    args: WalletStockControlArgs) -> Result<()> {
    let a = ctx.accounts; let principal = a.principal.key();
    require!(ctx.remaining_accounts.is_empty(), CoreError::FundingTokenRouting);
    if a.stock_control_approval.data.version != 0 {
        let data = read_stock_approval(&a.stock_control_approval.to_account_info())?;
        check_approval(&data, principal, a.binding.key(), a.custody.key(), a.wallet_authority.key(),
            a.mint.key(), a.token_program.key(), a.owner.key(), &args, false)?;
        return Ok(());
    }
    owner(&a.principal, a.owner.key(), args.authority_generation)?;
    let bound = principal_registry::validate_funding_wallet(&a.binding, &a.wallet_config, &a.wallet_authority)?;
    require!(bound.principal == principal && bound.wallet == a.wallet_authority.key()
        && a.custody.principal == principal && a.custody.domain == a.principal.domain
        && a.custody.binding == a.binding.key() && a.custody.wallet == a.wallet_authority.key()
        && a.custody.profile == WALLET_STOCK_TOKENS && a.custody.unused == 0 && a.custody.payable == 0,
        CoreError::FundingCustody);
    let clock = Clock::get()?;
    let mint = wallet_tokens::read_mint_with_controls(&a.mint.to_account_info(), a.token_program.key(),
        clock.epoch, args.controls_digest)?;
    wallet_tokens::custody_snapshot(&a.custody, &a.custody_token.to_account_info(), &mint)?;
    set_approval(&mut a.stock_control_approval, ctx.bumps.stock_control_approval, principal,
        a.binding.key(), a.custody.key(), a.wallet_authority.key(), a.mint.key(), a.token_program.key(),
        a.owner.key(), &args, false, clock.unix_timestamp)
}
