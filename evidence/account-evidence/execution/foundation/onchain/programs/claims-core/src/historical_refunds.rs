//! Receipt-bound return routes for ordinary cash and governed wallet payers.
//!
//! The original Core repayment receipt authenticates the payer and source. A
//! refund does not request a new debit, consult today's owner or restore spent
//! repayment permission. Governed returns additionally credit the canonical
//! custody ledger, so returned cash is not treated as unsolicited surplus.
use anchor_lang::prelude::*;
use anchor_spl::{associated_token, token::{self, TokenAccount}};
use dexter_contracts::{CORE_ID, funded::{FinanceReceiptData, FINANCE_REPAY}};
use crate::{CoreError, funding_state::{FinancierCustody, WALLET_CASH}};

fn token_at(info: &AccountInfo, mint: Pubkey, wallet: Pubkey) -> Result<TokenAccount> {
    require!(*info.owner == token::ID && !info.executable, CoreError::FundingTokenRouting);
    let token = TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])?;
    require!(token.mint == mint && token.owner == wallet
        && info.key() == associated_token::get_associated_token_address(&wallet, &mint),
        CoreError::FundingTokenRouting);
    Ok(token)
}

fn governed_custody(info: &AccountInfo, destination: Pubkey, mint: Pubkey,
    principal: Pubkey, wallet: Pubkey) -> Result<FinancierCustody> {
    require!(*info.owner == CORE_ID && info.is_writable && !info.executable
        && info.data_len() == 8 + FinancierCustody::INIT_SPACE, CoreError::FundingCustody);
    let custody = FinancierCustody::try_deserialize(&mut &info.try_borrow_data()?[..])?;
    custody.check_key(info.key())?;
    require!(custody.profile == WALLET_CASH && custody.principal == principal
        && custody.wallet == wallet && !wallet.is_on_curve()
        && custody.token_account == destination && custody.mint == mint
        && custody.binding != Pubkey::default(), CoreError::FundingCorrectionLineage);
    Ok(custody)
}

/// Call only with the result of Core's strict original-repayment receipt reader.
/// The live custody account is uniquely determined by that historical source;
/// its current owner credential and permission state are intentionally irrelevant.
pub fn validate_refund_destination(source: &FinanceReceiptData,
    destination: &AccountInfo, mint: Pubkey,
    refund_custody: Option<&UncheckedAccount>) -> Result<()> {
    require!(source.kind == FINANCE_REPAY && source.mint == mint
        && source.debit_principal != Pubkey::default()
        && source.debit_authority != Pubkey::default()
        && destination.key() == source.debit_from, CoreError::FundingCorrectionLineage);
    let token = token_at(destination, mint, source.debit_authority)?;
    if source.debit_authority.is_on_curve() {
        require!(refund_custody.is_none(), CoreError::FundingCorrectionLineage);
    } else {
        let info = refund_custody.ok_or_else(|| error!(CoreError::FundingCorrectionLineage))?;
        let custody = governed_custody(&info.to_account_info(), destination.key(), mint,
            source.debit_principal, source.debit_authority)?;
        custody.check(token.amount)?;
    }
    Ok(())
}

/// Apply after all exact token transfers to this destination. Reread the ledger
/// so a preceding creditor credit to the same unchecked custody is retained.
/// A caller holding a typed mutable copy of this account must update that copy
/// instead; otherwise Anchor's exit serialization would overwrite this credit.
pub fn credit_refund_custody(info: &UncheckedAccount, destination: &AccountInfo,
    mint: Pubkey, principal: Pubkey, wallet: Pubkey, amount: u64) -> Result<()> {
    require!(amount > 0, CoreError::FundingCorrectionLineage);
    let mut custody = governed_custody(&info.to_account_info(), destination.key(), mint,
        principal, wallet)?;
    let token = token_at(destination, mint, wallet)?;
    custody.credit(amount)?;
    custody.check(token.amount)?;
    custody.try_serialize(&mut &mut info.try_borrow_mut_data()?[..])?;
    Ok(())
}
