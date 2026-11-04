//! Absolute owner-approved changes to the existing shared lifetime allowance.
//! Counters and accepted financial rights stay on the original Budget account.
use anchor_lang::prelude::*;
use dexter_contracts::SCHEMA_VERSION;
use solana_sha256_hasher::hashv;

use crate::{Budget, Principal, RegistryError};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct AmendAllowanceArgs {
    pub operation_id: [u8; 32],
    pub authority_generation: u64,
    pub expected_revision: u64,
    pub ceiling: u64,
}

#[account]
#[derive(InitSpace)]
pub struct AllowanceAmendment {
    pub version: u16,
    pub bump: u8,
    pub domain: [u8; 32],
    pub principal: Pubkey,
    pub budget: Pubkey,
    pub asset: Pubkey,
    pub operation_id: [u8; 32],
    pub authorizing_owner: Pubkey,
    pub authority_generation: u64,
    pub expected_revision: u64,
    pub applied_revision: u64,
    pub before_ceiling: u64,
    pub ceiling: u64,
    pub reserved: u64,
    pub consumed: u64,
    pub request_digest: [u8; 32],
}

#[derive(Accounts)]
#[instruction(args: AmendAllowanceArgs)]
pub struct AmendAllowance<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    // A historic exact retry must match the original authorizing signer. A
    // fresh amendment additionally checks the current owner below.
    pub owner: Signer<'info>,
    #[account(seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()],
        bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    #[account(mut, has_one = principal,
        seeds = [b"budget", principal.key().as_ref(), budget.asset.as_ref()], bump = budget.bump)]
    pub budget: Box<Account<'info, Budget>>,
    #[account(init_if_needed, payer = payer, space = 8 + AllowanceAmendment::INIT_SPACE,
        seeds = [b"budget-amendment", principal.key().as_ref(), args.operation_id.as_ref()], bump)]
    pub amendment: Box<Account<'info, AllowanceAmendment>>,
    pub system_program: Program<'info, System>,
}

/// Fixed-width, versioned intent encoding shared with independent SDK readers.
/// The absolute ceiling and expected revision are part of the immutable request.
pub fn amendment_digest(domain: &[u8; 32], principal: &Pubkey, budget: &Pubkey,
    asset: &Pubkey, owner: &Pubkey, args: &AmendAllowanceArgs) -> [u8; 32] {
    hashv(&[b"DEXTER:BUDGET_AMENDMENT:1", crate::ID.as_ref(), domain,
        principal.as_ref(), budget.as_ref(), asset.as_ref(), owner.as_ref(), &args.operation_id,
        &args.authority_generation.to_le_bytes(), &args.expected_revision.to_le_bytes(),
        &args.ceiling.to_le_bytes()]).to_bytes()
}

pub fn amend(ctx: Context<AmendAllowance>, args: AmendAllowanceArgs) -> Result<()> {
    let principal = &ctx.accounts.principal;
    let budget = &mut ctx.accounts.budget;
    let owner = ctx.accounts.owner.key();
    let digest = amendment_digest(&principal.domain, &principal.key(), &budget.key(),
        &budget.asset, &owner, &args);
    let receipt = &ctx.accounts.amendment;
    if receipt.version != 0 {
        require!(receipt.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
        require_keys_eq!(owner, receipt.authorizing_owner, RegistryError::OwnerAuthority);
        require!(receipt.domain == principal.domain && receipt.principal == principal.key()
            && receipt.budget == budget.key() && receipt.asset == budget.asset
            && receipt.operation_id == args.operation_id && receipt.authority_generation == args.authority_generation
            && receipt.expected_revision == args.expected_revision && receipt.ceiling == args.ceiling
            && receipt.request_digest == digest, RegistryError::AllowanceConflict);
        // Returning an immutable result never reapplies the old ceiling. Later
        // policy revisions, captures and reservations remain exactly as they are.
        return Ok(());
    }

    require!(principal.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
    require_keys_eq!(owner, principal.owner, RegistryError::OwnerAuthority);
    require!(args.authority_generation == principal.authority_generation, RegistryError::AllowanceGeneration);
    let before_ceiling = budget.ceiling;
    let reserved = budget.reserved;
    let consumed = budget.consumed;
    budget.amend_ceiling(args.expected_revision, args.ceiling)?;
    ctx.accounts.amendment.set_inner(AllowanceAmendment {
        version: SCHEMA_VERSION, bump: ctx.bumps.amendment, domain: principal.domain,
        principal: principal.key(), budget: budget.key(), asset: budget.asset,
        operation_id: args.operation_id, authorizing_owner: owner,
        authority_generation: args.authority_generation, expected_revision: args.expected_revision,
        applied_revision: budget.revision, before_ceiling, ceiling: args.ceiling, reserved, consumed, request_digest: digest,
    });
    Ok(())
}
