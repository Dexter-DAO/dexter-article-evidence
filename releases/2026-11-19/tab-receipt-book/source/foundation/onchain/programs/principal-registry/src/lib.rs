use anchor_lang::prelude::*;
pub use dexter_contracts::{AcceptArgs, StageArgs};
use dexter_contracts::{terms_digest, plus, CAPTURE, RELEASE,
    CORE_ID, REGISTRY_CONSUMER_SEED, SCHEMA_VERSION, billing::BillingPolicy};
use dexter_contracts::window::{OrderWindow, WindowRefusal};

pub mod state;
pub use state::*;
pub mod wallet;
pub use wallet::*;
pub mod allowance;
pub use allowance::*;
mod swig_framing;
pub mod wallet_funding;
pub use wallet_funding::*;
pub mod ownership;
pub use ownership::*;
pub mod recovery;
pub use recovery::*;
pub mod passkey;
pub use passkey::*;
pub mod repayment;
pub mod spending;
pub use repayment::*;
pub use spending::*;

declare_id!("9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G");

#[program]
pub mod principal_registry {
    use super::*;

    pub fn initialize_authority_deployment(ctx: Context<InitializeAuthorityDeployment>, genesis_hash: [u8;32]) -> Result<()> {
        passkey::initialize_deployment(ctx, genesis_hash)
    }

    pub fn initialize_passkey_authority(ctx: Context<InitializePasskeyAuthority>, args: InitializePasskeyAuthorityArgs) -> Result<()> {
        passkey::initialize_authority(ctx, args)
    }

    pub fn authorize_passkey_operation(ctx: Context<AuthorizePasskeyOperation>, args: AuthorizePasskeyOperationArgs) -> Result<()> {
        passkey::authorize_operation(ctx, args)
    }

    pub fn create_passkey_principal(ctx: Context<CreatePasskeyPrincipal>, args: CreatePasskeyPrincipalArgs) -> Result<()> {
        passkey::create_with_passkey(ctx, args)
    }

    pub fn grant_spending_permission(ctx: Context<GrantSpendingPermission>, args: GrantSpendingPermissionArgs) -> Result<()> {
        spending::grant_spending(ctx, args)
    }

    /// The owner grants one exact spending approval at the principal's next
    /// approval number. It permits one trade and closes after its expiry.
    pub fn grant_exact_spending(ctx: Context<GrantExactSpending>, args: GrantExactSpendingArgs) -> Result<()> {
        spending::exact_spending_grant(ctx, args)
    }

    pub fn revoke_spending_permission(ctx: Context<RevokeSpendingPermission>, args: RevokeSpendingPermissionArgs) -> Result<()> {
        spending::revoke_spending(ctx, args)
    }

    pub fn consume_spending_permission(ctx: Context<ConsumeSpendingPermission>, args: SpendingExecution) -> Result<()> {
        spending::consume_spending(ctx, args)
    }

    pub fn grant_repayment_permission(ctx: Context<GrantRepaymentPermission>, args: GrantRepaymentPermissionArgs) -> Result<()> {
        repayment::grant(ctx, args)
    }

    /// The owner grants one exact repayment approval at the principal's next
    /// approval number. It permits one payment and closes after its expiry.
    pub fn grant_exact_repayment(ctx: Context<GrantExactRepayment>, args: GrantExactRepaymentArgs) -> Result<()> {
        repayment::exact_repayment_grant(ctx, args)
    }

    /// ClaimsCore only (its `close_approval` reports the close). Closes an exact
    /// approval after its expiry, used or not; the deposit returns to its payer.
    pub fn close_approval(ctx: Context<CloseApproval>) -> Result<()> {
        close_exact_approval(&ctx.accounts.approval, &ctx.accounts.deposit_payer)
    }

    pub fn revoke_repayment_permission(ctx: Context<RevokeRepaymentPermission>, args: RevokeRepaymentPermissionArgs) -> Result<()> {
        repayment::revoke(ctx, args)
    }

    pub fn consume_repayment_permission(ctx: Context<ConsumeRepaymentPermission>, args: RepaymentExecution) -> Result<()> {
        repayment::consume(ctx, args)
    }

    pub fn bind_funding_wallet(ctx: Context<BindFundingWallet>, args: BindFundingWalletArgs) -> Result<()> {
        wallet_funding::bind_funding(ctx, args)
    }

    pub fn authorize_funding_wallet(ctx: Context<AuthorizeFundingWallet>, args: AuthorizeFundingWalletArgs) -> Result<()> {
        wallet_funding::authorize_funding(ctx, args)
    }

    pub fn rotate_principal_owner(ctx: Context<RotatePrincipalOwner>, args: RotatePrincipalOwnerArgs) -> Result<()> {
        ownership::rotate(ctx, args)
    }

    pub fn configure_recovery_policy(ctx: Context<ConfigureRecoveryPolicy>, args: ConfigureRecoveryPolicyArgs) -> Result<()> {
        recovery::configure_policy(ctx, args)
    }

    pub fn recover_principal_owner(ctx: Context<RecoverPrincipalOwner>, args: RecoverPrincipalOwnerArgs) -> Result<()> {
        recovery::recover_owner(ctx, args)
    }

    pub fn amend_allowance(ctx: Context<AmendAllowance>, args: AmendAllowanceArgs) -> Result<()> {
        allowance::amend(ctx, args)
    }

    pub fn create_principal(ctx: Context<CreatePrincipal>, args: CreatePrincipalArgs) -> Result<()> {
        require!((1..=3).contains(&args.kind), RegistryError::InvalidPolicy);
        require!(args.domain != [0; 32] && args.governing_policy != [0; 32], RegistryError::InvalidPolicy);
        let principal = &mut ctx.accounts.principal;
        if principal.version != 0 {
            require!(principal.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
            require!(principal.principal_id == args.principal_id && principal.domain == args.domain
                && principal.kind == args.kind && principal.governing_policy == args.governing_policy
                && principal.seed_owner == ctx.accounts.owner.key(), RegistryError::ConfigurationConflict);
            return Ok(());
        }
        principal.set_inner(Principal { deposit_payer: ctx.accounts.payer.key(), version: SCHEMA_VERSION,
            bump: ctx.bumps.principal, principal_id: args.principal_id, domain: args.domain, kind: args.kind,
            governing_policy: args.governing_policy, seed_owner: ctx.accounts.owner.key(),
            owner: ctx.accounts.owner.key(), authority_generation: 1, approvals: 0 });
        Ok(())
    }

    pub fn create_actor(ctx: Context<CreateActor>, args: CreateActorArgs) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
        require_owner_authority(&ctx.accounts.principal,&ctx.accounts.owner,ctx.remaining_accounts)?;
        let actor = &mut ctx.accounts.actor;
        if actor.version != 0 {
            require!(actor.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
            require!(actor.principal == ctx.accounts.principal.key() && actor.actor_id == args.actor_id
                && actor.current_key == args.current_key && actor.generation == 1
                && actor.valid_from == args.valid_from && actor.valid_until == args.valid_until
                && actor.active, RegistryError::ConfigurationConflict);
            return Ok(());
        }
        check_credential(args.current_key, args.valid_from, args.valid_until)?;
        actor.set_inner(Actor { deposit_payer: ctx.accounts.payer.key(), version: SCHEMA_VERSION, bump: ctx.bumps.actor,
            principal: ctx.accounts.principal.key(), actor_id: args.actor_id,
            current_key: args.current_key, generation: 1, valid_from: args.valid_from,
            valid_until: args.valid_until, active: true });
        Ok(())
    }

    pub fn create_budget(ctx: Context<CreateBudget>, args: CreateBudgetArgs) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
        require_owner_authority(&ctx.accounts.principal,&ctx.accounts.owner,ctx.remaining_accounts)?;
        require!(args.asset != Pubkey::default() && args.ceiling > 0, RegistryError::InvalidPolicy);
        let budget = &mut ctx.accounts.budget;
        if budget.version != 0 {
            budget.check()?;
            require!(budget.principal == ctx.accounts.principal.key() && budget.asset == args.asset
                && budget.ceiling == args.ceiling, RegistryError::ConfigurationConflict);
            return Ok(());
        }
        budget.set_inner(Budget { deposit_payer: ctx.accounts.payer.key(), version: SCHEMA_VERSION, bump: ctx.bumps.budget,
            principal: ctx.accounts.principal.key(), asset: args.asset, ceiling: args.ceiling,
            reserved: 0, consumed: 0, revision: 1 });
        Ok(())
    }

    /// A permission is never live without its sellers and term rules: they are
    /// stored at revision 1, in the instruction that creates it.
    pub fn create_mandate(ctx: Context<CreateMandate>, args: CreateMandateArgs, rules: MandateRules) -> Result<()> {
        create_permission(ctx, args, rules)
    }

    pub fn rotate_actor(ctx: Context<ManageActor>, args: RotateActorArgs) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
        require_owner_authority(&ctx.accounts.principal,&ctx.accounts.owner,ctx.remaining_accounts)?;
        let actor = &mut ctx.accounts.actor;
        require!(actor.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        let next = plus(args.expected_generation, 1)?;
        if actor.generation == next && actor.current_key == args.new_key
            && actor.valid_from == args.valid_from && actor.valid_until == args.valid_until && actor.active {
            return Ok(());
        }
        require!(actor.generation == args.expected_generation, RegistryError::CredentialGeneration);
        check_credential(args.new_key, args.valid_from, args.valid_until)?;
        actor.current_key = args.new_key;
        actor.generation = next;
        actor.valid_from = args.valid_from;
        actor.valid_until = args.valid_until;
        actor.active = true;
        Ok(())
    }

    pub fn revoke_actor(ctx: Context<ManageActor>, expected_generation: u64) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
        require_owner_authority(&ctx.accounts.principal,&ctx.accounts.owner,ctx.remaining_accounts)?;
        let actor = &mut ctx.accounts.actor;
        require!(actor.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        let next = plus(expected_generation, 1)?;
        if !actor.active && actor.generation == next { return Ok(()); }
        require!(actor.generation == expected_generation, RegistryError::CredentialGeneration);
        actor.generation = next;
        actor.active = false;
        Ok(())
    }

    pub fn revoke_mandate(ctx: Context<ManageMandate>, expected_revision: u64) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
        require_owner_authority(&ctx.accounts.principal,&ctx.accounts.owner,ctx.remaining_accounts)?;
        let mandate = &mut ctx.accounts.mandate;
        require!(mandate.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        let next = plus(expected_revision, 1)?;
        if !mandate.active && mandate.revision == next { return Ok(()); }
        require!(mandate.revision == expected_revision, RegistryError::MandateRevision);
        mandate.revision = next;
        mandate.active = false;
        Ok(())
    }

    /// The owner replaces the sellers and term rules of this permission; no
    /// term rules leaves the terms unrestricted. Every change advances the
    /// revision, so an order prepared against the earlier permission is refused.
    pub fn set_mandate_rules(ctx: Context<ManageMandate>, expected_revision: u64,
        rules: MandateRules) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
        require_owner_authority(&ctx.accounts.principal,&ctx.accounts.owner,ctx.remaining_accounts)?;
        let mandate = &mut ctx.accounts.mandate;
        require!(mandate.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        require!(rules.valid(), RegistryError::InvalidPolicy);
        let next = plus(expected_revision, 1)?;
        if mandate.active && mandate.revision == next && mandate.holds(&rules) {
            return Ok(());
        }
        require!(mandate.active, RegistryError::MandateRevoked);
        require!(mandate.revision == expected_revision, RegistryError::MandateRevision);
        mandate.set_rules(&rules);
        mandate.revision = next;
        Ok(())
    }

    /// The owner sets the longest settlement hold window an order under this
    /// permission may agree (0: orders are final at settlement). Like a rules
    /// change it is a new revision, so commits signed against the old one are
    /// refused.
    pub fn set_mandate_hold_window(ctx: Context<ManageMandate>, expected_revision: u64,
        max_hold_window_seconds: u32) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
        require_owner_authority(&ctx.accounts.principal,&ctx.accounts.owner,ctx.remaining_accounts)?;
        let mandate = &mut ctx.accounts.mandate;
        require!(mandate.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        let next = plus(expected_revision, 1)?;
        if mandate.active && mandate.revision == next && mandate.max_hold_window_seconds == max_hold_window_seconds {
            return Ok(());
        }
        require!(mandate.active, RegistryError::MandateRevoked);
        require!(mandate.revision == expected_revision, RegistryError::MandateRevision);
        mandate.max_hold_window_seconds = max_hold_window_seconds;
        mandate.revision = next;
        Ok(())
    }

    pub fn reserve(ctx: Context<Reserve>, args: AcceptArgs) -> Result<()> {
        reserve_order(ctx, args, None)
    }

    /// `reserve` for an order that shows its terms: `order_terms` is the full
    /// list the order's external contract hash commits to, ascending by name.
    pub fn reserve_pinned(ctx: Context<Reserve>, args: AcceptArgs, namespace: [u8; 32], unit: [u8; 32],
        unit_price: u64, order_terms: Vec<Term>) -> Result<()> {
        reserve_order(ctx, args, Some(ShownTerms { terms: order_terms, commitment: Commitment::Run { namespace, unit, unit_price } }))
    }

    /// `reserve_pinned` for an order whose signed terms commit to a billing policy
    /// (`bill` and `claim` are the policy's context): `order_terms` hashes to `policy.request_hash`.
    pub fn reserve_pinned_billing(ctx: Context<Reserve>, args: AcceptArgs, bill: Pubkey, claim: Pubkey,
        policy: BillingPolicy, order_terms: Vec<Term>) -> Result<()> {
        reserve_order(ctx, args, Some(ShownTerms { terms: order_terms, commitment: Commitment::Billing { bill, claim, policy: Box::new(policy) } }))
    }

    /// ClaimsCore only. Moves `amount` of a Tab's hold from reserved to
    /// consumed. ClaimsCore's Tab book makes a retried step a no-op before
    /// this call, so the Registry keeps no per-operation record.
    pub fn consume(ctx: Context<ServiceReservation>, args: StageArgs) -> Result<()> {
        let a = ctx.accounts;
        service(&a.principal, &mut a.budget, &mut a.reservation, args, CAPTURE)
    }

    /// ClaimsCore only. Returns `amount` of a Tab's hold to the budget.
    pub fn release(ctx: Context<ServiceReservation>, args: StageArgs) -> Result<()> {
        let a = ctx.accounts;
        service(&a.principal, &mut a.budget, &mut a.reservation, args, RELEASE)
    }

    /// ClaimsCore only, from `close_tab`. Closes a spent hold, returns its
    /// deposit to its payer and folds the closed Tab into the mandate's chain.
    pub fn close_hold(ctx: Context<CloseHold>, tab: Pubkey, steps: u32, fingerprint: [u8; 32]) -> Result<()> {
        let hold = &ctx.accounts.reservation;
        hold.check()?;
        require!(hold.remaining == 0, RegistryError::HoldOpen);
        let mandate = &mut ctx.accounts.mandate;
        mandate.closed_tabs_head = closed_tab_link(&mandate.closed_tabs_head, &tab, steps, &fingerprint);
        mandate.closed_tabs = plus(mandate.closed_tabs, 1)?;
        Ok(())
    }

    /// Permissionless. Returns lamports above a Registry record's rent minimum
    /// (from the Rent sysvar now) to the payer recorded when it was created.
    pub fn return_excess_rent(ctx: Context<ReturnExcessRent>) -> Result<()> {
        return_excess(&ctx.accounts.record, &ctx.accounts.deposit_payer)
    }
}

fn close_exact_approval<'info>(approval: &AccountInfo<'info>, payer: &AccountInfo<'info>) -> Result<()> {
    require_keys_eq!(*approval.owner, crate::ID, RegistryError::SpendingPermission);
    let (principal, number, expires_at, deposit_payer) = {
        let data = approval.try_borrow_data()?;
        require!(data.len() >= 8, RegistryError::SpendingPermission);
        if data[..8] == *ExactSpendingApproval::DISCRIMINATOR {
            let a = ExactSpendingApproval::try_deserialize(&mut &data[..])?;
            (a.principal, a.number, a.terms.expires_at, a.deposit_payer)
        } else {
            let a = ExactRepaymentApproval::try_deserialize(&mut &data[..])?;
            (a.principal, a.number, a.terms.expires_at, a.deposit_payer)
        }
    };
    require_keys_eq!(approval.key(), spending::approval_address(&principal, number).0, RegistryError::SpendingPermission);
    require!(Clock::get()?.unix_timestamp >= expires_at, RegistryError::ApprovalOpen);
    require_keys_eq!(payer.key(), deposit_payer, RegistryError::DepositPayer);
    let lamports = approval.lamports();
    **payer.try_borrow_mut_lamports()? = payer.lamports().checked_add(lamports)
        .ok_or_else(|| error!(RegistryError::DepositPayer))?;
    **approval.try_borrow_mut_lamports()? = 0;
    approval.assign(&anchor_lang::system_program::ID);
    approval.resize(0)?;
    Ok(())
}

/// Registry types that record their rent payer as the first field.
fn records_payer(discriminator: &[u8]) -> bool {
    [Principal::DISCRIMINATOR, Mandate::DISCRIMINATOR, Reservation::DISCRIMINATOR, Actor::DISCRIMINATOR, Budget::DISCRIMINATOR,
        SpendingPermission::DISCRIMINATOR, RepaymentPermission::DISCRIMINATOR, FundingWalletBinding::DISCRIMINATOR, RecoveryPolicy::DISCRIMINATOR,
        ExactSpendingApproval::DISCRIMINATOR, ExactRepaymentApproval::DISCRIMINATOR, PasskeyAuthority::DISCRIMINATOR, AuthorityDeployment::DISCRIMINATOR]
        .iter().any(|known| *known == discriminator)
}

fn return_excess<'info>(record: &AccountInfo<'info>, payer: &AccountInfo<'info>) -> Result<()> {
    require_keys_eq!(*record.owner, crate::ID, RegistryError::NoExcessRent);
    require!(record.is_writable && payer.is_writable && record.key() != payer.key(), RegistryError::NoExcessRent);
    let stored = {
        let data = record.try_borrow_data()?;
        require!(data.len() >= 40 && records_payer(&data[..8]), RegistryError::NoExcessRent);
        Pubkey::new_from_array(data[8..40].try_into().unwrap())
    };
    require_keys_eq!(stored, payer.key(), RegistryError::DepositPayer);
    let minimum = Rent::get()?.minimum_balance(record.data_len());
    let excess = record.lamports().checked_sub(minimum).unwrap_or(0);
    require!(excess > 0, RegistryError::NoExcessRent);
    **record.try_borrow_mut_lamports()? -= excess;
    **payer.try_borrow_mut_lamports()? = payer.lamports().checked_add(excess)
        .ok_or_else(|| error!(RegistryError::NoExcessRent))?;
    Ok(())
}

fn create_permission(ctx: Context<CreateMandate>, args: CreateMandateArgs, rules: MandateRules) -> Result<()> {
    check_principal(&ctx.accounts.principal)?;
    require_owner_authority(&ctx.accounts.principal,&ctx.accounts.owner,ctx.remaining_accounts)?;
    ctx.accounts.budget.check()?;
    require!(rules.valid(), RegistryError::InvalidPolicy);
    let mandate = &mut ctx.accounts.mandate;
    if mandate.version != 0 {
        require!(mandate.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        require!(mandate.principal == ctx.accounts.principal.key() && mandate.actor == ctx.accounts.actor.key()
            && mandate.budget == ctx.accounts.budget.key() && mandate.mandate_id == args.mandate_id
            && mandate.asset == ctx.accounts.budget.asset && mandate.source == args.source
            && mandate.per_operation_limit == args.per_operation_limit
            && mandate.expires_at == args.expires_at && mandate.revision == 1 && mandate.active
            && mandate.holds(&rules), RegistryError::ConfigurationConflict);
        return Ok(());
    }
    require!(ctx.accounts.actor.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
    require!(ctx.accounts.actor.active, RegistryError::CredentialInactive);
    require!(args.source != Pubkey::default()
        && args.per_operation_limit > 0 && args.per_operation_limit <= ctx.accounts.budget.ceiling,
        RegistryError::InvalidPolicy);
    require!(args.expires_at > Clock::get()?.unix_timestamp, RegistryError::Expired);
    mandate.set_inner(Mandate { deposit_payer: ctx.accounts.payer.key(), version: SCHEMA_VERSION,
        bump: ctx.bumps.mandate,
        principal: ctx.accounts.principal.key(), actor: ctx.accounts.actor.key(),
        budget: ctx.accounts.budget.key(), mandate_id: args.mandate_id,
        asset: ctx.accounts.budget.asset, seller_count: 0, sellers: [Pubkey::default(); MAX_SELLERS],
        source: args.source, per_operation_limit: args.per_operation_limit, expires_at: args.expires_at,
        revision: 1, active: true, rule_count: 0, rules: [TermRule::default(); MAX_TERM_RULES],
        window: OrderWindow::default(), closed_tabs: 0, closed_tabs_head: [0; 32], max_hold_window_seconds: 0 });
    mandate.set_rules(&rules);
    Ok(())
}

/// Creates a Tab's budget hold. ClaimsCore turns a retried commit into a no-op
/// before this call (the Tab's receipt book), and the mandate's window refuses
/// any second commit of one order number, so the Registry keeps no receipt.
fn reserve_order(ctx: Context<Reserve>, args: AcceptArgs, shown: Option<ShownTerms>) -> Result<()> {
    check_principal(&ctx.accounts.principal)?;
    let actor = &ctx.accounts.actor;
    let mandate = &ctx.accounts.mandate;
    let terms = &args.terms;
    require!(actor.version == SCHEMA_VERSION && mandate.version == SCHEMA_VERSION,
        RegistryError::SchemaVersion);
    // Epoch checks belong after the immutable accepted-operation replay.
    // Replacing the operational owner invalidates all old grants for new
    // commitments, while accepted reservations retain their servicing rights.
    let (actor_key, actor_bump) = actor_epoch_address(&ctx.accounts.principal.key(),
        &actor.actor_id, ctx.accounts.principal.authority_generation);
    let (mandate_key, mandate_bump) = mandate_epoch_address(&ctx.accounts.principal.key(),
        &mandate.mandate_id, ctx.accounts.principal.authority_generation);
    require!(actor.key() == actor_key && actor.bump == actor_bump
        && mandate.key() == mandate_key && mandate.bump == mandate_bump,
        RegistryError::AuthorityEpoch);
    require!(actor.active, RegistryError::CredentialInactive);
    require_keys_eq!(ctx.accounts.credential.key(), actor.current_key, RegistryError::CredentialKey);
    require!(actor.generation == terms.credential_generation, RegistryError::CredentialGeneration);
    require!(mandate.active, RegistryError::MandateRevoked);
    require!(mandate.revision == terms.mandate_revision, RegistryError::MandateRevision);
    let now = Clock::get()?.unix_timestamp;
    require!(actor.valid_from <= now && now < actor.valid_until && now < mandate.expires_at
        && now < terms.expires_at && terms.expires_at <= mandate.expires_at, RegistryError::Expired);
    require!(terms.domain == ctx.accounts.principal.domain, RegistryError::IntentMismatch);
    require_keys_eq!(terms.principal, ctx.accounts.principal.key(), RegistryError::IntentMismatch);
    require_keys_eq!(terms.actor, actor.key(), RegistryError::IntentMismatch);
    require_keys_eq!(terms.mandate, mandate.key(), RegistryError::IntentMismatch);
    require_keys_eq!(terms.asset, ctx.accounts.budget.asset, RegistryError::IntentMismatch);
    require!(mandate.sellers().contains(&terms.supplier), RegistryError::IntentMismatch);
    require_keys_eq!(terms.source, mandate.source, RegistryError::IntentMismatch);
    require!(terms.cap > 0 && terms.cap <= mandate.per_operation_limit, RegistryError::PerOperationLimit);
    // The owner bounds the hold window its agent may agree (0: final at settlement).
    require!(terms.hold_window_seconds <= mandate.max_hold_window_seconds, RegistryError::HoldWindow);
    // The owner's term rules. A path that shows no list cannot use a permission
    // with rules. A shown list names each field once, is the list the signed
    // terms commit to, shows only numbers its values fingerprint, and meets every rule.
    match &shown {
        None => require!(mandate.term_rules().is_empty(), RegistryError::TermsRequired),
        Some(order) => {
            require!((1..=MAX_ORDER_TERMS).contains(&order.terms.len()), RegistryError::TermsMismatch);
            require!(names_ascend(&order.terms), RegistryError::TermsMismatch);
            require!(order.commits_to(terms),
                RegistryError::TermsMismatch);
            require!(order.numbers_match(), RegistryError::NumberMismatch);
            for rule in mandate.term_rules() {
                order.meets(rule)?;
            }
            // A listed delivery_signer is the key the order names to sign for delivery.
            require!(order.names_its_delivery_signer(), RegistryError::DeliverySignerMismatch);
            // A listed billing_tariff or unit_price is the order's own price.
            require!(order.names_its_price(), RegistryError::PriceMismatch);
        }
    }
    require!(ctx.accounts.reservation.version == 0, RegistryError::ReservationExists);
    admit_order(&mut ctx.accounts.mandate.window, terms.order)?;
    ctx.accounts.budget.reserve(terms.cap)?;
    let budget = ctx.accounts.budget.key();
    let mandate = ctx.accounts.mandate.key();
    ctx.accounts.reservation.set_inner(Reservation { deposit_payer: ctx.accounts.payer.key(),
        version: SCHEMA_VERSION, bump: ctx.bumps.reservation, budget, mandate, order: terms.order,
        terms_digest: terms_digest(terms)?, cap: terms.cap, remaining: terms.cap, consumed: 0, released: 0 });
    Ok(())
}

/// Admit one order number through a window, with the Registry's errors.
pub fn admit_order(window: &mut OrderWindow, order: u64) -> Result<()> {
    window.admit(order).map_err(|refusal| match refusal {
        WindowRefusal::Zero => error!(RegistryError::OrderZero),
        WindowRefusal::TooFar => error!(RegistryError::OrderTooFar),
        WindowRefusal::BelowWindow => error!(RegistryError::OrderBelowWindow),
        WindowRefusal::Used => error!(RegistryError::OrderUsed),
    })
}

fn service(principal: &Account<Principal>, budget: &mut Account<Budget>, reservation: &mut Account<Reservation>,
    args: StageArgs, kind: u8) -> Result<()> {
    check_principal(principal)?;
    reservation.check()?;
    require_keys_eq!(reservation.budget, budget.key(), RegistryError::IntentMismatch);
    // Core authenticates the accepted servicing right and checks the financial
    // claim revision. Current actor/mandate liveness is deliberately irrelevant.
    match kind {
        CAPTURE => {
            budget.consume(args.amount)?;
            reservation.consume(args.amount)?;
        }
        RELEASE => {
            budget.release(args.amount)?;
            reservation.release(args.amount)?;
        }
        _ => return err!(RegistryError::InvalidPolicy),
    }
    Ok(())
}

fn check_principal(principal: &Principal) -> Result<()> {
    require!(principal.version == SCHEMA_VERSION && principal.authority_generation > 0,
        RegistryError::SchemaVersion);
    Ok(())
}

fn check_credential(key: Pubkey, valid_from: i64, valid_until: i64) -> Result<()> {
    require!(key != Pubkey::default() && valid_from >= 0 && valid_until > valid_from,
        RegistryError::InvalidPolicy);
    require!(valid_until > Clock::get()?.unix_timestamp, RegistryError::Expired);
    Ok(())
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct CreatePrincipalArgs {
    pub principal_id: [u8; 32],
    pub domain: [u8; 32],
    pub kind: u8,
    pub governing_policy: [u8; 32],
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct CreateActorArgs {
    pub actor_id: [u8; 32],
    pub current_key: Pubkey,
    pub valid_from: i64,
    pub valid_until: i64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct CreateBudgetArgs { pub asset: Pubkey, pub ceiling: u64 }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct CreateMandateArgs {
    pub mandate_id: [u8; 32],
    pub source: Pubkey,
    pub per_operation_limit: u64,
    pub expires_at: i64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct RotateActorArgs {
    pub expected_generation: u64,
    pub new_key: Pubkey,
    pub valid_from: i64,
    pub valid_until: i64,
}

#[derive(Accounts)]
#[instruction(args: CreatePrincipalArgs)]
pub struct CreatePrincipal<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub owner: Signer<'info>,
    #[account(init_if_needed, payer = payer, space = 8 + Principal::INIT_SPACE,
        seeds = [b"principal", owner.key().as_ref(), args.principal_id.as_ref()], bump)]
    pub principal: Box<Account<'info, Principal>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: CreateActorArgs)]
pub struct CreateActor<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(address = principal.owner @ RegistryError::OwnerAuthority)]
    /// CHECK: Current native or exact passkey owner authority is checked in the handler.
    pub owner: UncheckedAccount<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(init_if_needed, payer = payer, space = 8 + Actor::INIT_SPACE,
        seeds = [b"actor", principal.key().as_ref(), args.actor_id.as_ref(),
            &principal.authority_generation.to_le_bytes()[..authority_epoch_seed_len(principal.authority_generation)]], bump)]
    pub actor: Box<Account<'info, Actor>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: CreateBudgetArgs)]
pub struct CreateBudget<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(address = principal.owner @ RegistryError::OwnerAuthority)]
    /// CHECK: Current native or exact passkey owner authority is checked in the handler.
    pub owner: UncheckedAccount<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(init_if_needed, payer = payer, space = 8 + Budget::INIT_SPACE,
        seeds = [b"budget", principal.key().as_ref(), args.asset.as_ref()], bump)]
    pub budget: Box<Account<'info, Budget>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: CreateMandateArgs)]
pub struct CreateMandate<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(address = principal.owner @ RegistryError::OwnerAuthority)]
    /// CHECK: Current native or exact passkey owner authority is checked in the handler.
    pub owner: UncheckedAccount<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(has_one = principal, seeds = [b"actor", principal.key().as_ref(), actor.actor_id.as_ref(),
        &principal.authority_generation.to_le_bytes()[..authority_epoch_seed_len(principal.authority_generation)]],
        bump = actor.bump)]
    pub actor: Box<Account<'info, Actor>>,
    #[account(has_one = principal, seeds = [b"budget", principal.key().as_ref(), budget.asset.as_ref()],
        bump = budget.bump)]
    pub budget: Box<Account<'info, Budget>>,
    #[account(init_if_needed, payer = payer, space = 8 + Mandate::INIT_SPACE,
        seeds = [b"mandate", principal.key().as_ref(), args.mandate_id.as_ref(),
            &principal.authority_generation.to_le_bytes()[..authority_epoch_seed_len(principal.authority_generation)]], bump)]
    pub mandate: Box<Account<'info, Mandate>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ManageActor<'info> {
    #[account(address = principal.owner @ RegistryError::OwnerAuthority)]
    /// CHECK: Current native or exact passkey owner authority is checked in the handler.
    pub owner: UncheckedAccount<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut, has_one = principal,
        seeds = [b"actor", principal.key().as_ref(), actor.actor_id.as_ref(),
            &principal.authority_generation.to_le_bytes()[..authority_epoch_seed_len(principal.authority_generation)]], bump = actor.bump)]
    pub actor: Box<Account<'info, Actor>>,
}

#[derive(Accounts)]
pub struct ManageMandate<'info> {
    #[account(address = principal.owner @ RegistryError::OwnerAuthority)]
    /// CHECK: Current native or exact passkey owner authority is checked in the handler.
    pub owner: UncheckedAccount<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut, has_one = principal,
        seeds = [b"mandate", principal.key().as_ref(), mandate.mandate_id.as_ref(),
            &principal.authority_generation.to_le_bytes()[..authority_epoch_seed_len(principal.authority_generation)]], bump = mandate.bump)]
    pub mandate: Box<Account<'info, Mandate>>,
}

#[derive(Accounts)]
#[instruction(args: AcceptArgs)]
pub struct Reserve<'info> {
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump, seeds::program = CORE_ID)]
    pub core_authority: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    // Current-epoch PDA checks run only for fresh admission in the handler.
    // Anchor constraints must allow a historical accepted-operation replay.
    #[account(has_one = principal)]
    pub actor: Box<Account<'info, Actor>>,
    pub credential: Signer<'info>,
    /// Writable: the commit admits its order number through the mandate's window.
    #[account(mut, has_one = principal, has_one = actor, has_one = budget)]
    pub mandate: Box<Account<'info, Mandate>>,
    #[account(mut, has_one = principal, constraint = budget.asset == mandate.asset @ RegistryError::IntentMismatch,
        seeds = [b"budget", principal.key().as_ref(), budget.asset.as_ref()], bump = budget.bump)]
    pub budget: Box<Account<'info, Budget>>,
    #[account(init, payer = payer, space = 8 + Reservation::INIT_SPACE,
        seeds = [b"hold", mandate.key().as_ref(), &args.terms.order.to_le_bytes()], bump)]
    pub reservation: Box<Account<'info, Reservation>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ServiceReservation<'info> {
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump, seeds::program = CORE_ID)]
    pub core_authority: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut, has_one = principal,
        seeds = [b"budget", principal.key().as_ref(), budget.asset.as_ref()], bump = budget.bump)]
    pub budget: Box<Account<'info, Budget>>,
    #[account(mut, constraint = reservation.budget == budget.key() @ RegistryError::IntentMismatch,
        seeds = [b"hold", reservation.mandate.as_ref(), &reservation.order.to_le_bytes()], bump = reservation.bump)]
    pub reservation: Box<Account<'info, Reservation>>,
}

#[derive(Accounts)]
pub struct CloseHold<'info> {
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump, seeds::program = CORE_ID)]
    pub core_authority: Signer<'info>,
    #[account(mut, close = deposit_payer,
        seeds = [b"hold", reservation.mandate.as_ref(), &reservation.order.to_le_bytes()], bump = reservation.bump)]
    pub reservation: Box<Account<'info, Reservation>>,
    #[account(mut, address = reservation.mandate @ RegistryError::IntentMismatch)]
    pub mandate: Box<Account<'info, Mandate>>,
    /// CHECK: The hold's recorded rent payer receives its deposit.
    #[account(mut, address = reservation.deposit_payer @ RegistryError::DepositPayer)]
    pub deposit_payer: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct CloseApproval<'info> {
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump, seeds::program = CORE_ID)]
    pub core_authority: Signer<'info>,
    /// CHECK: An exact spending or repayment approval; read by type in the handler.
    #[account(mut)]
    pub approval: UncheckedAccount<'info>,
    /// CHECK: Must be the approval's recorded rent payer.
    #[account(mut)]
    pub deposit_payer: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct ReturnExcessRent<'info> {
    /// CHECK: A Registry record whose type stores its rent payer first; checked in the handler.
    #[account(mut)]
    pub record: UncheckedAccount<'info>,
    /// CHECK: Must equal the rent payer stored in the record.
    #[account(mut)]
    pub deposit_payer: UncheckedAccount<'info>,
}

#[error_code]
pub enum RegistryError {
    #[msg("Unsupported Registry schema version")] SchemaVersion,
    #[msg("Owner authority is required")] OwnerAuthority,
    #[msg("Configuration retry changes existing authority or policy")] ConfigurationConflict,
    #[msg("Policy is invalid for the supported authority profile")] InvalidPolicy,
    #[msg("Credential is inactive")] CredentialInactive,
    #[msg("Credential signer does not match the current actor credential")] CredentialKey,
    #[msg("Credential generation has changed")] CredentialGeneration,
    #[msg("Mandate is revoked")] MandateRevoked,
    #[msg("Mandate revision has changed")] MandateRevision,
    #[msg("Credential, mandate or exact authorization has expired or is not yet valid")] Expired,
    #[msg("Exact intent differs from principal, mandate or accepted terms")] IntentMismatch,
    #[msg("Operation exceeds the per-operation ceiling")] PerOperationLimit,
    #[msg("Shared lifetime expenditure budget is exhausted")] BudgetExhausted,
    #[msg("Amount must be positive")] ZeroAmount,
    #[msg("This Tab already has an accepted reservation")] ReservationExists,
    #[msg("Operation ID was already used for different terms or a different stage")] OperationConflict,
    #[msg("Accepted reservation no longer conserves its finite original capacity")] ReservationInvariant,
    #[msg("Current principal owner signature is required")] WalletOwner,
    #[msg("Wallet binding identity or schema mismatch")] WalletBinding,
    #[msg("Wallet graph differs from the admitted single Registry root")] WalletGraph,
    #[msg("Wallet or owner authority generation is stale")] WalletGeneration,
    #[msg("Wallet authorization nonce was already consumed or is out of order")] WalletNonce,
    #[msg("Wallet authorization is expired or has no positive amount")] WalletExpired,
    #[msg("Wallet operation must be an exact top-level admitted envelope")] WalletEnvelope,
    // Code 6024 belonged to the retired cash-guarantee wallet repayment (P1, Change 5); never reused.
    #[msg("Wallet operation bytes or effective privileges changed")] WalletBundleHash = 25,
    #[msg("Allowance amendment uses a stale owner authority generation")] AllowanceGeneration,
    #[msg("Allowance policy revision is invalid or has changed")] AllowanceRevision,
    #[msg("Allowance ceiling cannot be below consumed plus reserved authority")] AllowanceFloor,
    #[msg("Allowance operation ID was already used for a different amendment")] AllowanceConflict,
    #[msg("Wallet operation is not an admitted collection, free withdrawal, repayment, send or swap")] FundingAction,
    #[msg("Owner rotation ID conflicts with its original immutable request")] OwnerRotationConflict,
    #[msg("Repayment permission is absent, revoked, expired, exhausted or does not cover this exact obligation and route")] RepaymentPermission,
    #[msg("Spending permission is absent, revoked, expired, exhausted or does not cover this action, asset, route and outcome")] SpendingPermission,
    #[msg("Actor or mandate belongs to a previous principal authority generation")] AuthorityEpoch,
    #[msg("Recovery policy identity, guardian set or canonical digest is invalid")] RecoveryPolicy,
    #[msg("The required distinct enrolled guardian quorum did not sign")] RecoveryQuorum,
    #[msg("Recovery and operational owner keys must be distinct and the replacement owner must sign")] RecoveryOwner,
    #[msg("Recovery policy revision or digest has changed")] RecoveryRevision,
    #[msg("Recovery sequence has changed")] RecoverySequence,
    #[msg("Recovery request has no valid request identity or has expired")] RecoveryExpired,
    #[msg("Recovery request ID conflicts with its immutable completed receipt")] RecoveryReplay,
    #[msg("Passkey deployment attestation or canonical loader authority is invalid")] PasskeyDeployment,
    #[msg("Passkey authority alias, immutable key or profile is invalid")] PasskeyAuthority,
    #[msg("Passkey proof does not bind the exact continuing or prospective Principal")] PasskeyPrincipal,
    #[msg("Passkey proof nonce was consumed or is out of order")] PasskeyNonce,
    #[msg("Passkey authorization role does not authorize this action")] PasskeyRole,
    #[msg("Passkey current-owner generation is stale or invalid for its role")] PasskeyGeneration,
    #[msg("Passkey exact-operation proof has expired or lacks ceremony entropy")] PasskeyExpired,
    #[msg("Passkey proof/target is not the exact admitted top-level envelope")] PasskeyEnvelope,
    #[msg("Reconcile a completed control operation without a passkey prefix; do not consume backup proof nonces on historical no-ops")] PasskeyReplayPrefix,
    #[msg("This permission has term rules; the order must show its terms")] TermsRequired,
    #[msg("Order terms shown are not the terms the order commits to")] TermsMismatch,
    #[msg("Order terms do not meet a rule the owner set")] TermsNotPermitted,
    #[msg("The order's delivery_signer term is not the key the order names to sign for delivery")] DeliverySignerMismatch,
    #[msg("The order's billing_tariff or unit_price term is not the order's own price")] PriceMismatch,
    #[msg("A number shown with an order term is not the number its value fingerprints")] NumberMismatch,
    #[msg("A number or price in the order is outside the range the owner allows")] NumberOutOfRange,
    #[msg("Order number zero is not an order")] OrderZero,
    #[msg("Order number is more than 65,536 above the mandate's highest")] OrderTooFar,
    #[msg("Order number is 1,024 or more below the mandate's highest and can never be committed")] OrderBelowWindow,
    #[msg("Order number was already committed under this mandate")] OrderUsed,
    #[msg("The hold still has unused capacity")] HoldOpen,
    #[msg("Account is not the record's rent payer")] DepositPayer,
    #[msg("Record has no lamports above its rent minimum, or is not a Registry record with a rent payer")] NoExcessRent,
    #[msg("Exact approval number is not the principal's next number")] ApprovalNumber,
    #[msg("Exact approval has not expired")] ApprovalOpen,
    #[msg("The order's settlement hold window is longer than its permission allows")] HoldWindow,
}
