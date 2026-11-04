use anchor_lang::prelude::*;
pub use dexter_contracts::{AcceptArgs, StageArgs};
use dexter_contracts::{operation_digest, terms_digest, plus, ACCEPT, CAPTURE, RELEASE,
    CORE_ID, REGISTRY_CONSUMER_SEED, RUN_OPERATION_SEED, SCHEMA_VERSION};

pub mod state;
pub use state::*;
pub mod wallet;
pub use wallet::*;
pub mod allowance;
pub use allowance::*;
pub mod wallet_funding;
pub use wallet_funding::*;
pub mod ownership;
pub use ownership::*;
pub mod repayment;
pub mod spending;
pub use repayment::*;
pub use spending::*;

declare_id!("9cRnuwBP8gUBmFKJxxhLyR3GcqzKwMuUCTTFPngEtr8G");

#[program]
pub mod principal_registry {
    use super::*;

    pub fn grant_spending_permission(ctx: Context<GrantSpendingPermission>, args: GrantSpendingPermissionArgs) -> Result<()> {
        spending::grant_spending(ctx, args)
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

    pub fn amend_allowance(ctx: Context<AmendAllowance>, args: AmendAllowanceArgs) -> Result<()> {
        allowance::amend(ctx, args)
    }

    pub fn bind_wallet(ctx: Context<BindWallet>, args: BindWalletArgs) -> Result<()> {
        wallet::bind(ctx, args)
    }

    pub fn authorize_wallet(ctx: Context<AuthorizeWallet>, args: AuthorizeWalletArgs) -> Result<()> {
        wallet::authorize(ctx, args)
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
        principal.set_inner(Principal { version: SCHEMA_VERSION, bump: ctx.bumps.principal,
            principal_id: args.principal_id, domain: args.domain, kind: args.kind,
            governing_policy: args.governing_policy, seed_owner: ctx.accounts.owner.key(),
            owner: ctx.accounts.owner.key(), authority_generation: 1 });
        Ok(())
    }

    pub fn create_actor(ctx: Context<CreateActor>, args: CreateActorArgs) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
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
        actor.set_inner(Actor { version: SCHEMA_VERSION, bump: ctx.bumps.actor,
            principal: ctx.accounts.principal.key(), actor_id: args.actor_id,
            current_key: args.current_key, generation: 1, valid_from: args.valid_from,
            valid_until: args.valid_until, active: true });
        Ok(())
    }

    pub fn create_budget(ctx: Context<CreateBudget>, args: CreateBudgetArgs) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
        require!(args.asset != Pubkey::default() && args.ceiling > 0, RegistryError::InvalidPolicy);
        let budget = &mut ctx.accounts.budget;
        if budget.version != 0 {
            budget.check()?;
            require!(budget.principal == ctx.accounts.principal.key() && budget.asset == args.asset
                && budget.ceiling == args.ceiling, RegistryError::ConfigurationConflict);
            return Ok(());
        }
        budget.set_inner(Budget { version: SCHEMA_VERSION, bump: ctx.bumps.budget,
            principal: ctx.accounts.principal.key(), asset: args.asset, ceiling: args.ceiling,
            reserved: 0, consumed: 0, revision: 1 });
        Ok(())
    }

    pub fn create_mandate(ctx: Context<CreateMandate>, args: CreateMandateArgs) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
        ctx.accounts.budget.check()?;
        let mandate = &mut ctx.accounts.mandate;
        if mandate.version != 0 {
            require!(mandate.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
            require!(mandate.principal == ctx.accounts.principal.key() && mandate.actor == ctx.accounts.actor.key()
                && mandate.budget == ctx.accounts.budget.key() && mandate.mandate_id == args.mandate_id
                && mandate.asset == ctx.accounts.budget.asset && mandate.supplier == args.supplier
                && mandate.source == args.source && mandate.per_operation_limit == args.per_operation_limit
                && mandate.expires_at == args.expires_at && mandate.revision == 1 && mandate.active,
                RegistryError::ConfigurationConflict);
            return Ok(());
        }
        require!(ctx.accounts.actor.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        require!(ctx.accounts.actor.active, RegistryError::CredentialInactive);
        require!(args.supplier != Pubkey::default() && args.source != Pubkey::default()
            && args.per_operation_limit > 0 && args.per_operation_limit <= ctx.accounts.budget.ceiling,
            RegistryError::InvalidPolicy);
        require!(args.expires_at > Clock::get()?.unix_timestamp, RegistryError::Expired);
        mandate.set_inner(Mandate { version: SCHEMA_VERSION, bump: ctx.bumps.mandate,
            principal: ctx.accounts.principal.key(), actor: ctx.accounts.actor.key(),
            budget: ctx.accounts.budget.key(), mandate_id: args.mandate_id,
            asset: ctx.accounts.budget.asset, supplier: args.supplier, source: args.source,
            per_operation_limit: args.per_operation_limit, expires_at: args.expires_at,
            revision: 1, active: true });
        Ok(())
    }

    pub fn rotate_actor(ctx: Context<ManageActor>, args: RotateActorArgs) -> Result<()> {
        check_principal(&ctx.accounts.principal)?;
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
        let mandate = &mut ctx.accounts.mandate;
        require!(mandate.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        let next = plus(expected_revision, 1)?;
        if !mandate.active && mandate.revision == next { return Ok(()); }
        require!(mandate.revision == expected_revision, RegistryError::MandateRevision);
        mandate.revision = next;
        mandate.active = false;
        Ok(())
    }

    pub fn reserve(ctx: Context<Reserve>, args: AcceptArgs) -> Result<()> {
        let digest = operation_digest(&args.terms, ACCEPT, &args.operation_id, args.terms.cap, false)?;
        // Reconciliation precedes current credential, mandate and expiry checks.
        // It retrieves the old accepted outcome; it never grants fresh capacity.
        if ctx.accounts.operation.matches(ctx.accounts.principal.key(), args.operation_id,
            ACCEPT, digest, ctx.accounts.reservation.key())? { return Ok(()); }
        check_principal(&ctx.accounts.principal)?;
        let actor = &ctx.accounts.actor;
        let mandate = &ctx.accounts.mandate;
        let terms = &args.terms;
        require!(actor.version == SCHEMA_VERSION && mandate.version == SCHEMA_VERSION,
            RegistryError::SchemaVersion);
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
        require_keys_eq!(terms.supplier, mandate.supplier, RegistryError::IntentMismatch);
        require_keys_eq!(terms.source, mandate.source, RegistryError::IntentMismatch);
        require!(terms.cap > 0 && terms.cap <= mandate.per_operation_limit, RegistryError::PerOperationLimit);
        require!(ctx.accounts.reservation.version == 0, RegistryError::ReservationExists);
        ctx.accounts.budget.reserve(terms.cap)?;
        let reservation = &mut ctx.accounts.reservation;
        reservation.set_inner(Reservation { version: SCHEMA_VERSION, bump: ctx.bumps.reservation,
            terms: terms.clone(), terms_digest: terms_digest(terms)?,
            acceptance_operation: args.operation_id, remaining: terms.cap, consumed: 0, released: 0 });
        record_operation(&mut ctx.accounts.operation, ctx.bumps.operation,
            ctx.accounts.principal.key(), args.operation_id, ACCEPT, digest,
            reservation.key(), reservation, &ctx.accounts.budget);
        Ok(())
    }

    pub fn consume(ctx: Context<ServiceReservation>, args: StageArgs) -> Result<()> {
        let a = ctx.accounts;
        service(&a.principal, &mut a.budget, &mut a.reservation, &mut a.operation,
            ctx.bumps.operation, args, CAPTURE)
    }

    pub fn consume_completed_run(ctx: Context<ConsumeCompletedRun>, args: StageArgs) -> Result<()> {
        let a = ctx.accounts;
        service(&a.principal, &mut a.budget, &mut a.reservation, &mut a.operation,
            ctx.bumps.operation, args, CAPTURE)
    }

    pub fn release(ctx: Context<ServiceReservation>, args: StageArgs) -> Result<()> {
        let a = ctx.accounts;
        service(&a.principal, &mut a.budget, &mut a.reservation, &mut a.operation,
            ctx.bumps.operation, args, RELEASE)
    }
}

fn service(principal: &Account<Principal>, budget: &mut Budget, reservation: &mut Account<Reservation>,
    operation: &mut OperationReceipt, bump: u8, args: StageArgs, kind: u8) -> Result<()> {
    let digest = operation_digest(&reservation.terms, kind, &args.operation_id,
        args.amount, args.service_without_pool)?;
    if operation.matches(principal.key(), args.operation_id,
        kind, digest, reservation.key())? { return Ok(()); }
    check_principal(principal)?;
    reservation.check()?;
    require!(reservation.terms_digest == terms_digest(&reservation.terms)?, RegistryError::IntentMismatch);
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
    record_operation(operation, bump,
        principal.key(), args.operation_id, kind, digest,
        reservation.key(), reservation, budget);
    Ok(())
}

fn record_operation(receipt: &mut OperationReceipt, bump: u8, principal: Pubkey,
    operation_id: [u8; 32], kind: u8, request_digest: [u8; 32], reservation_key: Pubkey,
    reservation: &Reservation, budget: &Budget) {
    *receipt = OperationReceipt { version: SCHEMA_VERSION, bump, principal, operation_id,
        kind, request_digest, reservation: reservation_key, reservation_remaining: reservation.remaining,
        reservation_consumed: reservation.consumed, reservation_released: reservation.released,
        budget_reserved: budget.reserved, budget_consumed: budget.consumed };
}

fn check_principal(principal: &Principal) -> Result<()> {
    require!(principal.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
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
    pub supplier: Pubkey,
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
    pub owner: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(init_if_needed, payer = payer, space = 8 + Actor::INIT_SPACE,
        seeds = [b"actor", principal.key().as_ref(), args.actor_id.as_ref()], bump)]
    pub actor: Box<Account<'info, Actor>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: CreateBudgetArgs)]
pub struct CreateBudget<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(address = principal.owner @ RegistryError::OwnerAuthority)]
    pub owner: Signer<'info>,
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
    pub owner: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(has_one = principal, seeds = [b"actor", principal.key().as_ref(), actor.actor_id.as_ref()],
        bump = actor.bump)]
    pub actor: Box<Account<'info, Actor>>,
    #[account(has_one = principal, seeds = [b"budget", principal.key().as_ref(), budget.asset.as_ref()],
        bump = budget.bump)]
    pub budget: Box<Account<'info, Budget>>,
    #[account(init_if_needed, payer = payer, space = 8 + Mandate::INIT_SPACE,
        seeds = [b"mandate", principal.key().as_ref(), args.mandate_id.as_ref()], bump)]
    pub mandate: Box<Account<'info, Mandate>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ManageActor<'info> {
    #[account(address = principal.owner @ RegistryError::OwnerAuthority)]
    pub owner: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut, has_one = principal,
        seeds = [b"actor", principal.key().as_ref(), actor.actor_id.as_ref()], bump = actor.bump)]
    pub actor: Box<Account<'info, Actor>>,
}

#[derive(Accounts)]
pub struct ManageMandate<'info> {
    #[account(address = principal.owner @ RegistryError::OwnerAuthority)]
    pub owner: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut, has_one = principal,
        seeds = [b"mandate", principal.key().as_ref(), mandate.mandate_id.as_ref()], bump = mandate.bump)]
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
    #[account(has_one = principal,
        seeds = [b"actor", principal.key().as_ref(), actor.actor_id.as_ref()], bump = actor.bump)]
    pub actor: Box<Account<'info, Actor>>,
    pub credential: Signer<'info>,
    #[account(has_one = principal, has_one = actor, has_one = budget,
        seeds = [b"mandate", principal.key().as_ref(), mandate.mandate_id.as_ref()], bump = mandate.bump)]
    pub mandate: Box<Account<'info, Mandate>>,
    #[account(mut, has_one = principal, constraint = budget.asset == mandate.asset @ RegistryError::IntentMismatch,
        seeds = [b"budget", principal.key().as_ref(), budget.asset.as_ref()], bump = budget.bump)]
    pub budget: Box<Account<'info, Budget>>,
    #[account(init_if_needed, payer = payer, space = 8 + Reservation::INIT_SPACE,
        seeds = [b"reservation", principal.key().as_ref(), args.terms.tab_id.as_ref()], bump)]
    pub reservation: Box<Account<'info, Reservation>>,
    #[account(init_if_needed, payer = payer, space = 8 + OperationReceipt::INIT_SPACE,
        seeds = [b"operation", principal.key().as_ref(), args.operation_id.as_ref()], bump)]
    pub operation: Box<Account<'info, OperationReceipt>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(args: StageArgs)]
pub struct ServiceReservation<'info> {
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump, seeds::program = CORE_ID)]
    pub core_authority: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut, has_one = principal,
        constraint = budget.asset == reservation.terms.asset @ RegistryError::IntentMismatch,
        seeds = [b"budget", principal.key().as_ref(), budget.asset.as_ref()], bump = budget.bump)]
    pub budget: Box<Account<'info, Budget>>,
    #[account(mut, constraint = reservation.terms.principal == principal.key() @ RegistryError::IntentMismatch,
        seeds = [b"reservation", principal.key().as_ref(), reservation.terms.tab_id.as_ref()], bump = reservation.bump)]
    pub reservation: Box<Account<'info, Reservation>>,
    #[account(init_if_needed, payer = payer, space = 8 + OperationReceipt::INIT_SPACE,
        seeds = [b"operation", principal.key().as_ref(), args.operation_id.as_ref()], bump)]
    pub operation: Box<Account<'info, OperationReceipt>>,
    pub system_program: Program<'info, System>,
}

// Only the Core-authenticated completed-run CPI can allocate a Registry receipt here.
#[derive(Accounts)]
#[instruction(args: StageArgs)]
pub struct ConsumeCompletedRun<'info> {
    #[account(seeds = [REGISTRY_CONSUMER_SEED], bump, seeds::program = CORE_ID)]
    pub core_authority: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut, has_one = principal,
        constraint = budget.asset == reservation.terms.asset @ RegistryError::IntentMismatch,
        seeds = [b"budget", principal.key().as_ref(), budget.asset.as_ref()], bump = budget.bump)]
    pub budget: Box<Account<'info, Budget>>,
    #[account(mut, constraint = reservation.terms.principal == principal.key() @ RegistryError::IntentMismatch,
        seeds = [b"reservation", principal.key().as_ref(), reservation.terms.tab_id.as_ref()], bump = reservation.bump)]
    pub reservation: Box<Account<'info, Reservation>>,
    #[account(init_if_needed, payer = payer, space = 8 + OperationReceipt::INIT_SPACE,
        seeds = [RUN_OPERATION_SEED, principal.key().as_ref(), args.operation_id.as_ref()], bump)]
    pub operation: Box<Account<'info, OperationReceipt>>,
    pub system_program: Program<'info, System>,
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
    #[msg("Wallet operation is not the exact approved Core repayment")] WalletRepayment,
    #[msg("Wallet operation bytes or effective privileges changed")] WalletBundleHash,
    #[msg("Allowance amendment uses a stale owner authority generation")] AllowanceGeneration,
    #[msg("Allowance policy revision is invalid or has changed")] AllowanceRevision,
    #[msg("Allowance ceiling cannot be below consumed plus reserved authority")] AllowanceFloor,
    #[msg("Allowance operation ID was already used for a different amendment")] AllowanceConflict,
    #[msg("Wallet operation is not an admitted collection, free withdrawal, repayment, send or swap")] FundingAction,
    #[msg("Owner rotation ID conflicts with its original immutable request")] OwnerRotationConflict,
    #[msg("Repayment permission is absent, revoked, expired, exhausted or does not cover this exact obligation and route")] RepaymentPermission,
    #[msg("Spending permission is absent, revoked, expired, exhausted or does not cover this action, asset, route and outcome")] SpendingPermission,
}
