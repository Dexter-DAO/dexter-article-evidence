//! Canonical cash and LockedClaim-seniority verification.
//!
//! Economic law:
//!   unreserved_cash = max(canonical_usdc_balance - outstanding_locked_amount, 0)
//!
//! Senior LockedClaim settlement may consume the reserved tier. Every junior
//! debit (agent spend, Line repay/seize, cash tab settle, owner withdrawal, and
//! carry BUY input) is limited to `unreserved_cash`.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::program_pack::Pack;
use anchor_spl::token::spl_token::state::{
    Account as SplTokenAccount, AccountState as SplTokenAccountState,
};

use crate::constants::{SWIG_PROGRAM_ID, SWIG_VAULT_BINDING_SEED, SWIG_WALLET_ADDRESS_SEED};
use crate::state::{
    PrincipalNode, SwigVaultBinding, Vault, VaultError, SWIG_VAULT_BINDING_VERSION_ATOMIC_V2,
    SWIG_VAULT_BINDING_VERSION_ATTESTED_V3, SWIG_VAULT_BINDING_VERSION_AUTOMATION_V4,
    SWIG_VAULT_BINDING_VERSION_MINIMAL_V5, SWIG_VAULT_BINDING_VERSION_V1,
};
use crate::verify::swig_transfer::MoneyLeg;

pub fn canonical_swig_wallet(swig: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[SWIG_WALLET_ADDRESS_SEED, swig.as_ref()], &SWIG_PROGRAM_ID).0
}

pub fn canonical_swig_cash_ata(swig: &Pubkey, settlement_mint: &Pubkey) -> Pubkey {
    anchor_spl::associated_token::get_associated_token_address(
        &canonical_swig_wallet(swig),
        settlement_mint,
    )
}

/// Verify the one-to-one Swig→Vault identity and return the canonical Swig
/// wallet PDA. Cash-optional admission paths use this even when the ATA has not
/// been created yet.
pub fn verify_swig_vault_identity(
    program_id: &Pubkey,
    binding_key: &Pubkey,
    binding: &SwigVaultBinding,
    vault_key: &Pubkey,
    vault: &Vault,
    swig_key: &Pubkey,
    swig_wallet_key: &Pubkey,
) -> Result<Pubkey> {
    require!(
        binding.version == SWIG_VAULT_BINDING_VERSION_V1
            || binding.version == SWIG_VAULT_BINDING_VERSION_ATOMIC_V2
            || binding.version == SWIG_VAULT_BINDING_VERSION_ATTESTED_V3
            || binding.version == SWIG_VAULT_BINDING_VERSION_AUTOMATION_V4
            || binding.version == SWIG_VAULT_BINDING_VERSION_MINIMAL_V5,
        VaultError::UnsupportedSwigVaultBindingVersion
    );

    let expected_binding =
        Pubkey::find_program_address(&[SWIG_VAULT_BINDING_SEED, swig_key.as_ref()], program_id).0;
    require_keys_eq!(
        *binding_key,
        expected_binding,
        VaultError::SwigVaultBindingMismatch
    );
    require_keys_eq!(
        binding.swig,
        *swig_key,
        VaultError::SwigVaultBindingMismatch
    );
    require_keys_eq!(
        binding.vault,
        *vault_key,
        VaultError::SwigVaultBindingMismatch
    );
    require_keys_eq!(
        vault.swig_address,
        *swig_key,
        VaultError::SwigVaultBindingMismatch
    );

    let expected_wallet = canonical_swig_wallet(swig_key);
    require_keys_eq!(
        *swig_wallet_key,
        expected_wallet,
        VaultError::SwigVaultBindingMismatch
    );
    Ok(expected_wallet)
}

/// Seller reservations require more than reverse identity: the Swig must have
/// either have been born atomically with Dexter's canonical authority spine or
/// have passed the governed exact-account live-role attestation. An identity-
/// only V1 binding intentionally remains ineligible.
pub fn require_reserve_safe_swig_policy(binding: &SwigVaultBinding) -> Result<()> {
    require!(
        binding.version == SWIG_VAULT_BINDING_VERSION_ATOMIC_V2
            || binding.version == SWIG_VAULT_BINDING_VERSION_ATTESTED_V3
            || binding.version == SWIG_VAULT_BINDING_VERSION_AUTOMATION_V4
            || binding.version == SWIG_VAULT_BINDING_VERSION_MINIMAL_V5,
        VaultError::SwigAuthorityPolicyUnattested
    );
    Ok(())
}

/// Verify the complete Swig→Vault→cash-account identity and return the amount
/// available to junior debits.
#[allow(clippy::too_many_arguments)]
pub fn verify_canonical_reserved_cash(
    program_id: &Pubkey,
    binding_key: &Pubkey,
    binding: &SwigVaultBinding,
    vault_key: &Pubkey,
    vault: &Vault,
    swig_key: &Pubkey,
    swig_wallet_key: &Pubkey,
    cash_account_key: &Pubkey,
    cash_owner: &Pubkey,
    cash_mint: &Pubkey,
    cash_balance: u64,
    settlement_mint: &Pubkey,
) -> Result<u64> {
    let expected_wallet = verify_swig_vault_identity(
        program_id,
        binding_key,
        binding,
        vault_key,
        vault,
        swig_key,
        swig_wallet_key,
    )?;
    let expected_cash = anchor_spl::associated_token::get_associated_token_address(
        &expected_wallet,
        settlement_mint,
    );
    require!(
        *cash_account_key == expected_cash
            && *cash_owner == expected_wallet
            && *cash_mint == *settlement_mint,
        VaultError::NonCanonicalVaultCashAccount
    );

    Ok(cash_balance.saturating_sub(vault.outstanding_locked_amount))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalCashWitness {
    /// The exact canonical ATA address has not been initialized: system-owned,
    /// data-empty, and zero lamports.
    Absent,
    /// A strict classic-token account exists at the canonical ATA address.
    Present { balance: u64, unreserved_cash: u64 },
}

impl CanonicalCashWitness {
    pub fn unreserved_cash(self) -> u64 {
        match self {
            Self::Absent => 0,
            Self::Present {
                unreserved_cash, ..
            } => unreserved_cash,
        }
    }
}

/// Validate a fixed canonical cash witness for cash-optional Vault rails.
///
/// Tab settlement uses this at fixed account #8. Direction-2 asset-only
/// Protected Send reuses it so a Vault holding DEXTER/SPCX is not forced to
/// rent a USDC ATA merely to authorize an unrelated token movement.
/// Cash-consuming Buy/Sell callers reuse the identity/address check but must
/// reject `Absent` before taking their balance snapshot.
///
/// The account key is always the canonical classic-token ATA derived from the
/// Vault's bound Swig wallet and GraphConfig settlement mint. Its state is
/// exactly one of:
///   - system-owned, data-empty, and zero-lamport: genuinely uninitialized; or
///   - owned by classic SPL Token and decodable as an initialized token account
///     whose mint and authority exactly match that derivation.
///
/// No program-id sentinel and no caller-selected substitute can hide a funded
/// canonical ATA. Token-2022 and malformed account data fail closed.
#[allow(clippy::too_many_arguments)]
pub fn verify_canonical_cash_witness(
    program_id: &Pubkey,
    binding_key: &Pubkey,
    binding: &SwigVaultBinding,
    vault_key: &Pubkey,
    vault: &Vault,
    swig_key: &Pubkey,
    swig_wallet_key: &Pubkey,
    cash_account: &AccountInfo,
    settlement_mint: &Pubkey,
) -> Result<CanonicalCashWitness> {
    let expected_wallet = verify_swig_vault_identity(
        program_id,
        binding_key,
        binding,
        vault_key,
        vault,
        swig_key,
        swig_wallet_key,
    )?;
    verify_canonical_cash_witness_state(
        &expected_wallet,
        cash_account,
        settlement_mint,
        vault.outstanding_locked_amount,
    )
}

/// Validate the state at one already-bound Swig wallet's canonical classic
/// settlement ATA address.
///
/// This internal state-only half keeps the exact-address and strict decoding
/// law single-sourced after `verify_canonical_cash_witness` has established the
/// Vault↔Swig identity. It never accepts a caller-selected sentinel.
fn verify_canonical_cash_witness_state(
    swig_wallet_key: &Pubkey,
    cash_account: &AccountInfo,
    settlement_mint: &Pubkey,
    outstanding_locked_amount: u64,
) -> Result<CanonicalCashWitness> {
    let expected_cash = anchor_spl::associated_token::get_associated_token_address(
        swig_wallet_key,
        settlement_mint,
    );
    require_keys_eq!(
        *cash_account.key,
        expected_cash,
        VaultError::NonCanonicalVaultCashAccount
    );
    require!(
        !cash_account.executable,
        VaultError::NonCanonicalVaultCashAccount
    );

    if cash_account.owner == &anchor_lang::system_program::ID
        && cash_account.data_is_empty()
        && cash_account.lamports() == 0
    {
        return Ok(CanonicalCashWitness::Absent);
    }

    require_keys_eq!(
        *cash_account.owner,
        anchor_spl::token::ID,
        VaultError::NonCanonicalVaultCashAccount
    );
    let data = cash_account
        .try_borrow_data()
        .map_err(|_| error!(VaultError::NonCanonicalVaultCashAccount))?;
    let token_account = SplTokenAccount::unpack(&data)
        .map_err(|_| error!(VaultError::NonCanonicalVaultCashAccount))?;
    require!(
        token_account.state == SplTokenAccountState::Initialized
            && token_account.owner == *swig_wallet_key
            && token_account.mint == *settlement_mint,
        VaultError::NonCanonicalVaultCashAccount
    );

    Ok(CanonicalCashWitness::Present {
        balance: token_account.amount,
        unreserved_cash: token_account
            .amount
            .saturating_sub(outstanding_locked_amount),
    })
}

pub fn require_junior_debit(amount: u64, unreserved_cash: u64) -> Result<()> {
    require!(
        amount <= unreserved_cash,
        VaultError::JuniorDebitWouldViolateReservation
    );
    Ok(())
}

/// A Native Tab reservation is a senior seller claim on the Vault's current
/// cash until it either settles, crystallizes, or is owner-force-released.
/// Junior buyer-controlled cash exits cannot safely infer the aggregate amount
/// from `pending_voucher_count`, so they stop while any reservation (including
/// a still-resolving recovered legacy reservation) exists.
pub fn require_no_pending_tab_reservations(vault: &Vault) -> Result<()> {
    require!(
        vault.pending_voucher_count == 0
            && !crate::state::legacy_v6_resolution_pending(vault.live_session_count),
        VaultError::PendingVouchersExist
    );
    Ok(())
}

/// Release a senior reservation only when the aggregate ledger can cover it.
/// This must never saturate: an underflow means LockedClaim accounting has
/// diverged and the instruction must fail closed.
pub fn release_locked_reserve(outstanding: u64, amount: u64) -> Result<u64> {
    outstanding
        .checked_sub(amount)
        .ok_or_else(|| error!(VaultError::LockedReserveAccountingUnderflow))
}

/// Full cash reserve for exposure-increasing agent spending. Unlike a Line
/// repay/seize (which atomically reduces the very liability it pays), an agent
/// debit must leave both senior LockedClaims and the welded node's complete
/// principal + pro-forma interest recourse untouched.
pub fn full_agent_cash_available(
    cash_balance: u64,
    vault: &Vault,
    node: Option<(Pubkey, &PrincipalNode)>,
    now: i64,
) -> Result<u64> {
    require_no_pending_tab_reservations(vault)?;
    let mut reserved = vault.outstanding_locked_amount;
    if vault.node != Pubkey::default() {
        let (node_key, node) = node.ok_or_else(|| error!(VaultError::CreditNodeRequired))?;
        require_keys_eq!(node_key, vault.node, VaultError::WalletNodeMismatch);
        reserved = reserved
            .checked_add(node.subtree_draw)
            .ok_or_else(|| error!(VaultError::CarryMathOverflow))?;
        reserved = reserved
            .checked_add(crate::accrual::pro_forma_accrued(node, now)?)
            .ok_or_else(|| error!(VaultError::CarryMathOverflow))?;
    }
    Ok(cash_balance.saturating_sub(reserved))
}

pub fn require_canonical_money_leg_source(
    money_leg: &MoneyLeg,
    canonical_source: &Pubkey,
    settlement_mint: &Pubkey,
) -> Result<()> {
    require!(
        money_leg.source == *canonical_source && money_leg.mint == *settlement_mint,
        VaultError::DebtTransferMismatch
    );
    Ok(())
}

/// Draw capital must not share a Vault's LockedClaim reserve ledger. The caller
/// passes the canonical binding PDA as an unchecked account. An unbound PDA is
/// a system-owned, data-empty account; it may already hold donated lamports.
/// Treating any positive balance as "bound" would let a one-lamport transfer
/// grief every financier. If a program-owned binding exists, the Line draw is
/// rejected rather than guessing that Vault's reserve.
pub fn require_unbound_financier(binding_info: &AccountInfo) -> Result<()> {
    require!(
        binding_info.owner == &anchor_lang::system_program::ID && binding_info.data_is_empty(),
        VaultError::BoundVaultCannotFundLine
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::solana_program::program_option::COption;
    use anchor_spl::token::spl_token::state::AccountState;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct CashState {
        balance: u64,
        reserve: u64,
        debt: u64,
    }

    fn settle_claim(mut state: CashState) -> CashState {
        let settled = state.reserve.min(state.balance);
        state.balance -= settled;
        state.reserve = release_locked_reserve(state.reserve, settled).unwrap();
        state
    }

    fn repay_junior(mut state: CashState) -> CashState {
        let repay = state.debt.min(state.balance.saturating_sub(state.reserve));
        state.balance -= repay;
        state.debt -= repay;
        state
    }

    fn vault(swig: Pubkey, outstanding_locked_amount: u64) -> Vault {
        Vault {
            version: crate::state::VAULT_VERSION_V7,
            bump: 1,
            passkey_pubkey: [2; 33],
            swig_address: swig,
            cooling_off_seconds: 0,
            pending_voucher_count: 0,
            pending_withdrawal: None,
            identity_claim: [0; 32],
            dexter_authority: Pubkey::new_unique(),
            live_session_count: 0,
            outstanding_locked_amount,
            total_crystallized_amount: outstanding_locked_amount,
            total_settled_amount: 0,
            node: Pubkey::default(),
        }
    }

    fn node(subtree_draw: u64, accrued_fee: u64, now: i64) -> PrincipalNode {
        PrincipalNode {
            version: crate::state::PRINCIPAL_NODE_VERSION_V1,
            bump: 1,
            node_id: [3; 32],
            controller: Pubkey::new_unique(),
            parent: None,
            root_attestation: Some(Pubkey::new_unique()),
            cap: crate::state::RateCap {
                rate_amount: 1_000,
                period_secs: 1,
                bucket: 1_000,
                last_refill: now,
                ceiling: Some(1_000),
                burst_multiple: 1,
            },
            borrowed: subtree_draw,
            subtree_draw,
            borrow_recovery_at: Some(now + 1),
            shortfall: 0,
            frozen: false,
            child_count: 0,
            accrued_fee,
            rate_bps: 0,
            last_accrual: now,
            financier: Pubkey::new_unique(),
        }
    }

    fn binding(program_id: &Pubkey, swig: Pubkey, vault: Pubkey) -> (Pubkey, SwigVaultBinding) {
        let key =
            Pubkey::find_program_address(&[SWIG_VAULT_BINDING_SEED, swig.as_ref()], program_id).0;
        (
            key,
            SwigVaultBinding {
                version: SWIG_VAULT_BINDING_VERSION_V1,
                bump: 1,
                swig,
                vault,
            },
        )
    }

    fn token_account_data(
        mint: Pubkey,
        owner: Pubkey,
        amount: u64,
        state: AccountState,
    ) -> Vec<u8> {
        let mut data = vec![0u8; SplTokenAccount::LEN];
        SplTokenAccount::pack(
            SplTokenAccount {
                mint,
                owner,
                amount,
                delegate: COption::None,
                state,
                is_native: COption::None,
                delegated_amount: 0,
                close_authority: COption::None,
            },
            &mut data,
        )
        .unwrap();
        data
    }

    #[allow(clippy::too_many_arguments)]
    fn inspect_cash_witness_with_lamports(
        program_id: Pubkey,
        vault_key: Pubkey,
        vault: &Vault,
        binding_key: Pubkey,
        binding: &SwigVaultBinding,
        swig: Pubkey,
        wallet: Pubkey,
        cash_key: Pubkey,
        account_program_owner: Pubkey,
        data: &mut [u8],
        initial_lamports: u64,
        mint: Pubkey,
    ) -> Result<CanonicalCashWitness> {
        let mut lamports = initial_lamports;
        let account = AccountInfo::new(
            &cash_key,
            false,
            false,
            &mut lamports,
            data,
            &account_program_owner,
            false,
            0,
        );
        verify_canonical_cash_witness(
            &program_id,
            &binding_key,
            binding,
            &vault_key,
            vault,
            &swig,
            &wallet,
            &account,
            &mint,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn inspect_cash_witness(
        program_id: Pubkey,
        vault_key: Pubkey,
        vault: &Vault,
        binding_key: Pubkey,
        binding: &SwigVaultBinding,
        swig: Pubkey,
        wallet: Pubkey,
        cash_key: Pubkey,
        account_program_owner: Pubkey,
        data: &mut [u8],
        mint: Pubkey,
    ) -> Result<CanonicalCashWitness> {
        inspect_cash_witness_with_lamports(
            program_id,
            vault_key,
            vault,
            binding_key,
            binding,
            swig,
            wallet,
            cash_key,
            account_program_owner,
            data,
            0,
            mint,
        )
    }

    #[test]
    fn canonical_cash_witness_accepts_only_true_absence_at_the_exact_ata() {
        let program_id = crate::ID;
        let swig = Pubkey::new_unique();
        let vault_key = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let wallet = canonical_swig_wallet(&swig);
        let cash = canonical_swig_cash_ata(&swig, &mint);
        let (binding_key, binding) = binding(&program_id, swig, vault_key);
        let vault = vault(swig, 0);
        let mut empty = [];

        assert_eq!(
            inspect_cash_witness(
                program_id,
                vault_key,
                &vault,
                binding_key,
                &binding,
                swig,
                wallet,
                cash,
                anchor_lang::system_program::ID,
                &mut empty,
                mint,
            )
            .unwrap(),
            CanonicalCashWitness::Absent
        );

        let mut donated_empty = [];
        assert!(inspect_cash_witness_with_lamports(
            program_id,
            vault_key,
            &vault,
            binding_key,
            &binding,
            swig,
            wallet,
            cash,
            anchor_lang::system_program::ID,
            &mut donated_empty,
            1,
            mint,
        )
        .is_err());

        let mut noncanonical_empty = [];
        assert!(inspect_cash_witness(
            program_id,
            vault_key,
            &vault,
            binding_key,
            &binding,
            swig,
            wallet,
            Pubkey::new_unique(),
            anchor_lang::system_program::ID,
            &mut noncanonical_empty,
            mint,
        )
        .is_err());

        let mut empty_wrong_owner = [];
        assert!(inspect_cash_witness(
            program_id,
            vault_key,
            &vault,
            binding_key,
            &binding,
            swig,
            wallet,
            cash,
            Pubkey::new_unique(),
            &mut empty_wrong_owner,
            mint,
        )
        .is_err());

        let mut fake_absence = [1u8; 1];
        assert!(inspect_cash_witness(
            program_id,
            vault_key,
            &vault,
            binding_key,
            &binding,
            swig,
            wallet,
            cash,
            anchor_lang::system_program::ID,
            &mut fake_absence,
            mint,
        )
        .is_err());
    }

    #[test]
    fn canonical_cash_witness_strictly_decodes_classic_token_state() {
        let program_id = crate::ID;
        let swig = Pubkey::new_unique();
        let vault_key = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let wallet = canonical_swig_wallet(&swig);
        let cash = canonical_swig_cash_ata(&swig, &mint);
        let (binding_key, binding) = binding(&program_id, swig, vault_key);
        let vault = vault(swig, 30);

        let mut empty_token = token_account_data(mint, wallet, 0, AccountState::Initialized);
        assert_eq!(
            inspect_cash_witness(
                program_id,
                vault_key,
                &vault,
                binding_key,
                &binding,
                swig,
                wallet,
                cash,
                anchor_spl::token::ID,
                &mut empty_token,
                mint,
            )
            .unwrap(),
            CanonicalCashWitness::Present {
                balance: 0,
                unreserved_cash: 0,
            }
        );

        let mut funded = token_account_data(mint, wallet, 100, AccountState::Initialized);
        assert_eq!(
            inspect_cash_witness(
                program_id,
                vault_key,
                &vault,
                binding_key,
                &binding,
                swig,
                wallet,
                cash,
                anchor_spl::token::ID,
                &mut funded,
                mint,
            )
            .unwrap(),
            CanonicalCashWitness::Present {
                balance: 100,
                unreserved_cash: 70,
            }
        );

        for (account_mint, account_owner) in
            [(Pubkey::new_unique(), wallet), (mint, Pubkey::new_unique())]
        {
            let mut substituted =
                token_account_data(account_mint, account_owner, 100, AccountState::Initialized);
            assert!(inspect_cash_witness(
                program_id,
                vault_key,
                &vault,
                binding_key,
                &binding,
                swig,
                wallet,
                cash,
                anchor_spl::token::ID,
                &mut substituted,
                mint,
            )
            .is_err());
        }

        let mut malformed = [0u8; 8];
        assert!(inspect_cash_witness(
            program_id,
            vault_key,
            &vault,
            binding_key,
            &binding,
            swig,
            wallet,
            cash,
            anchor_spl::token::ID,
            &mut malformed,
            mint,
        )
        .is_err());

        let mut extended = vec![0u8; SplTokenAccount::LEN + 1];
        assert!(inspect_cash_witness(
            program_id,
            vault_key,
            &vault,
            binding_key,
            &binding,
            swig,
            wallet,
            cash,
            anchor_spl::token::ID,
            &mut extended,
            mint,
        )
        .is_err());

        let mut token_2022_data = token_account_data(mint, wallet, 100, AccountState::Initialized);
        assert!(inspect_cash_witness(
            program_id,
            vault_key,
            &vault,
            binding_key,
            &binding,
            swig,
            wallet,
            cash,
            anchor_spl::token_2022::ID,
            &mut token_2022_data,
            mint,
        )
        .is_err());

        let mut executable_lamports = 1;
        let mut executable_data = token_account_data(mint, wallet, 100, AccountState::Initialized);
        let token_program = anchor_spl::token::ID;
        let executable = AccountInfo::new(
            &cash,
            false,
            false,
            &mut executable_lamports,
            &mut executable_data,
            &token_program,
            true,
            0,
        );
        assert!(verify_canonical_cash_witness(
            &program_id,
            &binding_key,
            &binding,
            &vault_key,
            &vault,
            &swig,
            &wallet,
            &executable,
            &mint,
        )
        .is_err());

        for invalid_state in [AccountState::Uninitialized, AccountState::Frozen] {
            let mut invalid = token_account_data(mint, wallet, 100, invalid_state);
            assert!(inspect_cash_witness(
                program_id,
                vault_key,
                &vault,
                binding_key,
                &binding,
                swig,
                wallet,
                cash,
                anchor_spl::token::ID,
                &mut invalid,
                mint,
            )
            .is_err());
        }
    }

    #[test]
    fn canonical_cash_witness_transition_never_hides_new_cash() {
        use crate::instructions::settle_tab_voucher::require_cash_first_funding;

        let program_id = crate::ID;
        let swig = Pubkey::new_unique();
        let vault_key = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let wallet = canonical_swig_wallet(&swig);
        let cash = canonical_swig_cash_ata(&swig, &mint);
        let (binding_key, binding) = binding(&program_id, swig, vault_key);
        let vault = vault(swig, 20);

        let mut absent = [];
        let first = inspect_cash_witness(
            program_id,
            vault_key,
            &vault,
            binding_key,
            &binding,
            swig,
            wallet,
            cash,
            anchor_lang::system_program::ID,
            &mut absent,
            mint,
        )
        .unwrap();
        assert_eq!(first.unreserved_cash(), 0);
        assert!(require_cash_first_funding(0, 100, 100, first.unreserved_cash()).is_ok());

        for (balance, expected) in [(0, 0), (20, 0), (50, 30), (120, 100)] {
            let mut data = token_account_data(mint, wallet, balance, AccountState::Initialized);
            let next = inspect_cash_witness(
                program_id,
                vault_key,
                &vault,
                binding_key,
                &binding,
                swig,
                wallet,
                cash,
                anchor_spl::token::ID,
                &mut data,
                mint,
            )
            .unwrap();
            assert_eq!(next.unreserved_cash(), expected);

            let expected_cash = 100u64.min(expected);
            let expected_credit = 100 - expected_cash;
            assert!(require_cash_first_funding(
                expected_cash,
                expected_credit,
                100,
                next.unreserved_cash(),
            )
            .is_ok());
            if expected_cash > 0 {
                // A pure-credit plan built while the ATA was absent cannot be
                // replayed after the exact canonical address becomes funded.
                assert!(require_cash_first_funding(0, 100, 100, next.unreserved_cash()).is_err());
            }
        }
    }

    #[test]
    fn seventy_balance_seventy_reserve_leaves_zero_for_fifty_junior_debit() {
        let program_id = crate::ID;
        let swig = Pubkey::new_unique();
        let vault_key = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let swig_wallet = canonical_swig_wallet(&swig);
        let cash = canonical_swig_cash_ata(&swig, &mint);
        let binding_key =
            Pubkey::find_program_address(&[SWIG_VAULT_BINDING_SEED, swig.as_ref()], &program_id).0;
        let binding = SwigVaultBinding {
            version: SWIG_VAULT_BINDING_VERSION_V1,
            bump: 1,
            swig,
            vault: vault_key,
        };
        let vault = vault(swig, 70);
        let unreserved = verify_canonical_reserved_cash(
            &program_id,
            &binding_key,
            &binding,
            &vault_key,
            &vault,
            &swig,
            &swig_wallet,
            &cash,
            &swig_wallet,
            &mint,
            70,
            &mint,
        )
        .unwrap();
        assert_eq!(unreserved, 0);
        assert!(require_junior_debit(50, unreserved).is_err());
        assert!(require_junior_debit(0, unreserved).is_ok());
    }

    #[test]
    fn excess_cash_is_the_only_junior_tier() {
        let program_id = crate::ID;
        let swig = Pubkey::new_unique();
        let vault_key = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let wallet = canonical_swig_wallet(&swig);
        let cash = canonical_swig_cash_ata(&swig, &mint);
        let binding_key =
            Pubkey::find_program_address(&[SWIG_VAULT_BINDING_SEED, swig.as_ref()], &program_id).0;
        let binding = SwigVaultBinding {
            version: SWIG_VAULT_BINDING_VERSION_V1,
            bump: 1,
            swig,
            vault: vault_key,
        };
        for (balance, expected) in [(70, 0), (100, 30), (120, 50)] {
            let unreserved = verify_canonical_reserved_cash(
                &program_id,
                &binding_key,
                &binding,
                &vault_key,
                &vault(swig, 70),
                &swig,
                &wallet,
                &cash,
                &wallet,
                &mint,
                balance,
                &mint,
            )
            .unwrap();
            assert_eq!(unreserved, expected);
            assert_eq!(50u64.min(unreserved), expected);
        }
    }

    #[test]
    fn claim_and_junior_repay_are_order_independent_at_seventy_seventy_fifty() {
        for balance in [70, 100, 120] {
            let initial = CashState {
                balance,
                reserve: 70,
                debt: 50,
            };
            let claim_then_repay = repay_junior(settle_claim(initial));
            let repay_then_claim = settle_claim(repay_junior(initial));
            assert_eq!(claim_then_repay, repay_then_claim);
            assert_eq!(
                claim_then_repay,
                CashState {
                    balance: 0,
                    reserve: 0,
                    debt: 120u64.saturating_sub(balance),
                }
            );
        }
    }

    #[test]
    fn locked_reserve_release_fails_closed_on_underflow() {
        assert_eq!(release_locked_reserve(70, 70).unwrap(), 0);
        assert!(release_locked_reserve(69, 70).is_err());
    }

    #[test]
    fn donated_lamports_cannot_grief_an_unbound_financier() {
        let key = Pubkey::new_unique();
        let system_owner = anchor_lang::system_program::ID;
        let program_owner = crate::ID;
        let mut donated_lamports = 1;
        let mut empty_data = [];
        let unbound = AccountInfo::new(
            &key,
            false,
            false,
            &mut donated_lamports,
            &mut empty_data,
            &system_owner,
            false,
            0,
        );
        assert!(require_unbound_financier(&unbound).is_ok());

        let mut bound_lamports = 1;
        let mut binding_data = [1u8; 8];
        let bound = AccountInfo::new(
            &key,
            false,
            false,
            &mut bound_lamports,
            &mut binding_data,
            &program_owner,
            false,
            0,
        );
        assert!(require_unbound_financier(&bound).is_err());
    }

    #[test]
    fn seller_reservation_requires_atomic_or_exact_attested_policy_provenance() {
        let swig = Pubkey::new_unique();
        let vault_key = Pubkey::new_unique();
        let (_, mut identity_only) = binding(&crate::ID, swig, vault_key);
        assert_eq!(identity_only.version, SWIG_VAULT_BINDING_VERSION_V1);
        assert!(require_reserve_safe_swig_policy(&identity_only).is_err());

        identity_only.version = SWIG_VAULT_BINDING_VERSION_ATOMIC_V2;
        assert!(require_reserve_safe_swig_policy(&identity_only).is_ok());

        identity_only.version = SWIG_VAULT_BINDING_VERSION_ATTESTED_V3;
        assert!(require_reserve_safe_swig_policy(&identity_only).is_ok());

        identity_only.version = SWIG_VAULT_BINDING_VERSION_AUTOMATION_V4;
        assert!(require_reserve_safe_swig_policy(&identity_only).is_ok());

        identity_only.version = SWIG_VAULT_BINDING_VERSION_MINIMAL_V5;
        assert!(require_reserve_safe_swig_policy(&identity_only).is_ok());

        // Any version this program does not define stays refused.
        identity_only.version = SWIG_VAULT_BINDING_VERSION_MINIMAL_V5 + 1;
        assert!(require_reserve_safe_swig_policy(&identity_only).is_err());
    }

    #[test]
    fn agent_availability_reserves_locked_claims_line_principal_and_interest() {
        let now = 1_000;
        let node_key = Pubkey::new_unique();
        let mut vault = vault(Pubkey::new_unique(), 70);
        vault.node = node_key;
        let node = node(50, 5, now);

        // 70 LockedClaim + 50 Line principal + 5 banked interest = 125.
        assert_eq!(
            full_agent_cash_available(125, &vault, Some((node_key, &node)), now).unwrap(),
            0
        );
        assert_eq!(
            full_agent_cash_available(145, &vault, Some((node_key, &node)), now).unwrap(),
            20
        );
        assert!(require_junior_debit(
            1,
            full_agent_cash_available(125, &vault, Some((node_key, &node)), now).unwrap()
        )
        .is_err());

        assert!(full_agent_cash_available(125, &vault, None, now).is_err());
        assert!(
            full_agent_cash_available(125, &vault, Some((Pubkey::new_unique(), &node)), now)
                .is_err()
        );
    }

    #[test]
    fn pending_native_tab_stops_every_full_agent_cash_exit() {
        let now = 1_000;
        let mut buyer = vault(Pubkey::new_unique(), 0);
        buyer.pending_voucher_count = 1;

        assert!(require_no_pending_tab_reservations(&buyer).is_err());
        assert!(full_agent_cash_available(1_000, &buyer, None, now).is_err());

        buyer.pending_voucher_count = 0;
        assert!(require_no_pending_tab_reservations(&buyer).is_ok());
        assert_eq!(
            full_agent_cash_available(1_000, &buyer, None, now).unwrap(),
            1_000
        );
    }

    #[test]
    fn unresolved_legacy_tab_also_stops_junior_cash_exit() {
        let mut buyer = vault(Pubkey::new_unique(), 0);
        buyer.live_session_count = crate::state::LEGACY_V6_RESOLUTION_PENDING;
        assert!(require_no_pending_tab_reservations(&buyer).is_err());

        // The low count remains immaterial until the recovery marker clears.
        buyer.live_session_count = crate::state::LEGACY_V6_RESOLUTION_PENDING | 3;
        assert!(require_no_pending_tab_reservations(&buyer).is_err());
    }

    #[test]
    fn wrong_binding_wallet_mint_or_secondary_token_account_rejects() {
        let program_id = crate::ID;
        let swig = Pubkey::new_unique();
        let vault_key = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let wallet = canonical_swig_wallet(&swig);
        let cash = canonical_swig_cash_ata(&swig, &mint);
        let binding_key =
            Pubkey::find_program_address(&[SWIG_VAULT_BINDING_SEED, swig.as_ref()], &program_id).0;
        let binding = SwigVaultBinding {
            version: SWIG_VAULT_BINDING_VERSION_V1,
            bump: 1,
            swig,
            vault: vault_key,
        };
        let call = |binding_key: Pubkey,
                    binding: &SwigVaultBinding,
                    wallet_key: Pubkey,
                    account_key: Pubkey,
                    owner: Pubkey,
                    account_mint: Pubkey| {
            verify_canonical_reserved_cash(
                &program_id,
                &binding_key,
                binding,
                &vault_key,
                &vault(swig, 0),
                &swig,
                &wallet_key,
                &account_key,
                &owner,
                &account_mint,
                70,
                &mint,
            )
        };
        assert!(call(Pubkey::new_unique(), &binding, wallet, cash, wallet, mint).is_err());
        assert!(call(
            binding_key,
            &binding,
            Pubkey::new_unique(),
            cash,
            wallet,
            mint
        )
        .is_err());
        assert!(call(
            binding_key,
            &binding,
            wallet,
            Pubkey::new_unique(),
            wallet,
            mint
        )
        .is_err());
        assert!(call(
            binding_key,
            &binding,
            wallet,
            cash,
            wallet,
            Pubkey::new_unique()
        )
        .is_err());
    }
}
