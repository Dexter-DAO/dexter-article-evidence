//! Durable native-owner rotation for the same continuing principal. Accepted
//! claims and the fixed wallet collection route remain bound to that principal.
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{get_stack_height, TRANSACTION_LEVEL_STACK_HEIGHT};
use dexter_contracts::{plus, SCHEMA_VERSION};
use solana_sha256_hasher::hashv;
use crate::{Principal, RegistryError};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, InitSpace)]
pub struct RotatePrincipalOwnerArgs {
    pub operation_id: [u8; 32],
    pub expected_authority_generation: u64,
    pub new_owner: Pubkey,
}

#[account]
#[derive(InitSpace)]
pub struct OwnerRotationReceipt {
    pub version: u16,
    pub bump: u8,
    pub principal: Pubkey,
    pub operation_id: [u8; 32],
    pub old_owner: Pubkey,
    pub new_owner: Pubkey,
    pub previous_generation: u64,
    pub next_generation: u64,
    pub request_digest: [u8; 32],
}

#[derive(Accounts)]
#[instruction(args: RotatePrincipalOwnerArgs)]
pub struct RotatePrincipalOwner<'info> {
    #[account(mut, seeds = [b"principal", principal.seed_owner.as_ref(), principal.principal_id.as_ref()], bump = principal.bump)]
    pub principal: Box<Account<'info, Principal>>,
    // Historical replay is checked before current authority. Both signatures
    // still bind the exact original request; a fresh operation checks ownership.
    pub current_owner: Signer<'info>,
    #[account(address = args.new_owner @ RegistryError::WalletOwner)]
    pub new_owner: Signer<'info>,
    #[account(mut)] pub payer: Signer<'info>,
    #[account(init_if_needed, payer = payer, space = 8 + OwnerRotationReceipt::INIT_SPACE,
        seeds = [b"owner-rotation", principal.key().as_ref(), &args.operation_id], bump)]
    pub receipt: Box<Account<'info, OwnerRotationReceipt>>,
    pub system_program: Program<'info, System>,
}

pub fn rotate(ctx: Context<RotatePrincipalOwner>, args: RotatePrincipalOwnerArgs) -> Result<()> {
    require!(get_stack_height() == TRANSACTION_LEVEL_STACK_HEIGHT, RegistryError::WalletEnvelope);
    let principal = &mut ctx.accounts.principal;
    require!(principal.version == SCHEMA_VERSION, RegistryError::SchemaVersion);
    let old_owner = ctx.accounts.current_owner.key();
    require!(old_owner.is_on_curve() && args.new_owner.is_on_curve() && old_owner != args.new_owner,
        RegistryError::WalletOwner);
    let next_generation = plus(args.expected_authority_generation, 1)?;
    let digest = hashv(&[b"DEXTER:PRINCIPAL-OWNER-ROTATION:1", crate::ID.as_ref(), principal.key().as_ref(),
        &args.operation_id, old_owner.as_ref(), args.new_owner.as_ref(),
        &args.expected_authority_generation.to_le_bytes(), &next_generation.to_le_bytes()]).to_bytes();
    let receipt = &mut ctx.accounts.receipt;
    if receipt.version != 0 {
        require!(receipt.version == SCHEMA_VERSION && receipt.principal == principal.key()
            && receipt.operation_id == args.operation_id && receipt.old_owner == old_owner
            && receipt.new_owner == args.new_owner && receipt.previous_generation == args.expected_authority_generation
            && receipt.next_generation == next_generation && receipt.request_digest == digest,
            RegistryError::OwnerRotationConflict);
        return Ok(()); // Do not rotate authority or increment generation twice.
    }
    require_keys_eq!(old_owner, principal.owner, RegistryError::WalletOwner);
    require!(args.expected_authority_generation > 0
        && args.expected_authority_generation == principal.authority_generation, RegistryError::WalletGeneration);
    principal.owner = args.new_owner;
    principal.authority_generation = next_generation;
    receipt.set_inner(OwnerRotationReceipt { version: SCHEMA_VERSION, bump: ctx.bumps.receipt,
        principal: principal.key(), operation_id: args.operation_id, old_owner, new_owner: args.new_owner,
        previous_generation: args.expected_authority_generation, next_generation, request_digest: digest });
    emit!(PrincipalOwnerRotated { principal: principal.key(), old_owner, new_owner: args.new_owner,
        authority_generation: next_generation, receipt: receipt.key() });
    Ok(())
}

#[event]
pub struct PrincipalOwnerRotated {
    pub principal: Pubkey,
    pub old_owner: Pubkey,
    pub new_owner: Pubkey,
    pub authority_generation: u64,
    pub receipt: Pubkey,
}
