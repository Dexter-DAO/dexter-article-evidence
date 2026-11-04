//! Optional successful-operation reimbursement. Independent collection has no
//! dependency on this paid service. The accepted quote binds every token leg;
//! a failed financial operation rolls the reimbursement back atomically.
use anchor_lang::prelude::*;
use crate::CoreError;
use anchor_spl::{associated_token, token::{self, Mint, TokenAccount}};
use dexter_contracts::commercial::{SponsorFundingSource, SponsorQuote};
use dexter_contracts::funded::{FinanceReceiptData, CASH_SENT, CASH_RECEIVED,
    SPONSOR_REIMBURSEMENT, SPONSOR_MARGIN};
use principal_registry::Principal;
use crate::funding_state::*;
use crate::funding_journal::{inc, Change};

pub struct SponsorshipAccounts<'a, 'info> {
    pub payer_principal: Option<&'a Account<'info, Principal>>,
    pub payer_owner: Option<&'a Signer<'info>>,
    /// Read fresh SPL bytes after collection; a cached Anchor balance could
    /// predate the supplier credit when this is the same destination account.
    pub from: Option<AccountInfo<'info>>,
    pub destination: Option<AccountInfo<'info>>,
    pub sponsor_custody: Option<&'a mut Account<'info, FinancierCustody>>,
}

pub struct SponsorOperation {
    pub operation_id: [u8; 32],
    pub domain: [u8; 32],
    pub custody_profile: u8,
    pub supplier_principal: Pubkey,
    pub supplier_destination: Pubkey,
    pub supplier_net: u64,
}

pub fn apply_sponsorship<'a, 'info>(quote: Option<&SponsorQuote>, accounts: SponsorshipAccounts<'a, 'info>,
    operation: &SponsorOperation, mint: &Account<'info, Mint>, token_program: Pubkey,
    receipt: &mut FinanceReceiptData, changes: &mut Vec<Change>) -> Result<()> {
    let Some(quote) = quote else {
        require!(accounts.payer_principal.is_none() && accounts.payer_owner.is_none()
            && accounts.from.is_none() && accounts.destination.is_none() && accounts.sponsor_custody.is_none(),
            CoreError::FundingSponsorship);
        return Ok(());
    };
    let payer = accounts.payer_principal.ok_or_else(|| error!(CoreError::FundingSponsorship))?;
    let payer_owner = accounts.payer_owner.ok_or_else(|| error!(CoreError::FundingSponsorship))?;
    let from = accounts.from.ok_or_else(|| error!(CoreError::FundingSponsorship))?;
    let destination = accounts.destination.ok_or_else(|| error!(CoreError::FundingSponsorship))?;
    let sponsor = accounts.sponsor_custody.ok_or_else(|| error!(CoreError::FundingSponsorship))?;
    math(quote.validate_at(Clock::get()?.unix_timestamp))?;
    owner(payer, payer_owner.key(), quote.authority_generation)?;
    require!(quote.payer_principal == payer.key() && quote.payer_authority == payer_owner.key()
        && quote.operation_id == operation.operation_id && quote.asset.mint == mint.key()
        && quote.asset.token_program == token_program && token_program == token::ID
        && quote.from == from.key() && quote.destination == destination.key()
        && payer.domain == operation.domain && payer_owner.key().is_on_curve(), CoreError::FundingSponsorship);
    // A principal paying its own network fee needs no reimbursement quote.
    // Do not fabricate sponsor-margin income from its own internal cash move.
    require!(quote.payer_principal != quote.sponsor_principal, CoreError::FundingSponsorship);
    let total = math(quote.total())?;
    match (operation.custody_profile, quote.funding_source) {
        (WALLET_CASH | ESCROW_CASH, SponsorFundingSource::EarnedSupplierProceeds) => {
            require!(quote.payer_principal == operation.supplier_principal
                && quote.from == operation.supplier_destination && total <= operation.supplier_net,
                CoreError::FundingSponsorship);
        }
        (ESCROW_CASH, SponsorFundingSource::OwnerCash) => (),
        _ => return err!(CoreError::FundingSponsorship),
    }
    require!(from.is_writable && destination.is_writable && !from.executable && !destination.executable,
        CoreError::FundingTokenRouting);
    let source = read_token(&from)?;
    let target = read_token(&destination)?;
    require!(source.mint == mint.key() && source.owner == payer_owner.key()
        && from.key() == associated_token::get_associated_token_address(&payer_owner.key(), &mint.key()),
        CoreError::FundingSponsorship);
    sponsor.check_key(sponsor.key())?;
    require!(sponsor.principal == quote.sponsor_principal && sponsor.domain == operation.domain
        && sponsor.mint == mint.key() && sponsor.token_account == destination.key()
        && target.mint == mint.key() && target.owner == sponsor.wallet
        && destination.key() == associated_token::get_associated_token_address(&sponsor.wallet, &mint.key()),
        CoreError::FundingSponsorship);
    sponsor.check(target.amount)?;
    require!(target.state == token::spl_token::state::AccountState::Initialized
        && target.delegate.is_none() && target.close_authority.is_none(), CoreError::FundingCashControl);
    require!(source.state == token::spl_token::state::AccountState::Initialized, CoreError::FundingTokenRouting);
    transfer_checked_exact(&from, &destination, &payer_owner.to_account_info(), mint, token_program, total, &[])?;
    let after = read_token(&destination)?;
    sponsor.credit(total)?;
    sponsor.check(after.amount)?;

    receipt.sponsor_reimbursement = quote.reimbursement;
    receipt.sponsor_margin = quote.margin;
    receipt.sponsor_quote_hash = math(quote.digest())?;
    receipt.sponsor_payer_principal = payer.key();
    receipt.sponsor_payer_authority = payer_owner.key();
    receipt.sponsor_from = from.key();
    receipt.sponsor_destination = destination.key();
    receipt.sponsor_funding_source = Some(quote.funding_source);
    receipt.sponsor_rent_owner = quote.rent_owner;
    receipt.supplier_proceeds_reimbursement = if quote.funding_source == SponsorFundingSource::EarnedSupplierProceeds {
        total
    } else { 0 };
    // supplier_net is the original collection credit after seller fees, before
    // the independent approved reimbursement debit recorded by these fields.
    changes.push(inc(payer.key(), CASH_SENT, total));
    changes.push(inc(sponsor.principal, CASH_RECEIVED, total));
    changes.push(inc(sponsor.principal, SPONSOR_REIMBURSEMENT, quote.reimbursement));
    changes.push(inc(sponsor.principal, SPONSOR_MARGIN, quote.margin));
    Ok(())
}

fn read_token(info: &AccountInfo) -> Result<TokenAccount> {
    require_keys_eq!(*info.owner, token::ID, CoreError::FundingTokenRouting);
    TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])
}
