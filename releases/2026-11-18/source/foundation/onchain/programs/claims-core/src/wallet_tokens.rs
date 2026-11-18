//! Explicit token-program admission and gross/net wallet transfer accounting.
//!
//! Wallet support grants no financing eligibility. Token-2022 balances use the
//! wallet-only custody profile; withheld transfer fees are never spendable cash.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_spl::{associated_token, token, token_2022::spl_token_2022};
use spl_token_2022::{
    extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions,
        immutable_owner::ImmutableOwner, metadata_pointer::MetadataPointer,
        pausable::PausableAccount,
        transfer_fee::{TransferFee, TransferFeeAmount, TransferFeeConfig},
        transfer_hook::{TransferHook, TransferHookAccount}},
    state::{Account as TokenAccountState, AccountState, Mint as MintState},
};
use dexter_contracts::{minus, plus};
use crate::{CoreError, funding_state::{FinancierCustody, WALLET_CASH, ESCROW_CASH}};

/// Same canonical custody PDA and reserve ledger, without financing admission.
pub const WALLET_TOKENS: u8 = 3;
/// Explicit owner-approved issuer controls; never financing backing.
pub const WALLET_STOCK_TOKENS: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MintSnapshot {
    pub mint: Pubkey,
    pub token_program: Pubkey,
    pub decimals: u8,
    pub epoch_fee: Option<TransferFee>,
    /// A present TransferHook extension is supported only while its program is
    /// None. Every operation rechecks this; later activation grants no authority.
    pub disabled_hook: bool,
    /// Zero only for the strict generic profile. Stock admission is explicit.
    pub controls_digest: [u8; 32],
    pub pausable_account: bool,
}
impl MintSnapshot {
    pub fn fee(&self, gross: u64) -> Result<u64> {
        self.epoch_fee.as_ref().map_or(Ok(0), |fee|
            fee.calculate_fee(gross).ok_or_else(|| error!(CoreError::FundingAmount)))
    }
    pub fn net(&self, gross: u64) -> Result<u64> { minus(gross, self.fee(gross)?) }
    pub fn ata(&self, authority: &Pubkey) -> Pubkey {
        associated_token::get_associated_token_address_with_program_id(
            authority, &self.mint, &self.token_program)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenSnapshot {
    pub owner: Pubkey,
    pub mint: Pubkey,
    /// Spendable base-account amount. The withheld extension is separate.
    pub amount: u64,
    pub withheld: u64,
    pub native_reserve: Option<u64>,
    pub data_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferOutcome {
    pub gross_debit: u64,
    pub net_credit: u64,
    pub withheld_fee: u64,
}

pub fn token_program(program: Pubkey) -> Result<()> {
    require!(program == token::ID || program == spl_token_2022::ID,
        CoreError::FundingTokenRouting);
    Ok(())
}

/// Admit only the explicitly qualified extension combinations. Metadata has
/// no monetary or authority meaning here. Active hooks, permanent
/// delegates, freeze/default state, pausing, confidential balances, nontransferable,
/// interest/scaled amounts and every unknown extension remain unsupported.
#[inline(never)]
pub fn read_mint(info: &AccountInfo, program: Pubkey, epoch: u64) -> Result<MintSnapshot> {
    token_program(program)?;
    require_keys_eq!(*info.owner, program, CoreError::FundingTokenRouting);
    require!(!info.executable, CoreError::FundingTokenRouting);
    let bytes = info.try_borrow_data()?;
    let state = StateWithExtensions::<MintState>::unpack(&bytes)
        .map_err(|_| error!(CoreError::FundingTokenRouting))?;
    let extensions = state.get_extension_types()
        .map_err(|_| error!(CoreError::FundingCashControl))?;
    if program == token::ID {
        require!(bytes.len() == 82 && extensions.is_empty(), CoreError::FundingTokenRouting);
    } else {
        // A freeze authority can remove access without the Principal's consent.
        // Its support requires a separate explicit control-risk profile.
        require!(state.base.freeze_authority.is_none(), CoreError::FundingCashControl);
    }
    let mut epoch_fee = None;
    let mut disabled_hook = false;
    for (index, extension) in extensions.iter().enumerate() {
        require!(!extensions[..index].contains(extension), CoreError::FundingCashControl);
        match extension {
            ExtensionType::TransferFeeConfig => {
                let config = state.get_extension::<TransferFeeConfig>()
                    .map_err(|_| error!(CoreError::FundingCashControl))?;
                let fee = *config.get_epoch_fee(epoch);
                require!(u16::from(fee.transfer_fee_basis_points) <= 10_000,
                    CoreError::FundingCashControl);
                epoch_fee = Some(fee);
            }
            ExtensionType::MetadataPointer => {
                state.get_extension::<MetadataPointer>()
                    .map_err(|_| error!(CoreError::FundingCashControl))?;
            }
            // Variable-length metadata is opaque display information. TLV
            // structure is validated above; no address or permission is read
            // from metadata, and it cannot affect our transfer calculations.
            ExtensionType::TokenMetadata => {}
            ExtensionType::TransferHook => {
                let hook = state.get_extension::<TransferHook>()
                    .map_err(|_| error!(CoreError::FundingCashControl))?;
                require!(Option::<Pubkey>::from(hook.program_id).is_none(),
                    CoreError::FundingCashControl);
                disabled_hook = true;
            }
            _ => return err!(CoreError::FundingCashControl),
        }
    }
    Ok(MintSnapshot { mint: info.key(), token_program: program,
        decimals: state.base.decimals, epoch_fee, disabled_hook,
        controls_digest: [0; 32], pausable_account: false })
}

/// A zero commitment cannot relax generic token policy. A nonzero commitment
/// selects the separately owner-approved, transparent issuer-control profile.
pub fn read_mint_with_controls(info: &AccountInfo, program: Pubkey, epoch: u64,
    expected_digest: [u8; 32]) -> Result<MintSnapshot> {
    if expected_digest == [0; 32] { read_mint(info, program, epoch) }
    else { crate::stock_token_controls::read_mint(info, program, expected_digest) }
}

#[inline(never)]
pub fn read_account(info: &AccountInfo, mint: &MintSnapshot) -> Result<TokenSnapshot> {
    require_keys_eq!(*info.owner, mint.token_program, CoreError::FundingTokenRouting);
    require!(!info.executable, CoreError::FundingTokenRouting);
    let bytes = info.try_borrow_data()?;
    let state = StateWithExtensions::<TokenAccountState>::unpack(&bytes)
        .map_err(|_| error!(CoreError::FundingTokenRouting))?;
    require!(state.base.mint == mint.mint && state.base.state == AccountState::Initialized
        && state.base.delegate.is_none() && state.base.delegated_amount == 0
        && state.base.close_authority.is_none(), CoreError::FundingCashControl);
    let extensions = state.get_extension_types()
        .map_err(|_| error!(CoreError::FundingCashControl))?;
    if mint.token_program == token::ID {
        require!(bytes.len() == 165 && extensions.is_empty(), CoreError::FundingTokenRouting);
    }
    let mut withheld = 0;
    let mut has_fee = false;
    let mut has_hook = false;
    let mut has_pausable = false;
    for (index, extension) in extensions.iter().enumerate() {
        require!(!extensions[..index].contains(extension), CoreError::FundingCashControl);
        match extension {
            ExtensionType::ImmutableOwner => {
                state.get_extension::<ImmutableOwner>()
                    .map_err(|_| error!(CoreError::FundingCashControl))?;
            }
            ExtensionType::TransferFeeAmount => {
                has_fee = true;
                withheld = u64::from(state.get_extension::<TransferFeeAmount>()
                    .map_err(|_| error!(CoreError::FundingCashControl))?.withheld_amount);
            }
            ExtensionType::TransferHookAccount => {
                has_hook = true;
                let hook = state.get_extension::<TransferHookAccount>()
                    .map_err(|_| error!(CoreError::FundingCashControl))?;
                require!(!bool::from(hook.transferring), CoreError::FundingCashControl);
            }
            ExtensionType::PausableAccount => {
                has_pausable = true;
                require!(mint.controls_digest != [0; 32] && mint.pausable_account,
                    CoreError::FundingCashControl);
                state.get_extension::<PausableAccount>()
                    .map_err(|_| error!(CoreError::FundingCashControl))?;
            }
            _ => return err!(CoreError::FundingCashControl),
        }
    }
    require!(has_fee == mint.epoch_fee.is_some() && has_hook == mint.disabled_hook
        && has_pausable == mint.pausable_account,
        CoreError::FundingCashControl);
    Ok(TokenSnapshot { owner: state.base.owner, mint: state.base.mint,
        amount: state.base.amount, withheld,
        native_reserve: state.base.is_native.into(), data_len: bytes.len() })
}

pub fn account_identity(before: &TokenSnapshot, after: &TokenSnapshot) -> Result<()> {
    require!(before.owner == after.owner && before.mint == after.mint
        && before.native_reserve == after.native_reserve && before.data_len == after.data_len,
        CoreError::FundingCashControl);
    Ok(())
}

pub fn validate_wallet_authority(custody: &Account<FinancierCustody>, binding: &AccountInfo,
    authority: &AccountInfo) -> Result<()> {
    custody.check_key(custody.key())?;
    require!(custody.profile == WALLET_CASH || custody.profile == WALLET_TOKENS
        || custody.profile == WALLET_STOCK_TOKENS,
        CoreError::FundingWalletAuthority);
    let bound = principal_registry::read_funding_wallet_binding(binding)?;
    require!(binding.key() == custody.binding && bound.principal == custody.principal
        && bound.wallet == custody.wallet && authority.key() == custody.wallet
        && authority.is_signer && !authority.executable, CoreError::FundingWalletAuthority);
    Ok(())
}

#[inline(never)]
pub fn custody_snapshot(custody: &Account<FinancierCustody>, token: &AccountInfo,
    mint: &MintSnapshot) -> Result<TokenSnapshot> {
    custody.check_key(custody.key())?;
    require!(custody.mint == mint.mint && custody.token_account == token.key()
        && token.key() == mint.ata(&custody.wallet), CoreError::FundingTokenRouting);
    if mint.controls_digest != [0; 32] {
        require!(custody.profile == WALLET_STOCK_TOKENS, CoreError::FundingCustody);
    } else {
        require!(custody.profile == WALLET_TOKENS
            || (mint.token_program == token::ID
                && (custody.profile == WALLET_CASH || custody.profile == ESCROW_CASH)),
            CoreError::FundingCustody);
    }
    let value = read_account(token, mint)?;
    require_keys_eq!(value.owner, custody.wallet, CoreError::FundingTokenRouting);
    if custody.profile == WALLET_TOKENS || custody.profile == WALLET_STOCK_TOKENS {
        require!(custody.unused == 0 && custody.payable == 0, CoreError::FundingCustody);
    }
    custody.check(value.amount)?;
    Ok(value)
}

/// Direct transfer: verify the selected epoch's exact token fee as part of CPI,
/// then verify both spendable balance deltas and the destination withheld delta.
/// Hooks and privileged extensions cannot add unapproved effects to this path.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn transfer_checked<'info>(from: &AccountInfo<'info>, destination: &AccountInfo<'info>,
    authority: &AccountInfo<'info>, mint_info: &AccountInfo<'info>, program: &AccountInfo<'info>,
    gross: u64, expected_fee: u64, seeds: &[&[&[u8]]]) -> Result<TransferOutcome> {
    require!(gross > 0 && program.executable, CoreError::FundingAmount);
    require_keys_neq!(from.key(), destination.key(), CoreError::FundingTokenRouting);
    let mint = read_mint(mint_info, program.key(), Clock::get()?.epoch)?;
    require!(mint.fee(gross)? == expected_fee, CoreError::FundingTokenDelta);
    let net = minus(gross, expected_fee)?;
    let before = read_account(from, &mint)?;
    let dest_before = read_account(destination, &mint)?;
    require_keys_eq!(before.owner, authority.key(), CoreError::FundingTokenRouting);
    let instruction = if mint.epoch_fee.is_some() {
        spl_token_2022::extension::transfer_fee::instruction::transfer_checked_with_fee(
            &program.key(), from.key, mint_info.key, destination.key, authority.key,
            &[], gross, mint.decimals, expected_fee)?
    } else {
        spl_token_2022::instruction::transfer_checked(&program.key(), from.key,
            mint_info.key, destination.key, authority.key, &[], gross, mint.decimals)?
    };
    invoke_signed(&instruction, &[from.clone(), mint_info.clone(), destination.clone(),
        authority.clone(), program.clone()], seeds)?;
    require!(read_mint(mint_info, program.key(), Clock::get()?.epoch)? == mint,
        CoreError::FundingCashControl);
    let after = read_account(from, &mint)?;
    let dest_after = read_account(destination, &mint)?;
    account_identity(&before, &after)?;
    account_identity(&dest_before, &dest_after)?;
    require!(after.amount == minus(before.amount, gross)?
        && dest_after.amount == plus(dest_before.amount, net)?
        && after.withheld == before.withheld
        && dest_after.withheld == plus(dest_before.withheld, expected_fee)?,
        CoreError::FundingTokenDelta);
    Ok(TransferOutcome { gross_debit: gross, net_credit: net, withheld_fee: expected_fee })
}
