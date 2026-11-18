//! Usable wallet token setup, net funding, explicit surplus adoption and withdrawal.
//! These are owner-authorized cash operations, not admission of an asset as
//! backing for financing promises. Token-2022 custody always uses profile 3.
use anchor_lang::prelude::*;
use anchor_spl::{associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface}};
use dexter_contracts::{plus, minus, CORE_ID, POOL_ID};
use dexter_contracts::funded::{financial_digest, CAPITAL_ALLOCATED, CASH_RECEIVED, CASH_SENT,
    INITIAL_CAPITAL, FINANCE_CONTRIBUTE, FINANCE_ADOPT_SURPLUS, FINANCE_WITHDRAW};
use principal_registry::Principal;
use crate::{CoreError, funding::operation_id, funding_state::*,
    funding_journal::{inc, journal}, wallet_tokens::{self, WALLET_TOKENS}};

pub const TOKEN_LIFECYCLE_VERSION: u16 = 1;
pub const TOKEN_SETUP: u8 = 1;
pub const TOKEN_DEPOSIT: u8 = 2;
pub const TOKEN_ADOPT: u8 = 3;
pub const TOKEN_WITHDRAW: u8 = 4;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct InitializeWalletTokenCustodyArgs {
    pub request_id: [u8; 32],
    pub allocated_cash: u64,
    pub authority_generation: u64,
    pub provenance_hash: [u8; 32],
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct WalletTokenTransferArgs {
    pub request_id: [u8; 32],
    pub gross_amount: u64,
    pub expected_revision: u64,
    pub authority_generation: u64,
    pub expected_fee: u64,
    pub minimum_net: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct AdoptWalletTokenSurplusArgs {
    pub request_id: [u8; 32],
    pub amount: u64,
    pub expected_revision: u64,
    pub authority_generation: u64,
    pub provenance_hash: [u8; 32],
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, InitSpace)]
pub struct WalletTokenTransferReceiptData {
    pub version: u16,
    pub principal: Pubkey,
    pub operation_id: [u8; 32],
    pub request_digest: [u8; 32],
    pub kind: u8,
    pub mint: Pubkey,
    pub token_program: Pubkey,
    pub custody: Pubkey,
    pub source: Pubkey,
    pub destination: Pubkey,
    pub owner: Pubkey,
    pub authority_generation: u64,
    pub gross_debit: u64,
    pub net_credit: u64,
    pub withheld_fee: u64,
    pub fee_epoch: u64,
    /// Explicit allocation. Setup/adoption has zero actual transfer amounts.
    pub capital_allocated: u64,
    pub provenance_hash: [u8; 32],
    pub finance_receipt: Pubkey,
    pub finance_effects_hash: [u8; 32],
    pub receipt_hash: [u8; 32],
}

#[account]
#[derive(InitSpace)]
pub struct WalletTokenTransferReceipt { pub data: WalletTokenTransferReceiptData }

pub fn token_receipt_address(principal: Pubkey, id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[b"wallet-token-operation", principal.as_ref(), id], &CORE_ID).0
}

pub fn token_receipt_hash(data: &WalletTokenTransferReceiptData) -> Result<[u8; 32]> {
    let mut value = data.clone(); value.receipt_hash = [0; 32];
    financial_digest(b"DEXTER:WALLET-TOKEN-RESULT:1", &value)
}

fn recorded(result: &Account<WalletTokenTransferReceipt>, finance: &Account<FinanceReceipt>,
    principal: Pubkey, id: [u8; 32], digest: [u8; 32]) -> Result<bool> {
    if result.data.version == 0 {
        require!(finance.data.version == 0, CoreError::FundingReplay);
        return Ok(false);
    }
    let data = &result.data;
    let financial = dexter_contracts::funded::read_finance_receipt(&finance.to_account_info())?;
    require!(data.version == TOKEN_LIFECYCLE_VERSION && data.principal == principal
        && data.operation_id == id && data.request_digest == digest
        && result.key() == token_receipt_address(principal, &id)
        && data.finance_receipt == finance.key() && data.finance_effects_hash == financial.effects_hash
        && financial.related_receipt == result.key() && financial.request_digest == digest
        && data.receipt_hash == token_receipt_hash(data)?, CoreError::FundingReplay);
    Ok(true)
}

fn finish(result: &mut Account<WalletTokenTransferReceipt>, finance: &Account<FinanceReceipt>,
    mut data: WalletTokenTransferReceiptData) -> Result<()> {
    data.version = TOKEN_LIFECYCLE_VERSION;
    data.finance_receipt = finance.key(); data.finance_effects_hash = finance.data.effects_hash;
    data.receipt_hash = token_receipt_hash(&data)?; result.data = data; Ok(())
}

#[derive(Accounts)]
#[instruction(args: InitializeWalletTokenCustodyArgs)]
pub struct InitializeWalletTokenCustody<'info> {
    #[account(mut)] pub owner: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Strict Registry enrollment and complete current Swig graph below.
    pub binding: UncheckedAccount<'info>,
    /// CHECK: Registry authenticates this retained Swig configuration.
    pub wallet_config: UncheckedAccount<'info>,
    /// CHECK: Wallet identity authenticated by Registry; creation grants no signer.
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
            &operation_id(b"wallet-token-setup", principal.key(), &args.request_id)], bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    #[account(init_if_needed, payer = owner, space = 8 + WalletTokenTransferReceipt::INIT_SPACE,
        seeds = [b"wallet-token-operation", principal.key().as_ref(),
            &operation_id(b"wallet-token-setup", principal.key(), &args.request_id)], bump)]
    pub token_receipt: Box<Account<'info, WalletTokenTransferReceipt>>,
    /// CHECK: New cash allocation uses complete current books and projections.
    #[account(address = POOL_ID, executable)] pub pool_program: UncheckedAccount<'info>,
}

pub fn initialize<'info>(ctx: Context<'info, InitializeWalletTokenCustody<'info>>,
    args: InitializeWalletTokenCustodyArgs) -> Result<()> {
    let a = ctx.accounts; let principal = a.principal.key();
    let id = operation_id(b"wallet-token-setup", principal, &args.request_id);
    let digest = financial_digest(b"DEXTER:WALLET-TOKEN-SETUP:1", &(principal, a.owner.key(),
        a.binding.key(), a.wallet_authority.key(), a.custody_token.key(), a.mint.key(),
        a.token_program.key(), args.request_id, args.allocated_cash,
        args.authority_generation, args.provenance_hash))?;
    if recorded(&a.token_receipt, &a.receipt, principal, id, digest)? { return Ok(()); }
    owner(&a.principal, a.owner.key(), args.authority_generation)?;
    require!(a.custody.version == 0, CoreError::FundingCustody);
    let binding = principal_registry::validate_funding_wallet(&a.binding, &a.wallet_config, &a.wallet_authority)?;
    require!(binding.principal == principal && binding.wallet == a.wallet_authority.key(),
        CoreError::FundingWalletAuthority);
    let mint = wallet_tokens::read_mint(&a.mint.to_account_info(), a.token_program.key(), Clock::get()?.epoch)?;
    let snapshot = wallet_tokens::read_account(&a.custody_token.to_account_info(), &mint)?;
    require!(snapshot.owner == a.wallet_authority.key() && a.custody_token.key() == mint.ata(&snapshot.owner),
        CoreError::FundingTokenRouting);
    require!(args.allocated_cash <= snapshot.amount
        && (args.allocated_cash == 0 || args.provenance_hash != [0; 32]), CoreError::FundingAmount);
    a.custody.set_inner(FinancierCustody { version: dexter_contracts::funded::FUNDED_VERSION,
        bump: ctx.bumps.custody, domain: a.principal.domain, principal,
        binding: a.binding.key(), wallet: a.wallet_authority.key(), token_account: a.custody_token.key(),
        mint: a.mint.key(), profile: WALLET_TOKENS, accounted_cash: args.allocated_cash, unused: 0, payable: 0,
        contributed_capital: args.allocated_cash, withdrawn_cash: 0, revision: 1 });
    let mut financial = record_base(principal, id, digest, INITIAL_CAPITAL,
        a.mint.key(), Pubkey::default(), Pubkey::default(), args.allocated_cash);
    financial.related_receipt = a.token_receipt.key(); financial.capital_custody = a.custody.key();
    financial.capital_allocated = args.allocated_cash; financial.capital_authority = a.owner.key();
    financial.capital_owner_generation = args.authority_generation;
    financial.capital_provenance_hash = args.provenance_hash;
    journal(&mut a.receipt, financial, &[inc(principal, CAPITAL_ALLOCATED, args.allocated_cash)],
        ctx.remaining_accounts, &a.pool_program, false, true)?;
    finish(&mut a.token_receipt, &a.receipt, WalletTokenTransferReceiptData {
        principal, operation_id: id, request_digest: digest, kind: TOKEN_SETUP,
        mint: a.mint.key(), token_program: a.token_program.key(), custody: a.custody.key(),
        destination: a.custody_token.key(), owner: a.owner.key(),
        authority_generation: args.authority_generation, capital_allocated: args.allocated_cash,
        provenance_hash: args.provenance_hash, ..WalletTokenTransferReceiptData::default() })
}

#[derive(Accounts)]
#[instruction(args: WalletTokenTransferArgs)]
pub struct DepositWalletTokens<'info> {
    pub owner: Signer<'info>,
    #[account(mut)] pub payer: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub custody: Box<Account<'info, FinancierCustody>>,
    #[account(mut)] pub from: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)] pub custody_token: Box<InterfaceAccount<'info, TokenAccount>>,
    pub mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(init_if_needed, payer = payer, space = 8 + FinanceReceipt::INIT_SPACE,
        seeds = [b"finance-operation", principal.key().as_ref(),
            &operation_id(b"wallet-token-deposit", principal.key(), &args.request_id)], bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    #[account(init_if_needed, payer = payer, space = 8 + WalletTokenTransferReceipt::INIT_SPACE,
        seeds = [b"wallet-token-operation", principal.key().as_ref(),
            &operation_id(b"wallet-token-deposit", principal.key(), &args.request_id)], bump)]
    pub token_receipt: Box<Account<'info, WalletTokenTransferReceipt>>,
    /// CHECK: Exact Pool; current complete books/projections are required.
    #[account(address = POOL_ID, executable)] pub pool_program: UncheckedAccount<'info>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn deposit<'info>(ctx: Context<'info, DepositWalletTokens<'info>>, args: WalletTokenTransferArgs) -> Result<()> {
    let a = ctx.accounts; let principal = a.principal.key();
    let id = operation_id(b"wallet-token-deposit", principal, &args.request_id);
    let digest = financial_digest(b"DEXTER:WALLET-TOKEN-DEPOSIT:1", &(principal, a.owner.key(),
        a.custody.key(), a.from.key(), a.custody_token.key(), a.mint.key(), a.token_program.key(),
        args.request_id, args.gross_amount, args.authority_generation, args.expected_fee, args.minimum_net))?;
    if recorded(&a.token_receipt, &a.receipt, principal, id, digest)? { return Ok(()); }
    owner(&a.principal, a.owner.key(), args.authority_generation)?;
    require!(a.owner.key().is_on_curve() && args.gross_amount > 0 && args.minimum_net > 0,
        CoreError::FundingAmount);
    require!(a.custody.principal == principal && a.custody.domain == a.principal.domain
        && a.custody.revision == args.expected_revision, CoreError::FundingRevision);
    let epoch = Clock::get()?.epoch;
    let mint = wallet_tokens::read_mint(&a.mint.to_account_info(), a.token_program.key(), epoch)?;
    wallet_tokens::custody_snapshot(&a.custody, &a.custody_token.to_account_info(), &mint)?;
    let from = wallet_tokens::read_account(&a.from.to_account_info(), &mint)?;
    require!(from.owner == a.owner.key() && a.from.key() == mint.ata(&a.owner.key()), CoreError::FundingTokenRouting);
    require!(mint.net(args.gross_amount)? >= args.minimum_net, CoreError::FundingAmount);
    let effects = wallet_tokens::transfer_checked(&a.from.to_account_info(), &a.custody_token.to_account_info(),
        &a.owner.to_account_info(), &a.mint.to_account_info(), &a.token_program.to_account_info(),
        args.gross_amount, args.expected_fee, &[])?;
    a.custody_token.reload()?; a.from.reload()?;
    a.custody.credit(effects.net_credit)?;
    a.custody.contributed_capital = plus(a.custody.contributed_capital, effects.net_credit)?;
    a.custody.check(a.custody_token.amount)?;
    let mut financial = record_base(principal, id, digest, FINANCE_CONTRIBUTE,
        a.mint.key(), Pubkey::default(), Pubkey::default(), effects.net_credit);
    financial.related_receipt = a.token_receipt.key(); financial.capital_custody = a.custody.key();
    financial.capital_allocated = effects.net_credit; financial.capital_token_inflow = effects.net_credit;
    financial.capital_authority = a.owner.key(); financial.capital_from = a.from.key();
    financial.capital_owner_generation = args.authority_generation; financial.capital_provenance_hash = digest;
    journal(&mut a.receipt, financial, &[inc(principal, CAPITAL_ALLOCATED, effects.net_credit),
        inc(principal, CASH_RECEIVED, effects.net_credit)], ctx.remaining_accounts, &a.pool_program, false, true)?;
    finish(&mut a.token_receipt, &a.receipt, WalletTokenTransferReceiptData {
        principal, operation_id: id, request_digest: digest, kind: TOKEN_DEPOSIT,
        mint: a.mint.key(), token_program: a.token_program.key(), custody: a.custody.key(),
        source: a.from.key(), destination: a.custody_token.key(), owner: a.owner.key(),
        authority_generation: args.authority_generation, gross_debit: effects.gross_debit,
        net_credit: effects.net_credit, withheld_fee: effects.withheld_fee, fee_epoch: epoch,
        capital_allocated: effects.net_credit, provenance_hash: digest, ..WalletTokenTransferReceiptData::default() })
}

#[derive(Accounts)]
#[instruction(args: AdoptWalletTokenSurplusArgs)]
pub struct AdoptWalletTokenSurplus<'info> {
    pub owner: Signer<'info>,
    #[account(mut)] pub payer: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut)] pub custody: Box<Account<'info, FinancierCustody>>,
    pub custody_token: Box<InterfaceAccount<'info, TokenAccount>>,
    pub mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(init_if_needed, payer = payer, space = 8 + FinanceReceipt::INIT_SPACE,
        seeds = [b"finance-operation", principal.key().as_ref(),
            &operation_id(b"wallet-token-adopt", principal.key(), &args.request_id)], bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    #[account(init_if_needed, payer = payer, space = 8 + WalletTokenTransferReceipt::INIT_SPACE,
        seeds = [b"wallet-token-operation", principal.key().as_ref(),
            &operation_id(b"wallet-token-adopt", principal.key(), &args.request_id)], bump)]
    pub token_receipt: Box<Account<'info, WalletTokenTransferReceipt>>,
    /// CHECK: Current complete books/projections are required for allocation.
    #[account(address = POOL_ID, executable)] pub pool_program: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

pub fn adopt<'info>(ctx: Context<'info, AdoptWalletTokenSurplus<'info>>, args: AdoptWalletTokenSurplusArgs) -> Result<()> {
    let a = ctx.accounts; let principal = a.principal.key();
    let program = *a.mint.to_account_info().owner;
    let id = operation_id(b"wallet-token-adopt", principal, &args.request_id);
    let digest = financial_digest(b"DEXTER:WALLET-TOKEN-ADOPT:1", &(principal, a.owner.key(),
        a.custody.key(), a.custody_token.key(), a.mint.key(), program, args.request_id,
        args.amount, args.authority_generation, args.provenance_hash))?;
    if recorded(&a.token_receipt, &a.receipt, principal, id, digest)? { return Ok(()); }
    owner(&a.principal, a.owner.key(), args.authority_generation)?;
    require!(args.amount > 0 && args.provenance_hash != [0; 32], CoreError::FundingAmount);
    require!(a.custody.principal == principal && a.custody.domain == a.principal.domain
        && a.custody.revision == args.expected_revision, CoreError::FundingRevision);
    let mint = wallet_tokens::read_mint(&a.mint.to_account_info(), program, Clock::get()?.epoch)?;
    let snapshot = wallet_tokens::custody_snapshot(&a.custody, &a.custody_token.to_account_info(), &mint)?;
    require!(args.amount <= minus(snapshot.amount, a.custody.accounted_cash)?, CoreError::FundingAmount);
    a.custody.credit(args.amount)?;
    a.custody.contributed_capital = plus(a.custody.contributed_capital, args.amount)?;
    a.custody.check(snapshot.amount)?;
    let mut financial = record_base(principal, id, digest, FINANCE_ADOPT_SURPLUS,
        a.mint.key(), Pubkey::default(), Pubkey::default(), args.amount);
    financial.related_receipt = a.token_receipt.key(); financial.capital_custody = a.custody.key();
    financial.capital_allocated = args.amount; financial.capital_authority = a.owner.key();
    financial.capital_owner_generation = args.authority_generation;
    financial.capital_provenance_hash = args.provenance_hash;
    journal(&mut a.receipt, financial, &[inc(principal, CAPITAL_ALLOCATED, args.amount)],
        ctx.remaining_accounts, &a.pool_program, false, true)?;
    finish(&mut a.token_receipt, &a.receipt, WalletTokenTransferReceiptData {
        principal, operation_id: id, request_digest: digest, kind: TOKEN_ADOPT,
        mint: a.mint.key(), token_program: program, custody: a.custody.key(),
        destination: a.custody_token.key(), owner: a.owner.key(),
        authority_generation: args.authority_generation, capital_allocated: args.amount,
        provenance_hash: args.provenance_hash, ..WalletTokenTransferReceiptData::default() })
}

#[derive(Accounts)]
#[instruction(args: WalletTokenTransferArgs)]
pub struct WithdrawWalletTokens<'info> {
    #[account(mut)] pub payer: Signer<'info>,
    pub principal: Box<Account<'info, Principal>>,
    /// CHECK: Stable Registry binding and the Swig-elevated signer below.
    pub binding: UncheckedAccount<'info>,
    #[account(mut)] pub custody: Box<Account<'info, FinancierCustody>>,
    #[account(mut)] pub source: Box<InterfaceAccount<'info, TokenAccount>>,
    pub wallet_authority: Signer<'info>,
    #[account(mut)] pub destination: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(init_if_needed, payer = payer, space = 8 + FinanceReceipt::INIT_SPACE,
        seeds = [b"finance-operation", principal.key().as_ref(),
            &operation_id(b"wallet-token-withdraw", principal.key(), &args.request_id)], bump)]
    pub receipt: Box<Account<'info, FinanceReceipt>>,
    pub mint: Box<InterfaceAccount<'info, Mint>>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    /// CHECK: Complete current accounting is required for this new wallet path.
    #[account(address = POOL_ID, executable)] pub pool_program: UncheckedAccount<'info>,
    #[account(init_if_needed, payer = payer, space = 8 + WalletTokenTransferReceipt::INIT_SPACE,
        seeds = [b"wallet-token-operation", principal.key().as_ref(),
            &operation_id(b"wallet-token-withdraw", principal.key(), &args.request_id)], bump)]
    pub token_receipt: Box<Account<'info, WalletTokenTransferReceipt>>,
}

pub fn withdraw<'info>(ctx: Context<'info, WithdrawWalletTokens<'info>>, args: WalletTokenTransferArgs) -> Result<()> {
    let a = ctx.accounts; let principal = a.principal.key();
    let id = operation_id(b"wallet-token-withdraw", principal, &args.request_id);
    let digest = financial_digest(b"DEXTER:WALLET-TOKEN-WITHDRAW:1", &(principal, a.payer.key(),
        a.custody.key(), a.source.key(), a.destination.key(), a.mint.key(), a.token_program.key(),
        args.request_id, args.gross_amount, args.authority_generation, args.expected_fee, args.minimum_net))?;
    if recorded(&a.token_receipt, &a.receipt, principal, id, digest)? { return Ok(()); }
    owner(&a.principal, a.payer.key(), args.authority_generation)?;
    require!(args.gross_amount > 0 && args.minimum_net > 0, CoreError::FundingAmount);
    require!(a.custody.principal == principal && a.custody.domain == a.principal.domain
        && a.custody.revision == args.expected_revision, CoreError::FundingRevision);
    wallet_tokens::validate_wallet_authority(&a.custody, &a.binding, &a.wallet_authority)?;
    let epoch = Clock::get()?.epoch;
    let mint = wallet_tokens::read_mint(&a.mint.to_account_info(), a.token_program.key(), epoch)?;
    let source = wallet_tokens::custody_snapshot(&a.custody, &a.source.to_account_info(), &mint)?;
    let destination = wallet_tokens::read_account(&a.destination.to_account_info(), &mint)?;
    require!(destination.owner == a.principal.owner && a.destination.key() == mint.ata(&a.principal.owner),
        CoreError::FundingTokenRouting);
    require!(mint.net(args.gross_amount)? >= args.minimum_net, CoreError::FundingAmount);
    a.custody.debit_uncommitted(source.amount, args.gross_amount)?;
    let effects = wallet_tokens::transfer_checked(&a.source.to_account_info(), &a.destination.to_account_info(),
        &a.wallet_authority.to_account_info(), &a.mint.to_account_info(), &a.token_program.to_account_info(),
        args.gross_amount, args.expected_fee, &[])?;
    a.source.reload()?; a.destination.reload()?; a.custody.check(a.source.amount)?;
    let mut financial = record_base(principal, id, digest, FINANCE_WITHDRAW,
        a.mint.key(), Pubkey::default(), Pubkey::default(), effects.gross_debit);
    financial.related_receipt = a.token_receipt.key(); financial.debit_principal = principal;
    financial.debit_authority = a.wallet_authority.key(); financial.debit_from = a.source.key();
    journal(&mut a.receipt, financial, &[inc(principal, CASH_SENT, effects.gross_debit)],
        ctx.remaining_accounts, &a.pool_program, false, false)?;
    finish(&mut a.token_receipt, &a.receipt, WalletTokenTransferReceiptData {
        principal, operation_id: id, request_digest: digest, kind: TOKEN_WITHDRAW,
        mint: a.mint.key(), token_program: a.token_program.key(), custody: a.custody.key(),
        source: a.source.key(), destination: a.destination.key(), owner: a.payer.key(),
        authority_generation: args.authority_generation, gross_debit: effects.gross_debit,
        net_credit: effects.net_credit, withheld_fee: effects.withheld_fee, fee_epoch: epoch,
        ..WalletTokenTransferReceiptData::default() })
}
